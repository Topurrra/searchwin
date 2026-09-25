using Microsoft.Web.WebView2.Core;
using SearchKit.Shields;
using Catcher = SearchKit.FishCatcher.FishCatcher;
using FishLevel = SearchKit.FishCatcher.Level;

namespace Search;

// The ad blocker. No counter on the toolbar, no shield icon going green — it
// is simply true that the page is lighter.
//
// The Mac hands WebKit a compiled content rule list, enforced inside its
// networking before a request is made. WebView2 has no such list. The first
// version here gave the engine one request filter per unwanted domain, which
// was as cheap as it gets for 44 domains — and hopeless for a real list:
// each filter costs more to add than the last, so EasyList's hundred
// thousand would take an hour per tab (see the spike in Lessons Learned).
// So the engine is given one filter, "*", and every request a page makes
// crosses over to be decided here, against a compiled list
// (SearchKit.Shields.FilterList): about ten microseconds a request, on the
// UI thread, which is why the check allocates as little as it can and never
// waits on anything.
//
// The list itself: EasyList and EasyPrivacy when you've asked for them
// (they're downloads — see ShieldLists), and otherwise the 44 domains below,
// which are always there to fall back on. Around it, the rest of Shields:
// a stylesheet for ad slots before the page draws, tracking tags taken off
// addresses, YouTube's ads taken out of its player data, cookie banners
// answered "no", AMP pages swapped for the real one — each of them off for
// any site you pause.
public sealed partial class Shield : Model
{
    public static readonly Shield Shared = new();

    private bool enabled = true;
    /// On unless somebody said otherwise. Every open page is told when this
    /// changes, so it takes effect on the next request rather than the next
    /// launch.
    public bool Enabled
    {
        get => enabled;
        set { if (Set(ref enabled, value)) Changed(); }
    }

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
    /// stylesheets and page scripts follow at the next page.
    public void Pause(string host, bool paused)
    {
        if (paused) this.paused.Add(host); else this.paused.Remove(host);
        Store.Settings.Set("shield.paused", this.paused.OrderBy(h => h, StringComparer.Ordinal));
        Changed();
    }

    /// Anything a page's scripts or stylesheets depend on changed: the list,
    /// a site paused, Shields on or off. Every open page is armed again (see
    /// Browser.StartShield); each takes it from its next document.
    public event Action? Rearm;

    private void Changed()
    {
        pausedTest = null;
        generic = null;
        guarded.Clear();
        siteCss.Clear();
        Rearm?.Invoke();
    }

    /// Third parties whose only job is to watch or to sell. First-party
    /// requests are untouched: a site's own scripts are the site. What is
    /// blocked until the full lists are there, and whenever they aren't.
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

    /// The few slots that are reliably an advertisement and nothing else —
    /// the stylesheet when there is no list. Kept deliberately short: a
    /// generous cosmetic list is how a blocker starts eating the page it was
    /// meant to clean.
    private static readonly string[] Slots =
    [
        ".adsbygoogle", "ins.adsbygoogle", "[id^=\"google_ads_\"]",
        "[id^=\"div-gpt-ad\"]", "[id^=\"taboola-\"]", "#taboola-below-article",
        "iframe[src*=\"doubleclick.net\"]", "iframe[src*=\"googlesyndication\"]",
        "iframe[src*=\"amazon-adsystem\"]",
    ];

    private HashSet<string> unwanted = [];
    private string slotsLiteral = "\"\"";

    /// The built-in list: the domains and the slots. Tiny, and ready before
    /// the first page.
    public void Compile()
    {
        if (unwanted.Count > 0) return;
        Trouble = null;
        try
        {
            unwanted = [.. Unwanted];
            slotsLiteral = Bridge.Literal(string.Join(",", Slots) + "{display:none!important}");
        }
        catch (Exception e)
        {
            unwanted = [];
            Trouble = "Couldn't build the block list: " + e.Message;
        }
    }

    private FilterList? list;
    private string? listLiteral;

    /// The downloaded lists, compiled, when there are any.
    public FilterList? List => list;

    /// The lists in, or out (null: back to the built-in 44). `literal` is the
    /// list's shared stylesheet already written as a script string — 200 KB,
    /// so it is made on the worker that loaded the list (Prepare), not here.
    public void Use(FilterList? list, string? literal)
    {
        this.list = list;
        listLiteral = list == null ? null : literal ?? Prepare(list);
        Tell(nameof(List));
        Changed();
    }

    /// The part of Use that costs something, for the worker to do first.
    public static string Prepare(FilterList list) => Bridge.Literal(list.GenericCss);

    /// Whether a page on `site` is being protected at all.
    public bool Guards(string? site) => Enabled && Trouble == null && !IsPaused(site);

    // MARK: - requests

    /// Every page asks for it once, when its engine starts.
    internal void Protect(Tab tab, CoreWebView2 core)
    {
        var ward = tab.Ward = new Ward(this, tab, core);
        Tune(tab, core);
        core.WebResourceRequested += ward.Requested;
        core.NavigationStarting += ward.Starting;
        core.ContentLoading += ward.Loading;
        core.WebResourceResponseReceived += ward.ResponseReceived;
        ward.Dress(tab.Address);
        ArmWorkers(core);
    }

    /// Switched on or off for a page that is already open: the engine's own
    /// tracking prevention, and whether its requests cross over here at all.
    internal void Tune(Tab tab, CoreWebView2 core)
    {
        // The engine's own tracking prevention is the nearest thing Windows has
        // to what WebKit does underneath every page on the Mac. Balanced, not
        // Strict: Strict is Edge's own warning that sign-ins and embeds will
        // break.
        try
        {
            core.Profile.PreferredTrackingPreventionLevel = Enabled
                ? CoreWebView2TrackingPreventionLevel.Balanced
                : CoreWebView2TrackingPreventionLevel.None;
        }
        catch { }
        if (tab.Ward is not { } ward) return;
        var want = Enabled && Trouble == null;
        if (ward.Filtered == want) return;
        // One filter for everything the page and its frames ask for. Only the
        // page's own requests: a service worker's are raised on every page
        // with a filter that matches them, so they'd be heard once per tab
        // (see ArmWorkers below for those).
        try
        {
            if (want) core.AddWebResourceRequestedFilter("*", CoreWebView2WebResourceContext.All, CoreWebView2WebResourceRequestSourceKinds.Document);
            else core.RemoveWebResourceRequestedFilter("*", CoreWebView2WebResourceContext.All, CoreWebView2WebResourceRequestSourceKinds.Document);
        }
        catch
        {
            // A runtime too old to be told where a request comes from hears
            // them all, which only costs the duplicates.
#pragma warning disable CS0618
            try
            {
                if (want) core.AddWebResourceRequestedFilter("*", CoreWebView2WebResourceContext.All);
                else core.RemoveWebResourceRequestedFilter("*", CoreWebView2WebResourceContext.All);
            }
            catch { return; }
#pragma warning restore CS0618
        }
        ward.Filtered = want;
    }

    /// A service worker's (or a shared worker's) own requests never cross
    /// the Document-only filter above, so ads and trackers a site fetches
    /// from inside one went unblocked (a worker only registers once, in a
    /// site's first tab, and every page on that site shares the same one).
    /// Registered on a single page rather than every open one — a worker's
    /// requests are raised once per page with a matching filter, so putting
    /// it everywhere would decide the very same request once per open tab.
    /// Moved to another page only once the one holding it turns out to be
    /// gone: checked the cheap way, by touching it and seeing whether it
    /// throws, whenever a fresh page starts.
    private CoreWebView2? workerCore;

    private void ArmWorkers(CoreWebView2 core)
    {
        if (workerCore != null)
        {
            try { _ = workerCore.Source; return; } catch { workerCore = null; }
        }
        try
        {
            core.AddWebResourceRequestedFilter("*", CoreWebView2WebResourceContext.All,
                CoreWebView2WebResourceRequestSourceKinds.ServiceWorker | CoreWebView2WebResourceRequestSourceKinds.SharedWorker);
        }
        catch { return; }
        core.WebResourceRequested += WorkerRequested;
        workerCore = core;
    }

    /// A worker's own request, decided against the same list — but with no
    /// page to call first-party, since a worker isn't one page's alone.
    private void WorkerRequested(object? sender, CoreWebView2WebResourceRequestedEventArgs e)
    {
        if (!Enabled || Trouble != null) return;
        string raw;
        CoreWebView2WebResourceContext context;
        try { context = e.ResourceContext; raw = e.Request.Uri; }
        catch { return; }
        if (!raw.StartsWith("http", StringComparison.OrdinalIgnoreCase) || !Uri.TryCreate(raw, UriKind.Absolute, out var url)) return;
        bool refused;
        try { refused = Refuses(url, null, context); }
        catch { return; }
        if (!refused || sender is not CoreWebView2 core) return;
        e.Response = core.Environment.CreateWebResourceResponse(null, 403, "Blocked", "");
    }

    /// Whether a request from a page on `pageHost` should be refused.
    private bool Refuses(Uri url, string? pageHost, CoreWebView2WebResourceContext context)
    {
        if (list is { } rules) return rules.ShouldBlock(url, pageHost, Kind(context));
        // The built-in list: third parties only, as the Mac's rule list said
        // with its load type. A page that is itself on one of these domains
        // is that site, and its own requests are the site's.
        var host = Address.Host(url);
        return host != null && IsUnwanted(host) && Site(host) != Site(pageHost);
    }

    private static ResourceKind Kind(CoreWebView2WebResourceContext context) => context switch
    {
        // Never the page itself — that is decided before it gets here — so
        // a document is a frame's.
        CoreWebView2WebResourceContext.Document => ResourceKind.Subdocument,
        CoreWebView2WebResourceContext.Stylesheet => ResourceKind.Stylesheet,
        CoreWebView2WebResourceContext.Image => ResourceKind.Image,
        CoreWebView2WebResourceContext.Media => ResourceKind.Media,
        CoreWebView2WebResourceContext.Font => ResourceKind.Font,
        CoreWebView2WebResourceContext.Script => ResourceKind.Script,
        CoreWebView2WebResourceContext.XmlHttpRequest or CoreWebView2WebResourceContext.Fetch
            or CoreWebView2WebResourceContext.EventSource => ResourceKind.XmlHttpRequest,
        _ => ResourceKind.Other,
    };

    private bool IsUnwanted(string host)
    {
        // A trailing dot ("ads.doubleclick.net.") is the same host to DNS and
        // to every browser, but would otherwise shift every label in the walk
        // below by one and never match — a bypass free for the taking.
        host = host.TrimEnd('.');
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

    /// The part of a host a person would call the site: FishCatcher's Public
    /// Suffix List once it has loaded (alice.github.io and bob.github.io are
    /// two sites; one.com.cn and two.com.cn are two sites), the last-two-
    /// labels guess until then.
    private static string? Site(string? host) => host == null ? null : LiveRegistrableDomain.Instance.Of(host);

    // MARK: - addresses

    /// A cleaner address for `url`, or null when it is clean already:
    /// a redirect wrapper (google.com/url?q=…, l.facebook.com, t.co's
    /// cousins) opened to where it leads, and tracking tags (utm_…, fbclid,
    /// gclid…) taken off. `from` is the page a link was followed from: a
    /// site's links to itself keep their parameters — it can see where you
    /// came from anyway, and its own are the ones most likely to matter —
    /// so tags only come off on the way to another site.
    public Uri? Tidy(Uri url, Uri? from = null)
    {
        if (!Address.IsWeb(url) || Own(url) || IsPaused(Curtain.Host(url))) return null;
        var target = url;
        var changed = false;
        if (Redirects.Unwrap(url) is { } inner && Address.IsWeb(inner) && !Own(inner) && Vouched(inner))
        {
            target = inner;
            changed = true;
        }
        var crossing = from == null || changed || Site(Address.Host(from)) != Site(Address.Host(target));
        if (crossing && !IsPaused(Curtain.Host(target)) && Params.Clean(target) is { } clean && Address.IsWeb(clean))
        {
            target = clean;
            changed = true;
        }
        return changed ? target : null;
    }

    /// Search's own pages (tools.search, files.search) are never a place a
    /// tidied address may lead: going there as Search's own navigation is a
    /// door a web page must not be able to open.
    private static bool Own(Uri url) => url.Host.EndsWith(".search", StringComparison.OrdinalIgnoreCase);

    /// Whether unwrapping a redirect notice (Google's "Redirect Notice",
    /// Steam's phishing warning, YouTube's own) is safe. Those pages exist
    /// because phishers abuse exactly these open redirects, so silently
    /// skipping straight to the target throws that warning away; only do it
    /// when FishCatcher can vouch for the target itself. Off, or no
    /// verdict yet, or nothing to be concerned about (fail open, same as
    /// everywhere else FishCatcher is asked) is what "vouched" means here —
    /// anything Elevated or worse leaves the site's own warning standing.
    private static bool Vouched(Uri target) =>
        Browser.Shared is { Prefs.WarnsOfScams: true } && Catcher.Check(target)?.Level is null or FishLevel.Low;

    // MARK: - what goes into pages

    private string? pausedTest;
    private string? generic;
    private readonly Dictionary<string, string> guarded = [];
    private readonly Dictionary<string, string> siteCss = new(StringComparer.Ordinal);

    /// A test, in the page, for whether its site is paused — the site of the
    /// page on screen, even from inside one of its frames (ancestorOrigins
    /// names it) — or is one of Search's own (tools.search), which Shields
    /// leaves alone.
    private string PausedTest() => pausedTest ??=
        "(function () { var h = location.hostname; try { var a = location.ancestorOrigins; if (a && a.length) h = new URL(a[a.length - 1]).hostname; } catch (e) {} " +
        $"h = h.replace(/^www\\./, ''); return /\\.search$/.test(h) || [{string.Join(",", paused.Select(Bridge.Literal))}].indexOf(h) >= 0; }})()";

    /// A page script that steps aside on a paused site.
    private string Guarded(string name, string script)
    {
        if (guarded.TryGetValue(name, out var done)) return done;
        if (script.Length == 0) return "";
        return guarded[name] = $"(function () {{ if ({PausedTest()}) return;\n{script}\n}})();";
    }

    /// The stylesheet every page gets: the list's generic rules, or the
    /// built-in slots. Handed over once per tab — the same text for every
    /// site — and applied in the page unless the site is paused or asked for
    /// no generic rules ($generichide). Adopted rather than put in a
    /// `<style>`: that works before the document has an element to put it
    /// in, and a page can't stumble on it in its own DOM.
    private string Generic()
    {
        if (generic != null) return generic;
        var hide = list == null ? "{}" : "{" + string.Join(",", list.GenericHideSites.Select(s => Bridge.Literal(s) + ":1")) + "}";
        return generic = $$"""
        (function () {
          if ({{PausedTest()}}) return;
          var hide = {{hide}};
          for (var at = location.hostname; ; ) {
            if (hide[at] === 1) return;
            var dot = at.indexOf('.');
            if (dot < 0) break;
            at = at.slice(dot + 1);
          }
          try {
            var sheet = new CSSStyleSheet();
            sheet.replaceSync({{listLiteral ?? slotsLiteral}});
            document.adoptedStyleSheets = document.adoptedStyleSheets.concat([sheet]);
          } catch (e) {}
        })();
        """;
    }

    /// The scripts Shields puts in every page, for PageScripts.For.
    public IEnumerable<PageScript> Scripts(Preferences prefs)
    {
        if (Enabled && Trouble == null)
        {
            yield return new(Generic(), MainFrameOnly: true, AtEnd: false);
            // YouTube's ads come from YouTube's own servers, so no list can
            // block them: its player data is pruned instead. The script
            // checks for youtube.com itself; in frames too, for embeds.
            yield return new(Guarded("youtube", ShieldScripts.YouTube), MainFrameOnly: false, AtEnd: false);
        }
        // Consent dialogs often live in a frame of their own, which the
        // script recognises by its address.
        if (prefs.RejectsCookies)
            yield return new(Guarded("cookies", ShieldScripts.Cookies), MainFrameOnly: false, AtEnd: false);
        if (prefs.TidiesLinks)
            yield return new(Guarded("amp", ShieldScripts.Amp), MainFrameOnly: true, AtEnd: false);
    }

    /// What one site adds to the shared stylesheet: its own rules, and the
    /// generic ones that depend on where they are. Worked out once per site
    /// per list (under a millisecond; nothing for most sites).
    private string SiteCss(string? host)
    {
        if (host == null || list is not { } rules || !Guards(host.StartsWith("www.", StringComparison.Ordinal) ? host[4..] : host)) return "";
        if (siteCss.TryGetValue(host, out var css)) return css;
        if (siteCss.Count > 500) siteCss.Clear();
        return siteCss[host] = rules.SiteCss(host);
    }

    /// The per-site stylesheet as a page script, for that host only: a page
    /// on another site that inherits it before it is swapped leaves it be.
    /// The applied-already flag lives behind a Symbol, not a plain
    /// `__search…` name — one string a page can just ask
    /// `window.__searchShieldSite` for — so telling Search's stylesheet
    /// apart from anyone else's costs enumerating symbols, not a lookup.
    private static string SiteScript(string host, string css) => $$"""
    (function () {
      var mark = Symbol.for('search:shield-site');
      if (location.hostname !== {{Bridge.Literal(host)}} || window[mark]) return;
      Object.defineProperty(window, mark, { value: true });
      try {
        var sheet = new CSSStyleSheet();
        sheet.replaceSync({{Bridge.Literal(css)}});
        document.adoptedStyleSheets = document.adoptedStyleSheets.concat([sheet]);
      } catch (e) {}
    })();
    """;

    /// One tab's part in all this: its page's site, the per-site stylesheet
    /// it holds, and the navigation it is waiting on.
    internal sealed class Ward(Shield shield, Tab tab, CoreWebView2 core)
    {
        /// Whether the engine sends this page's requests here at all.
        public bool Filtered;

        private string? site = Curtain.Host(tab.Address);
        private string? pageHost = Address.Host(tab.Address);

        /// On one of Search's own pages, whose requests are its own.
        private bool own = tab.Address is { } there && Own(there);

        /// The top-level document on its way, whose own request is never
        /// refused: a page you asked for is a page you get.
        private Uri? top;

        private string? sheetId;
        private string sheetCss = "";
        private string? wantHost;
        private int version;

        private string? tidied;
        private long tidiedAt;

        /// The status of a redirect this Ward has just watched go by, keyed
        /// by where it leads — filled in from WebResourceResponseReceived,
        /// read back when NavigationStarting says this navigation followed a
        /// redirect. WebView2 doesn't hand NavigationStarting the status
        /// itself, only IsRedirected.
        private readonly Dictionary<string, int> redirectStatus = new(StringComparer.Ordinal);

        public void Requested(object? sender, CoreWebView2WebResourceRequestedEventArgs e)
        {
            // Timed for the bench: this is the cost every request pays.
            var started = System.Diagnostics.Stopwatch.GetTimestamp();
            try { Decide(e); }
            finally
            {
                tab.ShieldSeen++;
                tab.ShieldMs += System.Diagnostics.Stopwatch.GetElapsedTime(started).TotalMilliseconds;
            }
        }

        private void Decide(CoreWebView2WebResourceRequestedEventArgs e)
        {
            if (!shield.Enabled || shield.Trouble != null || shield.IsPaused(site) || own) return;
            string raw;
            CoreWebView2WebResourceContext context;
            try
            {
                context = e.ResourceContext;
                raw = e.Request.Uri;
            }
            catch { return; }
            if (!raw.StartsWith("http", StringComparison.OrdinalIgnoreCase)) return;
            var document = context == CoreWebView2WebResourceContext.Document;
            if (document && IsTop(raw)) return;
            if (!Uri.TryCreate(raw, UriKind.Absolute, out var url)) return;
            bool refused;
            try { refused = shield.Refuses(url, pageHost, context); }
            catch { return; }
            if (!refused) return;
            if (!document)
            {
                Refuse(e);
                return;
            }
            // A document the list would refuse is a frame's — unless it is
            // the page itself and NavigationStarting just hasn't been heard
            // yet. The answer waits one turn of the UI thread for it.
            var deferral = e.GetDeferral();
            UI.Soon(() =>
            {
                try { if (!IsTop(raw)) Refuse(e); }
                catch { }
                deferral.Complete();
            });
        }

        private bool IsTop(string raw) =>
            top != null && Uri.TryCreate(raw, UriKind.Absolute, out var url)
            && Uri.Compare(top, url, UriComponents.HttpRequestUrl, UriFormat.UriEscaped, StringComparison.OrdinalIgnoreCase) == 0;

        private void Refuse(CoreWebView2WebResourceRequestedEventArgs e)
        {
            e.Response = core.Environment.CreateWebResourceResponse(null, 403, "Blocked", "");
            tab.Blocked++;
        }

        public void Starting(object? sender, CoreWebView2NavigationStartingEventArgs e)
        {
            // Cancelled already: an extension's sign-in coming back, a link
            // handed to another app, a tool page a web page may not open.
            if (e.Cancel || !Uri.TryCreate(e.Uri, UriKind.Absolute, out var url)) return;

            // A cleaner address means this navigation gives way to one to
            // it. Only a new document: going back, or reloading, goes where
            // it went before.
            if (e.NavigationKind == CoreWebView2NavigationKind.NewDocument && Tidied(url, e) is { } clean)
            {
                e.Cancel = true;
                UI.Soon(() => Browser.Shared?.Go(tab, clean));
                return;
            }

            top = url;
            site = Curtain.Host(url);
            pageHost = Address.Host(url);
            own = Own(url);
            tab.Blocked = 0;
            tab.ShieldSeen = 0;
            tab.ShieldMs = 0;
            if (redirectStatus.Count > 0) redirectStatus.Clear();
            Dress(url);
        }

        /// Watches redirect responses go by so Starting can tell a 307/308
        /// apart from an ordinary one — NavigationStarting itself is only
        /// told IsRedirected, never the status.
        public void ResponseReceived(object? sender, CoreWebView2WebResourceResponseReceivedEventArgs e)
        {
            try
            {
                var status = e.Response.StatusCode;
                if (status is not (307 or 308)) return;
                var location = e.Response.Headers.GetHeader("Location");
                if (location == null || !Uri.TryCreate(e.Request.Uri, UriKind.Absolute, out var from)) return;
                if (!Uri.TryCreate(from, location, out var to)) return;
                if (redirectStatus.Count > 50) redirectStatus.Clear();
                redirectStatus[to.AbsoluteUri] = status;
            }
            catch { }
        }

        /// The address this navigation should have had instead, if any.
        /// Never for a navigation that isn't a plain GET — a form's POST, or
        /// a 307/308 redirect that must replay one — since Browser.Go always
        /// starts a fresh, bodyless GET (SearchKit.Shields.TidyDecision).
        private Uri? Tidied(Uri url, CoreWebView2NavigationStartingEventArgs e)
        {
            if (Browser.Shared is not { Prefs.TidiesLinks: true }) return null;
            bool hasBody;
            try { hasBody = e.RequestHeaders.Contains("Content-Type"); } catch { hasBody = false; }
            var status = e.IsRedirected && redirectStatus.TryGetValue(url.AbsoluteUri, out var s) ? s : 0;
            if (!TidyDecision.CanTidy(hasBody, status)) return null;
            Uri.TryCreate(core.Source, UriKind.Absolute, out var from);
            if (shield.Tidy(url, Address.IsWeb(from) ? from : null) is not { } clean) return null;
            // A site that sends the clean address straight back to the
            // tagged one wants it that way; the second time, it has it.
            var now = Environment.TickCount64;
            if (tidied == clean.AbsoluteUri && now - tidiedAt < 10_000) return null;
            tidied = clean.AbsoluteUri;
            tidiedAt = now;
            return clean;
        }

        /// The per-site stylesheet for the page at `url` goes in with it. The
        /// new one is added before the old one goes, so no document starts
        /// between them with neither.
        public void Dress(Uri? url)
        {
            var host = Address.IsWeb(url) && !Own(url!) ? Address.Host(url) : null;
            var css = shield.SiteCss(host);
            wantHost = css.Length > 0 ? host : null;
            if (css == sheetCss) return;
            sheetCss = css;
            _ = Swap(host, css);
        }

        private async Task Swap(string? host, string css)
        {
            var mine = ++version;
            string? id = null;
            if (host != null && css.Length > 0)
            {
                try { id = await core.AddScriptToExecuteOnDocumentCreatedAsync(Bridge.Wrap(SiteScript(host, css), mainFrameOnly: true, atEnd: false)); }
                catch { }
            }
            if (mine != version)
            {
                // Overtaken by a newer page while it went in.
                if (id != null) Remove(id);
                return;
            }
            if (sheetId != null) Remove(sheetId);
            sheetId = id;
        }

        private void Remove(string id)
        {
            try { core.RemoveScriptToExecuteOnDocumentCreated(id); } catch { }
        }

        /// The per-site stylesheet again, from outside the document, the
        /// moment it starts arriving — for the page that came faster than
        /// its stylesheet could be added. Harmless when it was in time: the
        /// script does nothing twice.
        public void Loading(object? sender, CoreWebView2ContentLoadingEventArgs e)
        {
            if (e.IsErrorPage || wantHost == null || sheetCss.Length == 0) return;
            try { _ = core.ExecuteScriptAsync(SiteScript(wantHost, sheetCss)); } catch { }
        }

        /// Shields changed under an open page: its stylesheet follows. The
        /// same stylesheet for the same site is the same script, so only a
        /// different one is swapped in.
        public void Refresh() =>
            Dress(Uri.TryCreate(core.Source, UriKind.Absolute, out var here) && Address.IsWeb(here) ? here : tab.Address);
    }
}

public sealed partial class Tab
{
    /// Shields' hold on this tab's page (see Shield.Ward).
    internal Shield.Ward? Ward { get; set; }

    /// Requests of this page that crossed over to be decided, and the time
    /// spent deciding them on the UI thread — for the bench.
    internal int ShieldSeen { get; set; }
    internal double ShieldMs { get; set; }
}

/// Shields' page scripts, as shipped in Search/Assets/js: read from the
/// executable the first time a page is armed, which is after the first
/// window.
public static class ShieldScripts
{
    private static string? youTube, cookies, amp;

    public static string YouTube => youTube ??= Load("js/youtube-shield.js");
    public static string Cookies => cookies ??= Load("js/cookie-reject.js");
    public static string Amp => amp ??= Load("js/amp-canonical.js");

    /// What amp-canonical.js posts: the page's real address.
    public const string AmpName = "shield.amp";

    private static string Load(string name)
    {
        try
        {
            using var stream = typeof(ShieldScripts).Assembly.GetManifestResourceStream(name);
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
    partial void StartShield()
    {
        Shield.Shared.Enabled = Prefs.Shielded;
        Shield.Shared.Compile();
        Tab.Tidy = url => Prefs.TidiesLinks ? Shield.Shared.Tidy(url) : null;
        Bridge.Handlers[ShieldScripts.AmpName] = LeaveAmp;

        // Shields on or off, a site paused, the lists in: every page already
        // open hears it, from its next request and its next document.
        Shield.Shared.Rearm += () =>
        {
            foreach (var tab in Tabs.Concat(ParkedTabs))
            {
                tab.Arm(Curtain.Css(Curtain.Host(tab.Address)), force: true);
                if (tab.Core is { } core) Shield.Shared.Tune(tab, core);
                tab.Ward?.Refresh();
            }
        };
        Prefs.On(nameof(Preferences.TidiesLinks), RearmAll);
        Prefs.On(nameof(Preferences.RejectsCookies), RearmAll);
        Prefs.On(nameof(Preferences.ShieldLists), () =>
        {
            if (Prefs.ShieldLists) ShieldLists.Refresh(force: true);
            else ShieldLists.Forget();
        });
        ShieldLists.Schedule();
    }

    private void RearmAll()
    {
        foreach (var tab in Tabs.Concat(ParkedTabs))
            tab.Arm(Curtain.Css(Curtain.Host(tab.Address)), force: true);
    }

    partial void AttachShield(Tab tab, CoreWebView2 core) => Shield.Shared.Protect(tab, core);

    /// An AMP page said where its real one is (amp-canonical.js). The page
    /// replaces itself with it, so Back doesn't land on the AMP page and
    /// bounce straight forward again; and only once per AMP page, in case the
    /// real one sends phones back.
    private void LeaveAmp(Tab tab, System.Text.Json.JsonElement body)
    {
        if (!Prefs.TidiesLinks || body.ValueKind != System.Text.Json.JsonValueKind.Object) return;
        if (!body.TryGetProperty("canonical", out var said) || said.ValueKind != System.Text.Json.JsonValueKind.String) return;
        if (!Uri.TryCreate(said.GetString(), UriKind.Absolute, out var canonical) || !Address.IsWeb(canonical)) return;
        if (!Uri.TryCreate(tab.Core?.Source, UriKind.Absolute, out var here) || !Address.IsWeb(here)) return;
        // The message names the page that sent it (its own address, not the
        // canonical). A page that posted this and has since navigated away —
        // this tab may already be showing something else by the time the
        // message is handled — must not get to replace() whatever loaded
        // after it with its own idea of where it should have gone.
        if (!body.TryGetProperty("href", out var sender) || sender.ValueKind != System.Text.Json.JsonValueKind.String) return;
        if (!string.Equals(sender.GetString(), here.AbsoluteUri, StringComparison.Ordinal)) return;
        if (Shield.Shared.IsPaused(Curtain.Host(here)) || canonical.Host.EndsWith(".search", StringComparison.OrdinalIgnoreCase)) return;
        if (tab.LeftAmp == here.AbsoluteUri) return;
        tab.LeftAmp = here.AbsoluteUri;
        var target = Shield.Shared.Tidy(canonical) ?? canonical;
        tab.Run($"location.replace({Bridge.Literal(target.AbsoluteUri)})");
    }
}

public sealed partial class Tab
{
    /// The AMP page this tab last left for its real one.
    internal string? LeftAmp { get; set; }
}
