using System.Text.Json.Nodes;
using SearchKit.Field;

namespace SearchKit.Web;

/// Who may reach the tool pages (https://tools.search/), and which paths a
/// tool page may hand the browser or the engine. Decided here, without a
/// window, so it can be tested.
public static class ToolGate
{
    /// A navigation into the tool pages: only Search's own (the field, a
    /// command, the player), one from a tool page itself, or back, forward
    /// and reload. `fromTools` is the document the tab really has — never
    /// an address set before the engine said so.
    public static bool MayNavigate(bool toTools, bool fromTools, bool ours, bool newDocument) =>
        !toTools || ours || fromTools || !newDocument;

    /// A new window a page asks for (window.open, a target=_blank link)
    /// never lands on a tool page, whoever asks: the tool pages open their
    /// own links through the browser, and nothing else has any business
    /// there.
    public static bool MayOpenWindow(bool toTools) => !toTools;

    /// A network share or a device path: `\\server\share`, `//server/share`,
    /// `\\?\…`, `\\.\…`, `\??\…`. Merely touching one can hand the Windows
    /// sign-in to whoever runs the server.
    public static bool IsRemote(string? path)
    {
        if (string.IsNullOrEmpty(path)) return false;
        var p = path.TrimStart().Replace('/', '\\');
        return p.StartsWith(@"\\", StringComparison.Ordinal) || p.StartsWith(@"\??\", StringComparison.Ordinal);
    }

    /// Why a tool page may not use `path`, or null when it may. A share or a
    /// device path only inside a folder chosen in Settings › Search, and
    /// never climbing out of it.
    public static string? PathRefused(string? path, IReadOnlyList<string> chosen)
    {
        if (!IsRemote(path)) return null;
        var p = path!.Trim();
        var climbs = p.Replace('/', '\\').Split('\\').Any(part => part == ".." || part == ".");
        var device = p.Replace('/', '\\') is var d && (d.StartsWith(@"\\?\", StringComparison.Ordinal)
            || d.StartsWith(@"\\.\", StringComparison.Ordinal) || d.StartsWith(@"\??\", StringComparison.Ordinal));
        if (!climbs && !device && IndexPlan.Covers(chosen, p)) return null;
        return "Search doesn't open network or device paths from a tool page";
    }

    /// Argument names that carry a path (`path`, `folderPath`, `roots`,
    /// `dir`…).
    private static bool PathKey(string key)
    {
        var k = key.ToLowerInvariant();
        return k.Contains("path") || k.Contains("folder") || k.Contains("dir") || k.Contains("root") || k.Contains("file");
    }

    /// Why a tool page's call is refused for a path it carries, or null.
    /// Every argument named like a path, at any depth, strings and lists of
    /// them.
    public static string? ArgsRefused(JsonNode? args, IReadOnlyList<string> chosen) => Scan(args, pathy: false, chosen, 0);

    private static string? Scan(JsonNode? node, bool pathy, IReadOnlyList<string> chosen, int depth)
    {
        if (node == null || depth > 8) return null;
        switch (node)
        {
            case JsonObject obj:
                foreach (var (key, value) in obj)
                    if (Scan(value, PathKey(key), chosen, depth + 1) is { } why) return why;
                return null;
            case JsonArray list:
                foreach (var item in list)
                    if (Scan(item, pathy, chosen, depth + 1) is { } why) return why;
                return null;
            case JsonValue value when pathy && value.TryGetValue<string>(out var text):
                return PathRefused(text, chosen);
            default:
                return null;
        }
    }
}
