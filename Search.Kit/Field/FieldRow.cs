namespace SearchKit.Field;

/// The field's groups, in the order they're drawn. Only `TopHit` holds a row
/// from another group: the one Enter takes.
public enum Group
{
    TopHit,
    Answer,
    Commands,
    Tabs,
    History,
    Bookmarks,
    Files,
    Apps,
    Clipboard,
    Search,
}

/// What Enter (or a click) on a row does. `Target` says what to do it with.
public enum RowAction
{
    /// Go to the address in `Target` in the current tab.
    Go,
    /// Switch to the open tab `Tab` (its address is in `Target`).
    SwitchTab,
    /// Run the command whose id is `Target`, with `Argument`.
    RunCommand,
    /// Ask the AI the question in `Target`.
    Ask,
    /// Open the file at `Target` in a tab (PDF, image, text, Markdown).
    OpenInTab,
    /// Open the audio or video file at `Target` in a player tab.
    Play,
    /// Open the file or folder at `Target` in its own app (engine:
    /// `open_search_result_path {path}`).
    OpenWithApp,
    /// Show the file at `Target` in its folder, selected, and nothing more:
    /// a program or a script, which opening would run (see FileKinds.Runs).
    Reveal,
    /// Start the app at `Target` (engine: `launch_cached_target {path}`).
    Launch,
    /// Put the text in `Target` on the clipboard.
    Copy,
    /// Put clipboard history entry `Target` (its id) back on the clipboard
    /// (engine: `copy_clipboard_entry_to_clipboard {id}`), so images and
    /// secrets travel without ever being in a row.
    CopyClip,
    /// A reserved slot, still waiting for its row. Not pickable.
    Pending,
}

/// One row under the field. `Key` is what makes two rows the same thing
/// (see RowKey): the same page from tabs and history, the same file from its
/// name and its contents.
public sealed record FieldRow(Group Group, string Key, string Title, string Detail, RowAction Action, string Target)
{
    /// The group the row came from. Differs from `Group` only for the top hit.
    public Group Origin { get; init; } = Group;

    /// The open tab, for `SwitchTab`.
    public Guid? Tab { get; init; }

    /// Whatever followed a command's words (`>new space Work` → "Work").
    public string Argument { get; init; } = "";

    /// The source's own score; only used inside a source.
    public double Score { get; init; }

    /// A clipboard entry or a file that holds a secret. Its text is never in
    /// the row; the UI may want to draw it differently.
    public bool Sensitive { get; init; }

    public bool IsPending => Action == RowAction.Pending;

    /// A reserved top hit, filled in place when `origin`'s first row arrives.
    public static FieldRow Placeholder(Group origin) =>
        new(Group.TopHit, "pending:" + origin, "", "", RowAction.Pending, "") { Origin = origin };
}

/// What makes two rows the same thing.
public static class RowKey
{
    /// A page, as the browser's history keys it (Address.Pretty, lower-cased):
    /// the host without `www.`, then the path unless it's just `/`. The query
    /// and fragment don't count, the scheme doesn't either.
    public static string Url(Uri url)
    {
        if (!url.IsAbsoluteUri) return "url:" + url.OriginalString.ToLowerInvariant();
        if (string.IsNullOrEmpty(url.Host)) return "url:" + url.AbsoluteUri.ToLowerInvariant();
        var host = url.Host.StartsWith("www.", StringComparison.OrdinalIgnoreCase) ? url.Host[4..] : url.Host;
        var path = Uri.UnescapeDataString(url.AbsolutePath);
        return "url:" + (path.Length == 0 || path == "/" ? host : host + path).ToLowerInvariant();
    }

    /// A history place's own key (already the pretty, lower-cased form).
    public static string Place(string key) => "url:" + key;

    public static string File(string path) =>
        "file:" + path.Replace('/', '\\').TrimEnd('\\').ToLowerInvariant();

    public static string App(string path) => "app:" + path.ToLowerInvariant();

    public static string Clip(long id) => "clip:" + id.ToString(System.Globalization.CultureInfo.InvariantCulture);

    public static string Command(string id, string argument) => "cmd:" + id + ":" + argument;

    public static string Answer(string text) => "answer:" + text;

    public static string WebSearch(string text) => "search:" + text;
}

/// Which files open in a tab, which in the player, and which in their own app.
public static class FileKinds
{
    private static readonly HashSet<string> InTab = new(StringComparer.OrdinalIgnoreCase)
    {
        "pdf", "txt", "md", "markdown", "log", "json", "csv", "xml", "yaml", "yml", "toml", "ini", "html", "htm",
        "png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif", "ico",
    };

    /// What WebView2 plays itself.
    private static readonly HashSet<string> Playable = new(StringComparer.OrdinalIgnoreCase)
    {
        "mp4", "m4v", "webm", "ogv", "mov", "mp3", "m4a", "aac", "wav", "ogg", "oga", "opus", "flac",
    };

    /// What opening runs rather than shows: programs, scripts, installers,
    /// shortcuts (whose target may be any of those), registry files, plus
    /// whatever PATHEXT adds on this machine.
    private static readonly HashSet<string> Running = new(StringComparer.OrdinalIgnoreCase)
    {
        "exe", "com", "bat", "cmd", "pif", "scr", "cpl", "msc", "msi", "msp", "mst", "hta", "reg", "inf",
        "lnk", "url", "appref-ms", "application", "settingcontent-ms", "scf", "jar", "ps1", "psm1",
        "vbs", "vbe", "js", "jse", "wsf", "wsh", "gadget",
    };

    private static readonly Lazy<HashSet<string>> Everything = new(() =>
    {
        var all = new HashSet<string>(Running, StringComparer.OrdinalIgnoreCase);
        foreach (var ext in (Environment.GetEnvironmentVariable("PATHEXT") ?? "").Split(';', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
            all.Add(ext.TrimStart('.'));
        return all;
    });

    /// Whether opening a file with this extension (with or without its dot)
    /// would run it.
    public static bool Runs(string extension) => Everything.Value.Contains(Bare(extension));

    /// `extension` with or without its dot. A folder always opens in
    /// Explorer; a program or a script is only ever shown there.
    public static RowAction ActionFor(string extension, bool folder = false)
    {
        if (folder) return RowAction.OpenWithApp;
        var bare = Bare(extension);
        if (Runs(bare)) return RowAction.Reveal;
        if (Playable.Contains(bare)) return RowAction.Play;
        if (InTab.Contains(bare)) return RowAction.OpenInTab;
        return RowAction.OpenWithApp;
    }

    /// A file's action by its path: what Windows would open, which ignores
    /// trailing dots and spaces (`setup.exe.` runs setup.exe).
    public static RowAction ActionForPath(string path, bool folder = false)
    {
        if (folder) return RowAction.OpenWithApp;
        var name = path[(path.LastIndexOfAny(['\\', '/']) + 1)..].TrimEnd('.', ' ');
        var dot = name.LastIndexOf('.');
        return ActionFor(dot < 0 ? "" : name[(dot + 1)..]);
    }

    private static string Bare(string extension) => extension.Trim().TrimStart('.').TrimEnd('.', ' ');
}
