using System.Globalization;
using System.Text.Json.Nodes;

namespace SearchKit.Field;

/// Clipboard history (the engine's listener, on by default). `clip:` shows
/// it all, pinned first; plain words show a couple of matches among
/// everything else. A secret the engine's detector flagged is never in a row:
/// in `clip:` it shows as the kind of secret it is, and anywhere else not at
/// all. Enter puts the entry itself back (`copy_clipboard_entry_to_clipboard {id}`).
///
/// The history is fetched once (`get_clipboard_history`) and kept until the
/// engine says it changed (`clipboard-history-updated`), or for half a
/// minute at most (an engine that restarted has no way to say so), so typing
/// filters it here rather than asking the engine again per keystroke.
public sealed class ClipboardSource : IEngineSource
{
    public const string ChangedEvent = "clipboard-history-updated";

    /// One entry, read out of the engine's JSON once. The text is kept only
    /// for entries that aren't secrets.
    private sealed record Entry(long Id, bool Image, string Text, IReadOnlyList<string> Kinds, string Label,
        string From, bool Pinned, long Width, long Height)
    {
        public bool Sensitive => Kinds.Count > 0;
    }

    private readonly IEngineCalls engine;
    private readonly bool inField;
    private readonly object gate = new();
    private IReadOnlyList<Entry>? cache;
    private int version;
    private int cached = -1;
    private long fetchedAt;
    private readonly long maxAge;

    /// `inField`: whether plain words (no `clip:`) show clipboard matches too.
    public ClipboardSource(IEngineCalls engine, bool inField = true, TimeSpan? maxAge = null)
    {
        this.engine = engine;
        this.inField = inField;
        this.maxAge = (long)(maxAge ?? TimeSpan.FromSeconds(30)).TotalMilliseconds;
        engine.EventReceived += (name, _) =>
        {
            if (name == ChangedEvent) Interlocked.Increment(ref version);
        };
    }

    public Group Group => Group.Clipboard;

    public bool Wants(FieldQuery query) =>
        (query.Scope == Scope.Clipboard && query.Kind is QueryKind.Words or QueryKind.Empty)
        || (inField && query.Scope == Scope.All && query.Kind == QueryKind.Words && query.Text.Length >= 3);

    public TimeSpan Delay(FieldQuery query) => TimeSpan.Zero;

    public async Task<IReadOnlyList<FieldRow>> SuggestAsync(FieldQuery query, int limit, CancellationToken cancel)
    {
        var entries = await LoadAsync(cancel).ConfigureAwait(false);
        var scoped = query.Scope == Scope.Clipboard;
        var pinned = new List<FieldRow>();
        var rest = new List<FieldRow>();
        // Newest first, as the engine keeps them; in `clip:` the pinned ones lead.
        foreach (var entry in entries)
        {
            if (Row(entry, query.Text, scoped) is not { } row) continue;
            (scoped && entry.Pinned ? pinned : rest).Add(row);
            if (!scoped && rest.Count >= limit) break;
        }
        return [.. pinned.Concat(rest).Take(limit)];
    }

    private async Task<IReadOnlyList<Entry>> LoadAsync(CancellationToken cancel)
    {
        var now = Volatile.Read(ref version);
        lock (gate)
        {
            if (cache != null && cached == now && Environment.TickCount64 - fetchedAt < maxAge) return cache;
        }
        var answer = await engine.CallAsync("get_clipboard_history", null, cancel).ConfigureAwait(false);
        var list = Nodes.Items(answer).Select(Read).OfType<Entry>().ToList();
        lock (gate)
        {
            cache = list;
            cached = now;
            fetchedAt = Environment.TickCount64;
        }
        return list;
    }

    private static Entry? Read(JsonNode node)
    {
        var id = Nodes.Long(node, "id");
        var image = Nodes.Str(node, "kind") == "image";
        var kinds = Nodes.Strings(node, "sensitiveKinds");
        var text = image || kinds.Count > 0 ? "" : Nodes.Str(node, "text");
        if (!image && kinds.Count == 0 && text.Length == 0) return null;
        return new Entry(id, image, text, kinds, Nodes.Str(node, "pinLabel"), Nodes.Str(node, "sourceApp"),
            Nodes.Bool(node, "isPinned"), Nodes.Long(node, "imageWidth"), Nodes.Long(node, "imageHeight"));
    }

    private static FieldRow? Row(Entry entry, string needle, bool scoped)
    {
        if (!scoped && (entry.Sensitive || entry.Image)) return null;
        string title;
        bool matches;
        if (entry.Sensitive)
        {
            // Matched on its label and kind only, never on the secret itself.
            var kinds = entry.Kinds.Where(k => k != "sensitive").Select(k => k.Replace('_', ' ')).ToList();
            title = "Hidden: " + (kinds.Count > 0 ? string.Join(", ", kinds) : "secret");
            matches = Has(title, needle) || Has(entry.Label, needle);
        }
        else if (entry.Image)
        {
            title = entry.Width > 0 && entry.Height > 0 ? $"Image {entry.Width}×{entry.Height}" : "Image";
            matches = Has(title, needle) || Has(entry.Label, needle);
        }
        else
        {
            title = Nodes.Line(entry.Text, 90);
            matches = Has(entry.Text, needle) || Has(entry.Label, needle);
        }
        if (!matches) return null;
        var detail = entry.Label.Length > 0 ? entry.Label : entry.From.Length > 0 ? entry.From : "Clipboard";
        return new FieldRow(Group.Clipboard, RowKey.Clip(entry.Id), title, detail, RowAction.CopyClip,
            entry.Id.ToString(CultureInfo.InvariantCulture)) { Sensitive = entry.Sensitive };
    }

    private static bool Has(string text, string needle) =>
        needle.Length == 0 || text.Contains(needle, StringComparison.OrdinalIgnoreCase);
}
