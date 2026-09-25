using System.Collections.Concurrent;
using System.IO.Pipes;
using System.Text;
using System.Text.Json.Nodes;

namespace SearchKit.Engine;

/// A call the engine answered with an error. `Detail` is the command's own
/// error value (often just the message again, sometimes an object).
public sealed class EngineException(string message, JsonNode? detail = null) : Exception(message)
{
    public JsonNode? Detail { get; } = detail;
}

/// The browser's side of the engine's pipe (see docs/brain/Engine Protocol.md).
///
/// Connects on the first call, starting the engine first if it isn't there.
/// Calls run concurrently and are matched to answers by id; events go to
/// `EventReceived`. If the engine goes away, the calls waiting on it fail and
/// the next call starts it again, so a crashed engine costs one error, not
/// a broken browser.
public sealed class EngineClient : IAsyncDisposable
{
    private readonly string pipe;
    private readonly Func<CancellationToken, Task>? start;
    private readonly TimeSpan startWait;
    private readonly SemaphoreSlim connecting = new(1, 1);
    private readonly SemaphoreSlim writing = new(1, 1);
    private readonly ConcurrentDictionary<long, TaskCompletionSource<JsonNode?>> pending = new();
    private NamedPipeClientStream? stream;
    private StreamWriter? writer;
    private long nextId;

    /// `start` launches the engine; without it the client only ever connects
    /// to one that is already running.
    public EngineClient(string pipe, Func<CancellationToken, Task>? start = null, TimeSpan? startWait = null)
    {
        this.pipe = pipe;
        this.start = start;
        this.startWait = startWait ?? TimeSpan.FromSeconds(8);
    }

    /// An engine event: its name and payload. Raised on a background thread.
    public event Action<string, JsonNode?>? EventReceived;

    /// A new connection: the first, or one to an engine started again after
    /// the last went away. What a running engine was asked to keep doing (the
    /// clipboard listener) has to be asked of this one again. Raised on a
    /// background thread, after the call that connected is under way.
    public event Action? Connected;

    /// The engine went away (it exited, or crashed). Nothing reconnects until
    /// the next call. Raised on a background thread.
    public event Action? Disconnected;

    public bool IsConnected => stream?.IsConnected == true;

    public async Task<JsonNode?> CallAsync(string method, JsonNode? args = null, CancellationToken cancel = default)
    {
        await EnsureConnectedAsync(cancel).ConfigureAwait(false);
        var id = Interlocked.Increment(ref nextId);
        var answer = new TaskCompletionSource<JsonNode?>(TaskCreationOptions.RunContinuationsAsynchronously);
        pending[id] = answer;
        var line = new JsonObject
        {
            ["id"] = id,
            ["method"] = method,
            ["params"] = args?.DeepClone() ?? new JsonObject(),
        }.ToJsonString();
        try
        {
            await WriteLineAsync(line, cancel).ConfigureAwait(false);
        }
        catch
        {
            pending.TryRemove(id, out _);
            throw;
        }
        using var cancelled = cancel.Register(() =>
        {
            if (pending.TryRemove(id, out var waiting)) waiting.TrySetCanceled(cancel);
        });
        return await answer.Task.ConfigureAwait(false);
    }

    private async Task EnsureConnectedAsync(CancellationToken cancel)
    {
        if (IsConnected) return;
        await connecting.WaitAsync(cancel).ConfigureAwait(false);
        try
        {
            if (IsConnected) return;
            Drop();
            var attempt = new NamedPipeClientStream(".", pipe, PipeDirection.InOut, PipeOptions.Asynchronous);
            if (!await TryConnectAsync(attempt, TimeSpan.FromMilliseconds(250), cancel).ConfigureAwait(false))
            {
                if (start == null)
                {
                    await attempt.DisposeAsync().ConfigureAwait(false);
                    throw new EngineException($"The engine isn't running (pipe {pipe}).");
                }
                await start(cancel).ConfigureAwait(false);
                var until = DateTime.UtcNow + startWait;
                var connected = false;
                while (!connected && DateTime.UtcNow < until)
                {
                    connected = await TryConnectAsync(attempt, TimeSpan.FromMilliseconds(200), cancel).ConfigureAwait(false);
                }
                if (!connected)
                {
                    await attempt.DisposeAsync().ConfigureAwait(false);
                    throw new EngineException("The engine didn't start.");
                }
            }
            stream = attempt;
            writer = new StreamWriter(attempt, new UTF8Encoding(false)) { NewLine = "\n", AutoFlush = false };
            var reading = new StreamReader(attempt, new UTF8Encoding(false));
            _ = Task.Run(() => ReadAsync(attempt, reading), CancellationToken.None);
            if (Connected is { } told) _ = Task.Run(() => { try { told(); } catch { } }, CancellationToken.None);
        }
        finally
        {
            connecting.Release();
        }
    }

    private static async Task<bool> TryConnectAsync(NamedPipeClientStream attempt, TimeSpan wait, CancellationToken cancel)
    {
        try
        {
            await attempt.ConnectAsync((int)wait.TotalMilliseconds, cancel).ConfigureAwait(false);
            return true;
        }
        catch (TimeoutException)
        {
            return false;
        }
        catch (IOException)
        {
            // The name exists but every instance is busy; try again shortly.
            await Task.Delay(50, cancel).ConfigureAwait(false);
            return false;
        }
    }

    private async Task WriteLineAsync(string line, CancellationToken cancel)
    {
        await writing.WaitAsync(cancel).ConfigureAwait(false);
        try
        {
            var to = writer ?? throw new EngineException("The engine went away.");
            await to.WriteLineAsync(line.AsMemory(), cancel).ConfigureAwait(false);
            await to.FlushAsync(cancel).ConfigureAwait(false);
        }
        // A broken pipe, or one the reader closed a moment ago because the
        // engine died while this call was being written.
        catch (Exception error) when (error is IOException or ObjectDisposedException or InvalidOperationException)
        {
            throw new EngineException($"The engine went away: {error.Message}");
        }
        finally
        {
            writing.Release();
        }
    }

    private async Task ReadAsync(NamedPipeClientStream from, StreamReader reading)
    {
        try
        {
            while (await reading.ReadLineAsync().ConfigureAwait(false) is { } line)
            {
                if (line.Length == 0) continue;
                JsonNode? message;
                try { message = JsonNode.Parse(line); }
                catch { continue; }
                if (message is not JsonObject body) continue;

                if (body["event"] is JsonNode name)
                {
                    try { EventReceived?.Invoke(name.GetValue<string>(), body["payload"]?.DeepClone()); }
                    catch { }
                    continue;
                }
                if (body["id"] is not JsonNode idNode || !pending.TryRemove(idNode.GetValue<long>(), out var waiting))
                    continue;
                if (body["error"] is JsonObject error)
                {
                    var text = error["message"]?.GetValue<string>() ?? "The engine couldn't do that.";
                    waiting.TrySetException(new EngineException(text, error["data"]?.DeepClone()));
                }
                else
                {
                    waiting.TrySetResult(body["result"]?.DeepClone());
                }
            }
        }
        catch (IOException) { }
        catch (ObjectDisposedException) { }
        finally
        {
            // Only when this was the connection in use: one closed on purpose
            // (DisposeAsync) or already replaced isn't the engine going away.
            var gone = ReferenceEquals(stream, from);
            if (gone) Drop();
            foreach (var id in pending.Keys)
            {
                if (pending.TryRemove(id, out var waiting))
                    waiting.TrySetException(new EngineException("The engine went away."));
            }
            if (gone)
            {
                try { Disconnected?.Invoke(); } catch { }
            }
        }
    }

    private void Drop()
    {
        try { stream?.Dispose(); } catch { }
        stream = null;
        writer = null;
    }

    public ValueTask DisposeAsync()
    {
        Drop();
        return ValueTask.CompletedTask;
    }
}
