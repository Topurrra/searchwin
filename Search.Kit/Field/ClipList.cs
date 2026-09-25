using System.Text.Json.Nodes;

namespace SearchKit.Field;

/// One entry of the engine's clipboard history (`get_clipboard_history`),
/// read once. `Text` is the whole text, secrets included: it is what a paste
/// puts in. What a list shows is `Face`, which for a secret is only the kind
/// of secret it is, until someone asks to see it.
public sealed record ClipEntry(long Id, bool Image, string Text, IReadOnlyList<string> Kinds, string Label,
    string From, bool Pinned, long CapturedMs, long Width, long Height, string Thumbnail)
{
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
        return Text.Contains(needle, StringComparison.OrdinalIgnoreCase);
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
}
