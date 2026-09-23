using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Search;

// The panels, and the few small pieces of chrome that belong to features
// still being carried over. Each is the shape the window already places;
// each moves to a file of its own as it is filled in.

/// Every site you have been to, searchable. PORT: Recall.swift (HistoryPanel).
public sealed class HistoryPanel : Plate
{
    public HistoryPanel(Browser browser) : base("History", Nothing.Make("Nothing yet."), () => browser.Recalling = false) { }
}

/// What you have downloaded. PORT: Recall.swift (DownloadsPanel).
public sealed class DownloadsPanel : Plate
{
    public DownloadsPanel(Browser browser) : base("Downloads", Nothing.Make("Nothing yet."), () => browser.Hoarding = false) { }
}

/// Everything there is to set. PORT: Settings.swift, ExtensionsUI.swift.
public sealed class SettingsPanel : Plate
{
    public SettingsPanel(Browser browser) : base("Settings", Nothing.Make("Coming over from the Mac."), () => browser.Tuning = false, width: 660) { }
}

/// The full list of bookmarks. PORT: Bookmarks.swift (BookmarksPanel, BookmarkOutline).
public sealed class BookmarksPanel : Plate
{
    public BookmarksPanel(Browser browser) : base("Bookmarks", Nothing.Make("No bookmarks yet."), () => browser.Bookmarking = false) { }
}

/// The passwords kept, by site. PORT: Passwords.swift, Import.swift.
public sealed class PasswordsPanel : Plate
{
    public PasswordsPanel(Browser browser) : base("Passwords", Nothing.Make("None kept yet."), () => browser.Managing = false) { }
}

/// What is hidden on this site. PORT: Hidden.swift.
public sealed class HiddenPanel : Plate
{
    public HiddenPanel(Browser browser) : base("Hidden here", Nothing.Make("Nothing hidden on this site."), () => browser.Reviewing = false, width: 320) { }
}

/// The first-launch walk-through. PORT: Welcome.swift.
public sealed class WelcomePanel : Grid
{
    public WelcomePanel(Browser browser)
    {
        Background = Palette.Ground;
        var stack = new StackPanel { Spacing = 18, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
        stack.Children.Add(Logomark.Make(96));
        stack.Children.Add(new Pill("Start", () =>
        {
            browser.Prefs.Welcomed = true;
            browser.Welcoming = false;
        }, filled: true) { HorizontalAlignment = HorizontalAlignment.Center });
        Children.Add(stack);
    }
}

/// The bookmarks, hanging from their button. PORT: Bookmarks.swift (BookmarksDropdown).
public static class BookmarksDropdown
{
    public static void Show(Browser browser, FrameworkElement anchor)
    {
        var flyout = new Flyout { Content = Nothing.Make("No bookmarks yet."), Placement = FlyoutPlacementMode.Bottom };
        flyout.Closed += (_, _) => browser.BookmarksOpen = false;
        flyout.ShowAt(anchor);
    }
}

/// The extensions' buttons, in the strip or the column's foot. PORT: ExtensionsUI.swift.
public sealed class ExtensionSlot : StackPanel
{
    public ExtensionSlot(Browser browser)
    {
        Orientation = Orientation.Horizontal;
        Spacing = Metrics.TabGap;
    }
}

/// "Add to Search", offered at the bottom on a Chrome Web Store page. PORT: Store.swift.
public sealed class StoreOffer : Grid
{
    public StoreOffer(Browser browser) { }
}

/// The accounts kept for the site, hanging from its sign-in box. PORT: Accounts.swift.
public sealed class AccountList : Grid
{
    public AccountList(Browser browser)
    {
        IsHitTestVisible = false;
    }
}
