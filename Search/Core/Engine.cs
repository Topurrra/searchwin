using SearchKit.Engine;

namespace Search;

/// The engine beside the browser: KeepItLocal Workspace's core, headless
/// (Engine/). Nothing starts it at launch — the first thing that needs it
/// does, after the window is up — and it goes away on its own a little
/// while after the browser does. Each test world has its own engine, pipe
/// and data, like everything else a test run keeps.
public static class Engine
{
    public static string Pipe => "search-engine" + (Store.World is { } w ? "-" + w : "");

    public static string DataDir => Path.Combine(Store.Folder, "Engine");

    public static readonly EngineClient Client = new(Pipe, Start);

    private static readonly Lazy<string?> exe = new(Executable);

    /// Whether there's an engine to start at all. The field asks per
    /// keystroke, so it's looked for once: without one, every call would
    /// spend a quarter of a second finding out.
    public static bool Available => exe.Value != null;

    private static Task Start(CancellationToken cancel) =>
        exe.Value is { } path
            ? EngineLauncher.StartAsync(new EngineSetup(path, DataDir, Pipe), cancel)
            : throw new EngineException("The engine isn't installed beside Search.");

    /// Beside Search.exe in a build or an install. A test run straight out of
    /// the repo also finds the engine's own build, the newer of release and
    /// debug.
    private static string? Executable()
    {
        var beside = Path.Combine(AppContext.BaseDirectory, "kil-engine.exe");
        if (File.Exists(beside)) return beside;
        if (!Store.Testing) return null;
        for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir != null; dir = dir.Parent)
        {
            var built = new[] { "release", "debug" }
                .Select(kind => new FileInfo(Path.Combine(dir.FullName, "Engine", "target", kind, "kil-engine.exe")))
                .Where(file => file.Exists)
                .OrderByDescending(file => file.LastWriteTimeUtc)
                .FirstOrDefault();
            if (built != null) return built.FullName;
        }
        return null;
    }
}
