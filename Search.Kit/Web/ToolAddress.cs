namespace SearchKit.Web;

/// The tool pages' addresses: what's typed (`search://tools/<id>`,
/// `search://play?path=<encoded>`) and the https://tools.search/ page it
/// is, both ways. What the field shows goes back to the same page when
/// Enter is pressed on it.
public static class ToolAddress
{
    public const string Host = "tools.search";

    private const string Tool = "#/tool/";
    private const string Play = "#/play?path=";

    /// `search://tools`, `search://tools/<id>` → the page;
    /// `search://play?path=<encoded>` → the player, for one file. Null for
    /// anything else, and for a path `refused` turns down.
    public static Uri? Resolve(string typed, Func<string, bool>? refused = null)
    {
        if (!Uri.TryCreate(typed.Trim(), UriKind.Absolute, out var url)) return null;
        if (url.Host == "play")
        {
            var path = QueryValue(url, "path");
            if (string.IsNullOrEmpty(path) || refused?.Invoke(path) == true) return null;
            return new Uri($"https://{Host}/index.html{Play}{Uri.EscapeDataString(path)}");
        }
        if (url.Host != "tools") return null;
        // A folder mapping serves files, never a folder's index: the page is
        // always named.
        var id = url.AbsolutePath.Trim('/');
        return new Uri(id.Length == 0 ? $"https://{Host}/index.html" : $"https://{Host}/index.html{Tool}{Uri.EscapeDataString(id)}");
    }

    private static string? QueryValue(Uri url, string key)
    {
        var pair = url.Query.TrimStart('?').Split('&')
            .Select(p => p.Split('=', 2))
            .FirstOrDefault(p => p[0] == key);
        return pair is { Length: 2 } ? Uri.UnescapeDataString(pair[1]) : null;
    }

    /// What the field shows for a tool page, or the player. A path keeps
    /// escaped only what would change its meaning when typed back (`%`,
    /// `&`, `#`, `+`), so it still reads as a path.
    public static string Pretty(Uri url)
    {
        var fragment = url.Fragment;
        if (fragment.StartsWith(Tool, StringComparison.Ordinal))
            return "search://tools/" + Uri.UnescapeDataString(fragment[Tool.Length..]);
        if (fragment.StartsWith(Play, StringComparison.Ordinal))
            return "search://play?path=" + Keep(Uri.UnescapeDataString(fragment[Play.Length..]));
        return "search://tools";
    }

    private static string Keep(string path) =>
        path.Replace("%", "%25").Replace("&", "%26").Replace("#", "%23").Replace("+", "%2B");
}
