using System.Text.Json.Nodes;
using SearchKit.Engine;

namespace SearchKit.Field;

/// A source that answers at once, on the UI thread, on every keystroke:
/// open tabs, history, bookmarks, commands, the typed text itself. The whole
/// keystroke has 5 ms; a source should need well under one.
public interface ILocalSource
{
    Group Group { get; }

    /// Rows for this query, best first, at most `limit`. Nothing to say is an
    /// empty list, never an exception.
    IReadOnlyList<FieldRow> Suggest(FieldQuery query, int limit);
}

/// A source the engine answers: files by name and by contents, apps, instant
/// answers, clipboard history. Asked off the UI thread, after `Delay`, and
/// cancelled by the next keystroke.
public interface IEngineSource
{
    Group Group { get; }

    /// Whether this query is for it at all. Cheap: it's asked per keystroke.
    bool Wants(FieldQuery query);

    /// How long to wait for the typing to pause before asking. Zero asks at once.
    TimeSpan Delay(FieldQuery query);

    /// Rows for this query, best first, at most `limit`. `cancel` fires when
    /// the next keystroke makes the question stale.
    Task<IReadOnlyList<FieldRow>> SuggestAsync(FieldQuery query, int limit, CancellationToken cancel);
}

/// What the field's engine sources need from the engine: calls and events.
/// `EngineCalls` is the real one; tests pass a fake.
public interface IEngineCalls
{
    Task<JsonNode?> CallAsync(string method, JsonNode? args, CancellationToken cancel);

    /// An engine event (name, payload), raised on a background thread.
    event Action<string, JsonNode?>? EventReceived;
}

/// The field's view of the browser's EngineClient.
public sealed class EngineCalls : IEngineCalls
{
    private readonly EngineClient client;

    public EngineCalls(EngineClient client)
    {
        this.client = client;
        client.EventReceived += (name, payload) => EventReceived?.Invoke(name, payload);
    }

    public Task<JsonNode?> CallAsync(string method, JsonNode? args, CancellationToken cancel) =>
        client.CallAsync(method, args, cancel);

    public event Action<string, JsonNode?>? EventReceived;
}
