using System.Globalization;
using System.Numerics;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Media;

namespace Search;

// The few things the panels share beyond Kit: how a date is said, how a list
// scrolls, and how a card keeps what is drawn inside it within its corners.

/// Dates, the way the lists say them.
public static class When
{
    /// "3 min. ago", "2 hr. ago" — the Mac's abbreviated relative style,
    /// written out by hand because Windows has no formatter for it.
    public static string Said(DateTime date)
    {
        var seconds = Math.Max(0, (DateTime.UtcNow - date.ToUniversalTime()).TotalSeconds);
        static string Of(double n, string unit) => $"{(int)n} {unit} ago";
        if (seconds < 60) return Of(seconds, "sec.");
        if (seconds < 3600) return Of(seconds / 60, "min.");
        if (seconds < 86400) return Of(seconds / 3600, "hr.");
        var days = seconds / 86400;
        if (days < 7) return (int)days == 1 ? "1 day ago" : Of(days, "days");
        if (days < 30) return Of(days / 7, "wk.");
        if (days < 365) return Of(days / 30, "mo.");
        return Of(days / 365, "yr.");
    }

    /// The time of day. Once a list is grouped by day, that is all a row needs.
    public static string Clock(DateTime date) => date.ToLocalTime().ToString("t", CultureInfo.CurrentCulture);

    /// A day, by its local midnight.
    public static string Day(DateTime day)
    {
        var today = DateTime.Today;
        if (day.Date == today) return "Today";
        if (day.Date == today.AddDays(-1)) return "Yesterday";
        return day.ToString("d MMMM", CultureInfo.CurrentCulture);
    }
}

public static class Parts
{
    /// A list that scrolls without a bar, up to a height and no further.
    public static ScrollViewer Scroller(UIElement content, double maxHeight, bool bar = false) => new()
    {
        Content = content,
        MaxHeight = maxHeight,
        VerticalScrollBarVisibility = bar ? ScrollBarVisibility.Auto : ScrollBarVisibility.Hidden,
        HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
        HorizontalScrollMode = ScrollMode.Disabled,
        ZoomMode = ZoomMode.Disabled,
        IsTabStop = false,
    };

    /// WinUI rounds a panel's own ground but not what is drawn inside it, so a
    /// row lit under the pointer would poke square corners out of its card.
    /// The card's lines are clipped to the card's curve instead, on the
    /// compositor, the way the Mac's clipShape does it.
    public static void Round(FrameworkElement element, float radius)
    {
        var visual = ElementCompositionPreview.GetElementVisual(element);
        var compositor = visual.Compositor;
        var shape = compositor.CreateRoundedRectangleGeometry();
        shape.CornerRadius = new Vector2(radius);
        visual.Clip = compositor.CreateGeometricClip(shape);
        element.SizeChanged += (_, e) => shape.Size = new Vector2((float)e.NewSize.Width, (float)e.NewSize.Height);
    }

    /// A card whose lines stay inside its corners.
    public static Card Card(params UIElement[] lines)
    {
        var card = new Card(lines);
        Round(card.Lines, 10);
        return card;
    }

    /// Arrives by fading in, the way the Mac's .transition(.opacity) does.
    public static T FadeIn<T>(T element, TimeSpan? duration = null) where T : UIElement
    {
        element.Opacity = 0;
        element.OpacityTransition = new ScalarTransition { Duration = duration ?? Motion.Settle };
        if (element is FrameworkElement fe) fe.Loaded += (_, _) => element.Opacity = 1;
        else UI.Soon(() => element.Opacity = 1);
        return element;
    }

    /// A line of words for a panel's foot.
    public static TextBlock Note(string text)
    {
        var t = Kit.Text(text, 12, Palette.Muted);
        return t;
    }

    /// Two things at either end of a row.
    public static Grid Ends(UIElement left, UIElement? right, double spacing = 8)
    {
        var row = new Grid { ColumnSpacing = spacing };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.Children.Add(left);
        if (right is FrameworkElement r)
        {
            r.VerticalAlignment = VerticalAlignment.Center;
            Grid.SetColumn(r, 1);
            row.Children.Add(r);
        }
        return row;
    }

    /// Whether Ctrl is down right now — Ctrl-click sends a page to a tab of
    /// its own, as ⌘-click does on the Mac.
    public static bool Apart => Keys.Down(Keys.Control);
}
