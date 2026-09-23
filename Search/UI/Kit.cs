using System.Numerics;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Markup;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using Microsoft.UI.Xaml.Shapes;

namespace Search;

// The pieces every part of the window is made of, so the strip, the column,
// the field and every panel read as one kind of thing. Built once here; each
// view only says what goes where.

public static class Kit
{
    public static TextBlock Text(string text, double size, Brush? brush = null, bool medium = false, bool semibold = false) => new()
    {
        Text = text,
        FontSize = size,
        Foreground = brush ?? Palette.Ink,
        FontWeight = semibold ? FontWeights.SemiBold : medium ? FontWeights.Medium : FontWeights.Normal,
        TextTrimming = TextTrimming.CharacterEllipsis,
        TextWrapping = TextWrapping.NoWrap,
        VerticalAlignment = VerticalAlignment.Center,
        IsHitTestVisible = false,
    };

    public static Border Rounded(double radius, Brush? fill = null, Brush? stroke = null) => new()
    {
        CornerRadius = new CornerRadius(radius),
        Background = fill,
        BorderBrush = stroke,
        BorderThickness = new Thickness(stroke == null ? 0 : 1),
    };

    /// A soft shadow under something that floats over the page. `depth` is
    /// how far it stands off the page.
    public static void Lift(UIElement element, float depth = 32)
    {
        element.Shadow = new ThemeShadow();
        element.Translation = new Vector3(0, 0, depth);
    }

    private static ControlTemplate? bareField;

    /// A text field with nothing of its own around it: no border, no
    /// underline, no clear button — the plate it sits on is the field. The
    /// selection is a tenth of the ink, not a block of accent colour, which
    /// over a pale ground would be the loudest thing in the window.
    public static TextBox Field(double size, string placeholder = "")
    {
        bareField ??= (ControlTemplate)XamlReader.Load("""
            <ControlTemplate TargetType="TextBox"
                xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
              <ScrollViewer x:Name="ContentElement"
                  HorizontalScrollMode="Auto" HorizontalScrollBarVisibility="Hidden"
                  VerticalScrollMode="Disabled" VerticalScrollBarVisibility="Hidden"
                  IsHorizontalRailEnabled="True" ZoomMode="Disabled" IsTabStop="False"
                  Padding="{TemplateBinding Padding}" />
            </ControlTemplate>
            """);
        var field = new TextBox
        {
            Template = bareField,
            FontSize = size,
            Foreground = Palette.Ink,
            Background = Palette.Clear,
            BorderThickness = new Thickness(0),
            Padding = new Thickness(0),
            MinHeight = 0,
            MinWidth = 0,
            AcceptsReturn = false,
            TextWrapping = TextWrapping.NoWrap,
            IsSpellCheckEnabled = false,
            IsTextPredictionEnabled = false,
            SelectionHighlightColor = Palette.Brush(Tone.Selection),
            SelectionHighlightColorWhenNotFocused = Palette.Brush(Tone.Selection),
            VerticalAlignment = VerticalAlignment.Center,
        };
        if (placeholder.Length > 0) field.PlaceholderText = placeholder;
        return field;
    }

    /// A field with its placeholder drawn by hand, in a grey that stays
    /// readable on a pale ground.
    public static Grid Placeheld(TextBox field, string placeholder, double size)
    {
        var hint = Text(placeholder, size, Palette.Brush(Tone.Ink, 0.3));
        hint.VerticalAlignment = VerticalAlignment.Center;
        var grid = new Grid();
        grid.Children.Add(hint);
        grid.Children.Add(field);
        void Update() => hint.Visibility = field.Text.Length == 0 ? Visibility.Visible : Visibility.Collapsed;
        field.TextChanged += (_, _) => Update();
        Update();
        return grid;
    }
}

/// A small square holding one symbol. Lit when what it opens is open.
public sealed class Door : Press
{
    private readonly Border ground;
    private readonly IconElement glyph;
    private bool on;
    private bool enabled = true;

    public Door(string icon, string help = "", Action? act = null, double size = 26)
    {
        Width = size;
        Height = size;
        ground = Kit.Rounded(8);
        glyph = Icons.Element(icon, 12);
        glyph.HorizontalAlignment = HorizontalAlignment.Center;
        glyph.VerticalAlignment = VerticalAlignment.Center;
        Children.Add(ground);
        Children.Add(glyph);
        Motion.Fades(this);
        if (help.Length > 0) ToolTipService.SetToolTip(this, help);
        Hovered += _ => Paint();
        Clicked += _ => { if (enabled) act?.Invoke(); };
        Paint();
    }

    public string Icon
    {
        get => (glyph as FontIcon)?.Glyph ?? "";
        set { if (glyph is FontIcon font) font.Glyph = value; }
    }

    public bool On { get => on; set { on = value; Paint(); } }

    public bool Enabled
    {
        get => enabled;
        set
        {
            enabled = value;
            Opacity = value ? 1 : 0.3;
            IsHitTestVisible = value;
            Paint();
        }
    }

    private void Paint()
    {
        glyph.Foreground = on ? Palette.Ink : IsHovering && enabled ? Palette.Brush(Tone.Ink, 0.7) : Palette.Muted;
        ground.Background = on ? Palette.Wash : IsHovering && enabled ? Palette.Hover : Palette.Clear;
    }
}

/// A row that is an action rather than a page. Quiet until the pointer is on it.
public sealed class Quiet : Press
{
    public Quiet(string icon, string title, Action act, double height = 28)
    {
        Height = height;
        var ground = Kit.Rounded(9);
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, Padding = new Thickness(10, 0, 0, 0) };
        var glyph = Icons.Make(icon, 10);
        glyph.Width = 15;
        var label = Kit.Text(title, 12.5);
        row.Children.Add(glyph);
        row.Children.Add(label);
        Children.Add(ground);
        Children.Add(row);
        void Paint()
        {
            var ink = IsHovering ? Palette.Brush(Tone.Ink, 0.7) : Palette.Faint;
            glyph.Foreground = ink;
            label.Foreground = ink;
            ground.Background = IsHovering ? Palette.Hover : Palette.Clear;
        }
        Hovered += _ => Paint();
        Clicked += _ => act();
        Paint();
    }
}

/// What stands for a page when there is no room for its title: the site's
/// icon if there is one, and a letter in a faint square until there is.
public sealed class Mark : Grid
{
    private readonly Image image = new() { Stretch = Stretch.UniformToFill };
    private readonly Border plate;
    private readonly TextBlock letter;
    private readonly Border clip;

    public Mark(double size = 16)
    {
        Width = size;
        Height = size;
        IsHitTestVisible = false;
        plate = Kit.Rounded(size * 0.22, Palette.Brush(Tone.Ink, 0.06));
        letter = Kit.Text("", size * 0.56, Palette.Muted, medium: true);
        letter.HorizontalAlignment = HorizontalAlignment.Center;
        letter.TextAlignment = TextAlignment.Center;
        plate.Child = letter;
        clip = new Border { CornerRadius = new CornerRadius(size * 0.22), Child = image };
        Children.Add(plate);
        Children.Add(clip);
        Motion.Fades(this);
    }

    public void Show(ImageSource? icon, string monogram, bool dim = false)
    {
        image.Source = icon;
        clip.Visibility = icon != null ? Visibility.Visible : Visibility.Collapsed;
        plate.Visibility = icon == null ? Visibility.Visible : Visibility.Collapsed;
        letter.Text = monogram;
        Opacity = dim ? 0.45 : 1;
    }
}

/// An almost-closed ring, turning — small enough to sit inside a tab without
/// becoming the loudest thing in it.
public sealed class Ring : Grid
{
    public Ring(double size = 10)
    {
        Width = size;
        Height = size;
        IsHitTestVisible = false;
        var circumference = Math.PI * (size - 1.4);
        var arc = new Ellipse
        {
            Width = size,
            Height = size,
            Stroke = Palette.Brush(Tone.Muted, 0.7),
            StrokeThickness = 1.4,
            StrokeStartLineCap = PenLineCap.Round,
            StrokeEndLineCap = PenLineCap.Round,
            // 78% of the way round, in units of the stroke's width.
            StrokeDashArray = [circumference * 0.78 / 1.4, 1000],
            RenderTransformOrigin = new Windows.Foundation.Point(0.5, 0.5),
        };
        var turn = new RotateTransform();
        arc.RenderTransform = turn;
        Children.Add(arc);
        var spin = new DoubleAnimation { From = 0, To = 360, Duration = TimeSpan.FromSeconds(0.85), RepeatBehavior = RepeatBehavior.Forever };
        Storyboard.SetTarget(spin, turn);
        Storyboard.SetTargetProperty(spin, "Angle");
        var board = new Storyboard();
        board.Children.Add(spin);
        Loaded += (_, _) => board.Begin();
        Unloaded += (_, _) => board.Stop();
    }
}

/// A capsule button with words in it: filled for the one thing a line is
/// about, washed for the rest.
public sealed class Pill : Press
{
    public Pill(string title, Action act, bool filled = false)
    {
        var ground = Kit.Rounded(11);
        var label = Kit.Text(title, 12, filled ? Palette.Ground : Palette.Ink);
        label.Margin = new Thickness(11, 4, 11, 4);
        ground.Child = label;
        Children.Add(ground);
        VerticalAlignment = VerticalAlignment.Center;
        void Paint() => ground.Background = filled
            ? Palette.Brush(Tone.Ink, IsHovering ? 0.8 : 1)
            : IsHovering ? Palette.Hover : Palette.Wash;
        Hovered += _ => Paint();
        Clicked += _ => act();
        Paint();
    }
}

/// On or off: a small track with a knob, the Mac's switch at the Mac's size.
public sealed class Switch : Press
{
    private readonly Border track;
    private readonly Ellipse knob;
    private bool on;
    public event Action<bool>? Toggled;

    public Switch(bool on, Action<bool>? changed = null)
    {
        Width = 32;
        Height = 18;
        VerticalAlignment = VerticalAlignment.Center;
        track = Kit.Rounded(9);
        knob = new Ellipse { Width = 14, Height = 14, Fill = new SolidColorBrush(Microsoft.UI.Colors.White), HorizontalAlignment = HorizontalAlignment.Left, Margin = new Thickness(2, 0, 0, 0) };
        Motion.Glides(knob, Motion.Quick);
        Children.Add(track);
        Children.Add(knob);
        this.on = on;
        if (changed != null) Toggled += changed;
        Clicked += _ => { On = !On; Toggled?.Invoke(On); };
        Paint();
    }

    public bool On { get => on; set { on = value; Paint(); } }

    private void Paint()
    {
        track.Background = on ? Palette.Ink : Palette.Brush(Tone.Ink, 0.12);
        knob.Fill = on ? Palette.Ground : new SolidColorBrush(Microsoft.UI.Colors.White);
        knob.Translation = new Vector3(on ? 14 : 0, 0, 0);
    }
}

/// A choice of a few, side by side, the chosen one lifted.
public sealed class Segmented<T> : Grid where T : notnull
{
    private readonly List<(T value, Border box, TextBlock label)> items = [];
    private T selected;
    public event Action<T>? Changed;

    public Segmented(IEnumerable<(T value, string title)> options, T selected, Action<T>? changed = null)
    {
        this.selected = selected;
        if (changed != null) Changed += changed;
        var ground = Kit.Rounded(8, Palette.Wash);
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 2, Padding = new Thickness(2) };
        ground.Child = row;
        Children.Add(ground);
        VerticalAlignment = VerticalAlignment.Center;
        foreach (var (value, title) in options)
        {
            var press = new Press();
            var box = Kit.Rounded(6);
            var label = Kit.Text(title, 12);
            label.Margin = new Thickness(10, 3, 10, 3);
            box.Child = label;
            press.Children.Add(box);
            press.Clicked += _ => { Selected = value; Changed?.Invoke(value); };
            row.Children.Add(press);
            items.Add((value, box, label));
        }
        Paint();
    }

    public T Selected { get => selected; set { selected = value; Paint(); } }

    private void Paint()
    {
        foreach (var (value, box, label) in items)
        {
            var chosen = EqualityComparer<T>.Default.Equals(value, selected);
            box.Background = chosen ? Palette.Ground : Palette.Clear;
            label.Foreground = chosen ? Palette.Ink : Palette.Muted;
        }
    }
}

// The panels' own pieces: the plate, the cards, the lines — the same in
// Settings, History, Downloads, Passwords and Bookmarks.

/// The plate: a rounded card with a title, a cross, whatever the panel is
/// about, and — when there is one — a foot below a hairline.
public class Plate : Grid
{
    public Plate(string title, UIElement content, Action close, UIElement? foot = null, double width = 560)
    {
        Width = width;
        CornerRadius = new CornerRadius(16);
        Background = Palette.Ground;
        BorderBrush = Palette.Hairline;
        BorderThickness = new Thickness(1);
        HorizontalAlignment = HorizontalAlignment.Center;
        VerticalAlignment = VerticalAlignment.Center;
        Kit.Lift(this, 48);

        var stack = new StackPanel();
        var head = new Grid { Padding = new Thickness(22, 18, 22, 14) };
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        head.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var heading = Kit.Text(title, 17, semibold: true);
        head.Children.Add(heading);
        var cross = new Door(Icons.Close, "Done   Esc", close);
        Grid.SetColumn(cross, 1);
        head.Children.Add(cross);
        stack.Children.Add(head);

        var body = new Border { Padding = new Thickness(22, 0, 22, 0), Child = content };
        stack.Children.Add(body);

        if (foot != null)
        {
            stack.Children.Add(new Rectangle { Height = 1, Fill = Palette.Hairline, Margin = new Thickness(0, 18, 0, 0) });
            stack.Children.Add(new Border { Padding = new Thickness(22, 14, 22, 14), Child = foot });
        }
        else
        {
            stack.Children.Add(new Border { Height = 20 });
        }
        Children.Add(stack);
    }
}

/// A group of lines in one hairline box.
public sealed class Card : Grid
{
    public StackPanel Lines { get; } = new();

    public Card(params UIElement[] lines)
    {
        CornerRadius = new CornerRadius(11);
        Background = Palette.Ground;
        BorderBrush = Palette.Hairline;
        BorderThickness = new Thickness(1);
        Children.Add(Lines);
        foreach (var line in lines) Add(line);
    }

    /// A line, with the hairline between it and the one before.
    public Card Add(UIElement line)
    {
        if (Lines.Children.Count > 0) Lines.Children.Add(new Rule());
        Lines.Children.Add(line);
        return this;
    }
}

/// The hairline between two lines of a card, inset like the text.
public sealed class Rule : Grid
{
    public Rule(double inset = 14)
    {
        Height = 1;
        Background = Palette.Hairline;
        Margin = new Thickness(inset, 0, 0, 0);
    }
}

/// One thing to set or do: what it is on the left, the control on the right.
public sealed class Line : Grid
{
    public Line(string title, string? detail, UIElement? control)
    {
        Padding = new Thickness(14, 11, 14, 11);
        ColumnSpacing = 16;
        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var words = new StackPanel { Spacing = 3, VerticalAlignment = VerticalAlignment.Center };
        var heading = Kit.Text(title, 13);
        heading.TextWrapping = TextWrapping.Wrap;
        words.Children.Add(heading);
        if (!string.IsNullOrEmpty(detail))
        {
            var more = Kit.Text(detail, 11.5, Palette.Muted);
            more.TextWrapping = TextWrapping.Wrap;
            more.MaxLines = 3;
            words.Children.Add(more);
        }
        Children.Add(words);
        if (control is FrameworkElement fe)
        {
            fe.VerticalAlignment = VerticalAlignment.Center;
            Grid.SetColumn(fe, 1);
            Children.Add(fe);
        }
    }
}

/// A small heading over a card, for when a panel has more than one.
public static class Caption
{
    public static TextBlock Make(string text)
    {
        var t = Kit.Text(text, 11.5, Palette.Muted, medium: true);
        t.Margin = new Thickness(2, 0, 0, 0);
        return t;
    }
}

/// The field for narrowing a list. The wash, the glass, the caret.
public sealed class Hunt : Grid
{
    public TextBox Field { get; }
    public event Action<string>? Changed;

    public Hunt(string prompt = "Search")
    {
        CornerRadius = new CornerRadius(10);
        Background = Palette.Wash;
        Padding = new Thickness(12, 8, 12, 8);
        var row = new Grid { ColumnSpacing = 8 };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var glass = Icons.Make(Icons.Search, 12);
        row.Children.Add(glass);
        Field = Kit.Field(13);
        var held = Kit.Placeheld(Field, prompt, 13);
        Grid.SetColumn(held, 1);
        row.Children.Add(held);
        var clear = new Door(Icons.Clear, "", () => Field.Text = "", 18);
        clear.Visibility = Visibility.Collapsed;
        Grid.SetColumn(clear, 2);
        row.Children.Add(clear);
        Field.TextChanged += (_, _) =>
        {
            clear.Visibility = Field.Text.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
            Changed?.Invoke(Field.Text);
        };
        Children.Add(row);
    }
}

/// What a panel says when its list is empty.
public static class Nothing
{
    public static TextBlock Make(string text)
    {
        var t = Kit.Text(text, 13, Palette.Muted);
        t.Margin = new Thickness(14, 18, 14, 18);
        t.TextWrapping = TextWrapping.Wrap;
        return t;
    }
}

/// A small text action inside a row — Show, Copy, Remove.
public sealed class Quick : Press
{
    public Quick(string title, Action act, Brush? tint = null)
    {
        var ground = Kit.Rounded(10, Palette.Wash);
        var label = Kit.Text(title, 11.5, tint ?? Palette.Ink);
        label.Margin = new Thickness(8, 4, 8, 4);
        ground.Child = label;
        Children.Add(ground);
        VerticalAlignment = VerticalAlignment.Center;
        Hovered += on => ground.Background = on ? Palette.Hover : Palette.Wash;
        Clicked += _ => act();
    }
}
