using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Search;

/// The Mac's menu bar, in one menu. macOS keeps an app's commands at the top
/// of the screen — File, View, Tabs, Bookmarks, History, and Settings under the
/// app's name — and Windows has no such bar, so they live behind one button:
/// at the far end of the strip, or in the column's foot. The same commands, in
/// the same groups, with the same keys beside them.
///
/// Built afresh each time it opens, so what it lists — the places you were,
/// the tabs you closed, the bookmarks, which look is on — is never stale.
public static class AppMenu
{
    public static void Show(Browser browser, FrameworkElement anchor, FlyoutPlacementMode placement)
    {
        var menu = new MenuFlyout { Placement = placement };
        Fill(browser, menu.Items);
        menu.ShowAt(anchor);
    }

    private static void Fill(Browser b, IList<MenuFlyoutItemBase> items)
    {
        var tab = b.Active;
        var page = tab is { IsBlank: false };

        // File
        items.Add(Item("New Tab", "Ctrl+T", Icons.Plus, () => b.NewTab()));
        items.Add(Item("New Private Tab", "Ctrl+Shift+N", Icons.Private, b.NewShyTab));
        items.Add(Item("Reopen Closed Tab", "Ctrl+Shift+T", null, b.Reopen, b.Ghosts.Count > 0));
        items.Add(new MenuFlyoutSeparator());

        // History
        var history = Sub("History", Icons.History);
        var recent = b.RecentlyVisited;
        if (recent.Count > 0)
        {
            history.Items.Add(Heading("Recently Visited"));
            foreach (var trace in recent)
            {
                var url = trace.Url;
                history.Items.Add(Item(Short(trace.Title.Length == 0 ? trace.Key : trace.Title), null, null, () => b.Visit(url)));
            }
        }
        if (b.Ghosts.Count > 0)
        {
            if (history.Items.Count > 0) history.Items.Add(new MenuFlyoutSeparator());
            history.Items.Add(Heading("Recently Closed"));
            foreach (var ghost in Enumerable.Reverse(b.Ghosts).Take(10).ToList())
                history.Items.Add(Item(Short(ghost.Label), null, null, () => b.Reopen(ghost)));
        }
        if (history.Items.Count > 0) history.Items.Add(new MenuFlyoutSeparator());
        history.Items.Add(Item("Show History…", "Ctrl+Y", null, () => b.Recalling = true));
        history.Items.Add(Item("Downloads…", "Ctrl+Shift+J", Icons.Download, () => b.Hoarding = true));
        history.Items.Add(new MenuFlyoutSeparator());
        history.Items.Add(Item("Clear History", null, null, b.ClearHistory));
        items.Add(history);

        // Bookmarks
        var marks = Sub("Bookmarks", Icons.Star);
        marks.Items.Add(Item("Add This Page", "Ctrl+Shift+B", null, b.BookmarkCurrent, page));
        marks.Items.Add(Item("Show Bookmarks…", null, null, () => b.Bookmarking = true));
        if (!b.Bookmarks.IsEmpty)
        {
            marks.Items.Add(new MenuFlyoutSeparator());
            Tree(b, b.Bookmarks.Roots, marks.Items);
        }
        items.Add(marks);

        // View
        var view = Sub("View", Icons.Window);
        foreach (var look in new[] { Look.Light, Look.Dark, Look.System })
        {
            var chosen = look;
            view.Items.Add(Radio(chosen.Title(), "look", b.Prefs.Look == chosen, () => b.Prefs.Look = chosen));
        }
        view.Items.Add(new MenuFlyoutSeparator());
        var side = new ToggleMenuFlyoutItem { Text = "Show Tabs in Sidebar", IsChecked = b.Prefs.Sidebar, KeyboardAcceleratorTextOverride = "Ctrl+Shift+S" };
        side.Click += (_, _) => b.ToggleSidebar();
        view.Items.Add(side);
        view.Items.Add(Item(b.Folded ? "Show Sidebar" : "Hide Sidebar", "Ctrl+S", null, b.ToggleFold, b.Prefs.Sidebar));
        var wear = Sub("Tabs Wear", null);
        foreach (var glyph in new[] { Glyph.Letters, Glyph.Icons })
        {
            var chosen = glyph;
            wear.Items.Add(Radio(chosen.Title(), "glyph", b.Prefs.Glyph == chosen, () => b.Prefs.Glyph = chosen));
        }
        view.Items.Add(wear);
        view.Items.Add(new MenuFlyoutSeparator());
        view.Items.Add(Item("Zoom In", "Ctrl+=", null, () => b.Zoom(1.1), page));
        view.Items.Add(Item("Zoom Out", "Ctrl+-", null, () => b.Zoom(1 / 1.1), page));
        view.Items.Add(Item("Actual Size", "Ctrl+0", null, b.ResetZoom, page));
        items.Add(view);

        // Page
        var here = Sub("Page", Icons.Globe);
        here.Items.Add(Item("Find on Page…", "Ctrl+F", Icons.Search, b.OpenFind, page));
        here.Items.Add(Item("Reading Mode", "Ctrl+Shift+R", null, b.ToggleReader, page));
        here.Items.Add(Item("Float Video", "Ctrl+Shift+P", null, b.ToggleFloat, page));
        here.Items.Add(Item("Stop Sound in Tab", "Ctrl+Shift+M", Icons.Speaker, b.PauseMedia, page));
        here.Items.Add(new MenuFlyoutSeparator());
        here.Items.Add(Item("Hide Elements…", "Ctrl+Shift+H", null, b.ToggleHiding, page));
        here.Items.Add(Item("Hidden on This Site…", "Ctrl+Shift+U", null, () => b.Reviewing = true, page));
        here.Items.Add(new MenuFlyoutSeparator());
        here.Items.Add(Item("Copy Address", "Ctrl+Shift+C", null, b.CopyAddress, page));
        here.Items.Add(Item("Paste and Go", "Ctrl+Shift+V", null, b.PasteAndGo));
        here.Items.Add(Item("Print…", "Ctrl+P", null, b.PrintPage, page));
        here.Items.Add(new MenuFlyoutSeparator());
        here.Items.Add(Item("Developer Tools", "F12", null, () => Inspector.Toggle(b), page));
        items.Add(here);
        items.Add(new MenuFlyoutSeparator());

        // Under the app's name, on the Mac.
        items.Add(Item("Passwords…", "Ctrl+Alt+L", Icons.Key, () => b.Managing = true));
        items.Add(Item("Settings…", "Ctrl+,", Icons.Settings, () => b.Tuning = true));
        items.Add(new MenuFlyoutSeparator());
        items.Add(Item("Welcome…", null, null, () => b.Welcoming = true));
        items.Add(Item("Send Feedback…", null, null, Feedback));
    }

    /// The bookmarks, as menus within menus.
    private static void Tree(Browser b, List<Bookmark> nodes, IList<MenuFlyoutItemBase> into)
    {
        foreach (var node in nodes)
        {
            if (node.IsFolder)
            {
                var folder = Sub(Short(node.Title), Icons.Folder);
                if (node.Children is { Count: > 0 } kids) Tree(b, kids, folder.Items);
                else folder.Items.Add(Item("Empty", null, null, () => { }, false));
                into.Add(folder);
            }
            else if (Uri.TryCreate(node.Url, UriKind.Absolute, out var url))
            {
                into.Add(Item(Short(node.Title), null, null, () => b.Visit(url, Keys.Down(Keys.Control))));
            }
        }
    }

    /// A draft in the mail app that already says which build this is. You
    /// read it and send it yourself: nothing here sends anything.
    private static void Feedback()
    {
        var subject = Uri.EscapeDataString($"Search feedback — {Updater.Version} (Windows)");
        var body = Uri.EscapeDataString($"\n\n—\nSearch {Updater.Version} for Windows, {Environment.OSVersion.VersionString}");
        _ = Windows.System.Launcher.LaunchUriAsync(new Uri($"mailto:hello@officecommun.com?subject={subject}&body={body}"));
    }

    private static string Short(string text) => text.Length > 60 ? text[..59] + "…" : text;

    private static MenuFlyoutItem Item(string text, string? keys, string? icon, Action act, bool enabled = true)
    {
        var item = new MenuFlyoutItem { Text = text, IsEnabled = enabled };
        if (keys != null) item.KeyboardAcceleratorTextOverride = keys;
        if (icon != null) item.Icon = new FontIcon { Glyph = icon, FontFamily = Icons.Font };
        item.Click += (_, _) => act();
        return item;
    }

    private static MenuFlyoutSubItem Sub(string text, string? icon)
    {
        var sub = new MenuFlyoutSubItem { Text = text };
        if (icon != null) sub.Icon = new FontIcon { Glyph = icon, FontFamily = Icons.Font };
        return sub;
    }

    private static RadioMenuFlyoutItem Radio(string text, string group, bool on, Action act)
    {
        var item = new RadioMenuFlyoutItem { Text = text, GroupName = group, IsChecked = on };
        item.Click += (_, _) => act();
        return item;
    }

    /// A quiet label over a group of items, as the Mac's menus have.
    private static MenuFlyoutItem Heading(string text) => new() { Text = text, IsEnabled = false };
}
