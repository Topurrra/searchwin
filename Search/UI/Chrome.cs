using System.ComponentModel;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Search;

/// The window's own three buttons — minimise, maximise, close — drawn by the
/// app rather than by Windows, so they can live where the Mac keeps its
/// traffic lights' equivalent: at the end of the strip, or in the column's
/// corner, riding with it when it folds away.
public sealed class WindowButtons : StackPanel
{
    private readonly Press maximize;
    private readonly FontIcon maxGlyph;

    public WindowButtons(double width, double height)
    {
        Orientation = Orientation.Horizontal;
        Add(Icons.Minimize, "Minimize", width, height, false, () => App.Presenter?.Minimize(), out _);
        maximize = Add(Icons.Maximize, "Maximize", width, height, false, () =>
        {
            if (App.Presenter is not { } p) return;
            if (p.State == OverlappedPresenterState.Maximized) p.Restore(); else p.Maximize();
        }, out maxGlyph);
        Add(Icons.ChromeClose, "Close", width, height, true, App.CloseWindow, out _);
        App.WindowChanged += Refresh;
        Refresh();
    }

    private Press Add(string glyph, string help, double width, double height, bool danger, Action act, out FontIcon icon)
    {
        var press = new Press { Width = width, Height = height };
        var ground = new Border();
        icon = Icons.Make(glyph, 10, Palette.Muted);
        icon.HorizontalAlignment = HorizontalAlignment.Center;
        icon.VerticalAlignment = VerticalAlignment.Center;
        press.Children.Add(ground);
        press.Children.Add(icon);
        ToolTipService.SetToolTip(press, help);
        var shown = icon;
        var red = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0xC4, 0x2B, 0x1C));
        var white = new SolidColorBrush(Colors.White);
        void Paint()
        {
            ground.Background = press.IsHovering ? (danger ? red : Palette.Hover) : Palette.Clear;
            shown.Foreground = press.IsHovering ? (danger ? white : Palette.Ink) : Palette.Muted;
        }
        press.Hovered += _ => Paint();
        press.Clicked += _ => act();
        Paint();
        Children.Add(press);
        return press;
    }

    private void Refresh()
    {
        var max = App.Presenter?.State == OverlappedPresenterState.Maximized;
        maxGlyph.Glyph = max ? Icons.Restore : Icons.Maximize;
        ToolTipService.SetToolTip(maximize, max ? "Restore" : "Maximize");
    }
}

/// Back, forward, reload. They watch the live tab, not the window: whether
/// there is anywhere to go back to is the tab's to say, and it changes with
/// every page.
public sealed class Helm : StackPanel
{
    private readonly Browser browser;
    private readonly Door back, forward, reload;
    private Tab? watched;

    public Helm(Browser browser)
    {
        this.browser = browser;
        Orientation = Orientation.Horizontal;
        Spacing = 2;
        back = new Door(Icons.Back, "Back   Alt+Left", browser.Back);
        forward = new Door(Icons.Forward, "Forward   Alt+Right", browser.Forward);
        reload = new Door(Icons.Reload, "Reload   Ctrl+R", () =>
        {
            if (browser.Active is { Loading: true } tab) tab.Stop(); else browser.Reload();
        });
        Children.Add(back);
        Children.Add(forward);
        Children.Add(reload);
        browser.On(nameof(Browser.Active), Watch);
        Watch();
    }

    private void Watch()
    {
        if (watched != null) watched.PropertyChanged -= OnTab;
        watched = browser.Active;
        if (watched != null) watched.PropertyChanged += OnTab;
        Paint();
    }

    private void OnTab(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(Tab.CanGoBack) or nameof(Tab.CanGoForward) or nameof(Tab.Loading) or nameof(Tab.IsBlank) or nameof(Tab.Address))
            Paint();
    }

    private void Paint()
    {
        var tab = watched;
        // Nowhere to go and nothing to reload: the doors stay in place,
        // greyed, so the row doesn't shift when a tab arrives.
        back.Enabled = tab is { IsBlank: false, CanGoBack: true };
        forward.Enabled = tab is { IsBlank: false, CanGoForward: true };
        reload.Enabled = tab is { IsBlank: false };
        // Reload, or stop while it is still coming.
        reload.Icon = tab?.Loading == true ? Icons.Close : Icons.Reload;
        ToolTipService.SetToolTip(reload, tab?.Loading == true ? "Stop   Esc" : "Reload   Ctrl+R");
    }
}

/// What a right-click on any tab offers, wherever the tab is drawn.
public static class TabMenu
{
    public static MenuFlyout Make(Browser browser, Tab tab)
    {
        var menu = new MenuFlyout();
        menu.Opening += (_, _) =>
        {
            menu.Items.Clear();
            MenuFlyoutItem Item(string text, Action act, bool enabled = true)
            {
                var item = new MenuFlyoutItem { Text = text, IsEnabled = enabled };
                item.Click += (_, _) => act();
                menu.Items.Add(item);
                return item;
            }
            if (tab.Pin == null) Item("Pin", () => browser.PinTab(tab), !tab.IsBlank);
            else
            {
                Item("Change Letter", () => browser.EditLetter(tab));
                Item("Unpin", () => browser.Unpin(tab));
            }
            menu.Items.Add(new MenuFlyoutSeparator());
            Item("Rename", () => browser.BeginTabRename(tab));
            Item("Duplicate", () => { browser.Select(tab); browser.Duplicate(); }, !tab.IsBlank);
            Item("Copy Address", () => { browser.Select(tab); browser.CopyAddress(); }, !tab.IsBlank);
            menu.Items.Add(new MenuFlyoutSeparator());
            Item("Close Tab", () => browser.Close(tab));
            Item("Close Other Tabs", () => browser.CloseOthers(tab), browser.Tabs.Count > 1);
        };
        return menu;
    }
}
