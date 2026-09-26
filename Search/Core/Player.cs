using System.Diagnostics;
using System.Text;
using System.Text.Json.Nodes;
using SearchKit.Media;

namespace Search;

/// The player's side of the FFmpeg pack (Tools/src/routes/play): a file
/// WebView2 can't play — MKV with AC-3, AVI, WMV… — is remuxed into an MP4
/// it can (SearchKit.Media.Remux: streams copied, audio made AAC only when
/// needed, video converted only when it must be), with its text subtitles
/// and those beside it as WebVTT. The result is cached under
/// Store.Folder\Cache\Play, one folder per file and way of making it,
/// kept to a few GB and a month, and served to the page by files.search
/// like any file, Range requests and all.
public static class Player
{
    private static string Cache => Path.Combine(Store.Folder, "Cache", "Play");

    private const long Cap = 4L << 30;
    private static readonly TimeSpan MaxAge = TimeSpan.FromDays(30);
    private const string Ready = "ready.json";

    private sealed record Job(Task<JsonObject> Work, CancellationTokenSource Cancel, string Path);

    private static readonly Dictionary<string, Job> jobs = [];
    private static readonly Lock gate = new();

    /// `host:play.prepare {path, how}` — how: `remux` (copy what plays),
    /// `transcode` (the video too, after a copy didn't play), `check` (a
    /// file in a container WebView2 reads: whether it decodes every stream
    /// — an MP4 with AC-3 plays silent otherwise — and the subtitle files
    /// beside it). Answers `{media, subtitles: [{path, label, language}],
    /// duration}`, `{playable, subtitles}` for a check, or
    /// `{needsPack: true}`.
    public static async Task<JsonNode?> Prepare(JsonObject args)
    {
        var path = args["path"]?.GetValue<string>() ?? "";
        var how = args["how"]?.GetValue<string>() switch
        {
            "transcode" => "transcode",
            "check" => "check",
            _ => "remux",
        };
        var file = new FileInfo(path);
        if (!await Task.Run(() => file.Exists)) throw new FileNotFoundException("That file isn't there any more.");
        _ = MediaInput.Open(file.FullName);
        var key = Remux.Key(file.FullName, file.Length, file.LastWriteTimeUtc, how);
        var done = Path.Combine(Cache, key);
        // Made before, pack or no pack.
        if (await Task.Run(() => Read(done)) is { } made) return made;
        if (Packs.FfmpegBin is not { } bin)
            return how == "check"
                ? new JsonObject { ["playable"] = true, ["subtitles"] = new JsonArray() }
                : new JsonObject { ["needsPack"] = true };

        Job job;
        lock (gate)
        {
            if (!jobs.TryGetValue(key, out job!))
            {
                var cancel = new CancellationTokenSource();
                job = new Job(Task.Run(() => Make(bin, file.FullName, how, done, cancel.Token)), cancel, file.FullName);
                jobs[key] = job;
                _ = job.Work.ContinueWith(_ =>
                {
                    lock (gate) jobs.Remove(key);
                    cancel.Dispose();
                }, TaskScheduler.Default);
            }
        }
        return (await job.Work).DeepClone();
    }

    /// `host:play.cancel {path}`: the page moved on; stop making it.
    public static void Cancel(string path)
    {
        lock (gate)
            foreach (var job in jobs.Values.Where(j => string.Equals(j.Path, path, StringComparison.OrdinalIgnoreCase)))
                job.Cancel.Cancel();
    }

    /// The pack is going: nothing may still be running from it.
    public static void Forget()
    {
        lock (gate)
            foreach (var job in jobs.Values) job.Cancel.Cancel();
    }

    private static JsonObject? Read(string folder)
    {
        var ready = Path.Combine(folder, Ready);
        try
        {
            if (!File.Exists(ready) || JsonNode.Parse(File.ReadAllText(ready)) is not JsonObject answer) return null;
            File.SetLastWriteTimeUtc(ready, DateTime.UtcNow);
            // Where it is now, not where it was made.
            if (answer["media"] is JsonValue) answer["media"] = Path.Combine(folder, Remux.Media);
            foreach (var sub in answer["subtitles"] as JsonArray ?? [])
                if (sub is JsonObject s) s["path"] = Path.Combine(folder, s["file"]?.GetValue<string>() ?? "");
            return answer;
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or System.Text.Json.JsonException)
        {
            return null;
        }
    }

    private static async Task<JsonObject> Make(string bin, string input, string how, string done, CancellationToken cancel)
    {
        var work = Path.Combine(Cache, $".{Path.GetFileName(done)}-{Guid.NewGuid().ToString("N")[..8]}");
        Directory.CreateDirectory(work);
        try
        {
            var started = Stopwatch.GetTimestamp();
            var answer = new JsonObject();
            var subtitles = new JsonArray();
            var probeArgs = new List<string> { "-v", "error", "-print_format", "json", "-show_format", "-show_streams" };
            probeArgs.AddRange(MediaInput.Open(input));
            var probe = JsonNode.Parse(await Run(Path.Combine(bin, "ffprobe.exe"),
                probeArgs, null, cancel));
            if (how == "check") answer["playable"] = Remux.Plays(probe);
            else
            {
                var plan = Remux.Plan(probe, input, work, transcode: how == "transcode")
                    ?? throw new InvalidOperationException("There's nothing in this file to play.");
                await Run(Path.Combine(bin, "ffmpeg.exe"), plan.Arguments, seconds => Say(input, plan.Duration, seconds), cancel);
                answer["media"] = Remux.Media;
                answer["duration"] = plan.Duration;
                answer["video"] = plan.HasVideo;
                for (var n = 0; n < plan.Subtitles.Count; n++)
                    subtitles.Add(Track(plan.Subtitles[n], Remux.SubtitleFile(n)));
            }
            // The subtitle files beside it, each made WebVTT on its own: one
            // that won't convert is left out, not the film.
            var folder = Path.GetDirectoryName(input)!;
            var beside = Remux.Beside(input, Directory.EnumerateFiles(folder).Select(Path.GetFileName)!);
            foreach (var sub in beside)
            {
                var name = Remux.SubtitleFile(subtitles.Count);
                try
                {
                    var besidePath = Path.Combine(folder, sub.Beside!);
                    var subtitleArgs = new List<string> { "-hide_banner", "-nostdin", "-loglevel", "error", "-y" };
                    subtitleArgs.AddRange(MediaInput.Sidecar(besidePath));
                    subtitleArgs.AddRange(["-f", "webvtt", Path.Combine(work, name)]);
                    await Run(Path.Combine(bin, "ffmpeg.exe"),
                        subtitleArgs,
                        null, cancel);
                    subtitles.Add(Track(sub, name));
                }
                catch (InvalidOperationException error) { Log.Write($"player: {sub.Beside}: {error.Message}"); }
            }
            answer["subtitles"] = subtitles;
            File.WriteAllText(Path.Combine(work, Ready), answer.ToJsonString());
            if (Directory.Exists(done)) Directory.Delete(done, recursive: true);
            Directory.Move(work, done);
            Log.Write($"player: {how} {Path.GetFileName(input)} in {Stopwatch.GetElapsedTime(started).TotalSeconds:F1} s");
            Evict(done);
            return Read(done) ?? throw new IOException("The prepared file went missing.");
        }
        finally
        {
            if (Directory.Exists(work))
                try { Directory.Delete(work, recursive: true); } catch (IOException) { }
        }
    }

    // A JsonNode, so JsonArray.Add takes it as a node (its generic Add isn't AOT-safe).
    private static JsonNode Track(Subtitle sub, string file) =>
        new JsonObject { ["file"] = file, ["label"] = sub.Label, ["language"] = sub.Language };

    private static long lastSaid;

    /// How far along, to the player page, a few times a second.
    private static void Say(string input, double duration, double seconds)
    {
        var now = Environment.TickCount64;
        if (now - Interlocked.Read(ref lastSaid) < 250) return;
        Interlocked.Exchange(ref lastSaid, now);
        var fraction = duration > 0 ? Math.Clamp(seconds / duration, 0, 1) : (double?)null;
        UI.Do(() => ToolsHost.Tell("play-progress", new JsonObject { ["path"] = input, ["fraction"] = fraction, ["seconds"] = seconds }));
    }

    /// Runs one of the pack's programs: its output (or, with `progress`,
    /// the seconds it reports), or its last words when it fails.
    private static async Task<string> Run(string program, IReadOnlyList<string> arguments, Action<double>? progress, CancellationToken cancel)
    {
        var info = new ProcessStartInfo(program)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            StandardOutputEncoding = Encoding.UTF8,
            StandardErrorEncoding = Encoding.UTF8,
        };
        foreach (var argument in arguments) info.ArgumentList.Add(argument);
        using var process = Process.Start(info) ?? throw new InvalidOperationException("FFmpeg didn't start.");
        try { process.PriorityClass = ProcessPriorityClass.BelowNormal; } catch (InvalidOperationException) { }
        using var stop = cancel.Register(() =>
        {
            try { process.Kill(entireProcessTree: true); } catch (InvalidOperationException) { }
        });
        var errors = process.StandardError.ReadToEndAsync(CancellationToken.None);
        var output = new StringBuilder();
        while (await process.StandardOutput.ReadLineAsync(CancellationToken.None) is { } line)
        {
            if (progress != null && Remux.Progress(line) is { } seconds) progress(seconds);
            else if (progress == null) output.AppendLine(line);
        }
        await process.WaitForExitAsync(CancellationToken.None);
        cancel.ThrowIfCancellationRequested();
        if (process.ExitCode != 0)
        {
            var said = (await errors).Trim().Split('\n').LastOrDefault()?.Trim();
            throw new InvalidOperationException(string.IsNullOrEmpty(said) ? $"FFmpeg stopped ({process.ExitCode})." : said);
        }
        return output.ToString();
    }

    /// Keeps the cache to its size and age, the folder just made aside;
    /// work folders nothing is making any more go too.
    private static void Evict(string keep)
    {
        try
        {
            var cached = new List<(string, long, DateTime)>();
            foreach (var dir in new DirectoryInfo(Cache).EnumerateDirectories())
            {
                if (dir.Name.StartsWith('.'))
                {
                    if (DateTime.UtcNow - dir.CreationTimeUtc > TimeSpan.FromDays(1)) dir.Delete(recursive: true);
                    continue;
                }
                if (string.Equals(dir.FullName, keep, StringComparison.OrdinalIgnoreCase)) continue;
                var ready = new FileInfo(Path.Combine(dir.FullName, Ready));
                var used = ready.Exists ? ready.LastWriteTimeUtc : dir.CreationTimeUtc;
                cached.Add((dir.FullName, dir.EnumerateFiles().Sum(f => f.Length), used));
            }
            var mine = new DirectoryInfo(keep).EnumerateFiles().Sum(f => f.Length);
            foreach (var gone in Remux.Evict(cached, Math.Max(0, Cap - mine), DateTime.UtcNow, MaxAge))
                try { Directory.Delete(gone, recursive: true); } catch (IOException) { }
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            Log.Write($"player: cache: {error.Message}");
        }
    }
}
