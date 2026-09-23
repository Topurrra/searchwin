using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.Web.WebView2.Core;

namespace Search;

// One web view per tab, kept alive for as long as the tab is. Switching tabs
// hides the old view and shows the new one — the page does not reload, does
// not lose its scroll position, and does not forget what you typed into it.
// That is the whole trick behind switching feeling instant.
//
// Kept alive, that is, while it is worth what it costs. A tab nobody has
// looked at for half an hour is suspended (see Browser.Sleep): its renderer
// stops, gives most of its memory back, and wakes where it was.

/// Whoever handles navigation and windows for a page, applied when the page
/// is built, whenever that is.
public interface IPageHost
{
    void Attach(Tab tab, CoreWebView2 core);
}

public sealed class Tab : Model
{
    public Guid Id { get; } = Guid.NewGuid();

    /// A tab that keeps nothing: its own cookies, no history, no place in the
    /// session. Signed in as nobody, and forgotten when it goes.
    public bool Shy { get; }

    /// A tab a script opened through the bench, beside yours. Never selected
    /// for you, never in the session or the history.
    public bool Bench { get; }

    /// The WebView2 profile its cookies and sign-ins live in.
    public string Profile { get; }

    public IPageHost? Host { get; set; }

    public Tab(bool shy = false, bool bench = false, string? profile = null)
    {
        Shy = shy;
        Bench = bench;
        Profile = profile ?? Space.Profile(Space.Current);
    }

    // MARK: - the page

    private WebView2? built;
    private bool ready;
    private readonly List<Action<CoreWebView2>> waiting = [];

    /// The page. Built the first time anyone asks for it, not when the tab is —
    /// a session of twenty tabs coming back is twenty objects, not twenty
    /// renderers fighting the first frame.
    public WebView2 Web => built ??= Build();

    /// The view if there is one yet, for callers that must not be the reason
    /// there is.
    public WebView2? Built => built;

    /// The engine behind the view, once it has started.
    public CoreWebView2? Core => ready ? built?.CoreWebView2 : null;

    /// Raised on the UI thread when the page has started, for anyone who wants
    /// to hear from it from the beginning.
    public event Action<Tab, CoreWebView2>? Started;

    /// Runs `act` against the engine now if it is running, or the moment it is.
    public void WhenReady(Action<CoreWebView2> act)
    {
        if (Core is { } core) { act(core); return; }
        _ = Web;
        waiting.Add(act);
    }

    private WebView2 Build()
    {
        // Visible from the start, and underneath whatever is showing: WebView2
        // started while collapsed, or at no opacity, can come up never drawing
        // at all. It is hidden once it is running, if it isn't the live one.
        var view = new WebView2();
        unpainted = true;
        view.PointerPressed += (_, _) => Uncover();
        view.PointerWheelChanged += (_, _) => Uncover();
        Search.Web.Stage.Children.Insert(0, view);
        _ = Start(view);
        return view;
    }

    private async Task Start(WebView2 view)
    {
        try
        {
            var env = await Search.Web.Environment;
            var options = env.CreateCoreWebView2ControllerOptions();
            options.ProfileName = Profile;
            options.IsInPrivateModeEnabled = Shy;
            await view.EnsureCoreWebView2Async(env, options);
        }
        catch (Exception e)
        {
            // Closed before it started, or the runtime is missing. Neither is
            // anything a tab can recover from on its own.
            System.Diagnostics.Debug.WriteLine($"page didn't start: {e.Message}");
            return;
        }
        if (built != view) return; // let go of while starting
        var core = view.CoreWebView2;
        ready = true;
        Configure(core);
        // The first page must not arrive before the scripts that watch it.
        await arming;
        if (built != view) return;
        Show(shown);
        Host?.Attach(this, core);
        Started?.Invoke(this, core);
        var queued = waiting.ToArray();
        waiting.Clear();
        foreach (var act in queued) act(core);
    }

    private void Configure(CoreWebView2 core)
    {
        var settings = core.Settings;
        // The Web Inspector, on the keys Chrome and Edge use (see Inspector).
        settings.AreDevToolsEnabled = true;
        settings.IsStatusBarEnabled = false;
        settings.IsZoomControlEnabled = true;
        settings.IsPinchZoomEnabled = true;
        settings.IsSwipeNavigationEnabled = true;
        settings.IsGeneralAutofillEnabled = false;
        // Passwords are the browser's own (see Vault), never Edge's.
        settings.IsPasswordAutosaveEnabled = false;
        settings.IsReputationCheckingRequired = false;
        settings.AreBrowserAcceleratorKeysEnabled = true;
        settings.AreDefaultContextMenusEnabled = true;
        // The app's own find bar stands in for Edge's (see FindBar).
        core.Profile.PreferredColorScheme = Palette.Dark ? CoreWebView2PreferredColorScheme.Dark : CoreWebView2PreferredColorScheme.Light;

        core.DocumentTitleChanged += (_, _) => Title = core.DocumentTitle ?? "";
        core.SourceChanged += (_, _) =>
        {
            if (!Uri.TryCreate(core.Source, UriKind.Absolute, out var fresh)) return;
            // about:blank is never a destination. Letting it overwrite the
            // address is how a tab put down loses the only thing that could
            // bring it back.
            if (fresh.AbsoluteUri == "about:blank") return;
            var moved = Search.Address.Host(fresh) != Search.Address.Host(Address);
            Address = fresh;
            if (moved) AdoptIcon();
        };
        core.HistoryChanged += (_, _) =>
        {
            CanGoBack = core.CanGoBack;
            CanGoForward = core.CanGoForward;
        };
        core.NavigationStarting += (_, e) =>
        {
            Loading = true;
            Progress = 0.1;
        };
        core.ContentLoading += (_, _) => Progress = 0.5;
        core.DOMContentLoaded += (_, _) =>
        {
            Progress = 0.8;
            ShowFirstFrame();
        };
        core.NavigationCompleted += (_, _) =>
        {
            Loading = false;
            Progress = 1;
            ShowFirstFrame();
        };
        core.IsDocumentPlayingAudioChanged += (_, _) => Noisy = core.IsDocumentPlayingAudio;
        core.ContainsFullScreenElementChanged += (_, _) => Immersed = core.ContainsFullScreenElement;
        core.FaviconChanged += (_, _) => Favicons.Shared.Fetch(this);
        core.WebMessageReceived += (_, e) => Received(e);

        Arm(veils);
    }

    // MARK: - what the row shows

    private string title = "";
    public string Title { get => title; private set { if (Set(ref title, value)) Tell(nameof(Label)); } }

    private Uri? address;
    public Uri? Address
    {
        get => address;
        private set
        {
            if (!Set(ref address, value)) return;
            Tell(nameof(IsBlank));
            Tell(nameof(Label));
        }
    }

    private double progress;
    public double Progress { get => progress; private set => Set(ref progress, value); }

    private bool loading;
    public bool Loading { get => loading; private set => Set(ref loading, value); }

    private bool canGoBack, canGoForward;
    public bool CanGoBack { get => canGoBack; private set => Set(ref canGoBack, value); }
    public bool CanGoForward { get => canGoForward; private set => Set(ref canGoForward, value); }

    private string? failure;
    /// Set when the page never arrived — no host, no network, a refused
    /// connection. Shown in place of the page rather than in a dialog.
    public string? Failure { get => failure; set => Set(ref failure, value); }

    private double reading;
    /// How far down the page you are, nought to one. The tab's own pill fills
    /// with it.
    public double Reading { get => reading; set => Set(ref reading, value); }

    private bool reader;
    /// True while the page has been stripped back to its article.
    public bool Reader { get => reader; set => Set(ref reader, value); }

    private ImageSource? icon;
    /// The site's icon, for tabs set to wear one. From the cache the moment
    /// the tab has an address, and from the page a moment after it loads.
    public ImageSource? Icon { get => icon; set => Set(ref icon, value); }

    private bool typing;
    /// True while the caret is in something on the page that takes typing.
    public bool Typing { get => typing; set => Set(ref typing, value); }

    private bool immersed;
    /// True while the page has taken over the screen.
    public bool Immersed { get => immersed; set => Set(ref immersed, value); }

    private bool floating;
    /// True while this tab's page is out in the little window.
    public bool Floating { get => floating; set => Set(ref floating, value); }

    private bool noisy;
    /// True while something on the page is making noise, so the row can say
    /// which tab it is coming from.
    public bool Noisy { get => noisy; set => Set(ref noisy, value); }

    private string? pin;
    /// One letter, when the tab has been pinned. A pinned tab keeps its place
    /// at the head of the row and gives up its title for that letter.
    public string? Pin { get => pin; set => Set(ref pin, value); }

    private string? name;
    /// A name you gave it, in place of whatever the page calls itself. It
    /// stays through navigation: a tab you named is a tab you are keeping for
    /// a job, not for a page.
    public string? Name { get => name; set { if (Set(ref name, value)) Tell(nameof(Label)); } }

    private string? storePlaced;
    /// The extension whose store page has its own "Add to Search" button in
    /// place — so the bar at the bottom doesn't offer it twice.
    public string? StorePlaced { get => storePlaced; set => Set(ref storePlaced, value); }

    private ImageSource? cover;
    /// A picture of the page as it was left, over the stage while it wakes.
    public ImageSource? Cover { get => cover; private set => Set(ref cover, value); }

    /// The tab whose page opened this one, when a script did. Sign-in flows
    /// hand you back to it when they are done.
    public Guid? Opener { get; set; }

    /// When you last looked at it. The switcher lists pages by this.
    public DateTime Touched { get; private set; } = DateTime.UtcNow;
    public void Touch() => Touched = DateTime.UtcNow;

    /// A tab that has never been anywhere shows the address field instead of a
    /// page.
    public bool IsBlank => Address == null;

    /// The title if the page has offered one, the address until it does.
    public string Label
    {
        get
        {
            if (!string.IsNullOrEmpty(Name)) return Name!;
            if (Title.Length > 0) return Title;
            if (Address != null) return Search.Address.Pretty(Address);
            return "New Tab";
        }
    }

    /// The letter a pinned tab is reduced to, and what a tab shows in place of
    /// an icon it doesn't have yet.
    public string Monogram
    {
        get
        {
            var host = Search.Address.Host(Address)?.Replace("www.", "") ?? "";
            return host.Length > 0 ? char.ToUpperInvariant(host[0]).ToString() : "•";
        }
    }

    private void AdoptIcon()
    {
        if (Search.Address.Host(Address) is { } host) Icon = Favicons.Shared.Cached(host);
    }

    // MARK: - going places

    public void Go(Uri url)
    {
        // Set straight away rather than waiting for the engine: the tab has to
        // stop being blank in the same frame the field disappears, or the empty
        // state flashes back for an instant on its way out.
        Address = url;
        Title = "";
        Failure = null;
        Reading = 0;
        Reader = false;
        Typing = false;
        Immersed = false;
        Pending = null;
        Cover = null;
        AdoptIcon();
        WhenReady(core => core.Navigate(url.AbsoluteUri));
    }

    private Uri? pending;
    /// Set on a tab brought back from the last session and not yet opened. It
    /// has a name and an address in the row, and costs nothing until you go to
    /// it — the difference between a browser that starts in half a second with
    /// twenty tabs and one that doesn't.
    public Uri? Pending { get => pending; private set { if (Set(ref pending, value)) Tell(nameof(Asleep)); } }

    /// True for a tab that has a place and an address but is holding no page —
    /// brought back from the last session, or put down with Ctrl+W while
    /// pinned.
    public bool Asleep => Pending != null;

    /// Brought back from the last session: everything the row needs to draw
    /// it, and nothing fetched.
    public void Restore(Uri url, string title, string? name = null)
    {
        Address = url;
        Title = title;
        Name = name;
        Pending = url;
        AdoptIcon();
    }

    /// A tab opened by a link is not blank, even though the engine hasn't
    /// started loading it yet.
    public void SetAddressOptimistically(Uri url)
    {
        Address = url;
        Failure = null;
        AdoptIcon();
    }

    /// Opened for the first time since the app started, or coming back from
    /// Ctrl+W while pinned. Answers whether there was anything to wake.
    public bool Wake()
    {
        if (Pending is not { } url) return false;
        Pending = null;
        Failure = null;
        Reading = 0;
        Reader = false;
        Typing = false;
        Immersed = false;
        WhenReady(core => core.Navigate(url.AbsoluteUri));
        return true;
    }

    /// Ctrl+W on a pinned tab. The letter keeps its place in the row and the
    /// address is remembered; everything the page was holding is let go, so a
    /// pin you are not reading costs a line in a file and nothing else.
    public void Rest()
    {
        if (Address is not { } url) return;
        Pending = url;
        Reading = 0;
        Noisy = false;
        Stale = false;
        Discard();
    }

    // MARK: - sleeping

    private bool dozing;
    /// Suspended for not being looked at: its renderer stopped and its memory
    /// mostly given back, its page exactly where it was.
    public bool Dozing { get => dozing; private set => Set(ref dozing, value); }

    /// Nobody has looked at this page for a while. WebView2 can do what the
    /// Mac had to fake: freeze the page where it is and hand back its memory,
    /// then thaw it untouched — back list, scroll, half-typed form and all.
    public async Task<bool> Doze()
    {
        if (Core is not { } core || built == null) return false;
        if (built.Visibility == Visibility.Visible) return false;
        try
        {
            core.MemoryUsageTargetLevel = CoreWebView2MemoryUsageTargetLevel.Low;
            var worked = await core.TrySuspendAsync();
            Dozing = worked;
            return worked;
        }
        catch { return false; }
    }

    /// Coming back to a dozing tab: showing it resumes it, and it is told to
    /// use what memory it wants again.
    public void Rouse()
    {
        if (!Dozing || Core is not { } core) return;
        Dozing = false;
        try
        {
            core.MemoryUsageTargetLevel = CoreWebView2MemoryUsageTargetLevel.Normal;
            if (core.IsSuspended) core.Resume();
        }
        catch { }
    }

    /// Whether the page holds something typed and not yet sent — a draft, a
    /// half-filled form. A page that can't answer holds nothing.
    public async Task<bool> Unsaved()
    {
        var answer = await Eval("!!(window.__officeForms && window.__officeForms.unsaved && window.__officeForms.unsaved())");
        return answer is { ValueKind: JsonValueKind.True };
    }

    // MARK: - showing

    private bool unpainted;
    private bool shown;
    private static int lifted;

    /// In, quickly: the page is there.
    private void ShowFirstFrame()
    {
        if (!unpainted || built == null) return;
        unpainted = false;
        Uncover(0.45);
    }

    /// Put on the stage or taken off it. Only the live tab's view is visible;
    /// the rest keep their pages, hidden.
    public void Show(bool on)
    {
        shown = on;
        if (built == null || !ready) return;
        if (on)
        {
            Rouse();
            built.Visibility = Visibility.Visible;
            // The live page on top of any other still starting underneath.
            Microsoft.UI.Xaml.Controls.Canvas.SetZIndex(built, ++lifted);
        }
        else
        {
            built.Visibility = Visibility.Collapsed;
        }
    }

    /// The picture comes off the moment there is something better under it
    /// — the page, painted — or you reach for the page yourself.
    public void Uncover(double delay = 0)
    {
        if (Cover is not { } shown) return;
        if (delay <= 0) { Cover = null; return; }
        UI.After(delay, () => { if (Cover == shown) Cover = null; });
    }

    // MARK: - asking the page

    /// Runs a script in the page and hands back what it returned, parsed.
    /// Null when there is no page, or the page couldn't answer.
    public async Task<JsonElement?> Eval(string script)
    {
        if (Core is not { } core) return null;
        try
        {
            var json = await core.ExecuteScriptAsync(script);
            if (string.IsNullOrEmpty(json) || json == "null") return null;
            using var doc = JsonDocument.Parse(json);
            return doc.RootElement.Clone();
        }
        catch { return null; }
    }

    /// Fire and forget.
    public void Run(string script) => WhenReady(core => _ = core.ExecuteScriptAsync(script));

    private void Received(CoreWebView2WebMessageReceivedEventArgs e)
    {
        JsonElement message;
        try
        {
            using var doc = JsonDocument.Parse(e.WebMessageAsJson);
            message = doc.RootElement.Clone();
        }
        catch { return; }
        if (message.ValueKind != JsonValueKind.Object || !message.TryGetProperty("name", out var nameEl)) return;
        var body = message.TryGetProperty("body", out var b) ? b : default;
        var which = nameEl.GetString() ?? "";
        if (which == Scroll.Name) { Scroll.Take(this, body); return; }
        if (Bridge.Handlers.TryGetValue(which, out var handle)) handle(this, body);
    }

    // MARK: - scripts

    private string veils = "";
    private double armedZoom = 1;
    private bool armed;
    private readonly List<string> scriptIds = [];

    /// What gets injected into the *next* document: the scroll reporter, the
    /// pointing mode, and this site's stylesheet of things you have hidden. The
    /// stylesheet goes in before the document has a body, so nothing is ever
    /// seen arriving and then leaving again.
    public void Arm(string css, double? zoom = null, bool force = false)
    {
        // Scripts go in asynchronously; swapping them for the same ones on
        // every navigation would only open a gap a new document could slip
        // through with none.
        if (!force && armed && css == veils && (zoom ?? armedZoom) == armedZoom) return;
        armed = Core != null;
        veils = css;
        if (zoom is { } z) armedZoom = z;
        if (Core is not { } core) return;
        foreach (var id in scriptIds) core.RemoveScriptToExecuteOnDocumentCreated(id);
        scriptIds.Clear();
        var all = new List<PageScript> { new(Bridge.Prelude, false, false), new(Scroll.Script, true, true) };
        all.AddRange(PageScripts.For(this, css));
        // Whatever you last set this site to, before it draws a single frame
        // at the wrong size.
        Zoom = armedZoom;
        all.Add(new PageScript(Zooming.AtStart(armedZoom), true, false));
        var adding = new List<Task>();
        foreach (var script in all)
        {
            if (string.IsNullOrWhiteSpace(script.Source)) continue;
            var source = script.Source == Bridge.Prelude ? script.Source : Bridge.Wrap(script.Source, script.MainFrameOnly, script.AtEnd);
            adding.Add(Add(core, source));
        }
        arming = Task.WhenAll(adding);
    }

    private Task arming = Task.CompletedTask;

    private async Task Add(CoreWebView2 core, string source)
    {
        try { scriptIds.Add(await core.AddScriptToExecuteOnDocumentCreatedAsync(source)); }
        catch { }
    }

    // MARK: - zoom

    private double zoom = 1;
    /// How much bigger the page is being drawn, as the page itself reports it.
    public double Zoom { get => zoom; set => Set(ref zoom, value); }

    public Action<Tab, double>? OnZoom;

    /// The engine's own zoom on top of that, as the page last reported it.
    public double NativeZoom { get; set; } = 1;

    // MARK: - the rest of the controls

    /// Set when the renderer went away while nobody was looking at the tab.
    /// Coming back to it loads the page again rather than showing the white
    /// that is left.
    public bool Stale { get; set; }

    /// The renderer behind this page just died. Asking for the address again
    /// is the one thing that doesn't depend on anything the crash took with it.
    public void RecoverFromCrash()
    {
        if (Address is not { } url) return;
        Failure = null;
        if (Core is { } core) core.Navigate(url.AbsoluteUri);
    }

    /// Coming back to a tab whose page quietly died while you were elsewhere.
    public void Revive()
    {
        if (!Stale) return;
        Stale = false;
        RecoverFromCrash();
    }

    /// Again from the network. A pin put down with Ctrl+W has no view left to
    /// reload; waking it is the reload.
    public void Reload()
    {
        if (Wake()) return;
        if (Core is not { } core) return;
        if (core.Source == "about:blank" && Address is { } url) core.Navigate(url.AbsoluteUri);
        else core.Reload();
    }

    public void Stop() => Core?.Stop();
    public void Back() { if (Core is { CanGoBack: true } core) core.GoBack(); }
    public void Forward() { if (Core is { CanGoForward: true } core) core.GoForward(); }

    /// Called when the tab is thrown away. Without it the view keeps running
    /// whatever the page left behind — timers, video, sockets.
    public void Close()
    {
        OnZoom = null;
        Discard();
    }

    /// The view and everything listening to it, gone. The tab keeps its
    /// address; `Web` builds again the next time anyone asks for it.
    private void Discard()
    {
        var view = built;
        built = null;
        ready = false;
        waiting.Clear();
        scriptIds.Clear();
        armed = false;
        Dozing = false;
        if (view == null) return;
        Search.Web.Stage.Children.Remove(view);
        try { view.Close(); } catch { }
    }
}

/// Carries the page's scroll position back to its tab — and the page's zoom,
/// which it knows better than anyone.
public static class Scroll
{
    public const string Name = "officeScroll";

    /// Reports at most once a frame, and passively, so a page that scrolls
    /// smoothly without us keeps scrolling smoothly with us.
    public const string Script = """
    (function () {
      var waiting = false;
      function tell(first) {
        var root = document.documentElement;
        var y = window.scrollY || root.scrollTop || 0;
        var ceiling = Math.max(1, (root.scrollHeight || 0) - window.innerHeight);
        window.__officePost('officeScroll', { y: y, max: ceiling, dpr: window.devicePixelRatio, first: !!first });
      }
      window.addEventListener('scroll', function () {
        if (waiting) return;
        waiting = true;
        requestAnimationFrame(function () { waiting = false; tell(); });
      }, { passive: true });
      window.addEventListener('resize', function () {
        if (waiting) return;
        waiting = true;
        requestAnimationFrame(function () { waiting = false; tell(); });
      }, { passive: true });
      tell(true);
    })();
    """;

    /// The scale Windows draws the window at, so the page's pixel ratio can
    /// be read as the zoom it is.
    public static double Scale { get; set; } = 1;

    public static void Take(Tab tab, JsonElement body)
    {
        if (body.ValueKind != JsonValueKind.Object) return;
        if (body.TryGetProperty("y", out var y) && body.TryGetProperty("max", out var max))
        {
            var ceiling = max.GetDouble();
            tab.Reading = ceiling > 0 ? Math.Clamp(y.GetDouble() / ceiling, 0, 1) : 0;
        }
        // The engine's own zoom — Ctrl and the wheel — shows only as the page's
        // pixel ratio changing under it. The first report of each page is
        // where it starts; a change after that is somebody zooming, and the
        // line at the bottom says how far.
        if (body.TryGetProperty("dpr", out var dpr) && dpr.ValueKind == JsonValueKind.Number)
        {
            var native = Math.Round(dpr.GetDouble() / Math.Max(0.5, Scale), 2);
            var first = body.TryGetProperty("first", out var f) && f.ValueKind == JsonValueKind.True;
            if (!first && Math.Abs(native - tab.NativeZoom) > 0.004) tab.OnZoom?.Invoke(tab, native * tab.Zoom);
            tab.NativeZoom = native;
        }
    }
}
