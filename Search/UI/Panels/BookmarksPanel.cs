using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Search;

/// The full list of bookmarks. PORT: Bookmarks.swift (BookmarksPanel, BookmarkOutline).
public sealed class BookmarksPanel : Plate
{
    public BookmarksPanel(Browser browser) : base("Bookmarks", Nothing.Make("No bookmarks yet."), () => browser.Bookmarking = false) { }
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
