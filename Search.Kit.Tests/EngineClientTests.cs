using System.IO.Pipes;
using System.Text;
using System.Text.Json.Nodes;
using SearchKit.Engine;

namespace SearchKit.Tests;

/// The client against a stand-in engine: a pipe server in this process that
/// speaks the protocol, so every edge (order, errors, events, a crash) can be
/// made to happen on purpose.
public class EngineClientTests
{
    private static string NewPipe() => $"search-kit-test-{Guid.NewGuid():N}";

    /// Serves one connection: answers each call with `respond`, after `delay`.
    private static async Task FakeEngine(string pipe, Func<JsonObject, JsonObject> respond,
        Func<JsonObject, int>? delay = null, Func<StreamWriter, Task>? onConnect = null, int calls = int.MaxValue)
    {
        await using var server = new NamedPipeServerStream(pipe, PipeDirection.InOut, 1,
            PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
        await server.WaitForConnectionAsync();
        var reader = new StreamReader(server, new UTF8Encoding(false));
        var writer = new StreamWriter(server, new UTF8Encoding(false)) { NewLine = "\n", AutoFlush = true };
        var gate = new SemaphoreSlim(1, 1);
        if (onConnect != null) await onConnect(writer);
        var answered = new List<Task>();
        for (var i = 0; i < calls && await reader.ReadLineAsync() is { } line; i++)
        {
            var request = JsonNode.Parse(line)!.AsObject();
            answered.Add(Task.Run(async () =>
            {
                await Task.Delay(delay?.Invoke(request) ?? 0);
                var reply = respond(request);
                reply["id"] = request["id"]!.DeepClone();
                await gate.WaitAsync();
                try { await writer.WriteLineAsync(reply.ToJsonString()); } finally { gate.Release(); }
            }));
        }
        await Task.WhenAll(answered);
    }

    [Fact]
    public async Task A_call_gets_its_answer()
    {
        var pipe = NewPipe();
        var engine = FakeEngine(pipe, r => new JsonObject { ["result"] = r["method"]!.GetValue<string>() + "!" }, calls: 1);
        await using var client = new EngineClient(pipe);
        var answer = await client.CallAsync("hello");
        Assert.Equal("hello!", answer!.GetValue<string>());
        await engine;
    }

    [Fact]
    public async Task Answers_that_come_back_out_of_order_find_their_calls()
    {
        var pipe = NewPipe();
        var engine = FakeEngine(pipe,
            r => new JsonObject { ["result"] = r["params"]!["n"]!.GetValue<int>() * 10 },
            delay: r => r["params"]!["n"]!.GetValue<int>() == 1 ? 300 : 0,
            calls: 3);
        await using var client = new EngineClient(pipe);
        var slow = client.CallAsync("x", new JsonObject { ["n"] = 1 });
        var fast = client.CallAsync("x", new JsonObject { ["n"] = 2 });
        var other = client.CallAsync("x", new JsonObject { ["n"] = 3 });
        Assert.Equal(20, (await fast)!.GetValue<int>());
        Assert.Equal(30, (await other)!.GetValue<int>());
        Assert.False(slow.IsCompleted);
        Assert.Equal(10, (await slow)!.GetValue<int>());
        await engine;
    }

    [Fact]
    public async Task An_error_carries_its_message_and_detail()
    {
        var pipe = NewPipe();
        var engine = FakeEngine(pipe, _ => new JsonObject
        {
            ["error"] = new JsonObject { ["message"] = "Unknown algorithm: nope", ["data"] = new JsonObject { ["code"] = 7 } },
        }, calls: 1);
        await using var client = new EngineClient(pipe);
        var error = await Assert.ThrowsAsync<EngineException>(() => client.CallAsync("encode_decode"));
        Assert.Equal("Unknown algorithm: nope", error.Message);
        Assert.Equal(7, error.Detail!["code"]!.GetValue<int>());
        await engine;
    }

    [Fact]
    public async Task Events_reach_the_listener()
    {
        var pipe = NewPipe();
        var heard = new TaskCompletionSource<(string, JsonNode?)>();
        var engine = FakeEngine(pipe, _ => new JsonObject { ["result"] = null },
            onConnect: async w =>
            {
                await Task.Delay(100);
                await w.WriteLineAsync("""{"event":"fm-progress","payload":{"done":3}}""");
            }, calls: 1);
        await using var client = new EngineClient(pipe);
        client.EventReceived += (name, payload) => heard.TrySetResult((name, payload));
        await client.CallAsync("start");
        var (name, payload) = await heard.Task.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.Equal("fm-progress", name);
        Assert.Equal(3, payload!["done"]!.GetValue<int>());
        await engine;
    }

    [Fact]
    public async Task No_engine_and_no_way_to_start_one_is_a_clear_error()
    {
        await using var client = new EngineClient(NewPipe());
        var error = await Assert.ThrowsAsync<EngineException>(() => client.CallAsync("anything"));
        Assert.Contains("isn't running", error.Message);
    }

    [Fact]
    public async Task A_missing_engine_is_started_on_the_first_call()
    {
        var pipe = NewPipe();
        Task? engine = null;
        var starts = 0;
        await using var client = new EngineClient(pipe, _ =>
        {
            starts++;
            engine = FakeEngine(pipe, _ => new JsonObject { ["result"] = "up" }, calls: 1);
            return Task.CompletedTask;
        });
        Assert.Equal("up", (await client.CallAsync("engine.hello"))!.GetValue<string>());
        Assert.Equal(1, starts);
        await engine!;
    }

    [Fact]
    public async Task A_crash_fails_the_waiting_call_and_the_next_call_starts_it_again()
    {
        var pipe = NewPipe();
        var starts = 0;
        Task? engine = null;
        await using var client = new EngineClient(pipe, _ =>
        {
            starts++;
            engine = starts == 1
                // First engine: reads one call and dies without answering.
                ? Task.Run(async () =>
                {
                    await using var server = new NamedPipeServerStream(pipe, PipeDirection.InOut, 1,
                        PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
                    await server.WaitForConnectionAsync();
                    await new StreamReader(server).ReadLineAsync();
                })
                : FakeEngine(pipe, _ => new JsonObject { ["result"] = "again" }, calls: 1);
            return Task.CompletedTask;
        });
        var lost = await Assert.ThrowsAsync<EngineException>(() => client.CallAsync("work"));
        Assert.Contains("went away", lost.Message);
        await engine!;
        Assert.Equal("again", (await client.CallAsync("work"))!.GetValue<string>());
        Assert.Equal(2, starts);
        await engine!;
    }
}

/// The real engine, when it has been built (Engine/target/debug/kil-engine.exe).
public class RealEngineTests
{
    private static string? FindEngine()
    {
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir != null; dir = dir.Parent)
        {
            var exe = Path.Combine(dir.FullName, "Engine", "target", "debug", "kil-engine.exe");
            if (File.Exists(exe)) return exe;
        }
        return null;
    }

    [Fact]
    public async Task The_engine_starts_answers_and_names_its_commands()
    {
        if (!OperatingSystem.IsWindows() || FindEngine() is not { } exe) return;
        var pipe = $"search-kit-real-{Guid.NewGuid():N}";
        var data = Path.Combine(Path.GetTempPath(), pipe);
        await using var client = new EngineClient(pipe,
            cancel => EngineLauncher.StartAsync(new EngineSetup(exe, data, pipe, LingerSeconds: 1), cancel));
        var hello = await client.CallAsync("engine.hello");
        Assert.Equal("kil-engine", hello!["engine"]!.GetValue<string>());
        Assert.True(hello["commands"]!.GetValue<int>() > 300);
        var encoded = await client.CallAsync("encode_decode",
            new JsonObject { ["algorithm"] = "base64", ["mode"] = "encode", ["input"] = "search" });
        Assert.Equal("c2VhcmNo", encoded!.GetValue<string>());
        var methods = (await client.CallAsync("engine.methods"))!.AsArray().Select(n => n!.GetValue<string>()).ToList();
        Assert.Contains("search_local_files", methods);
        Assert.Contains("get_clipboard_history", methods);
    }
}
