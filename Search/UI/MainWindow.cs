using System.Numerics;
using Microsoft.UI;
using Microsoft.UI.Input;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.Graphics;
using Windows.UI.ViewManagement;

namespace Search;

// A window, a row of titles, and a field. Typing an address gets you a page;
// there is nothing else to learn and nothing else to press.
public sealed class MainWindow : Window
{
    private readonly Browser browser;
    private readonly Grid root = new();
    private readonly Stage stage;
    private readonly TabBar tabBar;
    private readonly SideBar sideBar;
    private readonly Press foldEdge = new() { Width = 6, HorizontalAlignment = HorizontalAlignment.Left };
    private readonly Omnibox omnibox;
    private readonly Grid panels = new();
    private readonly Bars bars;
    private readonly UISettings system = new();
    private Later? leaving;
    private Later? arriving;
    private bool insideSide;

    public Browser Browser => browser;

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hwnd);

    /// Brought to the front, over whatever app sent the link.
    public void Foreground()
    {
        if (App.Presenter?.State == OverlappedPresenterState.Minimized) App.Presenter.Restore();
        SetForegroundWindow(WinRT.Interop.WindowNative.GetWindowHandle(this));
    }

    public MainWindow()
    {
        Title = "Search";
        App.Root = root;
        // The stage exists before the first tab does: a tab restored from last
        // time is built the moment the browser is.
        Web.Stage = new Grid();
        browser = new Browser();
        Shortcuts.Browser = browser;

        ExtendsContentIntoTitleBar = true;
        root.Background = Palette.Ground;
        Content = root;

        stage = new Stage(browser);
        root.Children.Add(stage);

        tabBar = new TabBar(browser);
        root.Children.Add(tabBar);

        // The column, the fold's edge, and the column slid out over the page
        // while it is folded (see Fold.swift on the Mac).
        sideBar = new SideBar(browser);
        Motion.Glides(sideBar);
        root.Children.Add(sideBar);
        foldEdge.Hovered += over => { if (over) Arrive(); else Pass(); };
        root.Children.Add(foldEdge);
        sideBar.PointerEntered += (_, _) => { insideSide = true; Peek(true); };
        sideBar.PointerExited += (_, _) => { insideSide = false; if (Folding) Peek(false); };

        bars = new Bars(browser);
        root.Children.Add(bars);

        omnibox = new Omnibox(browser);
        root.Children.Add(omnibox);

        root.Children.Add(panels);

        // Every shortcut, from the page or from the browser's own fields alike
        // (see KeyHook).
        KeyHook.Start(this);
        root.PointerPressed += (_, e) =>
        {
            // The mouse's own back and forward buttons.
            var props = e.GetCurrentPoint(root).Properties;
            if (props.IsXButton1Pressed) { browser.Back(); e.Handled = true; }
            if (props.IsXButton2Pressed) { browser.Forward(); e.Handled = true; }
        };

        browser.OnAny(Changed);
        browser.Prefs.OnAny(name =>
        {
            if (name is nameof(Preferences.Sidebar) or nameof(Preferences.SideWidth) or nameof(Preferences.SideHides) or nameof(Preferences.UsesSpaces)) Arrange();
            if (name == nameof(Preferences.Look)) Relook();
        });
        system.ColorValuesChanged += (_, _) => UI.Do(() => { if (browser.Prefs.Look == Look.System) Relook(); });
        root.SizeChanged += (_, _) => Regions();
        // The display's own scale, so a page's pixel ratio can be read as the
        // zoom it is (see Scroll).
        root.Loaded += (_, _) =>
        {
            Scroll.Scale = root.XamlRoot.RasterizationScale;
            root.XamlRoot.Changed += (r, _) => Scroll.Scale = r.RasterizationScale;
        };
        // Whatever moved — a column coming in, a row added, a tab reflowing —
        // the title bar's shape follows. Coalesced to once a frame, and only
        // handed to Windows when it actually changed.
        root.LayoutUpdated += (_, _) => Regions();

        Dress();
        Relook();
        Arrange();
        ShowField();
        ShowPanels();
        Closed += (_, _) => Leave();
    }

    // MARK: - the window

    private void Dress()
    {
        var window = AppWindow;
        window.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "Search.ico"));
        if (window.Presenter is OverlappedPresenter presenter)
        {
            App.Presenter = presenter;
            // No title bar and none of Windows' own buttons: the strip is the
            // title bar, and the three buttons are the app's (see
            // WindowButtons). The border and its corners stay.
            presenter.SetBorderAndTitleBar(true, false);
            presenter.PreferredMinimumWidth = 640;
            presenter.PreferredMinimumHeight = 420;
        }
        window.Changed += (_, e) =>
        {
            if (e.DidPresenterChange || e.DidSizeChange) App.WindowChanged?.Invoke();
            if (e.DidSizeChange || e.DidPositionChange) Regions();
        };

        // Where you left it, at the size you left it.
        var saved = Store.Settings.String("window.frame")?.Split(',').Select(s => int.TryParse(s, out var n) ? n : int.MinValue).ToArray();
        if (saved is { Length: 4 } f && f.All(n => n != int.MinValue) && f[2] >= 640 && f[3] >= 420 && OnScreen(f))
            window.MoveAndResize(new RectInt32(f[0], f[1], f[2], f[3]));
        else
        {
            var scale = DpiScale();
            var area = DisplayArea.GetFromWindowId(window.Id, DisplayAreaFallback.Primary).WorkArea;
            var w = (int)Math.Min(1180 * scale, area.Width * 0.9);
            var h = (int)Math.Min(780 * scale, area.Height * 0.9);
            window.MoveAndResize(new RectInt32(area.X + (area.Width - w) / 2, area.Y + (area.Height - h) / 2, w, h));
        }
        if (Store.Settings.Bool("window.maximized")) App.Presenter?.Maximize();
    }

    private static bool OnScreen(int[] f)
    {
        var area = DisplayArea.GetFromRect(new RectInt32(f[0], f[1], f[2], f[3]), DisplayAreaFallback.None);
        return area != null;
    }

    private double DpiScale() => root.XamlRoot?.RasterizationScale ?? GetDpiForWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)) / 96.0;

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern uint GetDpiForWindow(IntPtr hwnd);

    private void Leave()
    {
        KeyHook.Stop();
        var maximized = App.Presenter?.State == OverlappedPresenterState.Maximized;
        Store.Settings.Set("window.maximized", maximized);
        if (!maximized && App.Presenter?.State != OverlappedPresenterState.Minimized)
        {
            var r = AppWindow.Position;
            var s = AppWindow.Size;
            Store.Settings.Set("window.frame", $"{r.X},{r.Y},{s.Width},{s.Height}");
        }
        browser.FlushSession();
    }

    /// Light or dark, from Settings or from Windows, for the chrome and the
    /// pages alike.
    private void Relook()
    {
        var dark = browser.Prefs.Look switch
        {
            Look.Dark => true,
            Look.Light => false,
            _ => system.GetColorValue(UIColorType.Background) is var bg && bg.R < 128,
        };
        Palette.SetDark(dark);
        root.RequestedTheme = dark ? ElementTheme.Dark : ElementTheme.Light;
        foreach (var tab in browser.Tabs)
            if (tab.Core is { } core)
                core.Profile.PreferredColorScheme = dark
                    ? Microsoft.Web.WebView2.Core.CoreWebView2PreferredColorScheme.Dark
                    : Microsoft.Web.WebView2.Core.CoreWebView2PreferredColorScheme.Light;
    }

    // MARK: - what shows where

    private bool Immersed => browser.Active?.Immersed == true;
    private bool SideShowing => browser.Prefs.Sidebar && !browser.Folded && !Immersed;
    private bool Folding => browser.Prefs.Sidebar && browser.Folded && !Immersed;

    private void Changed(string name)
    {
        switch (name)
        {
            case nameof(Browser.Folded):
            case nameof(Browser.Peeking):
            case nameof(Browser.Active):
                Arrange();
                ShowField();
                WatchImmersion();
                break;
            case nameof(Browser.FieldShowing):
            case nameof(Browser.Editing):
                ShowField();
                if (!browser.FieldShowing) HandBack();
                break;
            case nameof(Browser.EditingTab):
                // The address typed into a row is done with, and the pointer
                // went elsewhere while it was: the column goes the way it
                // would have.
                if (browser.EditingTab == null && !insideSide && browser.Peeking) Peek(false);
                if (browser.EditingTab == null) HandBack();
                break;
            case nameof(Browser.Tuning):
            case nameof(Browser.Recalling):
            case nameof(Browser.Hoarding):
            case nameof(Browser.Bookmarking):
            case nameof(Browser.Managing):
            case nameof(Browser.Welcoming):
            case nameof(Browser.Reviewing):
                ShowPanels();
                break;
            case nameof(Browser.Tabs):
            case nameof(Browser.MakingSpace):
                Regions();
                break;
        }
    }

    private Tab? immersion;

    private void WatchImmersion()
    {
        if (immersion == browser.Active) return;
        if (immersion != null) immersion.PropertyChanged -= OnImmersed;
        immersion = browser.Active;
        if (immersion != null) immersion.PropertyChanged += OnImmersed;
    }

    private void OnImmersed(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
    {
        if (e.PropertyName != nameof(Tab.Immersed)) return;
        // A page taking over the screen takes the window with it.
        App.FullScreen(Immersed);
        Arrange();
    }

    /// The column or the strip, the page beside or below it.
    private void Arrange()
    {
        var sidebar = browser.Prefs.Sidebar && !Immersed;
        var width = browser.Prefs.SideWidth;
        tabBar.Visibility = !browser.Prefs.Sidebar && !Immersed ? Visibility.Visible : Visibility.Collapsed;
        sideBar.Visibility = sidebar ? Visibility.Visible : Visibility.Collapsed;

        // The column has its own corner for the buttons, so the page beside it
        // starts at the very top; the strip needs a band.
        var top = Immersed || browser.Prefs.Sidebar ? 0 : Metrics.Strip;
        var left = SideShowing ? width : 0;
        stage.Margin = new Thickness(left, top, 0, 0);

        // Folded: out of sight, or slid out over the page for a look.
        var peeking = Folding && browser.Peeking;
        sideBar.Translation = new Vector3((float)(sidebar && (SideShowing || peeking) ? 0 : -width - 20), 0, peeking ? 32 : 0);
        sideBar.Shadow = peeking ? new ThemeShadow() : null;
        foldEdge.Visibility = Folding ? Visibility.Visible : Visibility.Collapsed;
        Canvas.SetZIndex(sideBar, peeking ? 5 : 0);
        Regions();
    }

    /// The pointer on the edge: out at once, or after a short dwell when the
    /// column is folded for good — the edge is met far more often by a hand
    /// on its way somewhere else than by one reaching for the tabs.
    private void Arrive()
    {
        if (!browser.Prefs.SideHides) { Peek(true); return; }
        Pass();
        arriving = UI.After(0.15, () => Peek(true));
    }

    private void Pass()
    {
        arriving?.Cancel();
        arriving = null;
    }

    /// Out at once; in only once the pointer has stayed away for a grace.
    private void Peek(bool @out)
    {
        leaving?.Cancel();
        leaving = null;
        if (!Folding) return;
        if (@out)
        {
            if (!browser.Peeking) browser.Peek(true);
        }
        else
        {
            leaving = UI.After(0.3, () =>
            {
                if (browser.EditingTab != null) return;
                browser.Peek(false);
            });
        }
    }

    /// The address field: raised over a page by Ctrl+L or Ctrl+K, and standing
    /// on its own whenever a tab has nowhere to be yet.
    private void ShowField()
    {
        var showing = browser.FieldShowing;
        var over = !(browser.Active?.IsBlank ?? true);
        omnibox.Visibility = showing ? Visibility.Visible : Visibility.Collapsed;
        // Centred on the page, not on the window: the column of tabs is not
        // what the field is standing over.
        omnibox.Margin = new Thickness(SideShowing ? browser.Prefs.SideWidth : 0, over || browser.Prefs.Sidebar ? 0 : Metrics.Strip, 0, 0);
        if (showing) omnibox.Show(over);
        Regions();
    }

    /// Give the keyboard back to the page once the field is done with it.
    /// WebAuthn refuses to run on a document that isn't focused, and so do a
    /// number of paste and shortcut handlers pages install for themselves.
    private void HandBack()
    {
        if (browser.FieldShowing || browser.EditingTab != null) return;
        UI.Soon(() => browser.Active?.Built?.Focus(FocusState.Programmatic));
    }

    /// The panels. All the same kind of thing, so they are built the same way:
    /// a dimmed ground a click on which puts the panel away.
    private void ShowPanels()
    {
        panels.Children.Clear();
        UIElement? panel = null;
        Action close = () => { };
        if (browser.Welcoming) { panels.Children.Add(new WelcomePanel(browser)); Regions(); return; }
        if (browser.Recalling) { panel = new HistoryPanel(browser); close = () => browser.Recalling = false; }
        else if (browser.Hoarding) { panel = new DownloadsPanel(browser); close = () => browser.Hoarding = false; }
        else if (browser.Tuning) { panel = new SettingsPanel(browser); close = () => browser.Tuning = false; }
        else if (browser.Bookmarking) { panel = new BookmarksPanel(browser); close = () => browser.Bookmarking = false; }
        else if (browser.Managing) { panel = new PasswordsPanel(browser); close = () => browser.Managing = false; }
        else if (browser.Reviewing)
        {
            // No dimming for this one: the whole point is to keep looking at
            // the page while the list offers to put things back on it.
            var clear = new Grid { Background = Palette.Clear };
            clear.Tapped += (_, _) => browser.Reviewing = false;
            panels.Children.Add(clear);
            var hidden = new HiddenPanel(browser)
            {
                HorizontalAlignment = HorizontalAlignment.Right,
                VerticalAlignment = VerticalAlignment.Top,
                Margin = new Thickness(0, Metrics.Strip + 8, 14, 0),
            };
            panels.Children.Add(hidden);
            Regions();
            return;
        }
        if (panel != null)
        {
            var dim = new Grid { Background = new SolidColorBrush(Windows.UI.Color.FromArgb(26, 0, 0, 0)) };
            dim.Tapped += (_, e) => { if (ReferenceEquals(e.OriginalSource, dim)) close(); };
            panels.Children.Add(dim);
            panels.Children.Add(panel);
        }
        Regions();
    }

    // MARK: - where the window is grabbed

    private bool regionsDue;

    /// The strip is the title bar: dragged to move the window, double-clicked
    /// to fill the screen, everywhere but over what takes its own clicks. In
    /// the column's mode the column's empty parts are the title bar, and a band
    /// too thin to be in a page's way stands in for one along the top.
    private void Regions()
    {
        if (regionsDue) return;
        regionsDue = true;
        UI.Soon(() =>
        {
            regionsDue = false;
            try { SetRegions(); } catch { }
        });
    }

    private void SetRegions()
    {
        if (root.XamlRoot == null) return;
        var scale = root.XamlRoot.RasterizationScale;
        var source = InputNonClientPointerSource.GetForWindowId(AppWindow.Id);
        RectInt32 R(double x, double y, double w, double h) =>
            new((int)Math.Round(x * scale), (int)Math.Round(y * scale), (int)Math.Round(Math.Max(0, w) * scale), (int)Math.Round(Math.Max(0, h) * scale));
        RectInt32 Of(FrameworkElement e)
        {
            var at = e.TransformToVisual(root).TransformPoint(new Windows.Foundation.Point(0, 0));
            return R(at.X, at.Y, e.ActualWidth, e.ActualHeight);
        }

        var caption = new List<RectInt32>();
        var through = new List<RectInt32>();
        var W = root.ActualWidth;
        var H = root.ActualHeight;
        // The walk-through's top edge is its title bar, short of its own three
        // buttons at the far end.
        if (browser.Welcoming) caption.Add(R(0, 0, W - 3 * 46, 32));
        var covered = browser.Welcoming || browser.Tuning || browser.Recalling || browser.Hoarding || browser.Bookmarking || browser.Managing
            || (browser.FieldShowing && !(browser.Active?.IsBlank ?? true));

        if (!covered && !Immersed)
        {
            if (!browser.Prefs.Sidebar)
            {
                caption.Add(R(0, 0, W, Metrics.Strip));
                foreach (var e in tabBar.Passthrough()) through.Add(Of(e));
            }
            else
            {
                var width = browser.Prefs.SideWidth;
                var sideOut = SideShowing || (Folding && browser.Peeking);
                if (sideOut)
                {
                    caption.Add(R(0, 0, width, H));
                    foreach (var e in sideBar.Passthrough()) through.Add(Of(e));
                }
                // The top edge over the page.
                caption.Add(R(sideOut && SideShowing ? width : 0, 0, W, 8));
                if (Folding && !browser.Peeking) through.Add(R(0, 0, 6, H));
            }
        }
        var key = string.Join(";", caption.Select(r => $"{r.X},{r.Y},{r.Width},{r.Height}")) + "|"
            + string.Join(";", through.Select(r => $"{r.X},{r.Y},{r.Width},{r.Height}"));
        if (key == lastRegions) return;
        lastRegions = key;
        source.SetRegionRects(NonClientRegionKind.Caption, caption.ToArray());
        source.SetRegionRects(NonClientRegionKind.Passthrough, through.ToArray());
    }

    private string lastRegions = "";
}
