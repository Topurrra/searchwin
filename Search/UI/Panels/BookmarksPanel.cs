using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.Foundation;
using Windows.System;

namespace Search;

// Bookmarks, drawn. Shown as the app's own kind of list rather than a system
// menu, on purpose: a menu can't be dragged into, and can't be asked a second
// thing by right-clicking it. A folder you actually keep things in wants both.

/// The list itself: folders that open in place rather than to the side, each
/// row draggable into another folder or back out to the top, each row good
/// for a right-click too. Used both in the small dropdown off the button and
/// in the full manager — the interaction is the same size either way.
public sealed partial class BookmarkOutline : Grid
{
    private const double Indent = 18;

    private readonly Bookmarks bookmarks;
    private readonly Action<Uri, bool> open;
    private readonly StackPanel rows = new() { Spacing = 1 };
    private readonly Canvas lifted = new() { IsHitTestVisible = false };
    private readonly HashSet<Guid> expanded = [];
    private readonly List<OutlineRow> drawn = [];

    private Guid? renaming;
    /// The field a rename is typed into. One that was drawn over by a newer
    /// one still says it lost focus; only the newest is listened to.
    private TextBox? editor;
    private Guid? dragging;
    private Point grabbed;
    private Point origin;
    private OutlineRow? target;
    private FrameworkElement? ghost;

    /// `open` is handed the address and whether it wants a tab of its own.
    public BookmarkOutline(Browser browser, Action<Uri, bool> open)
    {
        bookmarks = browser.Bookmarks;
        this.open = open;
        Background = Palette.Clear;
        Children.Add(rows);
        Children.Add(lifted);

        Action changed = () => UI.Do(Fill);
        Loaded += (_, _) =>
        {
            bookmarks.Changed -= changed;
            bookmarks.Changed += changed;
            Fill();
        };
        Unloaded += (_, _) => bookmarks.Changed -= changed;
    }

    public void Fill()
    {
        rows.Children.Clear();
        drawn.Clear();
        Rows(bookmarks.Roots, 0);
    }

    private void Rows(List<Bookmark> nodes, int depth)
    {
        foreach (var node in nodes)
        {
            var row = new OutlineRow(this, node, depth, expanded.Contains(node.Id), renaming == node.Id);
            rows.Children.Add(row);
            drawn.Add(row);
            if (!node.IsFolder || !expanded.Contains(node.Id)) continue;
            if (node.Children is { Count: > 0 } kids) Rows(kids, depth + 1);
            else
            {
                var empty = Kit.Text("Empty", 12, Palette.Faint);
                empty.Margin = new Thickness((depth + 1) * Indent + 26, 5, 0, 5);
                rows.Children.Add(empty);
            }
        }
    }

    // MARK: - what a row asks of the list

    private void Toggle(Guid id)
    {
        if (!expanded.Remove(id)) expanded.Add(id);
        Fill();
    }

    private void Open(Bookmark node, bool apart)
    {
        if (node.Url != null && Uri.TryCreate(node.Url, UriKind.Absolute, out var url)) open(url, apart);
    }

    /// The title becomes a field, in place.
    public void Rename(Guid id)
    {
        renaming = id;
        Fill();
    }

    private void Renamed(Guid id, TextBox field, string? title)
    {
        if (renaming != id || field != editor) return;
        renaming = null;
        editor = null;
        var trimmed = title?.Trim() ?? "";
        if (trimmed.Length > 0 && Bookmarks.Find(id, bookmarks.Roots)?.Title != trimmed) bookmarks.Update(id, trimmed, null);
        else Fill();
    }

    private MenuFlyout Menu(Bookmark node)
    {
        var menu = new MenuFlyout();
        menu.Opening += (_, _) =>
        {
            menu.Items.Clear();
            MenuFlyoutItem Item(string text, Action act, IList<MenuFlyoutItemBase> into)
            {
                var item = new MenuFlyoutItem { Text = text };
                item.Click += (_, _) => act();
                into.Add(item);
                return item;
            }
            var top = menu.Items;
            if (!node.IsFolder)
            {
                Item("Open", () => Open(node, false), top);
                Item("Open in New Tab", () => Open(node, true), top);
                menu.Items.Add(new MenuFlyoutSeparator());
            }
            Item("Rename…", () => Rename(node.Id), top);

            var move = new MenuFlyoutSubItem { Text = "Move to" };
            var into = move.Items;
            Item("Top Level", () => bookmarks.Move(node.Id, null), into);
            // A folder three deep looks it; a folder is never offered a place
            // inside itself.
            var targets = Bookmarks.Folders(bookmarks.Roots).Where(t => !Bookmarks.Holds(t.node.Id, node)).ToList();
            if (targets.Count > 0) move.Items.Add(new MenuFlyoutSeparator());
            foreach (var (folder, depth) in targets)
            {
                var id = folder.Id;
                Item(new string(' ', depth * 3) + folder.Title, () => bookmarks.Move(node.Id, id), into);
            }
            menu.Items.Add(move);

            menu.Items.Add(new MenuFlyoutSeparator());
            Item("Remove", () => bookmarks.Remove(node.Id), top).Foreground = Palette.Brush(Tone.Red);
        };
        return menu;
    }

    // MARK: - dragging

    /// A row picked up. It stays where it was, faded, and a copy of it
    /// follows the pointer; a folder under the pointer lights up to say it
    /// will take it, the list itself to say it goes back to the top.
    private void Lift(OutlineRow row, Point start)
    {
        dragging = row.Node.Id;
        grabbed = start;
        origin = row.TransformToVisual(this).TransformPoint(start);
        row.Opacity = 0.35;

        var face = new Grid
        {
            Width = row.ActualWidth,
            Height = row.ActualHeight,
            CornerRadius = new CornerRadius(8),
            Background = Palette.Ground,
            BorderBrush = Palette.Hairline,
            BorderThickness = new Thickness(1),
            Padding = new Thickness(10 + 18, 0, 10, 0),
            Opacity = 0.92,
        };
        var title = Kit.Text(row.Node.Title, 12.5);
        face.Children.Add(title);
        Kit.Lift(face, 16);
        ghost = face;
        lifted.Children.Add(face);
        Carry(origin);
    }

    private void Carry(Point at)
    {
        if (ghost != null)
        {
            Canvas.SetLeft(ghost, at.X - grabbed.X);
            Canvas.SetTop(ghost, at.Y - grabbed.Y);
        }
        var inside = at.X >= 0 && at.Y >= 0 && at.X <= ActualWidth && at.Y <= ActualHeight;
        OutlineRow? over = null;
        if (inside)
            foreach (var row in drawn)
            {
                if (!row.Node.IsFolder || row.Node.Id == dragging) continue;
                var box = row.TransformToVisual(this).TransformBounds(new Rect(0, 0, row.ActualWidth, row.ActualHeight));
                if (box.Contains(at)) { over = row; break; }
            }
        if (over != target)
        {
            target?.Target(false);
            target = over;
            target?.Target(true);
        }
        Background = inside && over == null ? Palette.Wash : Palette.Clear;
    }

    private void Land()
    {
        var id = dragging;
        var into = target?.Node.Id;
        var home = Background == Palette.Wash;
        dragging = null;
        target?.Target(false);
        target = null;
        Background = Palette.Clear;
        lifted.Children.Clear();
        ghost = null;
        foreach (var row in drawn) row.Opacity = 1;
        if (id is not { } moving) return;
        if (into != null) bookmarks.Move(moving, into);
        else if (home) bookmarks.Move(moving, null);
    }

    /// One bookmark or one folder.
    private sealed partial class OutlineRow : Press
    {
        public Bookmark Node { get; }
        private readonly Border ground = Kit.Rounded(8);
        private bool targeted;

        public OutlineRow(BookmarkOutline outline, Bookmark node, int depth, bool isOpen, bool renaming)
        {
            Node = node;
            Children.Add(ground);
            ground.BackgroundTransition = new BrushTransition { Duration = Motion.Quick };

            var line = new Grid { ColumnSpacing = 8, Padding = new Thickness(depth * Indent + 10, 6, 10, 6) };
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(10) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            Children.Add(line);

            FrameworkElement mark;
            if (node.IsFolder)
            {
                var chevron = Icons.Make(Icons.Right, 9, Palette.Faint);
                chevron.HorizontalAlignment = HorizontalAlignment.Center;
                chevron.VerticalAlignment = VerticalAlignment.Center;
                chevron.RenderTransformOrigin = new Point(0.5, 0.5);
                chevron.RenderTransform = new RotateTransform { Angle = isOpen ? 90 : 0 };
                line.Children.Add(chevron);

                var folder = new Grid { Width = 15, Height = 15, VerticalAlignment = VerticalAlignment.Center };
                var plate = new Mark(15);
                plate.Show(null, "");
                folder.Children.Add(plate);
                var glyph = Icons.Make(Icons.Folder, 9, Palette.Muted);
                glyph.HorizontalAlignment = HorizontalAlignment.Center;
                glyph.VerticalAlignment = VerticalAlignment.Center;
                folder.Children.Add(glyph);
                mark = folder;
            }
            else
            {
                var site = new Mark(15) { VerticalAlignment = VerticalAlignment.Center };
                var host = node.Host ?? "";
                site.Show(Favicons.Shared.Cached(host), host.Length > 0 ? host[..1].ToUpperInvariant() : "•");
                mark = site;
            }
            Grid.SetColumn(mark, 1);
            line.Children.Add(mark);

            if (renaming)
            {
                var field = Kit.Field(12.5);
                field.Text = node.Title;
                // Esc here gives the old name back; it doesn't close the panel.
                field.Tag = KeyHook.OwnEscape;
                outline.editor = field;
                var done = false;
                void Finish(string? title)
                {
                    if (done) return;
                    done = true;
                    outline.Renamed(node.Id, field, title);
                }
                field.KeyDown += (_, e) =>
                {
                    if (e.Key == VirtualKey.Enter) { e.Handled = true; Finish(field.Text); }
                    else if (e.Key == VirtualKey.Escape) { e.Handled = true; Finish(null); }
                };
                field.LostFocus += (_, _) => Finish(field.Text);
                field.Loaded += (_, _) =>
                {
                    field.Focus(FocusState.Programmatic);
                    field.SelectAll();
                    field.StartBringIntoView();
                };
                Grid.SetColumn(field, 2);
                line.Children.Add(field);
            }
            else
            {
                var title = Kit.Text(node.Title, 12.5);
                Grid.SetColumn(title, 2);
                line.Children.Add(title);
            }

            if (node.IsFolder && node.Children is { Count: > 0 } kids)
            {
                var count = Kit.Text($"{Bookmarks.CountOf(kids)}", 11, Palette.Faint);
                count.Margin = new Thickness(8, 0, 0, 0);
                Grid.SetColumn(count, 3);
                line.Children.Add(count);
            }

            Draggable = !renaming;
            DragSpace = outline;
            DragBegan += start => outline.Lift(this, start);
            DragMoved += by => outline.Carry(new Point(outline.origin.X + by.X, outline.origin.Y + by.Y));
            DragEnded += outline.Land;
            Hovered += _ => Paint();
            Clicked += _ =>
            {
                if (renaming) return;
                if (node.IsFolder) outline.Toggle(node.Id);
                else outline.Open(node, Parts.Apart);
            };
            MiddleClicked += () => { if (!node.IsFolder) outline.Open(node, true); };
            ContextFlyout = outline.Menu(node);
            Paint();
        }

        /// Lit as the place a dragged row will land.
        public void Target(bool on)
        {
            targeted = on;
            Paint();
        }

        private void Paint() =>
            ground.Background = targeted ? Palette.Hover : IsHovering ? Palette.Wash : Palette.Clear;
    }
}

/// The button's dropdown: the tree, and the two things that aren't in it.
public static class BookmarksDropdown
{
    public static void Show(Browser browser, FrameworkElement anchor)
    {
        var presenter = new Style(typeof(FlyoutPresenter));
        presenter.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(0)));
        presenter.Setters.Add(new Setter(Control.BackgroundProperty, Palette.Ground));
        presenter.Setters.Add(new Setter(Control.BorderBrushProperty, Palette.Hairline));
        presenter.Setters.Add(new Setter(Control.BorderThicknessProperty, new Thickness(1)));
        presenter.Setters.Add(new Setter(Control.CornerRadiusProperty, new CornerRadius(14)));
        presenter.Setters.Add(new Setter(FrameworkElement.MinWidthProperty, 0.0));
        presenter.Setters.Add(new Setter(FrameworkElement.MinHeightProperty, 0.0));

        var flyout = new Flyout
        {
            // Beside the column's foot in the sidebar, under the button in
            // the strip — where the Mac's popover points.
            Placement = browser.Prefs.Sidebar ? FlyoutPlacementMode.Right : FlyoutPlacementMode.Bottom,
            FlyoutPresenterStyle = presenter,
        };

        var body = new StackPanel { Width = 280 };
        var top = new Grid();
        body.Children.Add(top);
        body.Children.Add(new Grid { Height = 1, Background = Palette.Hairline });
        var foot = new StackPanel { Spacing = 1, Padding = new Thickness(6) };
        foot.Children.Add(Foot(Icons.Bookmark, "Add This Page", browser.BookmarkCurrent));
        foot.Children.Add(Foot(null, "Manage Bookmarks…", () =>
        {
            flyout.Hide();
            browser.Bookmarking = true;
        }));
        body.Children.Add(foot);

        // A page opened from here takes the dropdown with it: the page is
        // what you asked for.
        var outline = new BookmarkOutline(browser, (url, apart) =>
        {
            flyout.Hide();
            browser.Bookmarking = false;
            browser.Visit(url, apart);
        })
        { Margin = new Thickness(6) };
        var list = Parts.Scroller(outline, 360, bar: true);

        void Fill()
        {
            top.Children.Clear();
            if (browser.Bookmarks.IsEmpty)
            {
                var none = Kit.Text("No bookmarks yet", 12.5, Palette.Muted);
                none.Margin = new Thickness(14);
                top.Children.Add(none);
            }
            else top.Children.Add(list);
        }
        Action changed = () => UI.Do(Fill);
        browser.Bookmarks.Changed += changed;
        Fill();

        flyout.Content = body;
        flyout.Closed += (_, _) =>
        {
            browser.Bookmarks.Changed -= changed;
            browser.BookmarksOpen = false;
        };
        flyout.ShowAt(anchor);
    }

    private static Press Foot(string? symbol, string title, Action act)
    {
        var press = new Press();
        var ground = Kit.Rounded(8);
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, Padding = new Thickness(10, 6, 10, 6) };
        var icon = Icons.Element(symbol ?? "", 11, Palette.Muted);
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

/// The full list, for taking things out of it or filing them away.
public sealed partial class BookmarksPanel : Plate
{
    private readonly Browser browser;
    private readonly Grid content;
    private readonly TextBlock count = Parts.Note("");
    private readonly BookmarkOutline outline;
    private readonly ScrollViewer list;

    public BookmarksPanel(Browser browser) : this(browser, new Grid(), new Grid()) { }

    private BookmarksPanel(Browser browser, Grid content, Grid foot)
        : base("Bookmarks", content, () => browser.Bookmarking = false, foot, width: 600)
    {
        this.browser = browser;
        this.content = content;

        outline = new BookmarkOutline(browser, (url, apart) =>
        {
            browser.Bookmarking = false;
            browser.Visit(url, apart);
        })
        { Margin = new Thickness(6) };
        var card = Parts.Card(outline);
        var holder = new Grid { Padding = new Thickness(0, 0, 0, 2) };
        holder.Children.Add(card);
        list = Parts.Scroller(holder, 440);

        // Windows has no other browser's folders to bring in here, so the
        // foot makes one instead: a folder is where a drag has somewhere to go.
        var folder = new Pill("New Folder", () =>
        {
            var made = browser.Bookmarks.Insert(Bookmark.Folder("New Folder", []), null);
            outline.Rename(made.Id);
        });
        var row = new Grid { ColumnSpacing = 8 };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.Children.Add(folder);
        Grid.SetColumn(count, 2);
        row.Children.Add(count);
        foot.Children.Add(row);

        Action changed = () => UI.Do(Fill);
        Loaded += (_, _) =>
        {
            browser.Bookmarks.Changed -= changed;
            browser.Bookmarks.Changed += changed;
        };
        Unloaded += (_, _) => browser.Bookmarks.Changed -= changed;
        Fill();
    }

    private void Fill()
    {
        var bookmarks = browser.Bookmarks;
        var empty = bookmarks.IsEmpty;
        var showing = content.Children.Count > 0 ? content.Children[0] : null;
        if (empty && showing is not Card)
        {
            content.Children.Clear();
            content.Children.Add(Parts.Card(Nothing.Make("Nothing kept yet. Add this page with Ctrl+Shift+B.")));
        }
        else if (!empty && showing != list)
        {
            content.Children.Clear();
            content.Children.Add(list);
        }
        count.Text = bookmarks.Count == 1 ? "1 bookmark" : $"{bookmarks.Count} bookmarks";
    }
}
