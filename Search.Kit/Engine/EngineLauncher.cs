using System.Diagnostics;

namespace SearchKit.Engine;

/// Where the engine is, where it keeps its data, and which pipe it answers on.
public sealed record EngineSetup(string Executable, string DataDir, string Pipe, int LingerSeconds = 15);

public static class EngineLauncher
{
    /// Starts the engine and returns straight away; the client connects when
    /// the pipe appears. If one is already running for this pipe, the new one
    /// exits at once (code 2) and the client simply finds the old one.
    public static Task StartAsync(EngineSetup setup, CancellationToken cancel = default)
    {
        if (!File.Exists(setup.Executable))
            throw new EngineException($"The engine isn't installed ({setup.Executable}).");
        Directory.CreateDirectory(setup.DataDir);
        var info = new ProcessStartInfo(setup.Executable)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            WorkingDirectory = Path.GetDirectoryName(setup.Executable) ?? "",
        };
        info.ArgumentList.Add("--data-dir");
        info.ArgumentList.Add(setup.DataDir);
        info.ArgumentList.Add("--pipe");
        info.ArgumentList.Add(setup.Pipe);
        info.ArgumentList.Add("--linger");
        info.ArgumentList.Add(setup.LingerSeconds.ToString(System.Globalization.CultureInfo.InvariantCulture));
        using var started = Process.Start(info);
        // Background work, beside a browser that is in front: when the two
        // want the processor at once (an index being built while a page
        // loads), the page goes first. Its workers inherit this.
        try
        {
            if (started != null && OperatingSystem.IsWindows()) started.PriorityClass = ProcessPriorityClass.BelowNormal;
        }
        catch (Exception error) when (error is InvalidOperationException or System.ComponentModel.Win32Exception) { }
        return Task.CompletedTask;
    }
}
