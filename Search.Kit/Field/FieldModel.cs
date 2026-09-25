using SearchKit.Commands;

namespace SearchKit.Field;

public sealed class FieldOptions
{
    /// Finds `!yt`. Without it a bang is plain text.
    public Bangs? Bangs { get; init; }

    /// The browser's own address test (`t => Address.Url(t) != null`).
    public Func<string, bool>? IsAddress { get; init; }

    public FieldCaps Caps { get; init; } = new();

    /// Runs an action on the UI thread (the browser passes its
    /// DispatcherQueue). Engine rows arrive through it, and `Changed` is
    /// raised from it. Without it, they arrive on whatever thread finished.
    public Action<Action>? Post { get; init; }
}

/// The model behind the field's suggestions.
///
/// `Type` is the keystroke: it reads the text, asks the local sources, lays
/// the rows out, and returns (all on the UI thread, well under 5 ms). Then
/// each engine source that wants the query is asked off the UI thread, after
/// its own pause, and its rows join the board as they come (see FieldBoard
/// for how they may and may not move things). The next keystroke cancels
/// every engine question still out, and anything that answers anyway is
/// dropped as stale.
public sealed class FieldModel : IDisposable
{
    private readonly IReadOnlyList<ILocalSource> local;
    private readonly IReadOnlyList<IEngineSource> engine;
    private readonly FieldOptions options;
    private Round round = new(0, [], null);

    public FieldModel(IEnumerable<ILocalSource> local, IEnumerable<IEngineSource> engine, FieldOptions? options = null)
    {
        this.local = [.. local];
        this.engine = [.. engine];
        this.options = options ?? new FieldOptions();
    }

    public FieldBoard Board { get; } = new();

    public FieldQuery Query { get; private set; } = FieldQuery.None;

    /// The rows changed: after `Type`, and whenever engine rows land.
    public event Action? Changed;

    /// Done when every engine source asked about the current text has
    /// answered, failed or been cancelled. For tests and the bench.
    public Task WhenSettled => round.Settled.Task;

    /// A keystroke: the field now holds `typed`.
    public void Type(string typed)
    {
        if (round.Generation > 0 && typed == Query.Typed) return;
        round.Cancel.Cancel();

        var query = FieldQuery.Read(typed, options.Bangs, options.IsAddress);
        var caps = options.Caps;
        var rows = new List<FieldRow>();
        foreach (var source in local)
        {
            // A source's trouble costs its rows, never the keystroke.
            try { rows.AddRange(source.Suggest(query, caps.For(source.Group, query) + 2)); }
            catch { }
        }
        var asked = new List<IEngineSource>();
        foreach (var source in engine)
        {
            try { if (source.Wants(query)) asked.Add(source); }
            catch { }
        }
        var reserved = FieldLayout.Reserve(query, asked.Select(s => s.Group));
        var laid = FieldLayout.Build(query, rows, reserved, caps);
        var top = laid.Count > 0 && laid[0].Group == Group.TopHit && !laid[0].IsPending ? laid[0] : null;

        var next = new Round(round.Generation + 1, asked, reserved);
        Query = query;
        round = next;
        Board.Reset(next.Generation, laid, FieldLayout.Ending(query, top));
        Changed?.Invoke();
        foreach (var source in asked) _ = AskAsync(next, source, query);
    }

    /// Enter. The picked row or the top hit; if the top hit is an answer
    /// still on its way, waits for it (up to `wait`), then falls back to the
    /// typed text's own row. Null means nothing to take: the browser does
    /// what it does with the bare text.
    public async Task<FieldRow?> EnterAsync(TimeSpan wait)
    {
        if (!Board.Waiting) return Board.EnterRow;
        var filled = round.Filled.Task;
        await Task.WhenAny(filled, Task.Delay(wait)).ConfigureAwait(true);
        return Board.EnterRow ?? Board.Fallback;
    }

    public void Dispose() => round.Cancel.Cancel();

    private async Task AskAsync(Round asking, IEngineSource source, FieldQuery query)
    {
        var cancel = asking.Cancel.Token;
        var caps = options.Caps;
        // One more than the group shows, for the reserved top hit.
        var limit = caps.For(source.Group, query) + (asking.Reserved == source.Group ? 1 : 0);
        IReadOnlyList<FieldRow> rows = [];
        try
        {
            var delay = source.Delay(query);
            if (delay > TimeSpan.Zero) await Task.Delay(delay, cancel).ConfigureAwait(false);
            // Off the UI thread even when there's no pause: a source's own
            // work (hashing, filtering the clipboard) never lands on a keystroke.
            rows = await Task.Run(() => source.SuggestAsync(query, limit, cancel), cancel).ConfigureAwait(false);
        }
        catch (OperationCanceledException) { }
        // The engine went away, or answered nonsense: no rows from it.
        catch { }

        if (cancel.IsCancellationRequested)
        {
            asking.Done(source.Group);
            return;
        }
        Post(() =>
        {
            var changed = Board.Arrive(asking.Generation, source.Group, rows, caps.For(source.Group, query), caps.Total);
            if (asking.Done(source.Group)) changed |= Board.Settle(asking.Generation, source.Group);
            if (changed) Changed?.Invoke();
        });
    }

    private void Post(Action action)
    {
        if (options.Post is { } post) post(action);
        else action();
    }

    /// One question's engine work: what's still out, per group.
    private sealed class Round
    {
        private readonly Dictionary<Group, int> waiting = [];
        private int left;

        public Round(int generation, IReadOnlyList<IEngineSource> asked, Group? reserved)
        {
            Generation = generation;
            Reserved = reserved;
            foreach (var source in asked) waiting[source.Group] = waiting.GetValueOrDefault(source.Group) + 1;
            left = asked.Count;
            if (left == 0) Settled.TrySetResult();
            if (reserved == null) Filled.TrySetResult();
        }

        public int Generation { get; }
        public Group? Reserved { get; }
        public CancellationTokenSource Cancel { get; } = new();
        public TaskCompletionSource Settled { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        /// Done when the reserved top hit's group has answered.
        public TaskCompletionSource Filled { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);

        /// One source finished; true when it was the last of its group.
        public bool Done(Group group)
        {
            lock (waiting)
            {
                var last = --waiting[group] == 0;
                if (last && group == Reserved) Filled.TrySetResult();
                if (--left == 0) Settled.TrySetResult();
                return last;
            }
        }
    }
}
