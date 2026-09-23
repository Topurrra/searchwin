namespace Search;

/// The ad blocker. PORT: Sources/Search/Shield.swift.
public sealed class Shield : Model
{
    public static readonly Shield Shared = new();
    public bool Enabled { get; set; } = true;

    private string? trouble;
    /// Set when the blocker couldn't be put together, in words for Settings.
    public string? Trouble { get => trouble; private set => Set(ref trouble, value); }

    /// Put together again, after trouble.
    public void Compile() { }

    /// Whether blocking is off on this one site.
    public bool IsPaused(string host) => false;

    /// Blocking off (or back on) for one site that breaks with it.
    public void Pause(string host, bool paused) { }
}
