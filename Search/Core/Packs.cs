using System.Net.Http.Headers;
using System.Text.Json.Nodes;
using SearchKit.Packs;

namespace Search;

/// Settings › Packs: what Search adds only when asked (FFmpeg now; voice,
/// OCR and the rest later). Each is an exact build named in the manifest
/// Search ships with (SearchKit.Packs), downloaded from its publisher only
/// when the person clicks Install or Update — never checked for in the
/// background — verified against the manifest's SHA-256, and unpacked under
/// Store.Folder\Packs, so a test world has its own.
///
/// The engine is told where a pack's programs are through `packs.json` in
/// its data folder (Engine/src/core/packs.rs), read each time it looks.
public static class Packs
{
    public static readonly PackStore Shelf = new(Path.Combine(Store.Folder, "Packs"));

    public static IReadOnlyList<Pack> All => PackManifest.Shipped;

    /// Something about a pack changed: installed, removed, progress, trouble.
    /// Raised on the UI thread.
    public static event Action<string>? Changed;

    private sealed class Job(CancellationTokenSource cancel)
    {
        public CancellationTokenSource Cancel { get; } = cancel;
        public long Received;
    }

    private static readonly Dictionary<string, Job> jobs = [];
    private static readonly Dictionary<string, string> troubles = [];
    private static HttpClient? http;

    public static PackState State(Pack pack) => Shelf.State(pack);

    /// Bytes received so far, while it downloads.
    public static long? Received(string id) => jobs.TryGetValue(id, out var job) ? Interlocked.Read(ref job.Received) : null;

    /// What went wrong the last time, until the next try.
    public static string? Trouble(string id) => troubles.GetValueOrDefault(id);

    /// The folder holding FFmpeg's programs, when the pack is in.
    public static string? FfmpegBin =>
        Shelf.Folder("ffmpeg") is { } folder && File.Exists(Path.Combine(folder, "bin", "ffmpeg.exe"))
            ? Path.Combine(folder, "bin") : null;

    /// Download, verify and unpack. The person asked (a button, or
    /// `>install ffmpeg`): this is the only way a pack reaches the network.
    public static async void Install(string id)
    {
        if (PackManifest.Find(id) is not { } pack || jobs.ContainsKey(id)) return;
        var job = new Job(new CancellationTokenSource());
        jobs[id] = job;
        troubles.Remove(id);
        Tell(id);
        // Progress is Settings' to draw; tool pages hear only the outcome.
        var ticking = UI.Every(0.25, () => Changed?.Invoke(id));
        try
        {
            await Task.Run(() => Fetch(pack, job));
            Log.Write($"packs: {id} {pack.Version} installed");
            WriteEngineList();
            if (App.Window?.Browser is { } browser) browser.Announce($"{pack.Name} is ready");
        }
        catch (OperationCanceledException) when (job.Cancel.IsCancellationRequested)
        {
        }
        catch (Exception error)
        {
            troubles[id] = error switch
            {
                PackVerifyException => error.Message,
                HttpRequestException or IOException when !job.Cancel.IsCancellationRequested
                    => $"The download didn't finish ({error.Message}). Nothing was changed.",
                _ => error.Message,
            };
            Log.Write($"packs: {id}: {error.Message}");
        }
        finally
        {
            ticking.Stop();
            jobs.Remove(id);
            job.Cancel.Dispose();
            Tell(id);
        }
    }

    public static void Cancel(string id)
    {
        if (jobs.TryGetValue(id, out var job)) job.Cancel.Cancel();
    }

    /// Takes a pack off this PC. What was made with it stays; files it was
    /// busy with (a video being converted) make this wait for next time.
    public static void Remove(string id)
    {
        if (jobs.ContainsKey(id)) return;
        troubles.Remove(id);
        try
        {
            if (id == "ffmpeg") Player.Forget();
            Shelf.Remove(id);
            Log.Write($"packs: {id} removed");
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            troubles[id] = "It's in use right now — close what's using it and try again.";
        }
        WriteEngineList();
        Tell(id);
    }

    private static async Task Fetch(Pack pack, Job job)
    {
        http ??= Client();
        using var response = await http.GetAsync(pack.Download, HttpCompletionOption.ResponseHeadersRead, job.Cancel.Token);
        response.EnsureSuccessStatusCode();
        if (response.Content.Headers.ContentLength is { } length && length != pack.Size)
            throw new PackVerifyException($"The {pack.Name} download isn't the size Search expects, so it wasn't used.");
        await using var body = await response.Content.ReadAsStreamAsync(job.Cancel.Token);
        await Shelf.InstallAsync(pack, body, received => Interlocked.Exchange(ref job.Received, received), job.Cancel.Token);
    }

    private static HttpClient Client()
    {
        var client = new HttpClient { Timeout = Timeout.InfiniteTimeSpan };
        client.DefaultRequestHeaders.UserAgent.Add(new ProductInfoHeaderValue("Search", Updater.Version));
        return client;
    }

    private static void Tell(string id)
    {
        Changed?.Invoke(id);
        ToolsHost.Tell("packs-changed", new JsonObject { ["id"] = id, ["installed"] = Shelf.Installed(id) });
    }

    /// Where each installed pack's programs are, for the engine
    /// (Engine/src/core/packs.rs).
    public static void WriteEngineList()
    {
        var list = new JsonObject();
        if (FfmpegBin is { } bin) list["ffmpeg"] = bin;
        var file = Path.Combine(Engine.DataDir, "packs.json");
        try
        {
            if (list.Count == 0 && !File.Exists(file)) return;
            Store.WriteAtomic(file, System.Text.Encoding.UTF8.GetBytes(list.ToJsonString()));
        }
        catch (Exception error) { Log.Write($"packs: engine list: {error.Message}"); }
    }

    /// Settings › Packs, in front.
    public static void Show()
    {
        if (App.Window?.Browser is not { } browser) return;
        Store.Settings.Set("settings.page", "packs");
        browser.Tuning = false;
        browser.Tuning = true;
    }
}
