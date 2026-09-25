using System.Text.Json.Nodes;
using SearchKit.Field;

namespace SearchKit.Tests.Field;

/// A stand-in engine: answers each call with `Answer`, and records the calls.
internal sealed class FakeEngine : IEngineCalls
{
    public Func<string, JsonNode?, CancellationToken, Task<JsonNode?>> Answer { get; set; } =
        (_, _, _) => Task.FromResult<JsonNode?>(null);

    public List<(string Method, JsonNode? Args)> Calls { get; } = [];

    public Task<JsonNode?> CallAsync(string method, JsonNode? args, CancellationToken cancel)
    {
        lock (Calls) Calls.Add((method, args?.DeepClone()));
        return Answer(method, args, cancel);
    }

    public event Action<string, JsonNode?>? EventReceived;

    public void Raise(string name) => EventReceived?.Invoke(name, null);

    public static JsonNode? Json(string text) => JsonNode.Parse(text);
}

/// An engine source that answers from a function, after `delay`.
internal sealed class FakeSource(
    Group group,
    Func<FieldQuery, CancellationToken, Task<IReadOnlyList<FieldRow>>> answer,
    TimeSpan? delay = null,
    Func<FieldQuery, bool>? wants = null) : IEngineSource
{
    public int Asked;
    public List<string> Texts { get; } = [];

    public Group Group => group;
    public bool Wants(FieldQuery query) => wants?.Invoke(query) ?? query.Kind == QueryKind.Words;
    public TimeSpan Delay(FieldQuery query) => delay ?? TimeSpan.Zero;

    public Task<IReadOnlyList<FieldRow>> SuggestAsync(FieldQuery query, int limit, CancellationToken cancel)
    {
        Interlocked.Increment(ref Asked);
        lock (Texts) Texts.Add(query.Text);
        return answer(query, cancel);
    }
}

internal static class Rows
{
    public static FieldRow Page(Group group, string key, double score = 0) =>
        new(group, "url:" + key, key, key, RowAction.Go, "https://" + key + "/") { Score = score };

    public static FieldRow File(string name) =>
        new(Group.Files, RowKey.File(@"C:\t\" + name), name, @"C:\t", FileKinds.ActionFor(Path.GetExtension(name)), @"C:\t\" + name);

    public static FieldRow Search(string text) =>
        new(Group.Search, RowKey.WebSearch(text), text, "Google", RowAction.Go, "https://www.google.com/search?q=" + text);

    public static FieldRow Answer(string text) =>
        new(Group.Answer, RowKey.Answer(text), text, "", RowAction.Copy, text);

    public static IReadOnlyList<string> Keys(this FieldBoard board) => [.. board.Rows.Select(r => r.Key)];
}
