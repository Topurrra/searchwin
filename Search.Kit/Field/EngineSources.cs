using System.Net;
using System.Text;
using System.Text.Json.Nodes;

namespace SearchKit.Field;

/// Reading the engine's answers without trusting their shape: a missing or
/// oddly typed field is an empty value, never an exception.
internal static class Nodes
{
    public static string Str(JsonNode? node, string name) =>
        node is JsonObject o && o[name] is JsonValue v && v.TryGetValue<string>(out var s) ? s : "";

    public static long Long(JsonNode? node, string name)
    {
        if (node is not JsonObject o || o[name] is not JsonValue v) return 0;
        if (v.TryGetValue<long>(out var l)) return l;
        return v.TryGetValue<double>(out var d) ? (long)d : 0;
    }

    public static bool Bool(JsonNode? node, string name) =>
        node is JsonObject o && o[name] is JsonValue v && v.TryGetValue<bool>(out var b) && b;

    public static IEnumerable<JsonNode> Items(JsonNode? node, string? name = null)
    {
        var list = name == null ? node as JsonArray : (node as JsonObject)?[name] as JsonArray;
        if (list == null) yield break;
        foreach (var item in list) if (item != null) yield return item;
    }

    public static List<string> Strings(JsonNode? node, string name) =>
        [.. Items(node, name).OfType<JsonValue>().Select(v => v.TryGetValue<string>(out var s) ? s : "").Where(s => s.Length > 0)];

    /// One line, whitespace collapsed, at most `max` characters.
    public static string Line(string text, int max)
    {
        var line = new StringBuilder(Math.Min(text.Length, max + 1));
        var space = false;
        foreach (var c in text)
        {
            if (char.IsWhiteSpace(c)) { space = line.Length > 0; continue; }
            if (space) { line.Append(' '); space = false; }
            line.Append(c);
            if (line.Length > max) return line.ToString(0, max - 1) + "…";
        }
        return line.ToString();
    }

    public static string Folder(string path)
    {
        var slash = path.TrimEnd('\\', '/').LastIndexOfAny(['\\', '/']);
        return slash <= 0 ? "" : path[..slash];
    }

    public static string Name(string path)
    {
        var trimmed = path.TrimEnd('\\', '/');
        return trimmed[(trimmed.LastIndexOfAny(['\\', '/']) + 1)..];
    }
}

/// Files by name, from the engine's filename index
/// (`search_local_files {options: {query, limit}}`). Enter opens a PDF,
/// image or text file in a tab, audio and video in the player, anything else
/// in its own app, except a program or a script, which it shows in its
/// folder.
public sealed class FileNameSource(IEngineCalls engine) : IEngineSource
{
    public Group Group => Group.Files;

    public bool Wants(FieldQuery query) =>
        (query.Scope == Scope.Files && query.Kind == QueryKind.Words)
        || (query.Scope == Scope.All && query.Text.Length >= 2
            // "invoice.pdf" reads as an address; it's also a file name.
            && (query.Kind == QueryKind.Words || (query.Kind == QueryKind.Address && query.Text.IndexOfAny(['/', ':']) < 0)));

    public TimeSpan Delay(FieldQuery query) => TimeSpan.FromMilliseconds(query.Scope == Scope.Files ? 20 : 40);

    public async Task<IReadOnlyList<FieldRow>> SuggestAsync(FieldQuery query, int limit, CancellationToken cancel)
    {
        var args = new JsonObject { ["options"] = new JsonObject { ["query"] = query.Text, ["limit"] = limit } };
        var answer = await engine.CallAsync("search_local_files", args, cancel).ConfigureAwait(false);
        return [.. Nodes.Items(answer, "results").Select(File).OfType<FieldRow>().Take(limit)];
    }

    internal static FieldRow? File(JsonNode item)
    {
        var path = Nodes.Str(item, "path");
        if (path.Length == 0) return null;
        var name = Nodes.Str(item, "fileName");
        var folder = Nodes.Str(item, "entryType") == "folder";
        // By the path itself: what's opened is the path, whatever the index
        // says its extension is.
        var action = FileKinds.ActionForPath(path, folder);
        if (action != RowAction.Reveal && FileKinds.Runs(Nodes.Str(item, "extension"))) action = RowAction.Reveal;
        return new FieldRow(Group.Files, RowKey.File(path), name.Length > 0 ? name : Nodes.Name(path), Nodes.Folder(path),
            action, path)
        {
            Score = Nodes.Long(item, "score"),
            Sensitive = Nodes.Strings(item, "sensitiveKinds").Count > 0,
        };
    }
}

/// Files by what's inside them, from the engine's content index
/// (`search_file_contents {options: {query, limit}}`). Slower than names, so
/// it waits longer for the typing to pause. The snippet is the row's detail,
/// except for a file the index found a secret in.
public sealed class FileContentSource(IEngineCalls engine) : IEngineSource
{
    public Group Group => Group.Files;

    public bool Wants(FieldQuery query) =>
        query.Kind == QueryKind.Words && query.Text.Length >= 3 && query.Scope is Scope.All or Scope.Files;

    public TimeSpan Delay(FieldQuery query) => TimeSpan.FromMilliseconds(query.Scope == Scope.Files ? 60 : 120);

    public async Task<IReadOnlyList<FieldRow>> SuggestAsync(FieldQuery query, int limit, CancellationToken cancel)
    {
        var args = new JsonObject { ["options"] = new JsonObject { ["query"] = query.Text, ["limit"] = limit } };
        var answer = await engine.CallAsync("search_file_contents", args, cancel).ConfigureAwait(false);
        var rows = new List<FieldRow>();
        foreach (var item in Nodes.Items(answer, "results"))
        {
            if (rows.Count >= limit) break;
            if (FileNameSource.File(item) is not { } row) continue;
            var detail = row.Sensitive ? "Holds something sensitive" : Snippet(Nodes.Str(item, "snippet"));
            rows.Add(row with { Detail = detail.Length > 0 ? detail : row.Detail });
        }
        return rows;
    }

    /// The engine's snippet is HTML with `<b>` around the matches: plain text here.
    internal static string Snippet(string html)
    {
        if (html.Length == 0) return "";
        var text = new StringBuilder(html.Length);
        var inTag = false;
        foreach (var c in html)
        {
            if (c == '<') inTag = true;
            else if (c == '>') inTag = false;
            else if (!inTag) text.Append(c);
        }
        return Nodes.Line(WebUtility.HtmlDecode(text.ToString()), 140);
    }
}

/// Installed apps, from the engine's launcher cache
/// (`search_launch_targets {options: {query, limit, browseAll}}`). `apps:`
/// on its own lists them all, most used first.
public sealed class AppSource(IEngineCalls engine) : IEngineSource
{
    public Group Group => Group.Apps;

    public bool Wants(FieldQuery query) =>
        (query.Scope == Scope.Apps && query.Kind is QueryKind.Words or QueryKind.Empty)
        || (query.Scope == Scope.All && query.Kind == QueryKind.Words && query.Text.Length >= 2);

    public TimeSpan Delay(FieldQuery query) => TimeSpan.FromMilliseconds(query.Scope == Scope.Apps ? 0 : 30);

    public async Task<IReadOnlyList<FieldRow>> SuggestAsync(FieldQuery query, int limit, CancellationToken cancel)
    {
        var args = new JsonObject
        {
            ["options"] = new JsonObject { ["query"] = query.Text, ["limit"] = limit, ["browseAll"] = query.Kind == QueryKind.Empty },
        };
        var answer = await engine.CallAsync("search_launch_targets", args, cancel).ConfigureAwait(false);
        var rows = new List<FieldRow>();
        foreach (var item in Nodes.Items(answer, "results"))
        {
            if (rows.Count >= limit) break;
            var path = Nodes.Str(item, "path");
            var name = Nodes.Str(item, "name");
            if (path.Length == 0 || name.Length == 0) continue;
            rows.Add(new FieldRow(Group.Apps, RowKey.App(path), name, "App", RowAction.Launch, path));
        }
        return rows;
    }
}

