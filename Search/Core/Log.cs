namespace Search;

/// A line in debug.log beside the app's files — only in a test run, never in
/// the browser somebody is using.
public static class Log
{
    private static readonly object gate = new();

    public static void Write(string line)
    {
        if (!Store.Testing) return;
        try
        {
            lock (gate) File.AppendAllText(Store.File("debug.log"), $"{DateTime.Now:HH:mm:ss.fff} {line}\n");
        }
        catch { }
    }
}
