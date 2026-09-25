using System.Globalization;

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

    private readonly IEngineCalls engine;
    private readonly object gate = new();
    private IReadOnlyList<ClipEntry>? cache;
    private int version;
    private int cached = -1;
    private long fetchedAt;
    private readonly long maxAge;

    public ClipboardSource(IEngineCalls engine, TimeSpan? maxAge = null)
    {
        this.engine = engine;
        this.maxAge = (long)(maxAge ?? TimeSpan.FromSeconds(30)).TotalMilliseconds;
        engine.EventReceived += (name, _) =>
        {
            if (name == ChangedEvent) Interlocked.Increment(ref version);
        };
    }

    public Group Group => Group.Clipboard;

    public bool Wants(FieldQuery query) =>
        (query.Scope == Scope.Clipboard && query.Kind is QueryKind.Words or QueryKind.Empty)
        || (query.Scope == Scope.All && query.Kind == QueryKind.Words && query.Text.Length >= 3);

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

    private async Task<IReadOnlyList<ClipEntry>> LoadAsync(CancellationToken cancel)
    {
        var now = Volatile.Read(ref version);
        lock (gate)
        {
            if (cache != null && cached == now && Environment.TickCount64 - fetchedAt < maxAge) return cache;
        }
        var answer = await engine.CallAsync("get_clipboard_history", null, cancel).ConfigureAwait(false);
        // A secret's text is dropped as it's read: a row never needs it.
        var list = ClipList.Read(answer, secrets: false);
        lock (gate)
        {
            cache = list;
            cached = now;
            fetchedAt = Environment.TickCount64;
        }
        return list;
    }

    private static FieldRow? Row(ClipEntry entry, string needle, bool scoped)
    {
        if (!scoped && (entry.Sensitive || entry.Image)) return null;
        // A secret is matched on its label and kind only, never on itself.
        if (!entry.Matches(needle)) return null;
        var title = entry.Sensitive || entry.Image ? entry.Face : Nodes.Line(entry.Text, 90);
        var detail = entry.Label.Length > 0 ? entry.Label : entry.From.Length > 0 ? entry.From : "Clipboard";
        return new FieldRow(Group.Clipboard, RowKey.Clip(entry.Id), title, detail, RowAction.CopyClip,
            entry.Id.ToString(CultureInfo.InvariantCulture)) { Sensitive = entry.Sensitive };
    }
}
