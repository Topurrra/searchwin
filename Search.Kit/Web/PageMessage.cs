namespace SearchKit.Web;

/// What a page posts to Search (chrome.webview.postMessage), before any
/// handler reads it. The page writes the message, so everything in it is
/// the page's word; the engine only adds which document sent it (the
/// message's Source), and that is the one thing to go by for "which page
/// was this".
public static class PageMessage
{
    /// The longest message read at all, in characters of JSON. Search's own
    /// page scripts send a few hundred; parsing is on the UI thread, and a
    /// page could otherwise post megabytes for every handler to wait on.
    public const int MaxLength = 256 * 1024;

    /// Whether a message's JSON is short enough to be read.
    public static bool Fits(string? json) => json is { Length: > 0 and <= MaxLength };

    /// Whether the document that sent a message (`source`, the engine's
    /// word) is on the same host as the page the tab shows now. A message
    /// handled after the tab went somewhere else is about the page it left.
    public static bool SameHost(Uri? source, Uri? showing) =>
        source != null && showing != null && source.IsAbsoluteUri && showing.IsAbsoluteUri
        && string.Equals(source.Host, showing.Host, StringComparison.OrdinalIgnoreCase);

    /// Whether the sender is the very document on screen: the same address,
    /// apart from its fragment (#…), which a page changes without becoming
    /// another one.
    public static bool SameDocument(Uri? source, Uri? showing) =>
        source != null && showing != null && source.IsAbsoluteUri && showing.IsAbsoluteUri
        && Uri.Compare(source, showing, UriComponents.HttpRequestUrl, UriFormat.UriEscaped, StringComparison.Ordinal) == 0;
}
