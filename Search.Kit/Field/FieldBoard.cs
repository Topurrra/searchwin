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

/// Which engine group, if any, a new query's top hit is kept for.
public static class FieldLayout
{
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

    /// A new question: new rows, nothing picked.
    public void Reset(int generation, List<FieldRow> laid)
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
                // A program or a script is never what Enter takes unasked:
                // it goes below, and the slot waits for a row that may lead.
                var fills = slot >= 0 && row.Action != RowAction.Reveal;
                if (!fills && (have >= cap || shown >= total))
                {
                    if (slot < 0) break;
                    continue;
                }
                if (!keys.Add(row.Key)) continue;
                if (fills)
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
