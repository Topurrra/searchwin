namespace SearchKit.Shields;

/// Which turn of the downloaded lists is the current one. Switching them
/// off (the browser's ShieldLists.Forget) starts a new turn; a worker that
/// began in an older one — mid-download, mid-compile — has been overtaken,
/// and whatever it writes or hands over after that must be taken back out
/// or dropped, not left to bring back what was just deleted. Forget runs on
/// the UI thread whenever it likes, so a check made before a write can
/// always be stale by the time the write lands: the check that counts is
/// the one after it.
public sealed class ListGeneration
{
    private int current;
    private int loadedIn = -1;

    /// The turn work starting now belongs to.
    public int Current => Volatile.Read(ref current);

    /// Forget: a new turn, with nothing loaded in it.
    public void Advance() => Interlocked.Increment(ref current);

    /// Whether `turn` is still the current one.
    public bool Holds(int turn) => Current == turn;

    /// Whether a list is in force in the current turn.
    public bool Loaded => Volatile.Read(ref loadedIn) == Current;

    /// A list was loaded in `turn`. Marked in a turn that has already
    /// ended, it counts for nothing: `Loaded` stays false.
    public void MarkLoaded(int turn) => Volatile.Write(ref loadedIn, turn);

    /// Just after a write made in `turn`: if that turn is over, `undo`
    /// takes the write back out and the answer is true — stop here.
    public bool Overtaken(int turn, Action undo)
    {
        if (Holds(turn)) return false;
        undo();
        return true;
    }
}
