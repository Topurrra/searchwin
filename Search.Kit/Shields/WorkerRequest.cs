namespace SearchKit.Shields;

/// A service worker's (or a shared worker's) own request, as the browser
/// hears it: raised once, from whichever page holds the worker filter, with
/// no page of its own to say who it is for. What that request itself can
/// tell — apart from WebView2, so it can be tested without a browser.
public static class WorkerRequest
{
    /// A navigation the worker fetches for a page (a PWA's
    /// `fetch(event.request)`): the page itself, which Shields never refuses —
    /// a page you asked for is a page you get, served by its worker or not.
    /// `document` is WebView2's Document context; the Sec-Fetch headers say
    /// the same where the engine shows them.
    public static bool IsNavigation(bool document, string? fetchMode, string? fetchDest) =>
        document
        || string.Equals(fetchMode?.Trim(), "navigate", StringComparison.OrdinalIgnoreCase)
        || string.Equals(fetchDest?.Trim(), "document", StringComparison.OrdinalIgnoreCase);

    /// The site the request is on behalf of: its Origin, or else its Referer
    /// (for a worker's own fetches, the worker's script or the page it
    /// serves). Null when neither is a web address — then there is no page
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
}
