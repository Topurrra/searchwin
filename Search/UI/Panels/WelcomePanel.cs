using System.Numerics;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Shapes;
using Windows.System;

namespace Search;

/// The first time. Four short pages over the window, in the app's own
/// language: what this is, what to bring over, how to hold it, and whether
/// links from other apps should come here. Nothing is asked twice, and every
/// page can be skipped.
///
/// A UserControl rather than a Grid only so it can hold the keyboard: Enter
/// is Continue here, as it is the default button on the Mac, and nothing
/// typed should reach the address field waiting underneath.
public sealed partial class WelcomePanel : UserControl
{
    private const int Pages = 4;
    private const double Column = 520;

    private readonly Browser browser;
    private readonly Preferences prefs;
    private readonly Grid stage = new() { MaxWidth = Column, VerticalAlignment = VerticalAlignment.Center };
    private readonly StackPanel dots = new() { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center };
    private readonly Press back;
    private readonly Press skip;
    private readonly Big next;

    private int page;
    private UIElement? showing;
    private bool finished;

    // Bringing things over: the import itself lives with the passwords, and
    // opens once the welcome is done.
    private bool bringing;

    // The default browser.
    private bool isDefault = DefaultBrowser.IsDefault;
    private bool asked;
    private Microsoft.UI.Dispatching.DispatcherQueueTimer? watching;

    public WelcomePanel(Browser browser)
    {
        this.browser = browser;
        prefs = browser.Prefs;
        IsTabStop = true;
        UseSystemFocusVisuals = false;
        Opacity = 1;
        OpacityTransition = new ScalarTransition { Duration = Motion.Settle };

        var ground = new Grid { Background = Palette.Ground };
        var column = new Grid { Padding = new Thickness(40) };
        column.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        column.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        column.Children.Add(stage);

        // The bottom edge: where you are, and the ways on.
        var foot = new Grid { MaxWidth = Column, ColumnSpacing = 14 };
        foot.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        foot.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        foot.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        foot.Children.Add(dots);
        var ways = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 14, VerticalAlignment = VerticalAlignment.Center };
        back = Plain("Back", () => Turn(page - 1));
        skip = Plain("Skip", Finish);
        next = new Big("Continue", () => { if (page < Pages - 1) Turn(page + 1); else Finish(); }, filled: true);
        ways.Children.Add(back);
        ways.Children.Add(skip);
        ways.Children.Add(next);
        Grid.SetColumn(ways, 2);
        foot.Children.Add(ways);
        Grid.SetRow(foot, 1);
        column.Children.Add(foot);
        ground.Children.Add(column);

        // The window's own buttons stay where they always are, over the
        // walk-through, the way the Mac's traffic lights do.
        ground.Children.Add(new WindowButtons(46, 32) { HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Top });
        Content = ground;

        KeyDown += (_, e) =>
        {
            if (e.Key != VirtualKey.Enter) return;
            e.Handled = true;
            if (page < Pages - 1) Turn(page + 1); else Finish();
        };
        Loaded += (_, _) => Focus(FocusState.Programmatic);
        Unloaded += (_, _) => watching?.Stop();

        Place(Build(0), forward: true, animated: false);
        Foot();
    }

    // MARK: - turning

    private void Turn(int to)
    {
        if (to < 0 || to >= Pages || to == page) return;
        var forward = to > page;
        page = to;
        Place(Build(to), forward, animated: true);
        Foot();
        Focus(FocusState.Programmatic);
    }

    /// The new page slides in from the side it is going towards and the old
    /// one out the other, both fading — the Mac's asymmetric offset-and-
    /// opacity, on the compositor.
    private void Place(FrameworkElement next, bool forward, bool animated)
    {
        var shift = forward ? 40f : -40f;
        if (showing is UIElement old)
        {
            if (animated)
            {
                old.IsHitTestVisible = false;
                old.Translation = new Vector3(-shift, 0, 0);
                old.Opacity = 0;
                UI.After(Motion.Glide.TotalSeconds, () => stage.Children.Remove(old));
            }
            else stage.Children.Remove(old);
        }
        next.TranslationTransition = new Vector3Transition { Duration = Motion.Glide };
        next.OpacityTransition = new ScalarTransition { Duration = Motion.Glide };
        if (animated)
        {
            Motion.Jump(next, new Vector3(shift, 0, 0));
            var transition = next.OpacityTransition;
            next.OpacityTransition = null;
            next.Opacity = 0;
            next.OpacityTransition = transition;
            next.Loaded += (_, _) =>
            {
                next.Translation = Vector3.Zero;
                next.Opacity = 1;
            };
        }
        stage.Children.Add(next);
        showing = next;
    }

    private void Foot()
    {
        dots.Children.Clear();
        for (var i = 0; i < Pages; i++)
            dots.Children.Add(new Ellipse { Width = 6, Height = 6, Fill = i == page ? Palette.Ink : Palette.Brush(Tone.Faint, 0.6) });
        back.Visibility = page > 0 ? Visibility.Visible : Visibility.Collapsed;
        skip.Visibility = page < Pages - 1 ? Visibility.Visible : Visibility.Collapsed;
        next.Title = page < Pages - 1 ? "Continue" : "Start browsing";
    }

    private void Finish()
    {
        if (finished) return;
        finished = true;
        prefs.Welcomed = true;
        IsHitTestVisible = false;
        Opacity = 0;
        UI.After(Motion.Settle.TotalSeconds, () =>
        {
            browser.Welcoming = false;
            // Only now, so the passwords open over the window rather than
            // under the welcome.
            if (bringing) browser.Managing = true;
        });
    }

    private FrameworkElement Build(int which) => which switch
    {
        0 => Hello(),
        1 => Bring(),
        2 => Hold(),
        _ => Links(),
    };

    // MARK: - the pages

    private FrameworkElement Hello()
    {
        var stack = new StackPanel { Spacing = 22, HorizontalAlignment = HorizontalAlignment.Center };
        var height = 72 * 0.56;
        var mark = Logomark.Make(height * Logomark.Width / Logomark.Height);
        mark.HorizontalAlignment = HorizontalAlignment.Center;
        stack.Children.Add(mark);
        var words = new StackPanel { Spacing = 10 };
        var name = Kit.Text("Search", 34, medium: true);
        name.HorizontalAlignment = HorizontalAlignment.Center;
        words.Children.Add(name);
        var line = Wrapped("A browser with nothing in the way. The engine already in Windows, and as little around the page as we could manage.", 14.5, Palette.Muted);
        line.TextAlignment = TextAlignment.Center;
        line.HorizontalAlignment = HorizontalAlignment.Center;
        line.MaxWidth = 400;
        line.LineHeight = 14.5 * 1.33 + 3;
        words.Children.Add(line);
        stack.Children.Add(words);
        return stack;
    }

    private FrameworkElement Bring()
    {
        var stack = new StackPanel { Spacing = 22 };
        stack.Children.Add(Heading("Bring things over.",
            "Bookmarks and history from Chrome, Edge, Brave or Arc, and the passwords they export, into Search — the passwords in Windows' Credential Manager. Nothing in the other browser changes."));

        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        void Draw()
        {
            row.Children.Clear();
            if (!bringing)
                row.Children.Add(new Big("Bring them in", () => { bringing = true; Draw(); }, filled: true));
            else
            {
                row.Children.Add(Checked("They open when you start browsing"));
                var undo = Plain("Not now", () => { bringing = false; Draw(); });
                row.Children.Add(undo);
            }
        }
        Draw();
        stack.Children.Add(row);
        return stack;
    }

    private FrameworkElement Hold()
    {
        var stack = new StackPanel { Spacing = 22 };
        stack.Children.Add(Heading("Two ways to hold it.",
            "Titles across the top, or down the side. The grey slides to the tab you pick either way, and you can change your mind with Ctrl+Shift+S."));

        var ways = new Grid { ColumnSpacing = 12 };
        ways.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        ways.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var strip = new Way("Tab strip", sidebar: false);
        var side = new Way("Sidebar", sidebar: true);
        void Paint()
        {
            strip.Chosen = !prefs.Sidebar;
            side.Chosen = prefs.Sidebar;
        }
        strip.Clicked += _ => { prefs.Sidebar = false; Paint(); };
        side.Clicked += _ => { prefs.Sidebar = true; Paint(); };
        Paint();
        Grid.SetColumn(side, 1);
        ways.Children.Add(strip);
        ways.Children.Add(side);
        stack.Children.Add(ways);

        var wear = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        wear.Children.Add(Kit.Text("Tabs wear", 13, Palette.Muted));
        wear.Children.Add(new Segmented<Glyph>(Enum.GetValues<Glyph>().Select(g => (g, g.Title())), prefs.Glyph, v => prefs.Glyph = v));
        stack.Children.Add(wear);
        return stack;
    }

    private FrameworkElement Links()
    {
        var stack = new StackPanel { Spacing = 22 };
        stack.Children.Add(Heading("Links from other apps.",
            "A click in Mail, in Slack, in a PDF — Windows sends it to whichever browser is the default. It can be this one."));

        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        void Draw()
        {
            row.Children.Clear();
            if (isDefault)
            {
                row.Children.Add(Parts.FadeIn(Checked("Search is the default browser")));
                return;
            }
            row.Children.Add(new Big("Make Search the default", async () =>
            {
                asked = true;
                await DefaultBrowser.Become();
                isDefault = DefaultBrowser.IsDefault;
                Draw();
                watching?.Stop();
                if (!isDefault)
                    watching = DefaultBrowser.Watch(() =>
                    {
                        isDefault = true;
                        Draw();
                    });
            }, filled: true));
            if (asked)
            {
                var note = Kit.Text("Windows asks in its own Settings", 13, Palette.Faint);
                row.Children.Add(Parts.FadeIn(note));
            }
        }
        Draw();
        stack.Children.Add(row);

        var worth = new StackPanel { Spacing = 8 };
        var caption = Kit.Text("A FEW THINGS WORTH KNOWING", 11, Palette.Faint, medium: true);
        caption.CharacterSpacing = 55;
        caption.Margin = new Thickness(0, 6, 0, 0);
        worth.Children.Add(caption);
        worth.Children.Add(Key("Ctrl+T", "A new tab. Type a place, or words to search."));
        worth.Children.Add(Key("Ctrl+K", "Every open tab, by name."));
        worth.Children.Add(Key("Ctrl+,", "Settings, including passwords."));
        worth.Children.Add(Key("Alt+1", "Spaces: separate tabs and sign-ins. Turn them on in Settings › Tabs."));
        if (prefs.ClipboardHistory)
            worth.Children.Add(Key("Ctrl+Shift+V", "What you copied: Search keeps it, encrypted on this PC. Turn it off in Settings › Clipboard."));
        stack.Children.Add(worth);
        return stack;
    }

    // MARK: - pieces

    private static TextBlock Wrapped(string text, double size, Brush brush)
    {
        var t = Kit.Text(text, size, brush);
        t.TextWrapping = TextWrapping.Wrap;
        t.TextTrimming = TextTrimming.None;
        return t;
    }

    private static FrameworkElement Heading(string title, string line)
    {
        var stack = new StackPanel { Spacing = 8 };
        stack.Children.Add(Kit.Text(title, 26, medium: true));
        var words = Wrapped(line, 14, Palette.Muted);
        words.LineHeight = 14 * 1.33 + 2;
        stack.Children.Add(words);
        return stack;
    }

    private static FrameworkElement Checked(string text)
    {
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, VerticalAlignment = VerticalAlignment.Center };
        var check = Icons.Make(Icons.Check, 11, Palette.Ink);
        check.VerticalAlignment = VerticalAlignment.Center;
        row.Children.Add(check);
        row.Children.Add(Kit.Text(text, 13));
        return row;
    }

    /// A button that is only its word.
    private static Press Plain(string title, Action act)
    {
        var press = new Press { VerticalAlignment = VerticalAlignment.Center };
        var label = Kit.Text(title, 13, Palette.Muted);
        press.Children.Add(label);
        press.Hovered += on => label.Foreground = on ? Palette.Brush(Tone.Ink, 0.75) : Palette.Muted;
        press.Clicked += _ => act();
        return press;
    }

    /// A keystroke, in its little key, and what it does.
    private static FrameworkElement Key(string keys, string what)
    {
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 10 };
        var cap = new Grid { MinWidth = 44 };
        var face = Kit.Rounded(6, Palette.Wash);
        face.HorizontalAlignment = HorizontalAlignment.Center;
        var label = Kit.Text(keys, 12, Palette.Ink, medium: true);
        label.Margin = new Thickness(7, 3, 7, 3);
        face.Child = label;
        cap.Children.Add(face);
        row.Children.Add(cap);
        row.Children.Add(Kit.Text(what, 13, Palette.Muted));
        return row;
    }

    /// The big capsule: filled in ink for the way on.
    private sealed partial class Big : Press
    {
        private readonly TextBlock label;

        public Big(string title, Action act, bool filled = false)
        {
            VerticalAlignment = VerticalAlignment.Center;
            var ground = Kit.Rounded(17);
            label = Kit.Text(title, 13, filled ? Palette.Ground : Palette.Ink, medium: true);
            label.Margin = new Thickness(16, 9, 16, 9);
            ground.Child = label;
            Children.Add(ground);
            void Paint() => ground.Background = filled ? Palette.Ink : IsHovering ? Palette.Hover : Palette.Wash;
            Hovered += _ => Paint();
            Clicked += _ => act();
            Paint();
        }

        public string Title { set => label.Text = value; }
    }

    /// One of the two ways, as a small drawing of the window.
    private sealed partial class Way : Press
    {
        private readonly Border ground = Kit.Rounded(14);
        private readonly TextBlock label;
        private bool chosen;

        public Way(string title, bool sidebar)
        {
            ground.BorderThickness = new Thickness(1);
            ground.Padding = new Thickness(10);
            ground.BackgroundTransition = new BrushTransition { Duration = Motion.Quick };
            var stack = new StackPanel { Spacing = 10 };

            var window = new Grid
            {
                Height = 110,
                CornerRadius = new CornerRadius(8),
                Background = Palette.Ground,
                BorderBrush = Palette.Hairline,
                BorderThickness = new Thickness(1),
            };
            // Windows keeps its three buttons at the top right — at the end
            // of the strip, or in the column's corner.
            static StackPanel Buttons()
            {
                var three = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 3 };
                for (var i = 0; i < 3; i++) three.Children.Add(new Ellipse { Width = 5, Height = 5, Fill = Palette.Faint });
                return three;
            }
            static Border Bar(int i, double width = double.NaN) =>
                new() { Height = 8, Width = width, CornerRadius = new CornerRadius(3), Background = i == 0 ? Palette.Wash : Palette.Hover };
            if (sidebar)
            {
                var row = new Grid();
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(62) });
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1) });
                row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
                var column = new StackPanel { Spacing = 4, Padding = new Thickness(8) };
                var corner = Buttons();
                corner.HorizontalAlignment = HorizontalAlignment.Right;
                corner.Margin = new Thickness(0, 0, 0, 4);
                column.Children.Add(corner);
                for (var i = 0; i < 4; i++) column.Children.Add(Bar(i));
                row.Children.Add(column);
                var edge = new Grid { Background = Palette.Hairline };
                Grid.SetColumn(edge, 1);
                row.Children.Add(edge);
                window.Children.Add(row);
            }
            else
            {
                var row = new Grid { Padding = new Thickness(8), VerticalAlignment = VerticalAlignment.Top };
                var tabs = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 3 };
                for (var i = 0; i < 4; i++) tabs.Children.Add(Bar(i, 30));
                row.Children.Add(tabs);
                var corner = Buttons();
                corner.HorizontalAlignment = HorizontalAlignment.Right;
                corner.VerticalAlignment = VerticalAlignment.Center;
                row.Children.Add(corner);
                window.Children.Add(row);
            }
            stack.Children.Add(window);

            label = Kit.Text(title, 13);
            stack.Children.Add(label);
            ground.Child = stack;
            Children.Add(ground);
            Hovered += _ => Paint();
            Paint();
        }

        public bool Chosen { get => chosen; set { chosen = value; Paint(); } }

        private void Paint()
        {
            ground.Background = chosen ? Palette.Wash : IsHovering ? Palette.Hover : Palette.Clear;
            ground.BorderBrush = chosen ? Palette.Brush(Tone.Ink, 0.35) : Palette.Hairline;
            label.FontWeight = chosen ? FontWeights.Medium : FontWeights.Normal;
            label.Foreground = chosen ? Palette.Ink : Palette.Muted;
        }
    }
}
