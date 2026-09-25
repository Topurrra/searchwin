using System.Text.Json.Nodes;

namespace SearchKit.Field;

/// One entry of the engine's clipboard history (`get_clipboard_history`),
/// read once. `Text` is the whole text, secrets included: it is what a paste
/// puts in. What a list shows is `Face`, which for a secret is only the kind
/// of secret it is, until someone asks to see it.
public sealed record ClipEntry(long Id, bool Image, string Text, IReadOnlyList<string> Kinds, string Label,
    string From, bool Pinned, long CapturedMs, long Width, long Height, string Thumbnail)
{
    /// How much of an entry typing searches: its start. An entry can be a
    /// quarter of a megabyte; two hundred of those, searched whole on every
    /// keystroke, would take the list well past its budget.
    public const int Searched = 4096;

    /// The part of the text a search looks in, cut once.
    private readonly string searched = Text.Length > Searched ? Text[..Searched] : Text;

    /// Flagged by the engine's detector (a key, a token, a card number…).
    /// The engine keeps these for minutes, not days.
    public bool Sensitive => Kinds.Count > 0;

    /// What a row says: the text on one line, a picture's size, or — for a
    /// secret — what kind of secret, never the secret.
    public string Face => Sensitive ? "Hidden: " + Secret(Kinds) : Image ? Picture(Width, Height) : Nodes.Line(Text, 120);

    /// The text shown once a secret has been asked for by a click.
    public string Shown => Image ? Picture(Width, Height) : Nodes.Line(Text, 120);

    /// Whether `needle` finds it. A secret is found by its label and kind
    /// only, never by what it says.
    public bool Matches(string needle)
    {
        if (needle.Length == 0) return true;
        if (Label.Contains(needle, StringComparison.OrdinalIgnoreCase)) return true;
        if (Sensitive || Image) return Face.Contains(needle, StringComparison.OrdinalIgnoreCase);
        return searched.Contains(needle, StringComparison.OrdinalIgnoreCase);
    }

    /// "github token, sensitive" → "github token"; nothing named → "secret".
    public static string Secret(IReadOnlyList<string> kinds)
    {
        var named = kinds.Where(k => k != "sensitive").Select(k => k.Replace('_', ' ')).ToList();
        return named.Count > 0 ? string.Join(", ", named) : "secret";
    }

    private static string Picture(long width, long height) => width > 0 && height > 0 ? $"Image {width}×{height}" : "Image";

    /// Null for what isn't an entry at all (no text and not a picture).
    /// `secrets: false` drops a secret's text as it's read, for callers that
    /// never paste (the field's rows).
    public static ClipEntry? Read(JsonNode? node, bool secrets = true)
    {
        var image = Nodes.Str(node, "kind") == "image";
        var kinds = Nodes.Strings(node, "sensitiveKinds");
        var text = image || (kinds.Count > 0 && !secrets) ? "" : Nodes.Str(node, "text");
        if (!image && kinds.Count == 0 && text.Length == 0) return null;
        return new ClipEntry(Nodes.Long(node, "id"), image, text, kinds, Nodes.Str(node, "pinLabel"),
            Nodes.Str(node, "sourceApp"), Nodes.Bool(node, "isPinned"), Nodes.Long(node, "capturedAtMs"),
            Nodes.Long(node, "imageWidth"), Nodes.Long(node, "imageHeight"),
            Nodes.Str(node, "thumbnailPath") is { Length: > 0 } thumb ? thumb : Nodes.Str(node, "imagePath"));
    }
}

/// The clipboard history as a list to pick from (Ctrl+Shift+V): pinned
/// entries first, then the rest newest first, narrowed by what's typed.
public static class ClipList
{
    public static List<ClipEntry> Read(JsonNode? history, bool secrets = true) =>
        [.. Nodes.Items(history).Select(node => ClipEntry.Read(node, secrets)).OfType<ClipEntry>()];

    public static List<ClipEntry> Filter(IEnumerable<ClipEntry> entries, string needle, int limit = int.MaxValue)
    {
        needle = needle.Trim();
        return [.. entries
            .Where(e => e.Matches(needle))
            .OrderByDescending(e => e.Pinned)
            .ThenByDescending(e => e.CapturedMs)
            .Take(limit)];
    }

    /// Only what was copied at or after `sinceMs` (ms since 1970): what a
    /// test run may report, never what was on the clipboard before it began.
    public static List<ClipEntry> Since(IEnumerable<ClipEntry> entries, long sinceMs) =>
        [.. entries.Where(e => e.CapturedMs >= sinceMs)];
}

/// What may be done with an entry, decided here so it can be tested without
/// a window: where Shift+Enter may take it, how it goes back on the
/// clipboard, and whether a paste still lands where the list was opened.
public static class ClipGuard
{
    /// Shift+Enter takes an entry as the field would — a place, or a search.
    /// A secret never goes anywhere: searched for, the search engine,
    /// History and the address field would all see it; opened, an address
    /// that is itself the secret (a webhook, a link with a token in it)
    /// would be sent to its site and kept in History. Enter pastes it.
    public static bool MayGo(ClipEntry entry)
    {
        if (entry.Image || entry.Sensitive) return false;
        return entry.Text.Trim().Length > 0;
    }

    /// A secret put back on the clipboard is marked so no clipboard history
    /// keeps it — Windows', the cloud's, or this one.
    public static bool Quiet(ClipEntry entry) => entry.Sensitive;

    /// The page a paste was aimed at, as it was when the list opened:
    /// its origin, and which document it had (a count of navigations).
    public readonly record struct Page(string Origin, long Document);

    public static string Origin(Uri? url) =>
        url is { IsAbsoluteUri: true } ? url.GetLeftPart(UriPartial.Authority).ToLowerInvariant() : "";

    public enum Paste
    {
        /// Typed into the page, where the caret is.
        Insert,
        /// The page is another one now: copied instead, and said so.
        Moved,
        /// A secret, with the caret in a frame inside the page (another
        /// site's, maybe): copied instead, and said so.
        Framed,
    }

    /// Whether a paste still goes into the page: the same site, the same
    /// document, and — for a secret — the caret in the page itself, not in a
    /// frame inside it.
    public static Paste IntoPage(Page opened, Page now, bool sensitive, bool caretInMainFrame)
    {
        if (opened.Origin.Length == 0 || opened.Origin != now.Origin || opened.Document != now.Document) return Paste.Moved;
        if (sensitive && !caretInMainFrame) return Paste.Framed;
        return Paste.Insert;
    }

    /// Clipboard history is kept unless turned off — except in a test world,
    /// which keeps nothing of the real clipboard unless asked to
    /// (`"clip.history": true` in its settings).
    public static bool OnByDefault(bool testing) => !testing;

    /// How long after a key the list holds still for what arrives.
    public const int StillMs = 400;

    /// Whether the list may be redrawn for entries that arrived: not while
    /// the pointer rests on it, nor just after a key — the row under the
    /// pointer, or the one about to be taken by Enter, stays where it is.
    public static bool MayRedraw(bool pointerOver, long msSinceKey) => !pointerOver && msSinceKey >= StillMs;

    /// A new engine joined: what Search believes about the pause now, and
    /// what — if anything — to tell the engine. A pause the user chose is
    /// put back on an engine that started afresh; one the engine already
    /// has is kept, never lifted on reconnect.
    public static (bool Paused, bool? Tell) Rejoin(bool remembered, bool engine) =>
        (remembered || engine, remembered && !engine ? true : null);
}
