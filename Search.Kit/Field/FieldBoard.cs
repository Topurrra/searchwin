namespace SearchKit.Field;

/// How many rows each group may show. A scope (`files:`) lifts its group to
/// `Scoped`; `Total` caps the whole list.
public sealed class FieldCaps
{
    internal static readonly int Groups = Enum.GetValues<Group>().Length;

    private readonly int[] caps = new int[Groups];

    public FieldCaps()
    {
        this[Group.TopHit] = 1;
        this[Group.Answer] = 3;
        this[Group.Commands] = 6;
        this[Group.Tabs] = 3;
        this[Group.History] = 3;
        this[Group.Bookmarks] = 2;
        this[Group.Files] = 4;
        this[Group.Apps] = 3;
        this[Group.Clipboard] = 2;
        this[Group.Search] = 1;
    }

    public int this[Group group]
    {
        get => caps[(int)group];
        set => caps[(int)group] = value;
    }

    public int Scoped { get; set; } = 12;

    public int Total { get; set; } = 16;

    public int For(Group group, FieldQuery query) => ScopeGroup(query.Scope) == group ? Scoped : this[group];

    public static Group? ScopeGroup(Scope scope) => scope switch
    {
        Scope.Files => Group.Files,
        Scope.Apps => Group.Apps,
        Scope.Clipboard => Group.Clipboard,
        Scope.Tabs => Group.Tabs,
        Scope.History => Group.History,
        _ => null,
    };
}

/// Laying the rows out for a new query: group order, one copy of each thing,
/// caps, and the top hit.
public static class FieldLayout
{
    /// The top hit, when the local sources can decide it:
    /// - `>…`: the best command; `!…`, `?…`: the bang or the question;
    /// - `tabs:` / `history:`: the first of those;
    /// - an address or words: an open tab whose address or title starts with
    ///   what was typed, else the history place that the field is finishing
    ///   (its key starts with the text), else the typed text's own row (go
    ///   there, or search the web for it).
    /// Answers and the engine scopes (`files:`, `apps:`, `clip:`) don't come
    /// from here: their top hit is reserved (see `Reserve`).
    public static FieldRow? TopHit(FieldQuery query, IReadOnlyList<FieldRow> rows)
    {
        FieldRow? First(Group group) => rows.FirstOrDefault(r => r.Group == group);
        switch (query.Kind)
        {
            case QueryKind.Command: return First(Group.Commands);
            case QueryKind.Bang or QueryKind.Ask: return First(Group.Search);
            case QueryKind.Empty: return query.Scope is Scope.Tabs ? First(Group.Tabs) : query.Scope is Scope.History ? First(Group.History) : null;
            case QueryKind.Words or QueryKind.Address:
                if (query.Scope == Scope.Tabs) return First(Group.Tabs);
                if (query.Scope == Scope.History) return First(Group.History);
                if (query.Scope != Scope.All) return null;
                var needle = Needles.Of(query.Text);
                if (needle.Length == 0) return null;
                var tab = rows.FirstOrDefault(r => r.Group == Group.Tabs && r.Score >= TabSource.Strong);
                if (tab != null) return tab;
                var place = rows.FirstOrDefault(r => r.Group == Group.History && r.Key.StartsWith("url:" + needle, StringComparison.Ordinal));
                return place ?? First(Group.Search);
            default:
                return null;
        }
    }

    /// Which engine group the top hit waits for, if any: an answer for an
    /// answer query, the scope's group for `files:`/`apps:`/`clip:`. Only
    /// when a source for that group is going to be asked.
    public static Group? Reserve(FieldQuery query, IEnumerable<Group> asked)
    {
        Group? wanted = query.IsAnswer && query.Scope == Scope.All ? Group.Answer
            : query.Scope is Scope.Files or Scope.Apps or Scope.Clipboard ? FieldCaps.ScopeGroup(query.Scope)
            : null;
        return wanted is { } group && asked.Contains(group) ? group : null;
    }

    /// The first layout for a query, from the local rows (in any order;
    /// within a group, the source's order is kept). The top hit leads, or a
    /// placeholder when it's `reserved`; then each group in order, capped,
    /// with anything already shown dropped.
    public static List<FieldRow> Build(FieldQuery query, IReadOnlyList<FieldRow> local, Group? reserved, FieldCaps caps)
    {
        var ordered = local.Select((row, index) => (row, index))
            .OrderBy(p => p.row.Group).ThenBy(p => p.index)
            .Select(p => p.row).ToList();
        var laid = new List<FieldRow>();
        var seen = new HashSet<string>(StringComparer.Ordinal);
        if (reserved is { } waiting) laid.Add(FieldRow.Placeholder(waiting));
        else if (TopHit(query, ordered) is { } top)
        {
            laid.Add(top with { Group = Group.TopHit });
            seen.Add(top.Key);
        }
        var counts = new int[FieldCaps.Groups];
        foreach (var row in ordered)
        {
            if (laid.Count >= caps.Total) break;
            if (row.Group == Group.TopHit || counts[(int)row.Group] >= caps.For(row.Group, query)) continue;
            if (!seen.Add(row.Key)) continue;
            counts[(int)row.Group]++;
            laid.Add(row);
        }
        return laid;
    }

    /// The rest of the top hit's address that the field draws after the
    /// caret ("git" → "hub.com"), when it carries on from what was typed.
    public static string? Ending(FieldQuery query, FieldRow? top)
    {
        if (top == null || top.Origin is not (Group.History or Group.Tabs)) return null;
        if (query.Kind is not (QueryKind.Words or QueryKind.Address) || query.Scope != Scope.All) return null;
        var needle = Needles.Of(query.Text);
        if (needle.Length < 2 || !top.Key.StartsWith("url:", StringComparison.Ordinal)) return null;
        var place = top.Key.AsSpan(4);
        if (!place.StartsWith(needle, StringComparison.Ordinal) || place.Length == needle.Length) return null;
        return place[needle.Length..].ToString();
    }
}

/// The rows under the field, and which one is picked.
///
/// The rule for rows that arrive late (files, apps, answers, clipboard, all
/// from the engine): **nothing the user is about to press moves.**
/// - A late row fills a reserved slot in place, or is inserted; rows already
///   shown never change their order, and none is ever taken away for a late
///   one (a full group or list just drops the late rows).
/// - Nothing is inserted at or above the lock line: the picked row, the row
///   under the pointer (`Hold`), and the top hit Enter would take. A late
///   row whose group belongs higher goes just below that line instead.
/// - The picked row is remembered by what it is, not where it is.
///
/// Every change comes with the generation it was made for; an answer to an
/// older question is dropped. Safe to call from any thread.
public sealed class FieldBoard
{
    private readonly object gate = new();
    private List<FieldRow> rows = [];
    private string? picked;
    private string? held;
    /// One of the browser's own rows is picked, or under the pointer (see `Beside`).
    private bool pickedBeside;
    private bool heldBeside;
    /// Groups whose sources have all answered this round.
    private readonly HashSet<Group> settled = [];

    /// Stands for one of the browser's own rows in `Pick` and `Hold`. They
    /// aren't on the board, but they're drawn below its top hit, so they
    /// move if a reserved top hit goes away.
    public const int Beside = -1;

    /// The pointer is over the list (any row of it). Unlike `Hold`, this
    /// outlasts a new question: the pointer is still where it was.
    public bool PointerOver
    {
        get { lock (gate) return pointerOver; }
        set { lock (gate) pointerOver = value; }
    }

    private bool pointerOver;

    public int Generation { get; private set; }

    /// The rest of the top hit's address, drawn after the caret.
    public string? Ending { get; private set; }

    /// A snapshot of the rows, top first. Pending rows are reserved slots.
    public IReadOnlyList<FieldRow> Rows
    {
        get { lock (gate) return [.. rows]; }
    }

    /// The picked row's index, if one is picked and still there.
    public int? Picked
    {
        get { lock (gate) return IndexOf(picked); }
    }

    /// The row Enter takes: the picked one, else the top hit. Null while the
    /// top hit is still reserved (`Waiting`) or when there's nothing to take.
    public FieldRow? EnterRow
    {
        get
        {
            lock (gate)
            {
                if (IndexOf(picked) is { } p) return rows[p];
                return rows.Count > 0 && rows[0].Group == Group.TopHit && !rows[0].IsPending ? rows[0] : null;
            }
        }
    }

    /// Enter's row is a reserved top hit that hasn't arrived yet.
    public bool Waiting
    {
        get { lock (gate) return IndexOf(picked) == null && rows.Count > 0 && rows[0].IsPending && !settled.Contains(rows[0].Origin); }
    }

    /// The fallback when a reserved top hit never comes: the first row of
    /// the typed text's own group (search the web), if there is one.
    public FieldRow? Fallback
    {
        get { lock (gate) return rows.FirstOrDefault(r => r.Origin == Group.Search && !r.IsPending); }
    }

    /// A new question: new rows, nothing picked.
    public void Reset(int generation, List<FieldRow> laid, string? ending)
    {
        lock (gate)
        {
            Generation = generation;
            rows = laid;
            picked = null;
            held = null;
            pickedBeside = false;
            heldBeside = false;
            settled.Clear();
            Ending = ending;
        }
    }

    /// Late rows from `origin`'s source, best first. Returns whether anything changed.
    public bool Arrive(int generation, Group origin, IReadOnlyList<FieldRow> late, int cap, int total)
    {
        lock (gate)
        {
            if (generation != Generation || late.Count == 0) return false;
            var keys = new HashSet<string>(rows.Select(r => r.Key), StringComparer.Ordinal);
            var have = rows.Count(r => r.Origin == origin && !r.IsPending);
            var shown = rows.Count(r => !r.IsPending);
            var slot = rows.FindIndex(r => r.IsPending && r.Origin == origin);
            var changed = false;
            var floor = LockLine() + 1;
            foreach (var row in late)
            {
                if (slot < 0 && (have >= cap || shown >= total)) break;
                if (!keys.Add(row.Key)) continue;
                if (slot >= 0)
                {
                    // The reserved top hit: filled where it stands.
                    rows[slot] = row with { Group = Group.TopHit, Origin = origin };
                    slot = -1;
                }
                else
                {
                    var at = Math.Max(Math.Max(Natural(origin), floor), 0);
                    rows.Insert(at, row with { Group = origin, Origin = origin });
                    floor = at + 1;
                    have++;
                }
                shown++;
                changed = true;
            }
            return changed;
        }
    }

    /// `origin`'s sources have all answered: a reserved slot nothing filled
    /// goes away. Returns whether anything changed.
    ///
    /// Not while anything below it is picked or under the pointer: every row
    /// would move up one under the click. Then it stays, empty, as a spacer,
    /// until the next question.
    public bool Settle(int generation, Group origin)
    {
        lock (gate)
        {
            if (generation != Generation) return false;
            settled.Add(origin);
            if (Steady()) return false;
            return rows.RemoveAll(r => r.IsPending && r.Origin == origin) > 0;
        }
    }

    /// Something the user is about to press is on the list.
    private bool Steady() =>
        pointerOver || heldBeside || pickedBeside || IndexOf(held) != null || IndexOf(picked) != null;

    /// The arrow keys: one row down or up, skipping reserved slots; off
    /// either end lets go.
    public void Walk(int step)
    {
        lock (gate)
        {
            var at = IndexOf(picked) ?? (step > 0 ? -1 : rows.Count);
            do at += step;
            while (at >= 0 && at < rows.Count && rows[at].IsPending);
            picked = at >= 0 && at < rows.Count ? rows[at].Key : null;
            pickedBeside = false;
        }
    }

    /// Picks a row by index (a click), `Beside` for one of the browser's own
    /// rows, or lets go with null.
    public void Pick(int? index)
    {
        lock (gate)
        {
            picked = index is { } i && i >= 0 && i < rows.Count && !rows[i].IsPending ? rows[i].Key : null;
            pickedBeside = index == Beside;
        }
    }

    /// The row under the pointer (`Beside` for one of the browser's own), so
    /// it can't move while it's about to be clicked; null when the pointer
    /// leaves it.
    public void Hold(int? index)
    {
        lock (gate)
        {
            held = index is { } i && i >= 0 && i < rows.Count ? rows[i].Key : null;
            heldBeside = index == Beside;
        }
    }

    /// The lowest row that must not move: the picked row, the held row, the
    /// top hit. -1 when nothing is fixed.
    private int LockLine()
    {
        var line = rows.Count > 0 && rows[0].Group == Group.TopHit ? 0 : -1;
        if (IndexOf(picked) is { } p) line = Math.Max(line, p);
        if (IndexOf(held) is { } h) line = Math.Max(line, h);
        return line;
    }

    /// Where a row of `group` belongs: after the last row of its own group or
    /// any group drawn above it.
    private int Natural(Group group)
    {
        var at = rows.Count;
        for (var i = rows.Count - 1; i >= 0; i--)
        {
            if (rows[i].Group <= group) return i + 1;
            at = i;
        }
        return at;
    }

    private int? IndexOf(string? key)
    {
        if (key == null) return null;
        var i = rows.FindIndex(r => r.Key == key);
        return i < 0 ? null : i;
    }
}
