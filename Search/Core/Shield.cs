namespace Search;

/// The ad blocker. PORT: Sources/Search/Shield.swift.
public sealed class Shield
{
    public static readonly Shield Shared = new();
    public bool Enabled { get; set; } = true;
}
