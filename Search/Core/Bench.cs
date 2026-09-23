namespace Search;

/// A local socket a script can drive the browser through. PORT: Bench.swift.
public sealed class Bench
{
    public static readonly Bench Shared = new();
    public void Start(Browser browser) { }
    public void Stop() { }
}
