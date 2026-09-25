namespace SearchKit.Field;

/// One line of the list under the field: one of the browser's own rows
/// (`Local`), or one of the board's (`Board`). The other is -1.
public readonly record struct FieldSlot(int Local, int Board)
{
    public bool IsLocal => Local >= 0;

    public static FieldSlot Mine(int index) => new(index, -1);

    public static FieldSlot Engine(int index) => new(-1, index);
}

/// The list under the field when the browser's own rows meet the engine's.
///
/// The browser's rows (places, the search row, commands, bangs, open pages)
/// are decided as they always were, at once. The engine's (answers, files,
/// apps, clipboard) come later, on a FieldBoard. They mix like this:
/// - the board's top hit leads: an answer, or the first file of `files:`,
///   reserved as a pending slot before anything has arrived, so it fills in
///   place;
/// - then the browser's rows, untouched;
/// - then every other engine row, in the board's order.
///
/// A late row can only fill a reserved slot or land below the browser's
/// rows, so it never moves one of them. Among its own rows the board keeps
/// what's picked or under the pointer where it is.
public static class FieldMix
{
    public static List<FieldSlot> Compose(int local, IReadOnlyList<FieldRow> board)
    {
        var slots = new List<FieldSlot>(local + board.Count);
        var first = board.Count > 0 && board[0].Group == Group.TopHit ? 1 : 0;
        if (first == 1) slots.Add(FieldSlot.Engine(0));
        for (var i = 0; i < local; i++) slots.Add(FieldSlot.Mine(i));
        for (var i = first; i < board.Count; i++) slots.Add(FieldSlot.Engine(i));
        return slots;
    }

    /// The arrow keys over the mixed list: one line down or up, over reserved
    /// slots; off either end lets go.
    public static int? Step(IReadOnlyList<FieldSlot> slots, IReadOnlyList<FieldRow> board, int? at, int step)
    {
        if (slots.Count == 0) return null;
        var here = at ?? (step > 0 ? -1 : slots.Count);
        do here += step;
        while (here >= 0 && here < slots.Count && Pending(slots[here], board));
        return here >= 0 && here < slots.Count ? here : null;
    }

    /// Where a line went after the list was mixed again: the same local row
    /// (by index — the browser's rows don't change between keystrokes), or the
    /// same engine row by its key.
    public static int? Find(IReadOnlyList<FieldSlot> slots, IReadOnlyList<FieldRow> board, FieldSlot? was, string? key)
    {
        if (was is not { } slot) return null;
        for (var i = 0; i < slots.Count; i++)
        {
            if (slot.IsLocal ? slots[i].Local == slot.Local
                : !slots[i].IsLocal && key != null && board[slots[i].Board].Key == key)
                return i;
        }
        return null;
    }

    public static bool Pending(FieldSlot slot, IReadOnlyList<FieldRow> board) =>
        !slot.IsLocal && slot.Board < board.Count && board[slot.Board].IsPending;
}
