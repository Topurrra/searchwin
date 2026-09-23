using System.ComponentModel;
using System.Numerics;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Search;

/// The only chrome there is. Titles, one of them in a grey pill, and the pill
/// slides from the tab you left to the tab you picked rather than blinking out
/// of one and into the other.
public sealed class TabBar : Grid
{
    private readonly Browser browser;
    private readonly ScrollViewer run = new()
    {
        HorizontalScrollBarVisibility = ScrollBarVisibility.Hidden,
        VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
        HorizontalScrollMode = ScrollMode.Disabled,
        VerticalScrollMode = ScrollMode.Disabled,
        ZoomMode = ZoomMode.Disabled,
        IsTabStop = false,
    };
    private readonly Canvas lane = new() { Height = Metrics.Strip };
    /// The grey behind the live tab, and the reading it fills with.
    private readonly Grid wash = new() { Height = 28, IsHitTestVisible = false };
    private readonly Border washFill;
    private readonly Border washRead;
    private readonly Door plus;
    private readonly StackPanel doors = new() { Orientation = Orientation.Horizontal, Spacing = Metrics.TabGap, VerticalAlignment = VerticalAlignment.Center };
    private readonly Dictionary<Guid, TabPill> pills = [];
    private readonly Door bookmarks;
    private readonly SpaceDot dot;

    /// Which tab is under the hand, where it started, and how far it has come.
    private Tab? dragging;
    private int from;
    private double travel;
    private bool nearby;

    public TabBar(Browser browser)
    {
        this.browser = browser;
        Height = Metrics.Strip;
        VerticalAlignment = VerticalAlignment.Top;
        Background = Palette.Ground;

        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Metrics.Lights) });
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });   // the dot of the space
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });   // the run of tabs
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });   // the plus
        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });   // the doors
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });   // the window's buttons

        washFill = Kit.Rounded(9, Palette.Wash);
        washRead = new Border { Background = Palette.Brush(Tone.Ink, 0.055), HorizontalAlignment = HorizontalAlignment.Left, Width = 0 };
        var washClip = new Border { CornerRadius = new CornerRadius(9), Child = new Grid { Children = { washFill, washRead } } };
        wash.Children.Add(washClip);
        Motion.Glides(wash);
        Canvas.SetTop(wash, (Metrics.Strip - 28) / 2);
        lane.Children.Add(wash);
        // The space on screen, first, when there are spaces.
        dot = new SpaceDot(browser) { Margin = new Thickness(0, 0, Metrics.TabGap, 0) };
        SetColumn(dot, 1);
        Children.Add(dot);
        browser.Prefs.On(nameof(Preferences.UsesSpaces), () => Layout());

        run.Content = lane;
        run.VerticalAlignment = VerticalAlignment.Center;
        SetColumn(run, 2);
        Children.Add(run);

        // The way to a new page, right after the tabs rather than at the end of
        // their run, so it is there however far the run has scrolled. Out of
        // sight until the pointer is up here.
        plus = new Door(Icons.Plus, "New Tab   Ctrl+T", () => browser.NewTab(), 28)
        {
            Margin = new Thickness(Metrics.TabGap, 0, 0, 0),
            VerticalAlignment = VerticalAlignment.Center,
            Opacity = 0,
        };
        Motion.Fades(plus, Motion.Settle);
        SetColumn(plus, 3);
        Children.Add(plus);

        // Back, forward, reload, and the bookmarks, at the far end of the row.
        doors.Children.Add(new ExtensionSlot(browser));
        var helm = new Helm(browser) { Margin = new Thickness(0, 0, 8, 0) };
        doors.Children.Add(helm);
        bookmarks = new Door(Icons.Bookmark, "Bookmarks", () => browser.BookmarksOpen = !browser.BookmarksOpen);
        doors.Children.Add(bookmarks);
        doors.Margin = new Thickness(0, 0, 8, 0);
        SetColumn(doors, 5);
        Children.Add(doors);

        buttons = new WindowButtons(46, Metrics.Strip) { VerticalAlignment = VerticalAlignment.Top };
        SetColumn(buttons, 6);
        Children.Add(buttons);

        PointerEntered += (_, _) => Near(true);
        PointerExited += (_, _) => Near(false);
        SizeChanged += (_, _) => Layout();
        doors.SizeChanged += (_, _) => Layout();

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
                    if (browser.EditingTab is { } id && pills.TryGetValue(id, out var pill)) Motion.Shake(pill);
                    break;
                case nameof(Browser.BookmarksOpen):
                    if (browser.BookmarksOpen) BookmarksDropdown.Show(browser, bookmarks);
                    break;
            }
        });
        browser.Prefs.On(nameof(Preferences.Glyph), () => { foreach (var p in pills.Values) p.Paint(); });
        Rebuild();
    }

    private WindowButtons? buttons;

    /// What takes its own clicks: everything but the empty stretch of strip
    /// the window is dragged by.
    public IEnumerable<FrameworkElement> Passthrough()
    {
        yield return run;
        if (browser.Prefs.UsesSpaces) yield return dot;
        yield return plus;
        yield return doors;
        if (buttons != null) yield return buttons;
    }

    private void Near(bool on)
    {
        nearby = on;
        plus.Opacity = on ? 1 : 0;
        plus.IsHitTestVisible = on;
    }

    /// The pills follow the row: new ones made, gone ones let go, then laid
    /// out where they now belong.
    private void Rebuild()
    {
        var alive = browser.Tabs.Select(t => t.Id).ToHashSet();
        foreach (var (id, pill) in pills.ToList())
        {
            if (alive.Contains(id)) continue;
            pills.Remove(id);
            pill.Detach();
            lane.Children.Remove(pill);
        }
        foreach (var tab in browser.Tabs)
        {
            if (pills.ContainsKey(tab.Id)) continue;
            var pill = new TabPill(browser, tab, lane);
            pill.DragBegan += _ => BeginDrag(tab);
            pill.DragMoved += d => Drag(tab, d.X);
            pill.DragEnded += EndDrag;
            pills[tab.Id] = pill;
            lane.Children.Add(pill);
            // Arriving from the strip rather than from nowhere.
            pill.Opacity = 0;
            pill.Arrive(PlaceOf(tab));
            UI.Soon(() => pill.Opacity = 1);
        }
        Layout();
    }

    // MARK: - sizes

    /// The strip, less the room at its start, the plus, the doors at the far
    /// end and the window's buttons.
    private double Room
    {
        get
        {
            var far = doors.ActualWidth > 0 ? doors.ActualWidth + 8 : Metrics.Helm + 26 + 8;
            // What the space's dot takes before the tabs, when there are spaces.
            var dotted = browser.Prefs.UsesSpaces ? SpaceDot.Width_ + Metrics.TabGap : 0;
            return Math.Max(0, ActualWidth - Metrics.Lights - dotted - 12 - Metrics.PlusWidth - far - 3 * Metrics.TabGap - 3 * 46);
        }
    }

    /// Every loose tab is the same width, so the cross is always in the same
    /// place. Past a dozen or so they start giving ground; too narrow for a
    /// title they show their mark alone, down to the mark and its air. Past
    /// that, the run scrolls.
    private double Each
    {
        get
        {
            var pinned = browser.PinnedCount;
            var loose = browser.Tabs.Count - pinned;
            if (loose <= 0) return Metrics.TabWidth;
            var spent = pinned * Metrics.PinWidth + Math.Max(0, browser.Tabs.Count - 1) * Metrics.TabGap;
            return Math.Max(Metrics.TabMinWidth, Math.Min(Metrics.TabWidth, (Room - spent) / loose));
        }
    }

    private double EditWidth => Math.Min(340, ActualWidth - Metrics.Lights - 12 - 3 * 46);

    private double Span(Tab tab, double each)
    {
        if (browser.EditingTab == tab.Id) return EditWidth;
        return tab.Pin != null ? Metrics.PinWidth : each;
    }

    private double PlaceOf(Tab tab)
    {
        var each = Each;
        var x = 0.0;
        foreach (var t in browser.Tabs)
        {
            if (t == tab) return x;
            x += Span(t, each) + Metrics.TabGap;
        }
        return x;
    }

    /// Everything placed: each pill at its slot, the grey under the live one,
    /// the run as wide as the tabs while they fit and as wide as the room
    /// once they don't.
    private void Layout(bool reveal = false)
    {
        if (ActualWidth <= 0) return;
        var each = Each;
        var x = 0.0;
        double liveX = 0, liveSpan = 0;
        TabPill? livePill = null;
        foreach (var tab in browser.Tabs)
        {
            if (!pills.TryGetValue(tab.Id, out var pill)) continue;
            var span = Span(tab, each);
            var live = tab.Id == browser.ActiveID;
            pill.Place(span, live, browser.EditingTab == tab.Id);
            if (dragging != tab) pill.Translation = new Vector3((float)x, (float)((Metrics.Strip - 28) / 2), 0);
            if (live) { liveX = x; liveSpan = span; livePill = pill; }
            x += span + Metrics.TabGap;
        }
        var content = Math.Max(0, x - Metrics.TabGap);
        lane.Width = content;
        var room = Room;
        run.Width = Math.Min(content, room);
        var overflowing = content > room + 0.5;
        run.HorizontalScrollMode = overflowing ? ScrollMode.Enabled : ScrollMode.Disabled;

        // The grey: under the live tab, sliding when the live tab changes.
        if (livePill != null)
        {
            wash.Visibility = Visibility.Visible;
            wash.Width = liveSpan;
            // The grey sits at the pills' height by its own Canvas.Top; its
            // Translation carries only the slide along the row.
            var held = dragging != null && dragging.Id == browser.ActiveID;
            if (held) Motion.Jump(wash, new Vector3(livePill.Translation.X, 0, 0));
            else wash.Translation = new Vector3((float)liveX, 0, 0);
            var tab = livePill.Tab;
            // Not on a pinned square, nor a tab down to its mark: grey filling
            // from the left behind a single letter says nothing about anything.
            var reads = tab.Pin == null && liveSpan >= Metrics.TabTitled && browser.EditingTab != tab.Id;
            washRead.Width = reads ? liveSpan * tab.Reading : 0;
            WatchReading(tab, liveSpan, reads);
        }
        else
        {
            wash.Visibility = Visibility.Collapsed;
        }

        // The tab you are on is kept in view once the run scrolls.
        if (overflowing && (reveal || true) && livePill != null)
        {
            var target = Math.Max(0, Math.Min(liveX - 24, content - run.Width));
            if (liveX < run.HorizontalOffset || liveX + liveSpan > run.HorizontalOffset + run.Width)
                run.ChangeView(target, null, null, !reveal);
        }
    }

    private Tab? reading;
    private double readingSpan;
    private bool readingOn;

    private void WatchReading(Tab tab, double span, bool on)
    {
        readingSpan = span;
        readingOn = on;
        if (reading == tab) return;
        if (reading != null) reading.PropertyChanged -= OnReading;
        reading = tab;
        tab.PropertyChanged += OnReading;
    }

    private void OnReading(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(Tab.Reading) && sender is Tab tab)
            washRead.Width = readingOn ? readingSpan * tab.Reading : 0;
    }

    // MARK: - picking a tab up

    private void BeginDrag(Tab tab)
    {
        dragging = tab;
        from = browser.Tabs.IndexOf(tab);
        travel = 0;
        if (pills.TryGetValue(tab.Id, out var pill)) pill.Hold(true);
    }

    /// Pick a tab up and the others get out of its way as it passes them.
    private void Drag(Tab tab, double dx)
    {
        if (dragging != tab || !pills.TryGetValue(tab.Id, out var pill)) return;
        travel = dx;
        var each = Each;
        // A pinned square moves among pinned squares, a title among titles:
        // each has its own stride.
        var step = (tab.Pin != null ? Metrics.PinWidth : each) + Metrics.TabGap;
        var start = 0.0;
        for (var i = 0; i < from && i < browser.Tabs.Count; i++) start += Span(browser.Tabs[i], each) + Metrics.TabGap;
        // Under the hand exactly; only the others glide.
        Motion.Jump(pill, new Vector3((float)(start + travel), (float)((Metrics.Strip - 28) / 2), 0));
        if (tab.Id == browser.ActiveID) Motion.Jump(wash, new Vector3((float)(start + travel), 0, 0));
        var moved = (int)Math.Round(travel / step);
        var target = Math.Clamp(from + moved, 0, browser.Tabs.Count - 1);
        if (target != browser.Tabs.IndexOf(tab)) browser.Move(tab, target);
    }

    private void EndDrag()
    {
        if (dragging != null && pills.TryGetValue(dragging.Id, out var pill)) pill.Hold(false);
        dragging = null;
        travel = 0;
        Layout();
    }
}

/// One tab in the strip: a pinned square, a mark alone, or a title — and, when
/// it is the live one being clicked again, the address field.
public sealed class TabPill : Press
{
    private readonly Browser browser;
    public Tab Tab { get; }
    private readonly Border ground = Kit.Rounded(9);
    private readonly TabFace face;
    private readonly Grid square = new() { Width = 16, Height = 16, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
    private readonly TextBlock letter = Kit.Text("", 12, medium: true);
    private readonly Mark mark = new(16);
    private readonly Ring ring = new();
    private TextBox? pinField;
    private bool live, editing, pinned, compact;
    private double span;

    public TabPill(Browser browser, Tab tab, UIElement lane)
    {
        this.browser = browser;
        Tab = tab;
        Height = 28;
        Draggable = true;
        DragSpace = lane;
        Motion.Glides(this, Motion.Settle);
        Motion.Fades(this);
        Children.Add(ground);

        face = new TabFace(browser, tab, 6) { Padding = new Thickness(11, 6, 7, 6) };
        face.CloseAsked += () => browser.Close(tab);
        Children.Add(face);

        letter.HorizontalAlignment = HorizontalAlignment.Center;
        letter.TextAlignment = TextAlignment.Center;
        square.Children.Add(letter);
        square.Children.Add(mark);
        square.Children.Add(ring);
        Children.Add(square);

        ContextFlyout = TabMenu.Make(browser, tab);
        Hovered += _ => Paint();
        Clicked += clicks =>
        {
            if (live && pinned) browser.EditLetter(tab);
            else if (live && !pinned) browser.BeginTabEdit(tab);
            else browser.Select(tab);
        };
        // Put down, like Ctrl+W: Close() is what knows a pin isn't removed.
        MiddleClicked += () => browser.Close(tab);
        tab.PropertyChanged += OnTab;
    }

    public void Detach()
    {
        face.Detach();
        Tab.PropertyChanged -= OnTab;
    }

    private void OnTab(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(Tab.Pin) or nameof(Tab.Icon) or nameof(Tab.Loading) or nameof(Tab.Asleep) or nameof(Tab.Label) or nameof(Tab.Address))
            Paint();
    }

    public void Arrive(double x) => Motion.Jump(this, new Vector3((float)x, (float)((Metrics.Strip - 28) / 2), 0));

    /// Lifted a little while it is under the hand.
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

    public void Place(double span, bool live, bool editing)
    {
        var was = this.editing;
        this.span = span;
        this.live = live;
        this.editing = editing;
        Width = span;
        if (was != editing) face.Edit(editing);
        Paint();
    }

    public void Paint()
    {
        pinned = Tab.Pin != null && !editing;
        // Too narrow for a title: the site's mark alone, the title in the
        // tooltip, and Ctrl+W or the menu to close it — a cross on something
        // this small would be what a click to pick the tab lands on.
        compact = !editing && !pinned && span < Metrics.TabTitled;
        WantsDouble = live && pinned;

        face.Visibility = pinned || compact ? Visibility.Collapsed : Visibility.Visible;
        face.Set(live, IsHovering);
        square.Visibility = pinned || compact ? Visibility.Visible : Visibility.Collapsed;

        var colour = live ? Palette.Ink : IsHovering ? Palette.Brush(Tone.Ink, 0.7) : Palette.Muted;
        var editingPin = pinned && browser.EditingPin == Tab.Id;
        if (editingPin && pinField == null)
        {
            pinField = PinField.Make(browser, Tab, 12);
            square.Children.Add(pinField);
        }
        else if (!editingPin && pinField != null)
        {
            square.Children.Remove(pinField);
            pinField = null;
        }
        var icons = browser.Prefs.Glyph == Glyph.Icons;
        if (pinned)
        {
            var showIcon = icons && Tab.Icon != null && !editingPin;
            mark.Visibility = showIcon ? Visibility.Visible : Visibility.Collapsed;
            mark.Show(Tab.Icon, Tab.Pin ?? "", Tab.Asleep);
            letter.Visibility = !showIcon && !editingPin ? Visibility.Visible : Visibility.Collapsed;
            letter.Text = Tab.Pin ?? "";
            // A pin holding no page is still there and still yours; it just
            // isn't costing anything.
            letter.Foreground = live ? Palette.Brush(Tone.Ink, Tab.Asleep ? 0.45 : 1) : IsHovering ? Palette.Brush(Tone.Ink, 0.7) : Palette.Brush(Tone.Muted, Tab.Asleep ? 0.45 : 1);
            ring.Visibility = Visibility.Collapsed;
        }
        else if (compact)
        {
            letter.Visibility = Visibility.Collapsed;
            ring.Visibility = Tab.Loading ? Visibility.Visible : Visibility.Collapsed;
            mark.Visibility = Tab.Loading ? Visibility.Collapsed : Visibility.Visible;
            mark.Show(icons ? Tab.Icon : null, Tab.Monogram, Tab.Asleep);
        }
        ToolTipService.SetToolTip(this, pinned || compact ? Tab.Label : null);

        // The live tab's grey is the strip's, sliding under whichever it is.
        ground.Background = live ? Palette.Clear
            : IsHovering ? Palette.Hover
            // A letter with nothing behind it reads as debris: a pinned tab
            // keeps a faint ground of its own so the block reads as one thing.
            : pinned ? Palette.Brush(Tone.Wash, 0.55)
            : Palette.Clear;
        _ = colour;
    }
}
