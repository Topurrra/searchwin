using System.ComponentModel;
using System.Numerics;
using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;

namespace Search;

/// The tabs, down the left instead of across the top.
///
/// The same pieces as the strip — the grey that slides to the tab you picked,
/// the pinned squares, the cross that appears under the pointer — laid out the
/// other way. The window's buttons keep the column's corner; the column starts
/// under them and the page takes the whole height beside it.
public sealed class SideBar : Grid
{
    private const double Row = 28, Gap = 2, Square = 34, PinGap = 4, FootHeight = 26 + 10;

    private readonly Browser browser;
    private readonly Canvas pins = new();
    private readonly Canvas rows = new();
    private readonly Grid rowsWash = new() { Height = Row, IsHitTestVisible = false };
    private readonly Border rowsRead;
    private readonly Border pinsWash = Kit.Rounded(9, Palette.Wash);
    private readonly ScrollViewer scroller = new()
    {
        VerticalScrollBarVisibility = ScrollBarVisibility.Hidden,
        HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
        HorizontalScrollMode = ScrollMode.Disabled,
        ZoomMode = ZoomMode.Disabled,
        IsTabStop = false,
    };
    private readonly StackPanel list = new();
    private readonly Dictionary<Guid, SideRow> rowViews = [];
    private readonly Dictionary<Guid, PinSquare> squares = [];
    private readonly Rectangle edgeLine = new() { Width = 1, HorizontalAlignment = HorizontalAlignment.Right, IsHitTestVisible = false };
    private readonly Door bookmarks;

    private Tab? dragging;
    private int from;
    private Tab? pinDragging;
    private int pinFrom;

    public SideBar(Browser browser)
    {
        this.browser = browser;
        Width = browser.Prefs.SideWidth;
        Background = Palette.Ground;
        HorizontalAlignment = HorizontalAlignment.Left;

        RowDefinitions.Add(new RowDefinition { Height = new GridLength(Metrics.Strip) });
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(FootHeight) });

        // The corner: back, forward and reload on the left, the window's own
        // three buttons on the right.
        var corner = new Grid { Padding = new Thickness(10, 0, 6, 0) };
        var helm = new Helm(browser) { VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Left };
        corner.Children.Add(helm);
        var buttons = new WindowButtons(34, 30) { VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Right };
        corner.Children.Add(buttons);
        helmView = helm;
        buttonsView = buttons;
        Children.Add(corner);

        // The pinned squares, a grid of their own at the head of the column.
        pins.Children.Add(pinsWash);
        Motion.Glides(pinsWash);
        pins.Margin = new Thickness(10, 0, 10, 0);
        SetRow(pins, 1);
        Children.Add(pins);

        // The rows, and the row that makes another, scrolling as one when
        // there are more than the window holds.
        rowsRead = new Border { Background = Palette.Brush(Tone.Ink, 0.055), HorizontalAlignment = HorizontalAlignment.Left, Width = 0 };
        var washFill = Kit.Rounded(9, Palette.Wash);
        rowsWash.Children.Add(new Border { CornerRadius = new CornerRadius(9), Child = new Grid { Children = { washFill, rowsRead } } });
        Motion.Glides(rowsWash);
        rows.Children.Add(rowsWash);
        list.Children.Add(rows);
        list.Children.Add(new Quiet(Icons.Plus, "New tab", () => browser.NewTab(), Row) { Margin = new Thickness(0, Gap, 0, 8) });
        scroller.Content = list;
        scroller.Padding = new Thickness(10, 0, 10, 0);
        SetRow(scroller, 2);
        Children.Add(scroller);

        // One small door at the bottom: the bookmarks, and beside it the
        // extensions and the space.
        var foot = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 2, Padding = new Thickness(10, 0, 10, 10), VerticalAlignment = VerticalAlignment.Bottom };
        foot.Children.Add(new ExtensionSlot(browser));
        bookmarks = new Door(Icons.Bookmark, "Bookmarks", () => browser.BookmarksOpen = !browser.BookmarksOpen);
        foot.Children.Add(bookmarks);
        SetRow(foot, 3);
        Children.Add(foot);
        footView = foot;

        edgeLine.Fill = Palette.Hairline;
        SetRowSpan(edgeLine, 4);
        Children.Add(edgeLine);
        var edge = Edge();
        SetRowSpan(edge, 4);
        Children.Add(edge);
        edgeView = edge;

        SizeChanged += (_, _) => Layout();
        browser.OnAny(name =>
        {
            switch (name)
            {
                case nameof(Browser.Tabs):
                case nameof(Browser.PinnedCount):
                    Rebuild();
                    break;
                case nameof(Browser.ActiveID):
                    Layout(reveal: true);
                    break;
                case nameof(Browser.EditingTab):
                case nameof(Browser.EditingPin):
                    Layout();
                    break;
                case nameof(Browser.Refusals):
                    if (browser.EditingTab is { } id && rowViews.TryGetValue(id, out var row)) Motion.Shake(row);
                    break;
                case nameof(Browser.BookmarksOpen):
                    if (browser.BookmarksOpen && XamlRoot != null && Visibility == Visibility.Visible && browser.Prefs.Sidebar)
                        BookmarksDropdown.Show(browser, bookmarks);
                    break;
            }
        });
        browser.Prefs.On(nameof(Preferences.SideWidth), () => { Width = browser.Prefs.SideWidth; Layout(); });
        browser.Prefs.On(nameof(Preferences.Glyph), () =>
        {
            foreach (var r in rowViews.Values) r.Paint();
            foreach (var s in squares.Values) s.Paint();
        });
        Rebuild();
    }

    private FrameworkElement? helmView, buttonsView, footView, edgeView;

    /// What takes its own clicks; the rest of the column is the title bar.
    public IEnumerable<FrameworkElement> Passthrough()
    {
        if (helmView != null) yield return helmView;
        if (buttonsView != null) yield return buttonsView;
        if (pins.ActualHeight > 0) yield return pins;
        yield return list;
        if (footView != null) yield return footView;
        if (edgeView != null) yield return edgeView;
    }

    /// The column's edge: pull it to make the column wider or narrower,
    /// double-click it to put it back. The hairline darkens under the pointer
    /// so the edge says it can be taken before it is.
    private Press Edge()
    {
        var edge = new Press { Width = 9, HorizontalAlignment = HorizontalAlignment.Right, Margin = new Thickness(0, 0, -4, 0), Draggable = true };
        edge.SetCursor(InputSystemCursorShape.SizeWestEast);
        double? grabbed = null;
        void Paint()
        {
            var on = edge.IsHovering || grabbed != null;
            edgeLine.Fill = on ? Palette.Brush(Tone.Ink, 0.18) : Palette.Hairline;
            edgeLine.Width = on ? 2 : 1;
        }
        edge.Hovered += _ => Paint();
        edge.DragSpace = App.Root;
        edge.DragBegan += _ => { grabbed = browser.Prefs.SideWidth; Paint(); };
        edge.DragMoved += d =>
        {
            var wanted = (grabbed ?? browser.Prefs.SideWidth) + d.X;
            browser.Prefs.SideWidth = Math.Clamp(wanted, Metrics.SideMin, Metrics.SideMax);
        };
        edge.DragEnded += () => { grabbed = null; Paint(); };
        edge.WantsDouble = true;
        edge.Clicked += _ => browser.Prefs.SideWidth = Metrics.Side;
        return edge;
    }

    private IEnumerable<Tab> Pinned => browser.Tabs.Where(t => t.Pin != null);
    private IEnumerable<Tab> Loose => browser.Tabs.Where(t => t.Pin == null);

    private void Rebuild()
    {
        var pinnedIds = Pinned.Select(t => t.Id).ToHashSet();
        var looseIds = Loose.Select(t => t.Id).ToHashSet();
        foreach (var (id, row) in rowViews.ToList())
        {
            if (looseIds.Contains(id)) continue;
            rowViews.Remove(id);
            row.Detach();
            rows.Children.Remove(row);
        }
        foreach (var (id, square) in squares.ToList())
        {
            if (pinnedIds.Contains(id)) continue;
            squares.Remove(id);
            square.Detach();
            pins.Children.Remove(square);
        }
        foreach (var tab in Loose)
        {
            if (rowViews.ContainsKey(tab.Id)) continue;
            var row = new SideRow(browser, tab, rows);
            row.DragBegan += _ => BeginDrag(tab);
            row.DragMoved += d => Drag(tab, d.Y);
            row.DragEnded += EndDrag;
            rowViews[tab.Id] = row;
            rows.Children.Add(row);
            row.Opacity = 0;
            UI.Soon(() => row.Opacity = 1);
        }
        foreach (var tab in Pinned)
        {
            if (squares.ContainsKey(tab.Id)) continue;
            var square = new PinSquare(browser, tab, pins);
            square.DragBegan += _ => BeginPinDrag(tab);
            square.DragMoved += d => PinDrag(tab, d.X, d.Y);
            square.DragEnded += EndPinDrag;
            squares[tab.Id] = square;
            pins.Children.Add(square);
        }
        Layout();
    }

    // MARK: - the pinned squares

    /// Three columns is the block's own shape — up to six pins, that's two
    /// full rows. Only past six does the block widen, one column at a time,
    /// to stay at two rows for as long as that's a reasonable shape at all.
    private static int PinColumns(int count) => Math.Max(3, (count + 1) / 2);

    private double PinWidth(int count)
    {
        var cols = PinColumns(count);
        var available = browser.Prefs.SideWidth - 20 - (cols - 1) * PinGap;
        return Math.Max(20, available / cols);
    }

    private void Layout(bool reveal = false)
    {
        var width = browser.Prefs.SideWidth - 20;
        var pinned = Pinned.ToList();
        var cols = PinColumns(pinned.Count);
        var pw = PinWidth(pinned.Count);
        var ph = Math.Min(Square, pw);
        var pinRows = pinned.Count == 0 ? 0 : (pinned.Count + cols - 1) / cols;
        pins.Height = pinRows == 0 ? 0 : pinRows * ph + (pinRows - 1) * PinGap;
        pins.Margin = new Thickness(10, 0, 10, pinRows == 0 ? 0 : 10);
        var livePin = false;
        for (var i = 0; i < pinned.Count; i++)
        {
            var tab = pinned[i];
            if (!squares.TryGetValue(tab.Id, out var square)) continue;
            var live = tab.Id == browser.ActiveID;
            square.Place(pw, ph, live);
            var at = new Vector3((float)(i % cols * (pw + PinGap)), (float)(i / cols * (ph + PinGap)), 0);
            if (pinDragging != tab) square.Translation = at;
            if (live)
            {
                livePin = true;
                pinsWash.Width = pw;
                pinsWash.Height = ph;
                pinsWash.CornerRadius = new CornerRadius(ph * 9 / 34);
                if (pinDragging == tab) Motion.Jump(pinsWash, square.Translation); else pinsWash.Translation = at;
            }
        }
        pinsWash.Opacity = livePin ? 1 : 0;

        var loose = Loose.ToList();
        var y = 0.0;
        var liveRow = false;
        foreach (var tab in loose)
        {
            if (!rowViews.TryGetValue(tab.Id, out var row)) continue;
            var live = tab.Id == browser.ActiveID;
            row.Place(width, live, browser.EditingTab == tab.Id);
            if (dragging != tab) row.Translation = new Vector3(0, (float)y, 0);
            if (live)
            {
                liveRow = true;
                rowsWash.Width = width;
                if (dragging == tab) Motion.Jump(rowsWash, row.Translation); else rowsWash.Translation = new Vector3(0, (float)y, 0);
                WatchReading(tab, width);
                if (reveal)
                {
                    var top = y;
                    UI.Soon(() =>
                    {
                        if (top < scroller.VerticalOffset || top + Row > scroller.VerticalOffset + scroller.ViewportHeight)
                            scroller.ChangeView(null, Math.Max(0, top - scroller.ViewportHeight / 2), null, false);
                    });
                }
            }
            y += Row + Gap;
        }
        rowsWash.Opacity = liveRow ? 1 : 0;
        rows.Width = width;
        rows.Height = Math.Max(0, y - Gap);
    }

    private Tab? reading;
    private double readingWidth;

    private void WatchReading(Tab tab, double width)
    {
        readingWidth = width;
        rowsRead.Width = width * tab.Reading;
        if (reading == tab) return;
        if (reading != null) reading.PropertyChanged -= OnReading;
        reading = tab;
        tab.PropertyChanged += OnReading;
    }

    private void OnReading(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(Tab.Reading) && sender is Tab tab) rowsRead.Width = readingWidth * tab.Reading;
    }

    // MARK: - picking a row up

    private void BeginDrag(Tab tab)
    {
        dragging = tab;
        from = Loose.ToList().IndexOf(tab);
        if (rowViews.TryGetValue(tab.Id, out var row)) row.Hold(true);
    }

    /// Pick a row up and the others make way as it passes them.
    private void Drag(Tab tab, double dy)
    {
        if (dragging != tab || !rowViews.TryGetValue(tab.Id, out var row)) return;
        var step = Row + Gap;
        Motion.Jump(row, new Vector3(0, (float)(from * step + dy), 0));
        if (tab.Id == browser.ActiveID) Motion.Jump(rowsWash, row.Translation);
        var loose = Loose.ToList();
        var target = Math.Clamp(from + (int)Math.Round(dy / step), 0, loose.Count - 1);
        // Positions here are among the loose rows; the pinned block sits in
        // front of them in the real list.
        if (target != loose.IndexOf(tab)) browser.Move(tab, target + browser.PinnedCount);
    }

    private void EndDrag()
    {
        if (dragging != null && rowViews.TryGetValue(dragging.Id, out var row)) row.Hold(false);
        dragging = null;
        Layout();
    }

    private void BeginPinDrag(Tab tab)
    {
        pinDragging = tab;
        pinFrom = Pinned.ToList().IndexOf(tab);
        if (squares.TryGetValue(tab.Id, out var square)) square.Hold(true);
    }

    /// Pick a square up and the others make way — across a row, and down into
    /// the next, exactly as far as the fingers actually moved.
    private void PinDrag(Tab tab, double dx, double dy)
    {
        if (pinDragging != tab || !squares.TryGetValue(tab.Id, out var square)) return;
        var count = Pinned.Count();
        var cols = PinColumns(count);
        var pw = PinWidth(count);
        var ph = Math.Min(Square, pw);
        var stepX = pw + PinGap;
        var stepY = ph + PinGap;
        var home = new Vector3((float)(pinFrom % cols * stepX), (float)(pinFrom / cols * stepY), 0);
        Motion.Jump(square, home + new Vector3((float)dx, (float)dy, 0));
        if (tab.Id == browser.ActiveID) Motion.Jump(pinsWash, square.Translation);
        var moved = (int)Math.Round(dy / stepY) * cols + (int)Math.Round(dx / stepX);
        var target = Math.Clamp(pinFrom + moved, 0, Math.Max(0, count - 1));
        if (target != Pinned.ToList().IndexOf(tab)) browser.Move(tab, target);
    }

    private void EndPinDrag()
    {
        if (pinDragging != null && squares.TryGetValue(pinDragging.Id, out var square)) square.Hold(false);
        pinDragging = null;
        Layout();
    }
}

/// One tab, as a line in the column.
public sealed class SideRow : Press
{
    private readonly Browser browser;
    public Tab Tab { get; }
    private readonly Border ground = Kit.Rounded(9);
    private readonly TabFace face;
    private bool live, editing;

    public SideRow(Browser browser, Tab tab, UIElement lane)
    {
        this.browser = browser;
        Tab = tab;
        Height = 28;
        Draggable = true;
        DragSpace = lane;
        Motion.Glides(this, Motion.Settle);
        Motion.Fades(this);
        Children.Add(ground);
        face = new TabFace(browser, tab, 8) { Padding = new Thickness(10, 0, 7, 0) };
        face.CloseAsked += () => browser.Close(tab);
        Children.Add(face);
        ContextFlyout = TabMenu.Make(browser, tab);
        Hovered += _ => Paint();
        Clicked += _ =>
        {
            if (live) browser.BeginTabEdit(tab); else browser.Select(tab);
        };
        MiddleClicked += () => browser.Close(tab);
    }

    public void Detach() => face.Detach();

    public void Place(double width, bool live, bool editing)
    {
        Width = width;
        this.live = live;
        if (this.editing != editing)
        {
            this.editing = editing;
            face.Edit(editing);
            face.Padding = new Thickness(10, 0, editing ? 10 : 7, 0);
        }
        Paint();
    }

    public void Hold(bool held)
    {
        Canvas.SetZIndex(this, held ? 10 : 0);
        if (held) Kit.Lift(this, 12);
        else
        {
            Shadow = null;
            var t = Translation;
            Translation = new Vector3(t.X, t.Y, 0);
        }
    }

    public void Paint()
    {
        face.Set(live, IsHovering);
        ground.Background = !live && IsHovering ? Palette.Hover : Palette.Clear;
    }
}

/// A pinned tab as a cell in the block at the top of the column — as wide as
/// its row asks for, but never taller than the classic square.
public sealed class PinSquare : Press
{
    private readonly Browser browser;
    public Tab Tab { get; }
    private readonly Border ground = Kit.Rounded(9);
    private readonly Grid inner = new() { HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
    private readonly TextBlock letter = Kit.Text("", 12, medium: true);
    private readonly Mark mark = new(16);
    private TextBox? field;
    private bool live;
    private double scale = 34;

    public PinSquare(Browser browser, Tab tab, UIElement lane)
    {
        this.browser = browser;
        Tab = tab;
        Draggable = true;
        DragSpace = lane;
        Motion.Glides(this, Motion.Settle);
        Children.Add(ground);
        letter.HorizontalAlignment = HorizontalAlignment.Center;
        letter.TextAlignment = TextAlignment.Center;
        inner.Children.Add(letter);
        inner.Children.Add(mark);
        Children.Add(inner);
        ContextFlyout = TabMenu.Make(browser, tab);
        ToolTipService.SetToolTip(this, tab.Label);
        Hovered += _ => Paint();
        Clicked += _ =>
        {
            if (live) browser.EditLetter(tab); else browser.Select(tab);
        };
        // Put down, like Ctrl+W: Close() is what knows a pin isn't removed.
        MiddleClicked += () => browser.Close(tab);
        tab.PropertyChanged += OnTab;
        browser.On(nameof(Browser.EditingPin), Paint);
    }

    private void OnTab(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(Tab.Pin) or nameof(Tab.Icon) or nameof(Tab.Asleep) or nameof(Tab.Label))
        {
            ToolTipService.SetToolTip(this, Tab.Label);
            Paint();
        }
    }

    public void Detach() => Tab.PropertyChanged -= OnTab;

    public void Place(double width, double height, bool live)
    {
        Width = width;
        Height = height;
        this.live = live;
        // Everything inside scales off the shorter edge — the one that stays
        // put — so the glyph sits at its usual size rather than stretching to
        // chase the width.
        scale = Math.Min(width, height);
        WantsDouble = live;
        Paint();
    }

    public void Hold(bool held)
    {
        Canvas.SetZIndex(this, held ? 10 : 0);
        if (held) Kit.Lift(this, 12);
        else
        {
            Shadow = null;
            var t = Translation;
            Translation = new Vector3(t.X, t.Y, 0);
        }
    }

    public void Paint()
    {
        var side = scale * 16 / 34;
        inner.Width = side;
        inner.Height = side;
        ground.CornerRadius = new CornerRadius(scale * 9 / 34);
        ground.Background = live ? Palette.Clear : IsHovering ? Palette.Hover : Palette.Brush(Tone.Wash, 0.55);
        var editingPin = browser.EditingPin == Tab.Id;
        if (editingPin && field == null)
        {
            field = PinField.Make(browser, Tab, scale * 12 / 34);
            inner.Children.Add(field);
        }
        else if (!editingPin && field != null)
        {
            inner.Children.Remove(field);
            field = null;
        }
        var showIcon = browser.Prefs.Glyph == Glyph.Icons && Tab.Icon != null && !editingPin;
        mark.Visibility = showIcon ? Visibility.Visible : Visibility.Collapsed;
        mark.Width = mark.Height = side;
        mark.Show(Tab.Icon, Tab.Pin ?? "", Tab.Asleep);
        letter.Visibility = !showIcon && !editingPin ? Visibility.Visible : Visibility.Collapsed;
        letter.Text = Tab.Pin ?? "";
        letter.FontSize = Math.Max(8, scale * 12 / 34);
        letter.Foreground = Palette.Brush(live ? Tone.Ink : Tone.Muted, Tab.Asleep ? 0.45 : 1);
    }
}
