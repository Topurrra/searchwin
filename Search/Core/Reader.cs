namespace Search;

/// Reading mode. PORT: Sources/Search/Reader.swift.
public static class Reader
{
    public static void Toggle(Tab tab, Action<bool> done) => done(false);
}
