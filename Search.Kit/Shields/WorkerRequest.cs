namespace SearchKit.Shields;

/// A service worker's (or a shared worker's) own request, as the browser
/// hears it — apart from WebView2, so it can be tested without a browser.
///
/// What WebView2 (runtime 153) hands over for one, seen in a test world: the
/// context is always XmlHttpRequest — a worker's `fetch()` — whatever the
/// page asked for, a navigation included; there are no Sec-Fetch headers
/// and no Origin; there is the Accept the page's own request had, the
/// Upgrade-Insecure-Requests a navigation sends, and a Referer naming the
/// page. So those are what say what the request is, and for whom.
public static class WorkerRequest
{
    /// A navigation the worker fetches for a page (a PWA's
    /// `fetch(event.request)`): the page itself, which Shields never
    /// refuses — a page you asked for is a page you get, served by its
    /// worker or not. `document` is WebView2's Document context; the
    /// Sec-Fetch headers say so where the engine shows them; otherwise a
    /// navigation's Accept (HTML first) and its Upgrade-Insecure-Requests.
    public static bool IsNavigation(bool document, string? fetchMode, string? fetchDest, string? accept = null, string? upgradeInsecure = null) =>
        document
        || Is(fetchMode, "navigate")
        || Is(fetchDest, "document")
        || Is(upgradeInsecure, "1")
        || (accept ?? "").TrimStart().StartsWith("text/html", StringComparison.OrdinalIgnoreCase);

    /// What the page asked for, when the context only says "a worker's
    /// fetch": an image's or a stylesheet's Accept gives it away, so
    /// `$image` and `$~xmlhttprequest` rules hold for what a worker
    /// fetches as they do for the page. Anything else keeps `context`.
    public static ResourceKind Kind(ResourceKind context, string? accept)
    {
        if (context != ResourceKind.XmlHttpRequest || string.IsNullOrWhiteSpace(accept)) return context;
        var first = accept.TrimStart();
        if (first.StartsWith("image/", StringComparison.OrdinalIgnoreCase)) return ResourceKind.Image;
        if (first.StartsWith("text/css", StringComparison.OrdinalIgnoreCase)) return ResourceKind.Stylesheet;
        return context;
    }

    /// The site the request is on behalf of: its Origin, or else its Referer
    /// (for a worker's own fetches, the page it serves, or the worker's
    /// script). Null when neither is a web address — then there is no page
    /// to call first-party, and no `$domain=` to go by.
    public static Uri? Page(string? origin, string? referer)
    {
        foreach (var said in (ReadOnlySpan<string?>)[origin, referer])
        {
            if (string.IsNullOrWhiteSpace(said) || said.Trim() == "null") continue;
            if (Uri.TryCreate(said.Trim(), UriKind.Absolute, out var url) && url.Scheme is "http" or "https" && url.Host.Length > 0)
                return url;
        }
        return null;
    }

    private static bool Is(string? header, string value) =>
        string.Equals(header?.Trim(), value, StringComparison.OrdinalIgnoreCase);
}
