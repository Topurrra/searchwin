namespace SearchKit.Web;

/// What a tool page (https://tools.search/) may ask of the engine through
/// the browser. Unavailable and browser-owned operations are refused here.
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

    // These Workspace commands are registered by the engine copy, but Search
    // does not provide their required native windows or background lifecycle.
    private static readonly HashSet<string> Unavailable = new(StringComparer.Ordinal)
    {
        "read_hardening_state",
        "apply_hardening_tweak",
        "revert_hardening_tweak",
        "revert_all_hardening",
        "export_hardening_backup",
        "screenrec_start",
        "screenrec_stop",
        "screenrec_status",
        "screenrec_pause",
        "screenrec_resume",
        "screenrec_export_gif",
        "screenrec_open_region_selector",
        "screenrec_close_region_selector",
        "screenrec_open_redact_selector",
        "screenrec_close_redact_selector",
        "screenrec_list_windows",
        "screenrec_open_toolbar",
        "screenrec_close_toolbar",
        "create_reminder_task",
        "delete_reminder_task",
        "reconcile_reminder_tasks",
    };

    /// Why a tool page's command is refused, or null when it may go on.
    public static string? Refused(string cmd) => BrowserOnly.Contains(cmd)
        ? "Search's Settings › Clipboard decides that"
        : Unavailable.Contains(cmd) ? "This Workspace operation is not available in Search" : null;
}
