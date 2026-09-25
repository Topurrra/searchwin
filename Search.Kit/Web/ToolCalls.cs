namespace SearchKit.Web;

/// What a tool page (https://tools.search/) may ask of the engine through
/// the browser. Nearly everything goes through by the name the page used;
/// these don't.
public static class ToolCalls
{
    /// Whether clipboard history listens, and its pause, are Settings ›
    /// Clipboard's to decide: a page that could lift a pause, or start a
    /// listener the user turned off, would leave the browser believing
    /// something that isn't so.
    private static readonly HashSet<string> BrowserOnly = new(StringComparer.Ordinal)
    {
        "start_clipboard_listener",
        "set_clipboard_paused",
    };

    /// Why a tool page's command is refused, or null when it may go on.
    public static string? Refused(string cmd) =>
        BrowserOnly.Contains(cmd) ? "Search's Settings › Clipboard decides that" : null;
}
