namespace Search;

// What was open last time. A list of addresses and their names, and which one
// you were looking at — nothing else, because everything else is either on the
// page or in the history file next door.
public static class Session
{
    public sealed class Entry
    {
        public string Url { get; set; } = "";
        public string Title { get; set; } = "";
        public string? Pin { get; set; }
        /// The name you gave the tab, when you gave it one.
        public string? Name { get; set; }
    }

    public sealed class Shape
    {
        public List<Entry> Tabs { get; set; } = [];
        public int Active { get; set; }
    }

    /// The first space's is the session there always was; each other space
    /// keeps its own beside it.
    private static string FileName(Guid space) =>
        space == Space.FirstID ? "session.json" : $"session-{space.ToString().ToUpperInvariant()}.json";

    public static void Erase(Guid space)
    {
        if (space == Space.FirstID) return;
        try { File.Delete(Store.File(FileName(space))); } catch { }
    }

    public static Shape Read(Guid? space = null) =>
        Store.Read<Shape>(FileName(space ?? Space.FirstID)) ?? new Shape();

    /// `now` writes on the calling thread. Quitting doesn't wait for a
    /// background task, and a session handed to one on the way out is a
    /// session that may never reach the disk.
    public static void Write(Shape shape, Guid? space = null, bool now = false) =>
        Store.Write(FileName(space ?? Space.FirstID), shape, now);
}
