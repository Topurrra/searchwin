using System.Numerics;
using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Markup;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Windows.UI;

namespace Search;

// Lifted from Office Inspiration, with the ground turned white: there the work
// floats on an off-white canvas, here the page *is* the ground and everything
// the browser draws has to get out of its way.
//
// Every colour is a pair — one for a light window, one for a dark — and every
// brush handed out here is repainted in place when the window changes between
// them. Nothing else in the code knows which mode it is in.
public enum Tone { Ground, Ink, Muted, Faint, Hairline, Wash, Hover, Resting, Selection, Black, White, Red }

public static class Palette
{
    private static bool dark;
    public static bool Dark => dark;

    /// Fires after every brush has been repainted, for the few things that
    /// hold a colour rather than a brush.
    public static event Action? Turned;

    private static readonly Dictionary<(Tone, int), SolidColorBrush> brushes = [];

    private static double White(Tone tone) => tone switch
    {
        Tone.Ground => dark ? 0.11 : 1.0,
        Tone.Ink => dark ? 0.93 : 0.09,            // neutral-900 · neutral-100
        Tone.Muted => dark ? 0.58 : 0.55,          // neutral-500
        Tone.Faint => dark ? 0.32 : 0.83,          // neutral-300 · neutral-700
        Tone.Hairline => dark ? 0.20 : 0.91,       // neutral-200 · neutral-800
        Tone.Wash => dark ? 0.175 : 0.937,         // the live tab
        Tone.Hover => dark ? 0.15 : 0.965,         // the one under the pointer
        Tone.Resting => dark ? 0.30 : 0.80,
        // A tenth of the ink over the ground, already mixed: a field's
        // selection is drawn opaque whatever its brush says.
        Tone.Selection => dark ? 0.11 + (0.93 - 0.11) * 0.14 : 1 - 0.91 * 0.12,
        Tone.White => 1,
        _ => 0,
    };

    public static Color ColorOf(Tone tone, double alpha = 1)
    {
        if (tone == Tone.Red) return Color.FromArgb((byte)Math.Round(alpha * 255), 0xFF, 0x3B, 0x30);
        var w = (byte)Math.Round(White(tone) * 255);
        return Color.FromArgb((byte)Math.Round(Math.Clamp(alpha, 0, 1) * 255), w, w, w);
    }

    /// One brush per tone and opacity, shared by everything that asks, and
    /// repainted when the look turns over.
    public static SolidColorBrush Brush(Tone tone, double alpha = 1)
    {
        var key = (tone, (int)Math.Round(alpha * 1000));
        if (!brushes.TryGetValue(key, out var brush))
        {
            brush = new SolidColorBrush(ColorOf(tone, alpha));
            brushes[key] = brush;
        }
        return brush;
    }

    public static SolidColorBrush Ground => Brush(Tone.Ground);
    public static SolidColorBrush Ink => Brush(Tone.Ink);
    public static SolidColorBrush Muted => Brush(Tone.Muted);
    public static SolidColorBrush Faint => Brush(Tone.Faint);
    public static SolidColorBrush Hairline => Brush(Tone.Hairline);
    public static SolidColorBrush Wash => Brush(Tone.Wash);
    public static SolidColorBrush Hover => Brush(Tone.Hover);
    public static SolidColorBrush Clear { get; } = new(Colors.Transparent);

    public static void SetDark(bool value)
    {
        if (dark == value && brushes.Count > 0) return;
        dark = value;
        foreach (var ((tone, alpha), brush) in brushes) brush.Color = ColorOf(tone, alpha / 1000.0);
        Turned?.Invoke();
    }
}

public static class Metrics
{
    /// The tab strip. On the Mac the title bar grows to match it; here the
    /// window has no title bar of its own and the strip is the title bar.
    public const double Strip = 52;
    /// Where the first tab starts. The Mac leaves 100 for its traffic lights;
    /// Windows keeps its three buttons at the other end, so the row starts
    /// with the air a tab row has on its left and nothing else.
    public const double Lights = 12;
    /// Back, forward and reload: three doors and the air before the next one.
    public const double Helm = 3 * 26 + 2 * 2 + 8;
    /// The window's own three buttons — minimise, maximise, close — at the far
    /// end of the strip, the width Windows gives them.
    public const double Caption = 3 * 46;
    /// The same three in the column's corner, a little narrower.
    public const double SideCaption = 3 * 34;
    public const double SideLights = 0;
    /// The band left at the top when there is no strip.
    public const double Bare = 34;
    /// Tabs are a fixed width rather than the width of their titles, so the
    /// cross always lands in the same place and the row never rearranges
    /// itself while you read it. They give way when there are too many:
    /// narrower than TabTitled they show their site's mark alone, and they
    /// stop at TabMinWidth, the mark and its air. Past that the row scrolls.
    /// The Mac's is 186; Windows draws everything a quarter bigger on most
    /// screens, so the same number reads long here.
    public const double TabWidth = 160;
    public const double TabTitled = 80;
    public const double TabMinWidth = 36;
    public const double TabGap = 2;
    /// A pinned tab is a square the height of the row, holding one letter.
    public const double PinWidth = 30;
    /// The square at the end of the row that opens a new page.
    public const double PlusWidth = 30;
    /// The address field, in both the places it shows up.
    public const double FieldWidth = 560;
    /// The column of titles down the left, in the way that has one.
    public const double Side = 232;
    /// Wide enough for back, forward and reload and the window's three buttons
    /// beside them in the corner.
    public const double SideMin = 196;
    public const double SideMax = 440;
}

// One spring for anything that moves between two places, one for anything that
// arrives or leaves. Using the same two everywhere is most of why a thing feels
// like a single piece of software rather than a pile of views.
public static class Motion
{
    public static readonly TimeSpan Glide = TimeSpan.FromMilliseconds(340);
    public static readonly TimeSpan Settle = TimeSpan.FromMilliseconds(300);
    public static readonly TimeSpan Quick = TimeSpan.FromMilliseconds(140);

    /// The spring's look, as an easing: fast out, long soft landing.
    public static EasingFunctionBase Ease => new ExponentialEase { EasingMode = EasingMode.EaseOut, Exponent = 5 };

    /// Moves follow the element on its own, on the compositor: set
    /// Translation and it glides there.
    public static void Glides(UIElement e, TimeSpan? duration = null) =>
        e.TranslationTransition = new Vector3Transition { Duration = duration ?? Glide };

    public static void Fades(UIElement e, TimeSpan? duration = null) =>
        e.OpacityTransition = new ScalarTransition { Duration = duration ?? Quick };

    public static void Grows(UIElement e, TimeSpan? duration = null) =>
        e.ScaleTransition = new Vector3Transition { Duration = duration ?? Settle };

    /// Straight there, no glide — for the thing under the hand.
    public static void Jump(UIElement e, Vector3 to)
    {
        var transition = e.TranslationTransition;
        e.TranslationTransition = null;
        e.Translation = to;
        e.TranslationTransition = transition;
    }

    /// Animates one double property of one element.
    public static void To(DependencyObject target, string property, double to, TimeSpan duration, EasingFunctionBase? ease = null, Action? done = null)
    {
        var animation = new DoubleAnimation
        {
            To = to,
            Duration = duration,
            EasingFunction = ease ?? Ease,
            EnableDependentAnimation = true,
        };
        Storyboard.SetTarget(animation, target);
        Storyboard.SetTargetProperty(animation, property);
        var board = new Storyboard();
        board.Children.Add(animation);
        if (done != null) board.Completed += (_, _) => done();
        board.Begin();
    }

    /// Wrong address, said without a dialog: the field shivers and stops.
    /// Three there-and-backs, tapering to nothing, so it settles rather than
    /// stopping mid-swing.
    public static void Shake(UIElement e)
    {
        if (e.RenderTransform is not TranslateTransform shift)
        {
            shift = new TranslateTransform();
            e.RenderTransform = shift;
        }
        var frames = new DoubleAnimationUsingKeyFrames { EnableDependentAnimation = true };
        const int steps = 40;
        for (var i = 0; i <= steps; i++)
        {
            var t = i / (double)steps;
            // easeOut over half a second, as the Mac does it.
            var eased = 1 - Math.Pow(1 - t, 2);
            frames.KeyFrames.Add(new LinearDoubleKeyFrame
            {
                KeyTime = KeyTime.FromTimeSpan(TimeSpan.FromSeconds(0.5 * t)),
                Value = Math.Sin(eased * Math.PI * 6) * 7 * (1 - eased),
            });
        }
        Storyboard.SetTarget(frames, shift);
        Storyboard.SetTargetProperty(frames, "X");
        var board = new Storyboard();
        board.Children.Add(frames);
        board.Begin();
    }
}

/// The symbols, from Segoe Fluent Icons — what SF Symbols are on the Mac.
public static class Icons
{
    public const string Plus = "";
    public const string Back = "";
    public const string Forward = "";
    public const string Reload = "";
    public const string Close = "";
    /// Drawn, not a glyph (see Element).
    public const string Bookmark = "bookmark";
    public const string Search = "";
    public const string Speaker = "";
    public const string Private = "";
    public const string Robot = "";
    public const string Puzzle = "";
    public const string Microphone = "";
    public const string Video = "";
    public const string Up = "";
    public const string Down = "";
    public const string Key = "";
    public const string Settings = "";
    public const string Download = "";
    public const string Shield = "";
    public const string Warning = "\xE7BA";
    public const string Info = "";
    public const string Window = "";
    public const string Tabs = "";
    public const string History = "";
    public const string Folder = "";
    public const string Globe = "";
    public const string Check = "";
    public const string Pin = "";
    public const string Minimize = "";
    public const string Maximize = "";
    public const string Restore = "";
    public const string ChromeClose = "";
    public const string Left = "";
    public const string Right = "";
    public const string Clear = "";
    public const string Star = "";
    public const string Menu = "";
    // What the field's engine rows are (Omnibox).
    public const string Document = "";
    public const string Picture = "";
    public const string Music = "";
    public const string App = "";
    public const string Calculator = "";
    public const string Clipboard = "";

    public static readonly FontFamily Font = new("Segoe Fluent Icons, Segoe MDL2 Assets");

    /// Any of the above, or one of the few drawn by hand because the font has
    /// nothing like it — the bookmark's ribbon.
    public static IconElement Element(string glyph, double size, Brush? brush = null)
    {
        if (glyph != Bookmark) return Make(glyph, size, brush);
        var s = size / 12;
        string P(double x, double y) => $"{(x * s).ToString(System.Globalization.CultureInfo.InvariantCulture)},{(y * s).ToString(System.Globalization.CultureInfo.InvariantCulture)}";
        var data = $"F0 M{P(1, 0)} L{P(10, 0)} L{P(10, 13)} L{P(5.5, 9.6)} L{P(1, 13)} Z M{P(2.3, 1.3)} L{P(8.7, 1.3)} L{P(8.7, 10.4)} L{P(5.5, 8)} L{P(2.3, 10.4)} Z";
        return new PathIcon
        {
            Data = Paths.Parse(data),
            Foreground = brush ?? Palette.Muted,
            Width = 11 * s,
            Height = 13 * s,
            IsHitTestVisible = false,
        };
    }

    /// A glyph from the font. The drawn ones (Bookmark) aren't in it: asked
    /// for here they come out as the nearest glyph the font has, a star,
    /// rather than as a row of boxes — use Element for the real thing.
    public static FontIcon Make(string glyph, double size, Brush? brush = null) => new()
    {
        Glyph = glyph == Bookmark ? Star : glyph,
        FontFamily = Font,
        FontSize = size,
        Foreground = brush ?? Palette.Muted,
        IsHitTestVisible = false,
    };
}

/// Path data — absolute M, L, H, V, C and Z, what Figma writes for a flattened
/// shape — read into a geometry by hand, the way the Mac's Logomark reads its
/// own. XAML can parse it too, but only by reflection, which a native build
/// doesn't have.
public static class Paths
{
    public static Geometry Parse(string data)
    {
        var geometry = new PathGeometry();
        var text = data.Trim();
        if (text.StartsWith("F0")) { geometry.FillRule = FillRule.EvenOdd; text = text[2..]; }
        else if (text.StartsWith("F1")) { geometry.FillRule = FillRule.Nonzero; text = text[2..]; }

        PathFigure? figure = null;
        var last = new Windows.Foundation.Point();
        var start = last;
        foreach (System.Text.RegularExpressions.Match m in System.Text.RegularExpressions.Regex.Matches(text, "([MLHVCZ])([^MLHVCZ]*)"))
        {
            var n = System.Text.RegularExpressions.Regex.Matches(m.Groups[2].Value, @"-?(?:\d+\.?\d*|\.\d+)(?:[eE]-?\d+)?")
                .Select(x => double.Parse(x.Value, System.Globalization.CultureInfo.InvariantCulture)).ToArray();
            switch (m.Groups[1].Value)
            {
                case "M":
                    last = start = new(n[0], n[1]);
                    figure = new PathFigure { StartPoint = last, IsClosed = false, IsFilled = true };
                    geometry.Figures.Add(figure);
                    for (var k = 2; k + 1 < n.Length; k += 2)
                        figure.Segments.Add(new LineSegment { Point = last = new(n[k], n[k + 1]) });
                    break;
                case "L":
                    for (var k = 0; k + 1 < n.Length; k += 2)
                        figure?.Segments.Add(new LineSegment { Point = last = new(n[k], n[k + 1]) });
                    break;
                case "H":
                    foreach (var x in n) figure?.Segments.Add(new LineSegment { Point = last = new(x, last.Y) });
                    break;
                case "V":
                    foreach (var y in n) figure?.Segments.Add(new LineSegment { Point = last = new(last.X, y) });
                    break;
                case "C":
                    for (var k = 0; k + 5 < n.Length; k += 6)
                        figure?.Segments.Add(new BezierSegment
                        {
                            Point1 = new(n[k], n[k + 1]),
                            Point2 = new(n[k + 2], n[k + 3]),
                            Point3 = last = new(n[k + 4], n[k + 5]),
                        });
                    break;
                case "Z":
                    if (figure != null) figure.IsClosed = true;
                    last = start;
                    break;
            }
        }
        return geometry;
    }
}

/// Search's mark — a pill with an S cut out of it, read from its own path
/// data so it stays a crisp vector at any size. The same path as the Mac app
/// and the website's mark.
public static class Logomark
{
    public const double Width = 608, Height = 276;

    private const string Data = "M469.443 0C545.471 0.00013198 607.103 61.6325 607.104 137.66C607.104 213.688 545.471 275.321 469.443 275.321H137.66C61.6323 275.321 0 213.688 0 137.66C0.00016085 61.6325 61.6325 0.000140192 137.66 0H469.443ZM138.104 51.5977C127.234 51.5977 117.512 53.5115 108.938 57.3389C100.518 61.0132 93.8581 66.2188 88.959 72.9551C84.2132 79.5381 81.8398 87.3464 81.8398 96.3789C81.8399 105.258 83.6773 112.607 87.3516 118.425C91.0258 124.089 95.9251 128.682 102.049 132.203C108.173 135.571 114.833 138.327 122.028 140.471L151.652 149.197C158.389 151.188 163.9 154.249 168.187 158.383C172.473 162.516 174.617 168.028 174.617 174.917C174.617 182.572 171.402 188.849 164.972 193.748C158.695 198.494 150.122 200.867 139.252 200.867C132.21 200.867 125.702 199.413 119.731 196.504C113.914 193.442 109.091 189.308 105.264 184.103C101.436 178.744 99.2169 172.697 98.6045 165.961H97.6855L75.4102 171.013C76.3287 180.658 79.697 189.308 85.5146 196.963C91.3322 204.618 98.9103 210.665 108.249 215.104C117.741 219.544 128.076 221.765 139.252 221.765C151.193 221.765 161.68 219.774 170.713 215.794C179.746 211.813 186.711 206.225 191.61 199.029C196.662 191.834 199.188 183.414 199.188 173.769C199.188 164.124 197.352 156.239 193.678 150.115C190.003 143.838 185.104 138.863 178.98 135.188C172.857 131.514 166.044 128.605 158.542 126.462L128.229 117.735C121.799 115.898 116.516 113.219 112.383 109.698C108.402 106.177 106.412 101.354 106.412 95.2305C106.412 88.188 109.168 82.6758 114.68 78.6953C120.344 74.5619 128.152 72.4951 138.104 72.4951C147.901 72.4952 155.939 74.9448 162.216 79.8438C168.493 84.7428 172.397 91.1729 173.928 99.1338H174.847L196.663 93.8525C195.745 85.5853 192.605 78.3131 187.247 72.0361C181.889 65.6061 174.923 60.6306 166.35 57.1094C157.929 53.4351 148.514 51.5977 138.104 51.5977Z";

    /// The mark, fitted into a box of the given width, filled with `brush`.
    public static FrameworkElement Make(double width, Brush? brush = null)
    {
        var geometry = Paths.Parse("F0 " + Data);
        var path = new Microsoft.UI.Xaml.Shapes.Path
        {
            Data = geometry,
            Fill = brush ?? Palette.Ink,
            Stretch = Stretch.Uniform,
            Width = width,
            Height = width * Height / Width,
            IsHitTestVisible = false,
        };
        return path;
    }
}
