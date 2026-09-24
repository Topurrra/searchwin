using System.Diagnostics;
using System.Text.Json;
using Microsoft.Web.WebView2.Core;
using SearchKit.FishCatcher;
using Catcher = SearchKit.FishCatcher.FishCatcher;

namespace Search;

// FishCatcher, built in: a warning before a scam or phishing site loads.
//
// Every top-level navigation is checked the moment it starts, before a single
// request goes out: the address alone (look-alike names, a brand where it
// doesn't belong, a known-bad list, a small model of how phishing addresses
// are spelt) decides most cases, in a tenth of a millisecond, on this
// computer. A strong verdict stops the navigation and puts Search's own page
// up in its place; a weaker one gets a quiet line at the bottom. Then the
// page itself is looked at once it has drawn (fish-probe.js), for what the
// address can't show: a PayPal sign-in on a site that isn't PayPal, a form
// that posts your password somewhere else, a wallet asking for its recovery
// phrase, a fake "your computer is locked" screen.
//
// Warn, never block: "Continue anyway" is always there, and holds for that
// site until Search quits. And fail open: until the tables have loaded, for
// any address the engine can't read, and on any error, there is simply no
// verdict — FishCatcher never stands between you and a page because it
// broke.

/// A quiet word about a site: a few unusual signs, not enough to stop for.
public sealed record FishNote(string Host, string Text, string? RealSite);

public sealed partial class Tab
{
    private Verdict? fish;
    /// FishCatcher's last word on this tab's page: from its address when the
    /// navigation started, or from the page once it showed what it asks for.
    /// Null when there is none (not asked, not a web page, not loaded yet).
    public Verdict? Fish { get => fish; set => Set(ref fish, value); }

    /// How long the address check took, in milliseconds. The bench says so.
    public double FishMs { get; set; }

    private FishNote? caution;
    /// The quiet line at the bottom, while this tab is on a site with a few
    /// unusual signs (see Bars).
    public FishNote? Caution { get => caution; set => Set(ref caution, value); }

    /// The warning is answered and the tab goes back to whatever the engine
    /// is showing — the page you were on, when the warned-about one was
    /// stopped before it came — or to nothing at all.
    internal void Settle(Uri? where)
    {
        Failure = null;
        Address = where;
        Title = where != null ? Core?.DocumentTitle ?? "" : "";
    }
}

/// The page half of FishCatcher: fish-probe.js, which reports what a page
/// asks for (never what you type) as `fish.facts`.
public static class FishProbe
{
    public const string Name = "fish.facts";

    private static string? script;

    /// Read from the executable the first time a page is armed, which is
    /// after the first window: ~15 KB, once.
    public static string Script => script ??= Load();

    private static string Load()
    {
        try
        {
            using var stream = typeof(FishProbe).Assembly.GetManifestResourceStream("js/fish-probe.js");
            if (stream == null) return "";
            using var reader = new StreamReader(stream);
            return reader.ReadToEnd();
        }
        catch
        {
            return "";
        }
    }
}

public sealed partial class Browser
{
    /// Sites you chose to open despite the warning, until Search quits.
    private readonly HashSet<string> scamsContinued = new(StringComparer.OrdinalIgnoreCase);

    /// Sites whose quiet line you closed, until Search quits.
    private readonly HashSet<string> cautionsQuieted = new(StringComparer.OrdinalIgnoreCase);

    partial void StartFish()
    {
        Bridge.Handlers[FishProbe.Name] = FishFacts;
        Tab.Forewarn = Forewarn;
        // The tables (~450 KB) load on a worker once the first window is up,
        // so the first address typed is checked rather than waved through.
        // A navigation before then is waved through and checked late (Late).
        UI.After(0.5, () => { if (Prefs.WarnsOfScams) WarmUp(); });
        FishFeed.Schedule();

        Prefs.On(nameof(Preferences.WarnsOfScams), () =>
        {
            if (Prefs.WarnsOfScams) WarmUp();
            // The probe goes into the next page, or stays out of it. Each tab
            // keeps whatever it has hidden on its site (see Passkeys).
            foreach (var tab in Tabs.Concat(ParkedTabs))
            {
                tab.Arm(Curtain.Css(Curtain.Host(tab.Address)), force: true);
                if (!Prefs.WarnsOfScams) tab.Caution = null;
            }
            Announce(Prefs.WarnsOfScams ? "Scam and phishing sites get a warning" : "No more scam warnings");
        });
        Prefs.On(nameof(Preferences.ScamFeed), () =>
        {
            if (Prefs.ScamFeed) FishFeed.Refresh(force: true);
            else FishFeed.Forget();
        });
    }

    /// The tables, loaded on a worker, and one check run there too, so the
    /// first real one (on the UI thread) isn't also the one that pays for
    /// compiling the engine's code paths and regexes.
    private static void WarmUp() =>
        _ = Catcher.Warm().ContinueWith(_ => Catcher.Check("https://paypa1-warm.example/login"), TaskScheduler.Default);

    /// An address Search itself is about to hand the engine — typed, a
    /// bookmark, a link from another app, a tab waking — checked before the
    /// engine hears of it. Told first, the engine would already be looking
    /// the name up and opening a connection when it asked NavigationStarting.
    private bool Forewarn(Tab tab, Uri url)
    {
        Forget(tab);
        var verdict = Weigh(tab, url);
        return verdict != null && Judge(tab, url, verdict, loaded: false);
    }

    /// Links clicked in a page, redirects, a page moving itself: these reach
    /// Search only as they start, from inside the engine.
    ///
    /// NavigationStarting says so, and cancelling it stops the page — but it
    /// is not a gate: the engine asks it and sends the request at the same
    /// time, so by the time the answer is "no" the site may already have the
    /// address, path and all (Lessons Learned: FishCatcher). The gate is the
    /// request itself: every document request passes through here
    /// (WebResourceRequested, before it leaves), and the one NavigationStarting
    /// has just stopped is answered here with nothing, so it never goes out.
    partial void AttachFish(Tab tab, CoreWebView2 core)
    {
        // The top-level page whose navigation was just stopped.
        Uri? stopped = null;

        core.NavigationStarting += (sender, e) =>
        {
            // Cancelled already: an extension's sign-in coming back, a link
            // handed to another app.
            if (e.Cancel) return;
            Forget(tab);
            stopped = null;
            if (!Uri.TryCreate(e.Uri, UriKind.Absolute, out var url)) return;
            var verdict = Weigh(tab, url);
            if (verdict == null || !Judge(tab, url, verdict, loaded: false)) return;
            e.Cancel = true;
            stopped = url;
        };

        // Document requests only — the page and its frames, not the pictures
        // and scripts inside them — so this costs a lookup per page, not per
        // request.
        try
        {
            core.AddWebResourceRequestedFilter("*", CoreWebView2WebResourceContext.Document, CoreWebView2WebResourceRequestSourceKinds.Document);
        }
        catch
        {
#pragma warning disable CS0618
            core.AddWebResourceRequestedFilter("*", CoreWebView2WebResourceContext.Document);
#pragma warning restore CS0618
        }
        core.WebResourceRequested += (sender, e) =>
        {
            if (e.ResourceContext != CoreWebView2WebResourceContext.Document || !Prefs.WarnsOfScams) return;
            if (!Uri.TryCreate(e.Request.Uri, UriKind.Absolute, out var url) || !Watched(url)) return;
            if (Stopped(url)) { Refuse(e); return; }
            // A frame's document looks the same from here as the page's own.
            // Only an address FishCatcher warns about is worth telling apart;
            // for one of those, NavigationStarting may simply not have been
            // heard yet, so the answer waits one turn of the UI thread for it.
            if (scamsContinued.Contains(url.Host) || Catcher.Check(url) is not { Warns: true }) return;
            var deferral = e.GetDeferral();
            UI.Soon(() =>
            {
                try { if (Stopped(url)) Refuse(e); }
                catch { }
                deferral.Complete();
            });
        };

        bool Stopped(Uri url) => stopped != null && Uri.Compare(stopped, url, UriComponents.HttpRequestUrl, UriFormat.UriEscaped, StringComparison.OrdinalIgnoreCase) == 0;

        // An empty "No Content" answer: to the engine, a navigation that
        // decided not to happen. The page you were on stays, under the warning.
        void Refuse(CoreWebView2WebResourceRequestedEventArgs e) =>
            e.Response = core.Environment.CreateWebResourceResponse(null, 204, "No Content", "");
    }

    /// A new document: what was said about the last one no longer holds.
    private static void Forget(Tab tab)
    {
        if (tab.Failure is { Kind: TroubleKind.Scam }) tab.Failure = null;
        tab.Fish = null;
        tab.Caution = null;
    }

    /// The address check, timed for the bench. Null when there is nothing to
    /// say — including before the tables have loaded, when the page goes
    /// ahead and is looked at the moment they are (Late).
    private Verdict? Weigh(Tab tab, Uri url)
    {
        if (!Prefs.WarnsOfScams || !Watched(url)) return null;
        var started = Stopwatch.GetTimestamp();
        var verdict = Catcher.Check(url);
        tab.FishMs = Stopwatch.GetElapsedTime(started).TotalMilliseconds;
        if (verdict == null && !Catcher.IsReady && Catcher.LoadError == null) Late(tab, url);
        return verdict;
    }

    /// Web pages only, and never Search's own (tools.search, files.search).
    private static bool Watched(Uri url) =>
        Address.IsWeb(url) && !url.Host.EndsWith(".search", StringComparison.OrdinalIgnoreCase);

    private static bool SameHost(Uri? a, Uri? b) =>
        a != null && b != null && string.Equals(a.Host, b.Host, StringComparison.OrdinalIgnoreCase);

    /// What the engine is showing, if it is a web page.
    private static Uri? Showing(Tab tab) =>
        Uri.TryCreate(tab.Core?.Source, UriKind.Absolute, out var there) && Address.IsWeb(there) ? there : null;

    /// Acts on a verdict. Answers whether the warning page went up.
    private bool Judge(Tab tab, Uri url, Verdict verdict, bool loaded)
    {
        tab.Fish = verdict;
        if (scamsContinued.Contains(url.Host)) return false;
        if (verdict.Warns)
        {
            tab.Caution = null;
            tab.Fail(new PageTrouble(TroubleKind.Scam, PageTrouble.HostOf(url))
            {
                Url = url,
                Reasons = verdict.Reasons,
                RealSite = verdict.RealSite,
                Strong = verdict.Level == Level.Critical,
                Loaded = loaded,
            });
            return true;
        }
        if (verdict.Level == Level.Elevated && !cautionsQuieted.Contains(url.Host))
            tab.Caution = new FishNote(url.Host, Say(verdict), verdict.RealSite);
        return false;
    }

    /// The quiet line: the strongest reason, and what to do about it.
    private static string Say(Verdict verdict)
    {
        var strongest = verdict.Signals.OrderByDescending(s => s.Weight).FirstOrDefault();
        return strongest is null ? verdict.Summary : $"{strongest.Sentence}. Double-check the address.";
    }

    /// A page that started before the tables had loaded, looked at once they
    /// have. Late is still better than never: a warning over a page you are
    /// about to type a password into is the one that matters.
    private async void Late(Tab tab, Uri url)
    {
        Verdict? verdict;
        try { verdict = await Catcher.CheckAsync(url); }
        catch { return; }
        if (verdict == null || !Prefs.WarnsOfScams) return;
        // Somewhere else by now, or already judged by something newer.
        if (!SameHost(tab.Address, url) && !SameHost(Showing(tab), url)) return;
        if (tab.Fish != null || tab.Failure is { Kind: TroubleKind.Scam }) return;
        if (Judge(tab, url, verdict, loaded: true) && tab.Loading) tab.Stop();
    }

    /// The probe's facts about the page on screen: the address check again,
    /// with what the page asks for. Only a stronger word than the address
    /// alone gave is acted on.
    private void FishFacts(Tab tab, JsonElement body)
    {
        if (!Prefs.WarnsOfScams || Showing(tab) is not { } url || !Watched(url)) return;
        if (body.ValueKind != JsonValueKind.Object) return;
        // From a page the tab has since left.
        if (body.TryGetProperty("href", out var href) && href.ValueKind == JsonValueKind.String
            && Uri.TryCreate(href.GetString(), UriKind.Absolute, out var from) && !SameHost(from, url)) return;
        if (PageFacts.From(body) is not { } facts) return;
        var verdict = Catcher.Check(url, facts);
        if (verdict == null) return;
        if (tab.Failure is { Kind: TroubleKind.Scam }) return;
        if (tab.Fish is { } said && verdict.Level <= said.Level)
        {
            // Nothing to say out loud, but the bench sees the fuller score.
            if (verdict.Score > said.Score) tab.Fish = verdict;
            return;
        }
        Judge(tab, url, verdict, loaded: true);
    }

    // MARK: - what the warning offers

    /// "Go back": to the page you were on, when the warned-about one was
    /// stopped before it came; back a page when it did come; and to an empty
    /// tab when there is nowhere to go back to.
    public void LeaveScam()
    {
        if (Active is not { Failure: { Kind: TroubleKind.Scam } trouble } tab) return;
        var there = Showing(tab);
        tab.Fish = null;
        if (!SameHost(there, trouble.Url))
        {
            tab.Settle(there);
            return;
        }
        if (tab.CanGoBack)
        {
            tab.Back();
            return;
        }
        try { tab.Core?.Navigate("about:blank"); } catch { }
        tab.Settle(null);
    }

    /// "Continue anyway", having been told. For this site, until Search quits.
    public void ContinueToScam()
    {
        if (Active is not { Failure: { Kind: TroubleKind.Scam, Url: { } url } } tab) return;
        scamsContinued.Add(url.Host);
        // Covered rather than stopped: the page is there, under the warning.
        if (SameHost(Showing(tab), url))
        {
            tab.Settle(Showing(tab));
            return;
        }
        tab.Failure = null;
        Go(tab, url);
    }

    /// "Go to paypal.com": the site the page was pretending to be. The name
    /// comes from FishCatcher's own list of brands, never from the page.
    public void OpenRealSite()
    {
        if (Active is not { Failure: { Kind: TroubleKind.Scam, RealSite: { Length: > 0 } real } } tab) return;
        if (!Uri.TryCreate("https://" + real + "/", UriKind.Absolute, out var url)) return;
        tab.Failure = null;
        Go(tab, url);
    }

    /// Closes the quiet line, for this site until Search quits.
    public void QuietCaution()
    {
        if (Active is not { Caution: { } note } tab) return;
        cautionsQuieted.Add(note.Host);
        tab.Caution = null;
    }

    /// The quiet line's "Go to …".
    public void OpenCautionSite()
    {
        if (Active is not { Caution: { RealSite: { Length: > 0 } real } } tab) return;
        if (!Uri.TryCreate("https://" + real + "/", UriKind.Absolute, out var url)) return;
        tab.Caution = null;
        Go(tab, url);
    }
}

/// FishCatcher's daily feed: newly reported scam sites, as a signed list from
/// FishCatcher's registry (FeedClient.DefaultUrl). Opt-in — it's the one
/// network call FishCatcher makes — and kept under Store.Folder, so a test
/// world has its own. Fails open: a feed that doesn't come, or doesn't
/// verify, leaves the last good one (or the bundled lists) in force.
public static class FishFeed
{
    private static readonly string Folder = Path.Combine(Store.Folder, "FishCatcher");
    private static HttpClient? http;
    private static FeedClient? client;
    private static bool savedApplied;
    private static int running;
    private static Microsoft.UI.Dispatching.DispatcherQueueTimer? daily;

    /// Well after the first window, and then every hour — the client itself
    /// only goes to the network once a day.
    public static void Schedule()
    {
        UI.After(15, () =>
        {
            Refresh(force: false);
            daily ??= UI.Every(3600, () => Refresh(force: false));
        });
    }

    /// Loads the saved feed once, then downloads a new one if it's due (or
    /// `force`). On a thread of its own at low priority: checking the
    /// signature and reading a few hundred thousand names is real work, and
    /// none of it is the page's.
    public static void Refresh(bool force)
    {
        if (Browser.Shared is not { Prefs: { WarnsOfScams: true, ScamFeed: true } }) return;
        if (Interlocked.Exchange(ref running, 1) == 1) return;
        var worker = new Thread(() =>
        {
            try { Run(force).GetAwaiter().GetResult(); }
            catch (Exception e) { Log.Write($"fishcatcher feed: {e.Message}"); }
            finally { Volatile.Write(ref running, 0); }
        })
        {
            IsBackground = true,
            Priority = ThreadPriority.Lowest,
            Name = "FishCatcher feed",
        };
        worker.Start();
    }

    private static async Task Run(bool force)
    {
        await Catcher.Warm().ConfigureAwait(false);
        if (Catcher.Data is not { RegistryKeySpki.Length: > 0 } data) return;
        http ??= new HttpClient { Timeout = TimeSpan.FromSeconds(30) };
        client ??= new FeedClient(http, Folder, data.RegistryKeySpki);
        if (!savedApplied)
        {
            savedApplied = true;
            if (client.LoadSaved() is { } saved) Catcher.ApplyFeed(saved);
        }
        var result = await client.RefreshAsync(force).ConfigureAwait(false);
        if (result is { Outcome: FeedOutcome.Updated, Bundle: { } bundle }) Catcher.ApplyFeed(bundle);
        Log.Write($"fishcatcher feed: {result.Outcome}, {result.Count?.ToString() ?? "?"} sites");
    }

    /// Turned off: the download is forgotten and the bundled lists are back.
    public static void Forget()
    {
        savedApplied = false;
        try
        {
            if (client != null) client.Forget();
            else
            {
                foreach (var name in (string[])["fishcatcher-feed.json", "fishcatcher-feed-state.json"])
                    File.Delete(Path.Combine(Folder, name));
            }
        }
        catch { }
        Catcher.ClearFeed();
    }

    /// "Updated 3 hr. ago · 41,202 sites", for Settings; null before the first download.
    public static string? Said()
    {
        try
        {
            var path = Path.Combine(Folder, "fishcatcher-feed-state.json");
            if (!File.Exists(path)) return null;
            using var doc = JsonDocument.Parse(File.ReadAllBytes(path));
            var root = doc.RootElement;
            if (!root.TryGetProperty("updatedAt", out var at) || !at.TryGetInt64(out var ms)) return null;
            var when = When.Said(DateTimeOffset.FromUnixTimeMilliseconds(ms).UtcDateTime);
            return root.TryGetProperty("count", out var c) && c.TryGetInt64(out var n)
                ? $"Updated {when} · {n:N0} sites"
                : $"Updated {when}";
        }
        catch
        {
            return null;
        }
    }
}
