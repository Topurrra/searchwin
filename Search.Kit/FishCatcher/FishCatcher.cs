namespace SearchKit.FishCatcher;

/// FishCatcher for the browser: phishing and scam warnings, fully on the
/// device. It warns and never blocks, and it fails open: when it can't tell
/// (data not loaded yet, an address it can't read, anything unexpected) the
/// answer is "no verdict", never an error and never a warning.
///
/// Nothing loads until it's needed: the first Check (or Warm) starts reading
/// the bundled tables on a worker thread, which takes tens of milliseconds.
/// Until then Check returns null and CheckAsync waits. After that a check is
/// well under a millisecond and can run on the UI thread.
public static class FishCatcher
{
    private static readonly object gate = new();
    private static Task<FishData?>? loading;
    private static volatile FishData? data;
    // Changes asked for before the tables were loaded, applied once they are.
    private static FeedBundle? pendingFeed;
    private static string[]? pendingTrust;

    /// The tables, once loaded (null before, or if they couldn't be read).
    public static FishData? Data => data;

    public static bool IsReady => data is not null;

    /// Why loading failed, if it did (the engine then stays silent).
    public static Exception? LoadError { get; private set; }

    /// Starts loading in the background, once. Safe to call from any thread.
    public static Task Warm() => Load();

    private static Task<FishData?> Load()
    {
        lock (gate)
        {
            return loading ??= Task.Run(() =>
            {
                try
                {
                    var loaded = FishData.LoadBundled();
                    lock (gate)
                    {
                        if (pendingFeed is not null) loaded = loaded.WithFeed(pendingFeed);
                        if (pendingTrust is not null) loaded = loaded.WithTrusted(pendingTrust);
                        pendingFeed = null;
                        pendingTrust = null;
                        data = loaded;
                    }
                    return loaded;
                }
                catch (Exception e)
                {
                    LoadError = e;
                    return null;
                }
            });
        }
    }

    /// The verdict for an address, or null: not a web address, no verdict
    /// possible, or the tables aren't loaded yet (this starts loading them).
    public static Verdict? Check(Uri url, PageFacts? facts = null)
    {
        var d = data;
        if (d is null)
        {
            _ = Load();
            return null;
        }
        try { return Analyzer.Analyze(url, d, facts); }
        catch (Exception) { return null; } // fail open
    }

    /// Check for text as the page reports it (WebView2's Source, a link's href).
    public static Verdict? Check(string url, PageFacts? facts = null)
    {
        var d = data;
        if (d is null)
        {
            _ = Load();
            return null;
        }
        try { return Analyzer.Analyze(url, d, facts); }
        catch (Exception) { return null; }
    }

    /// Check, waiting for the tables if they're still loading.
    public static async Task<Verdict?> CheckAsync(Uri url, PageFacts? facts = null)
    {
        var d = data ?? await Load().ConfigureAwait(false);
        if (d is null) return null;
        try { return Analyzer.Analyze(url, d, facts); }
        catch (Exception) { return null; }
    }

    public static async Task<Verdict?> CheckAsync(string url, PageFacts? facts = null)
    {
        var d = data ?? await Load().ConfigureAwait(false);
        if (d is null) return null;
        try { return Analyzer.Analyze(url, d, facts); }
        catch (Exception) { return null; }
    }

    /// Findings for a page's links (shorteners, text that shows another site,
    /// downloads that hide a program; `deep` also scores each destination).
    public static IReadOnlyList<LinkFinding> CheckLinks(IEnumerable<PageLink> links, bool deep = false)
    {
        var d = data;
        if (d is null) { _ = Load(); return []; }
        try { return Links.Classify(links, d, deep); }
        catch (Exception) { return []; }
    }

    /// A warning for a download whose name hides what it is, or null.
    public static DownloadWarning? CheckDownload(string name, string? mime) => Links.InspectDownload(name, mime);

    /// Puts a verified feed in force (from FeedClient.LoadSaved or RefreshAsync).
    public static void ApplyFeed(FeedBundle bundle)
    {
        lock (gate)
        {
            if (data is { } d) data = d.WithFeed(bundle);
            else pendingFeed = bundle;
        }
    }

    /// Drops the feed and goes back to the bundled lists (the feed was turned off).
    public static void ClearFeed()
    {
        lock (gate)
        {
            pendingFeed = null;
            if (data is { } d) data = d.WithoutFeed();
        }
    }

    /// Your trust list: these domains are never scored.
    public static void SetTrusted(IEnumerable<string> domains)
    {
        var list = domains.ToArray();
        lock (gate)
        {
            if (data is { } d) data = d.WithTrusted(list);
            else pendingTrust = list;
        }
    }
}
