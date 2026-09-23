using System.Text.Json;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Microsoft.Web.WebView2.Core;
using Windows.Foundation;

namespace Search;

/// The extensions, behind one puzzle button — a list to press them from, pin
/// them out of, reload or remove them. The pinned ones also sit in the row
/// beside it, the way Chrome does it. Nothing at all with nothing installed.
public sealed class ExtensionSlot : StackPanel
{
    private readonly Browser browser;
    private readonly StackPanel pinned = new() { Orientation = Orientation.Horizontal, Spacing = 2 };
    private readonly Door puzzle;
    private readonly Dictionary<string, FrameworkElement> anchors = [];
    private Flyout? menu;

    /// Every slot on screen: one in the strip, one at the column's foot. A
    /// shortcut's popup hangs from whichever is showing.
    private static readonly List<WeakReference<ExtensionSlot>> slots = [];

    public ExtensionSlot(Browser browser)
    {
        this.browser = browser;
        Orientation = Orientation.Horizontal;
        Spacing = 2;
        VerticalAlignment = VerticalAlignment.Center;
        puzzle = new Door(Icons.Puzzle, "Extensions", Toggle);
        Children.Add(pinned);
        Children.Add(puzzle);
        slots.Add(new WeakReference<ExtensionSlot>(this));
        Extensions.Shared.OnAny(_ => Draw());
        if (slots.Count == 1) Extensions.Shared.PopupWanted += id => Showing()?.Press(id);
        Draw();
    }

    /// The side the list and the popups open toward: down from the top row,
    /// out to the right from the column.
    private FlyoutPlacementMode Edge
    {
        get
        {
            for (DependencyObject? up = this; up != null; up = VisualTreeHelper.GetParent(up))
                if (up is SideBar) return FlyoutPlacementMode.RightEdgeAlignedBottom;
            return FlyoutPlacementMode.BottomEdgeAlignedRight;
        }
    }

    private static ExtensionSlot? Showing()
    {
        foreach (var weak in slots)
            if (weak.TryGetTarget(out var slot) && slot.XamlRoot != null && slot.ActualWidth > 0 && slot.Visibility == Visibility.Visible)
                return slot;
        return slots.Select(w => w.TryGetTarget(out var s) ? s : null).FirstOrDefault(s => s != null);
    }

    private void Draw()
    {
        var extensions = Extensions.Shared;
        Visibility = extensions.Installed.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        pinned.Children.Clear();
        anchors.Clear();
        foreach (var item in extensions.Buttons.Where(i => i.Pinned == true))
        {
            var button = new ActionButton(item, () => Press(item.Id));
            button.ContextFlyout = ExtensionActions.Make(item.Id);
            anchors[item.Id] = button;
            pinned.Children.Add(button);
        }
    }

    private void Toggle()
    {
        if (menu != null)
        {
            menu.Hide();
            return;
        }
        menu = new Flyout
        {
            Content = new ExtensionMenu(browser, this),
            Placement = Edge,
            FlyoutPresenterStyle = ExtensionPopup.Presenter(),
            ShouldConstrainToRootBounds = true,
        };
        menu.Closed += (_, _) =>
        {
            menu = null;
            puzzle.On = false;
        };
        puzzle.On = true;
        menu.ShowAt(puzzle);
    }

    public void CloseMenu() => menu?.Hide();

    /// A popup is opened here, straight away, hanging from the extension's own
    /// button if it is pinned, else from the puzzle button.
    public void Press(string id)
    {
        var extensions = Extensions.Shared;
        if (extensions.Find(id) is not { } item) return;
        if (extensions.PopupUrl(item) is not { } url)
        {
            // Chrome would tell the extension its button was clicked
            // (action.onClicked). WebView2 has no way for the browser to.
            browser.Announce($"{item.Name} has no popup to open");
            return;
        }
        var anchor = anchors.TryGetValue(id, out var own) && own.XamlRoot != null ? own : (FrameworkElement)puzzle;
        if (anchor.XamlRoot != null && anchor.ActualWidth > 0)
        {
            ExtensionPopup.Show(browser, item, url, anchor, Edge);
            return;
        }
        // Neither button is on screen — a shortcut, with the strip away: the
        // window's top right corner, where the strip keeps them.
        ExtensionPopup.Show(browser, item, url, App.Root, FlyoutPlacementMode.BottomEdgeAlignedRight,
            new Point(Math.Max(0, App.Root.ActualWidth - 16), 8));
    }
}

/// An extension's button: its icon, in a square that lights under the
/// pointer. Its badge — the count some put in the corner — lives in the
/// engine, which doesn't hand it over, so there is none.
public sealed class ActionButton : Press
{
    public ActionButton(Installed item, Action press, double size = 15)
    {
        Width = 26;
        Height = 26;
        var ground = Kit.Rounded(8);
        Children.Add(ground);
        var icon = ExtensionIcon.Make(item, size);
        icon.HorizontalAlignment = HorizontalAlignment.Center;
        icon.VerticalAlignment = VerticalAlignment.Center;
        Children.Add(icon);
        ToolTipService.SetToolTip(this, Extensions.Shared.Manifest(item)?.Title ?? item.Name);
        Hovered += on => ground.Background = on ? Palette.Hover : Palette.Clear;
        Clicked += _ => press();
    }
}

public static class ExtensionIcon
{
    /// Its icon, or its initial rather than a puzzle piece that would pass
    /// for the button the list opens from.
    public static FrameworkElement Make(Installed item, double size)
    {
        if (Extensions.Shared.Icon(item, (int)Math.Ceiling(size)) is { } source)
            return new Image { Source = source, Width = size, Height = size, IsHitTestVisible = false };
        var plate = Kit.Rounded(size * 0.28, Palette.Wash);
        plate.Width = size;
        plate.Height = size;
        plate.IsHitTestVisible = false;
        var letter = Kit.Text(item.Name.Length > 0 ? item.Name[..1].ToUpperInvariant() : "?", size * 0.62, Palette.Muted, semibold: true);
        letter.HorizontalAlignment = HorizontalAlignment.Center;
        plate.Child = letter;
        return plate;
    }
}

/// What a right-click on an extension offers, in the row and in the list.
public static class ExtensionActions
{
    public static MenuFlyout Make(string id)
    {
        var menu = new MenuFlyout();
        menu.Opening += (_, _) =>
        {
            menu.Items.Clear();
            var extensions = Extensions.Shared;
            if (extensions.Find(id) is not { } item) return;
            void Item(string text, Action act)
            {
                var entry = new MenuFlyoutItem { Text = text };
                entry.Click += (_, _) => act();
                menu.Items.Add(entry);
            }
            var pinned = item.Pinned == true;
            Item(pinned ? "Unpin" : "Pin to Toolbar", () => extensions.SetPinned(id, !pinned));
            if (extensions.Manifest(item)?.OptionsPage != null) Item("Options…", () => extensions.OpenOptions(id));
            Item("Reload", () => extensions.Reload(id));
            menu.Items.Add(new MenuFlyoutSeparator());
            Item($"Remove “{item.Name}”…", () => ConfirmRemove(item));
        };
        return menu;
    }

    public static async void ConfirmRemove(Installed item)
    {
        if (await Extensions.Shared.Ask($"Remove “{item.Name}”?", "Its settings and data go with it.", null, "Remove", "Cancel"))
            Extensions.Shared.Remove(item.Id);
    }
}

/// The list behind the puzzle button: every extension that is on, a pin for
/// each, and the way to Settings.
public sealed class ExtensionMenu : StackPanel
{
    public ExtensionMenu(Browser browser, ExtensionSlot slot)
    {
        Width = 280;
        var extensions = Extensions.Shared;
        var buttons = extensions.Buttons;
        if (buttons.Count == 0)
        {
            var none = Kit.Text("None of your extensions is on", 12.5, Palette.Muted);
            none.Margin = new Thickness(14);
            Children.Add(none);
        }
        else
        {
            var list = new StackPanel { Spacing = 1, Padding = new Thickness(6) };
            foreach (var item in buttons) list.Children.Add(Row(item, slot));
            Children.Add(new ScrollViewer { Content = list, MaxHeight = 360, VerticalScrollBarVisibility = ScrollBarVisibility.Auto });
        }
        Children.Add(new Grid { Height = 1, Background = Palette.Hairline });
        var foot = new StackPanel { Spacing = 1, Padding = new Thickness(6) };
        foot.Children.Add(Foot(Icons.Globe, "Chrome Web Store…", () =>
        {
            slot.CloseMenu();
            browser.Open(Browser.WebStore, foreground: true);
        }));
        foot.Children.Add(Foot(Icons.Folder, "Load Unpacked…", () =>
        {
            slot.CloseMenu();
            UI.Soon(ExtensionsPage.ChooseFolder);
        }));
        foot.Children.Add(Foot(Icons.Settings, "Manage Extensions…", () =>
        {
            slot.CloseMenu();
            Store.Settings.Set("settings.page", "extensions");
            browser.Tuning = true;
        }));
        Children.Add(foot);
    }

    private static Press Row(Installed item, ExtensionSlot slot)
    {
        var extensions = Extensions.Shared;
        var row = new Press { Height = 30 };
        var ground = Kit.Rounded(8);
        row.Children.Add(ground);
        var line = new Grid { ColumnSpacing = 9, Padding = new Thickness(8, 0, 4, 0) };
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var icon = ExtensionIcon.Make(item, 16);
        icon.VerticalAlignment = VerticalAlignment.Center;
        line.Children.Add(icon);
        var name = Kit.Text(item.Name, 12.5);
        Grid.SetColumn(name, 1);
        line.Children.Add(name);
        var tools = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 2 };
        Grid.SetColumn(tools, 2);
        line.Children.Add(tools);
        row.Children.Add(line);

        var pinned = item.Pinned == true;
        var reload = new Door(Icons.Reload, "Reload from its folder", () => extensions.Reload(item.Id), 22);
        var pin = new Door(Icons.Pin, pinned ? "Unpin" : "Pin to toolbar", () => extensions.SetPinned(item.Id, !pinned), 22) { On = pinned };
        tools.Children.Add(reload);
        tools.Children.Add(pin);
        void Paint()
        {
            ground.Background = row.IsHovering ? Palette.Wash : Palette.Clear;
            reload.Visibility = row.IsHovering && item.Source != null ? Visibility.Visible : Visibility.Collapsed;
            pin.Visibility = row.IsHovering || pinned ? Visibility.Visible : Visibility.Collapsed;
        }
        row.Hovered += _ => Paint();
        // The list goes first; the popup, if there is one, then hangs from
        // the puzzle button it came out of.
        row.Clicked += _ =>
        {
            slot.CloseMenu();
            UI.After(0.15, () => slot.Press(item.Id));
        };
        ToolTipService.SetToolTip(row, extensions.Manifest(item)?.Title ?? item.Name);
        row.ContextFlyout = ExtensionActions.Make(item.Id);
        Paint();
        return row;
    }

    private static Press Foot(string glyph, string title, Action act)
    {
        var press = new Press();
        var ground = Kit.Rounded(8);
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, Padding = new Thickness(10, 6, 10, 6) };
        var icon = Icons.Make(glyph, 11);
        icon.Width = 14;
        row.Children.Add(icon);
        row.Children.Add(Kit.Text(title, 12.5));
        press.Children.Add(ground);
        press.Children.Add(row);
        press.Hovered += on => ground.Background = on ? Palette.Wash : Palette.Clear;
        press.Clicked += _ => act();
        return press;
    }
}

// An extension's popup, in a flyout of the browser's own.
//
// WebView2 has no popup of its own to offer: the engine runs the extension,
// but its toolbar button and the page that hangs from it are the browser's
// to draw. So the popup page is loaded here, in a view of the same profile
// as the tab in front — the one the extension is running in for that tab —
// in a flyout that hangs from the button.
//
// Chrome sizes a popup to its content, between 25 and 800 points wide and
// up to 600 tall; the page is measured after it loads and again as it
// changes, and the flyout follows. window.close() closes it.
public static class ExtensionPopup
{
    private static Flyout? open;
    private static Grid? stage;
    private static WebView2? web;
    private static string? extensionID;
    private static bool shown;
    private static Microsoft.UI.Dispatching.DispatcherQueueTimer? measuring;
    private static int ticks;

    /// Each extension's popup size, so the next opening starts there.
    private static readonly Dictionary<string, Size> lastSize = [];

    public static string? ExtensionID => extensionID;

    /// The flyout's own look: no padding, no size limits of its own, the
    /// window's ground and hairline — the page decides how big it is.
    public static Style Presenter()
    {
        var style = new Style(typeof(FlyoutPresenter));
        style.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(0)));
        style.Setters.Add(new Setter(FrameworkElement.MinWidthProperty, 0.0));
        style.Setters.Add(new Setter(FrameworkElement.MinHeightProperty, 0.0));
        style.Setters.Add(new Setter(FrameworkElement.MaxWidthProperty, 1000.0));
        style.Setters.Add(new Setter(FrameworkElement.MaxHeightProperty, 1000.0));
        style.Setters.Add(new Setter(Control.CornerRadiusProperty, new CornerRadius(12)));
        style.Setters.Add(new Setter(Control.BackgroundProperty, Palette.Ground));
        style.Setters.Add(new Setter(Control.BorderBrushProperty, Palette.Hairline));
        style.Setters.Add(new Setter(Control.BorderThicknessProperty, new Thickness(1)));
        style.Setters.Add(new Setter(ScrollViewer.HorizontalScrollBarVisibilityProperty, ScrollBarVisibility.Disabled));
        style.Setters.Add(new Setter(ScrollViewer.HorizontalScrollModeProperty, ScrollMode.Disabled));
        style.Setters.Add(new Setter(ScrollViewer.VerticalScrollBarVisibilityProperty, ScrollBarVisibility.Disabled));
        style.Setters.Add(new Setter(ScrollViewer.VerticalScrollModeProperty, ScrollMode.Disabled));
        return style;
    }

    public static async void Show(Browser browser, Installed item, Uri url, FrameworkElement anchor, FlyoutPlacementMode placement, Point? at = null)
    {
        Close();
        // Sized the way Chrome sizes a popup (see Preferred), unseen, while
        // the flyout already stands at the size this popup had last time;
        // then shown.
        var size = lastSize.TryGetValue(item.Id, out var known) ? known : new Size(360, 240);
        var ground = new Grid { Width = size.Width, Height = size.Height, Background = Palette.Ground };
        var view = new WebView2
        {
            Width = 25,
            Height = 25,
            HorizontalAlignment = HorizontalAlignment.Left,
            VerticalAlignment = VerticalAlignment.Top,
            Opacity = 0,
        };
        Motion.Fades(view, TimeSpan.FromMilliseconds(120));
        ground.Children.Add(view);
        var flyout = new Flyout
        {
            Content = ground,
            Placement = placement,
            FlyoutPresenterStyle = Presenter(),
            ShouldConstrainToRootBounds = true,
        };
        flyout.Closed += (_, _) =>
        {
            if (open == flyout) Forget();
            try { view.Close(); } catch { }
        };
        open = flyout;
        stage = ground;
        web = view;
        extensionID = item.Id;
        shown = false;
        if (at is { } spot) flyout.ShowAt(anchor, new FlyoutShowOptions { Position = spot, Placement = placement });
        else flyout.ShowAt(anchor);

        // The profile of the tab in front, which is where the extension is
        // running for it. A private tab has none; its popup is the space's.
        var tab = browser.Active;
        var profile = tab is { Shy: false } ? tab.Profile : Space.Profile(Space.Current);
        try
        {
            var env = await Web.Environment;
            var options = env.CreateCoreWebView2ControllerOptions();
            options.ProfileName = profile;
            // White behind the page, as Chrome paints a popup: many leave
            // their background unset, and their dark text over a dark ground
            // would vanish.
            options.DefaultBackgroundColor = Colors.White;
            await view.EnsureCoreWebView2Async(env, options);
        }
        catch
        {
            if (web == view) Close();
            return;
        }
        if (web != view) return;
        var core = view.CoreWebView2;
        core.Settings.AreDevToolsEnabled = true;
        core.Settings.IsStatusBarEnabled = false;
        core.Settings.IsZoomControlEnabled = false;
        core.Settings.IsSwipeNavigationEnabled = false;
        core.Profile.PreferredColorScheme = Palette.Dark ? CoreWebView2PreferredColorScheme.Dark : CoreWebView2PreferredColorScheme.Light;
        // The document is built — DOMContentLoaded, the moment Chrome sizes a
        // popup, before the page's scripts look at the room they have.
        core.DOMContentLoaded += (_, _) => FirstMeasure(view);
        core.NavigationCompleted += (_, _) => Follow(view);
        core.WindowCloseRequested += (_, _) => { if (web == view) Close(); };
        // A link that asks for a new window becomes a tab, and the popup goes
        // — the way it does in Chrome when you follow a link out of one.
        core.NewWindowRequested += (_, e) =>
        {
            e.Handled = true;
            if (Uri.TryCreate(e.Uri, UriKind.Absolute, out var link)) browser.Open(link, foreground: true);
            Close();
        };
        // A profile no tab has started yet this run is given its extensions
        // first; the popup's page is one of them.
        try { await Extensions.Shared.Adopt(core.Profile); } catch { }
        if (web != view) return;
        core.Navigate(url.AbsoluteUri);

        // Measured when the document is built or has loaded; a page slow to
        // do either is measured anyway after a moment, and shown regardless a
        // little later.
        UI.After(3, () =>
        {
            if (web != view) return;
            FirstMeasure(view);
            Follow(view);
        });
        UI.After(5, () => { if (web == view) Reveal(); });
    }

    public static void Close()
    {
        measuring?.Stop();
        measuring = null;
        var closing = open;
        Forget();
        closing?.Hide();
    }

    private static void Forget()
    {
        measuring?.Stop();
        measuring = null;
        open = null;
        stage = null;
        web = null;
        extensionID = null;
    }

    /// The page, at the flyout's size, in view.
    private static void Reveal()
    {
        if (shown || web is not { } view || stage is not { } ground) return;
        shown = true;
        view.Width = ground.Width;
        view.Height = ground.Height;
        view.Opacity = 1;
    }

    /// The size Chrome would give the popup (Blink's auto-size, between
    /// 25 × 25 and 800 × 600), worked out in the page while its view is
    /// still the 25-point square: the width the page names for itself if it
    /// names one, else its narrowest (min-content — what is positioned off
    /// to the side doesn't count), else, for a page with next to no width
    /// of its own, what its content spans; then, laid out at that width,
    /// the height it names or spans. Nothing of it is left on the page.
    private const string Preferred = """
    () => {
      const d = document.documentElement;
      if (!d) return null;
      const m = window.__searchSizing || (window.__searchSizing = {});
      const saved = d.getAttribute("style");
      const back = () => saved === null ? d.removeAttribute("style") : d.setAttribute("style", saved);
      const box = d.getBoundingClientRect();
      let w;
      if (Math.abs(box.width - innerWidth) > 1) w = m.w = box.width;
      else if (m.w && Math.abs(m.w - innerWidth) <= 1) w = m.w;
      else {
        d.style.setProperty("width", "min-content", "important");
        const narrowest = d.getBoundingClientRect().width;
        back();
        w = narrowest >= 100 ? narrowest : Math.max(narrowest, d.scrollWidth);
      }
      w = Math.min(800, Math.max(25, Math.ceil(w)));
      d.style.setProperty("width", w + "px", "important");
      let h = d.getBoundingClientRect().height;
      if (Math.abs(h - innerHeight) > 1) m.h = h;
      else if (m.h && Math.abs(m.h - innerHeight) <= 1) h = m.h;
      else {
        d.style.setProperty("height", "auto", "important");
        d.style.setProperty("min-height", "0", "important");
        h = d.getBoundingClientRect().height;
      }
      back();
      return [w, Math.min(600, Math.max(25, Math.ceil(h)))];
    }
    """;

    /// How far the page reaches: its own width if it names one wider than
    /// the view, else its narrowest, else what its content spans; and its
    /// height, if it spills past the view.
    private const string Reach = """
    (() => {
      const d = document.documentElement;
      if (!d) return null;
      let w;
      const own = d.getBoundingClientRect().width;
      if (own > innerWidth + 1) w = own;
      else {
        const saved = d.getAttribute("style");
        d.style.setProperty("width", "min-content", "important");
        const narrowest = d.getBoundingClientRect().width;
        saved === null ? d.removeAttribute("style") : d.setAttribute("style", saved);
        w = narrowest >= 100 ? narrowest : Math.max(narrowest, d.scrollWidth);
      }
      return [w, d.scrollHeight > d.clientHeight ? d.scrollHeight : 0];
    })()
    """;

    private static async Task<Size?> Measure(WebView2 view, string script)
    {
        try
        {
            if (view.CoreWebView2 is not { } core) return null;
            var json = await core.ExecuteScriptAsync(script);
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            if (root.ValueKind != JsonValueKind.Array || root.GetArrayLength() != 2) return null;
            return new Size(root[0].GetDouble(), root[1].GetDouble());
        }
        catch { return null; }
    }

    /// Measured while still the 25-point square and unseen, then shown at the
    /// size found.
    private static async void FirstMeasure(WebView2 view)
    {
        if (view != web || shown) return;
        var size = await Measure(view, $"({Preferred})()");
        if (view != web || shown || size is not { } found) return;
        Apply(found);
    }

    private static void Apply(Size size)
    {
        if (stage is not { } ground || web is not { } view) return;
        if (Math.Abs(size.Width - ground.Width) > 1 || Math.Abs(size.Height - ground.Height) > 1)
        {
            ground.Width = size.Width;
            ground.Height = size.Height;
            if (shown)
            {
                view.Width = size.Width;
                view.Height = size.Height;
            }
        }
        if (extensionID is { } id) lastSize[id] = size;
        Reveal();
    }

    /// From the page's first load: sized, then followed as it builds itself —
    /// a list filled in by a reply from the worker — the way Chrome measures
    /// on each layout. For its first two seconds the popup follows it either
    /// way; after that it only grows, so a page that settles doesn't set it
    /// rocking.
    private static void Follow(WebView2 view)
    {
        if (measuring != null || view != web) return;
        if (!shown) FirstMeasure(view);
        ticks = 0;
        measuring = UI.Every(0.25, () =>
        {
            if (view != web) { measuring?.Stop(); return; }
            ticks++;
            Grow(view);
            if (ticks > 24) { measuring?.Stop(); measuring = null; }
        });
    }

    private static async void Grow(WebView2 view)
    {
        if (!shown || stage is not { } ground) return;
        if (ticks <= 8)
        {
            if (await Measure(view, $"({Preferred})()") is not { } wanted || view != web) return;
            if (Math.Abs(wanted.Width - ground.Width) > 2 || Math.Abs(wanted.Height - ground.Height) > 2) Apply(wanted);
            return;
        }
        if (await Measure(view, Reach) is not { } reach || view != web) return;
        var grown = new Size(Math.Min(800, Math.Max(ground.Width, Math.Ceiling(reach.Width))), Math.Min(600, Math.Max(ground.Height, Math.Ceiling(reach.Height))));
        if (grown.Width != ground.Width || grown.Height != ground.Height) Apply(grown);
    }
}
