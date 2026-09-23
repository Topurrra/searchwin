using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Search;

// Everywhere you have been, and everything you have kept. Two lists in the
// same white-and-hairline panel as the rest, and in both cases the point is
// as much being able to remove a line as to read one.

/// Every site you have been to, searchable, a card to a day.
public sealed class HistoryPanel : Plate
{
    /// What was being looked for, still there the next time the panel opens —
    /// the Mac keeps it on the browser for the same reason.
    private static string hunted = "";

    /// Rows are drawn a batch at a time as the list scrolls, so two thousand
    /// places open as fast as twenty.
    private const int Batch = 120;

    private readonly Browser browser;
    private readonly Hunt hunt = new("Search everywhere you have been");
    private readonly Grid list = new();
    private readonly StackPanel days = new() { Spacing = 14, Padding = new Thickness(0, 0, 0, 2) };
    private readonly ScrollViewer scroller;
    private readonly Grid foot;
    private readonly TextBlock count = Parts.Note("");

    private List<Trace> traces = [];
    private int shown;
    private DateTime? lastDay;
    private Card? lastCard;
    private readonly Dictionary<Card, StackPanel> groups = [];

    public HistoryPanel(Browser browser) : this(browser, new StackPanel { Spacing = 14 }, new Grid()) { }

    private HistoryPanel(Browser browser, StackPanel content, Grid foot)
        : base("History", content, () => browser.Recalling = false, foot, width: 600)
    {
        this.browser = browser;
        this.foot = foot;
        scroller = Parts.Scroller(days, 420);
        scroller.ViewChanged += (_, _) =>
        {
            if (shown < traces.Count && scroller.VerticalOffset > scroller.ScrollableHeight - 600) More();
        };

        content.Children.Add(hunt);
        content.Children.Add(list);

        hunt.Field.Text = hunted;
        hunt.Field.SelectAll();
        hunt.Changed += text =>
        {
            if (text == hunted) return;
            hunted = text;
            Refresh();
        };
        Loaded += (_, _) => hunt.Field.Focus(FocusState.Programmatic);

        Browsing(fade: false);
        Refresh();
    }

    private void Refresh()
    {
        traces = browser.History.Everything(hunted);
        shown = 0;
        lastDay = null;
        lastCard = null;
        groups.Clear();
        days.Children.Clear();
        list.Children.Clear();
        if (traces.Count == 0)
            list.Children.Add(Parts.Card(Nothing.Make(hunted.Length == 0 ? "Nothing yet." : "Nothing matches.")));
        else
        {
            list.Children.Add(scroller);
            scroller.ChangeView(null, 0, null, true);
            More();
        }
        Count();
    }

    /// The next batch, into the day already open or under a new one.
    private void More()
    {
        var end = Math.Min(traces.Count, shown + Batch);
        for (var i = shown; i < end; i++)
        {
            var trace = traces[i];
            var day = trace.Last.ToLocalTime().Date;
            if (lastCard == null || lastDay != day)
            {
                var group = new StackPanel { Spacing = 6 };
                group.Children.Add(Caption.Make(When.Day(day)));
                lastCard = Parts.Card();
                group.Children.Add(lastCard);
                groups[lastCard] = group;
                days.Children.Add(group);
                lastDay = day;
            }
            var card = lastCard;
            TraceRow? row = null;
            row = new TraceRow(trace,
                go: () =>
                {
                    browser.Recalling = false;
                    browser.Visit(trace.Url, Parts.Apart);
                },
                forget: () =>
                {
                    browser.History.Forget(trace.Key);
                    Forget(card, row!);
                });
            card.Add(row);
        }
        shown = end;
    }

    /// One line gone, and the hairline that went with it — without redrawing
    /// the list, so the place you had scrolled to stays put.
    private void Forget(Card card, TraceRow row)
    {
        traces.Remove(row.Trace);
        shown--;
        var lines = card.Lines.Children;
        var at = lines.IndexOf(row);
        if (at < 0) return;
        lines.RemoveAt(at);
        if (at > 0) lines.RemoveAt(at - 1);
        else if (lines.Count > 0) lines.RemoveAt(0);
        if (lines.Count == 0 && groups.Remove(card, out var group))
        {
            days.Children.Remove(group);
            if (card == lastCard)
            {
                lastCard = null;
                lastDay = null;
            }
        }
        if (traces.Count == 0) Refresh();
        else Count();
    }

    private void Count() => count.Text = traces.Count == 1 ? "1 page" : $"{traces.Count} pages";

    // MARK: - the foot

    private void Browsing() => Browsing(fade: true);

    private void Browsing(bool fade)
    {
        foot.Children.Clear();
        (count.Parent as Panel)?.Children.Remove(count);
        var row = Parts.Ends(count, new Pill("Clear…", Sweeps));
        foot.Children.Add(fade ? Parts.FadeIn(row) : row);
    }

    /// Three separate things, worded so nobody has to guess which one signs
    /// them out of their bank.
    private void Sweeps()
    {
        var card = Parts.Card(
            new Line("History", "Everywhere you have been", new Pill("Clear", () =>
            {
                browser.ClearHistory();
                Refresh();
                Browsing();
            })),
            new Line("Cookies and sign-ins", "Signs you out of every site", new Pill("Sign out of everything", browser.ClearSites)),
            new Line("Cache", "Only what was fetched to draw pages", new Pill("Clear", browser.ClearCache)));
        var back = new Pill("Back", Browsing) { HorizontalAlignment = HorizontalAlignment.Right };
        var stack = new StackPanel { Spacing = 10 };
        stack.Children.Add(card);
        stack.Children.Add(back);
        foot.Children.Clear();
        foot.Children.Add(Parts.FadeIn(stack));
    }

    /// One line. A title, where it came from, and when — the three things you
    /// scan for, in the order you scan them.
    private sealed class TraceRow : Press
    {
        public Trace Trace { get; }

        public TraceRow(Trace trace, Action go, Action forget)
        {
            Trace = trace;
            Padding = new Thickness(14, 9, 14, 9);
            ColumnSpacing = 12;
            BackgroundTransition = new BrushTransition { Duration = Motion.Quick };
            ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

            var mark = new Mark(16) { VerticalAlignment = VerticalAlignment.Center };
            mark.Show(Favicons.Shared.Cached(Address.Host(trace.Url) ?? ""),
                trace.Key.Length > 0 ? trace.Key[..1].ToUpperInvariant() : "•");
            Children.Add(mark);

            var words = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
            words.Children.Add(Kit.Text(trace.Title.Length == 0 ? trace.Key : trace.Title, 13));
            words.Children.Add(Kit.Text(trace.Key, 11.5, Palette.Muted));
            Grid.SetColumn(words, 1);
            Children.Add(words);

            var time = Kit.Text(When.Clock(trace.Last), 11.5, Palette.Muted);
            time.Margin = new Thickness(8, 0, 0, 0);
            var remove = new Quick("Remove", forget, Palette.Brush(Tone.Red, 0.75)) { Visibility = Visibility.Collapsed, Margin = new Thickness(8, 0, 0, 0) };
            Grid.SetColumn(time, 2);
            Grid.SetColumn(remove, 2);
            Children.Add(time);
            Children.Add(remove);

            Hovered += on =>
            {
                Background = on ? Palette.Hover : Palette.Clear;
                time.Visibility = on ? Visibility.Collapsed : Visibility.Visible;
                remove.Visibility = on ? Visibility.Visible : Visibility.Collapsed;
            };
            Clicked += _ => go();
        }
    }
}
