using Microsoft.Web.WebView2.Core;

namespace Search;

// The ad blocker. No settings, no counter, no shield icon going green — it is
// put together once at launch and then it is simply true that the page is
// lighter.
//
// The Mac hands WebKit a compiled content rule list, enforced inside its
// networking before a request is made. WebView2 has no such list, so this is
// the nearest thing that costs as little: one request filter per unwanted
// domain, given to the engine itself. The engine matches every request against
// them in its own network service, and only a request to one of these domains
// ever crosses over to this process to be refused. Everything else — the page,
// its pictures, its own scripts — never wakes the app at all, which is the
// whole difference between this and a blocker written in JavaScript.
public sealed partial class Shield : Model
{
    public static readonly Shield Shared = new();

    private bool enabled = true;
    /// On unless somebody said otherwise. Every open page is told when this
    /// changes, so it takes effect on the next request rather than the next
    /// launch.
    public bool Enabled { get => enabled; set => Set(ref enabled, value); }

    private string? trouble;
    /// Set the one time putting the list together didn't work. The toggle in
    /// Settings can say "on" all it wants; nothing is actually blocked until
    /// this is null, so it is the one thing worth telling a person about
    /// rather than failing the quiet way a missing ad is quiet.
    public string? Trouble { get => trouble; private set => Set(ref trouble, value); }

    /// Sites it is off for — the ones it broke. A checkout that never
    /// finishes, a video that never starts: switching off here, for this site,
    /// beats switching off everywhere and forgetting to switch back.
    private readonly HashSet<string> paused = [.. Store.Settings.Strings("shield.paused")];

    /// Whether blocking is off on this one site.
    public bool IsPaused(string? host) => host != null && paused.Contains(host);

    /// Blocking off (or back on) for one site that breaks with it. Requests
    /// are decided as they are made, so it holds from the next one; the
    /// stylesheet for ad slots follows at the next page, the way the Mac tunes
    /// its rule list per page.
    public void Pause(string host, bool paused)
    {
        if (paused) this.paused.Add(host); else this.paused.Remove(host);
        Store.Settings.Set("shield.paused", this.paused.OrderBy(h => h, StringComparer.Ordinal));
    }

    /// Third parties whose only job is to watch or to sell. First-party
    /// requests are untouched: a site's own scripts are the site.
    private static readonly string[] Unwanted =
    [
        "doubleclick.net", "googlesyndication.com", "googleadservices.com",
        "googletagservices.com", "google-analytics.com", "googletagmanager.com",
        "adservice.google.com", "amazon-adsystem.com", "adnxs.com", "adsrvr.org",
        "criteo.com", "criteo.net", "taboola.com", "outbrain.com",
        "rubiconproject.com", "pubmatic.com", "openx.net", "casalemedia.com",
        "smartadserver.com", "sharethrough.com", "indexww.com", "bidswitch.net",
        "33across.com", "teads.tv", "moatads.com", "adroll.com",
        "scorecardresearch.com", "quantserve.com", "chartbeat.com",
        "hotjar.com", "mouseflow.com", "fullstory.com", "clarity.ms",
        "mixpanel.com", "amplitude.com", "segment.com", "segment.io",
        "branch.io", "appsflyer.com", "adjust.com", "analytics.tiktok.com",
        "connect.facebook.net", "ads-twitter.com", "analytics.twitter.com",
    ];

    /// The few slots that are reliably an advertisement and nothing else. Kept
    /// deliberately short — a generous cosmetic list is how a blocker starts
    /// eating the page it was meant to clean.
    private static readonly string[] Slots =
    [
        ".adsbygoogle", "ins.adsbygoogle", "[id^=\"google_ads_\"]",
        "[id^=\"div-gpt-ad\"]", "[id^=\"taboola-\"]", "#taboola-below-article",
        "iframe[src*=\"doubleclick.net\"]", "iframe[src*=\"googlesyndication\"]",
        "iframe[src*=\"amazon-adsystem\"]",
    ];

    private string[] filters = [];
    private HashSet<string> unwanted = [];
    private string? cosmetic;

    /// The filters the engine matches on, and the stylesheet for the slots.
    /// Two filters a domain: the engine's wildcard wants a dot before
    /// `*.doubleclick.net`, so the bare domain needs one of its own. Neither is
    /// trusted to be exact — a `*` matches slashes too — so a request that
    /// arrives here is checked against the host again before it is refused.
    public void Compile()
    {
        if (filters.Length > 0) return;
        Trouble = null;
        try
        {
            unwanted = [.. Unwanted];
            filters = Unwanted.SelectMany(d => new[] { $"*://{d}/*", $"*://*.{d}/*" }).ToArray();
            var css = string.Join(", ", Slots) + " { display: none !important; }";
            cosmetic = $$"""
            (function () {
              if (document.getElementById('office-shield')) return;
              var sheet = document.createElement('style');
              sheet.id = 'office-shield';
              sheet.textContent = {{Bridge.Literal(css)}};
              (document.head || document.documentElement).appendChild(sheet);
            })();
            """;
        }
        catch (Exception e)
        {
            filters = [];
            Trouble = "Couldn't build the block list: " + e.Message;
        }
    }

    /// Whether a page on `site` is being protected at all.
    public bool Guards(string? site) => Enabled && Trouble == null && !IsPaused(site);

    /// Every page asks for it once, when its engine starts. `site` is where
    /// the page is going, decided at each navigation — a rule list on the Mac
    /// is swapped in at the same moment, which is what makes "off for this
    /// site" true for the whole page rather than for the second half of it.
    internal void Protect(Tab tab, CoreWebView2 core)
    {
        if (filters.Length == 0) return;
        var site = Curtain.Host(tab.Address);
        string? slotsId = null;
        var adding = false;

        // The engine's own tracking prevention is the nearest thing Windows has
        // to what WebKit does underneath every page on the Mac. Balanced, not
        // Strict: Strict is Edge's own warning that sign-ins and embeds will
        // break, and this list is kept short for the same reason.
        Tune(core);

        // Only the page and its frames. A service worker's requests are raised
        // on every page with a filter that matches them, so a list on every
        // tab would hear each one once per tab.
        try
        {
            foreach (var filter in filters)
                core.AddWebResourceRequestedFilter(filter, CoreWebView2WebResourceContext.All, CoreWebView2WebResourceRequestSourceKinds.Document);
        }
        catch
        {
            // A runtime too old to be told where a request comes from hears
            // them all, which only costs the duplicates.
#pragma warning disable CS0618
            foreach (var filter in filters)
                core.AddWebResourceRequestedFilter(filter, CoreWebView2WebResourceContext.All);
#pragma warning restore CS0618
        }

        core.WebResourceRequested += (sender, e) =>
        {
            if (!Guards(site)) return;
            if (!Uri.TryCreate(e.Request.Uri, UriKind.Absolute, out var url)) return;
            if (url.Scheme is not ("http" or "https")) return;
            var host = Address.Host(url);
            if (host == null || !IsUnwanted(host)) return;
            // Third parties only, as the Mac's rule list said with its load
            // type. A page that is itself on one of these domains is that
            // site, and its own requests are the site's.
            if (Site(host) == Site(site)) return;
            e.Response = core.Environment.CreateWebResourceResponse(null, 403, "Blocked", "");
        };

        core.NavigationStarting += (sender, e) =>
        {
            if (!Uri.TryCreate(e.Uri, UriKind.Absolute, out var url)) return;
            site = Curtain.Host(url);
            // The stylesheet for the slots goes in with the page it belongs to
            // and stays out of the ones it doesn't.
            var wanted = Guards(site) && cosmetic != null;
            if (wanted && slotsId == null && !adding) _ = Add();
            else if (!wanted && slotsId != null)
            {
                core.RemoveScriptToExecuteOnDocumentCreated(slotsId);
                slotsId = null;
            }
        };

        async Task Add()
        {
            adding = true;
            try { slotsId = await core.AddScriptToExecuteOnDocumentCreatedAsync(cosmetic!); }
            catch { }
            adding = false;
            // Turned off while it was going in.
            if (slotsId != null && !Guards(site))
            {
                try { core.RemoveScriptToExecuteOnDocumentCreated(slotsId); } catch { }
                slotsId = null;
            }
        }
        if (Guards(site) && cosmetic != null) _ = Add();
    }

    /// Switched on or off for every page that is already open. Requests are
    /// decided as they happen, so only the engine's own level needs telling.
    internal void Tune(CoreWebView2 core)
    {
        try
        {
            core.Profile.PreferredTrackingPreventionLevel = Enabled
                ? CoreWebView2TrackingPreventionLevel.Balanced
                : CoreWebView2TrackingPreventionLevel.None;
        }
        catch { }
    }

    private bool IsUnwanted(string host)
    {
        // The host, then each parent in turn: ads.doubleclick.net, then
        // doubleclick.net. A handful of lookups, never a scan of the list.
        for (var at = host; ;)
        {
            if (unwanted.Contains(at)) return true;
            var dot = at.IndexOf('.');
            if (dot < 0) return false;
            at = at[(dot + 1)..];
        }
    }

    /// The part of a host a person would call the site: the last two labels,
    /// or three under a country's own second level (bbc.co.uk, abc.net.au).
    /// Good enough to tell a site from a stranger; no list of every suffix
    /// in the world is worth shipping for the difference.
    private static string? Site(string? host)
    {
        if (host == null) return null;
        var labels = host.Split('.');
        if (labels.Length <= 2) return host;
        var take = labels[^1].Length == 2 && labels[^2].Length <= 3 ? 3 : 2;
        return string.Join('.', labels[^take..]);
    }
}

public sealed partial class Browser
{
    partial void StartShield()
    {
        Shield.Shared.Enabled = Prefs.Shielded;
        Shield.Shared.Compile();
        // Settings says on or off; every page already open hears it (see
        // Follow, which says so out loud).
        Shield.Shared.On(nameof(Shield.Enabled), () =>
        {
            foreach (var tab in Tabs.Concat(ParkedTabs))
                if (tab.Core is { } core) Shield.Shared.Tune(core);
        });
    }

    partial void AttachShield(Tab tab, CoreWebView2 core) => Shield.Shared.Protect(tab, core);
}
