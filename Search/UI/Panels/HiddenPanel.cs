using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace Search;

/// What you have taken off this site, and the way back.
///
/// A list of selectors is not something anyone can read. So resting the pointer
/// on a row puts that one thing back on the page, outlined, and scrolls to it —
/// you decide what to restore by looking at it, not by decoding its name.
public sealed class HiddenPanel : Plate
{
    private readonly Browser browser;
    private readonly StackPanel body;
    private readonly StackPanel foot;

    public HiddenPanel(Browser browser) : this(browser, new StackPanel(), new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 }) { }

    private HiddenPanel(Browser browser, StackPanel body, StackPanel foot)
        : base(browser.HereHost ?? "This page", body, () => browser.Reviewing = false, foot, width: 380)
    {
        this.browser = browser;
        this.body = body;
        this.foot = foot;
        Fill();

        // Put back from here, or hidden from the page behind: the list is
        // whatever the site holds now.
        Action changed = () => UI.Soon(Fill);
        Loaded += (_, _) => Curtain.Changed += changed;
        Unloaded += (_, _) => Curtain.Changed -= changed;
        // Leaving the panel puts the page back the way it was.
        PointerExited += (_, e) => { if (Left(this, e)) browser.StopPeeking(); };
    }

    private void Fill()
    {
        body.Children.Clear();
        foot.Children.Clear();
        var veils = browser.HereVeils;
        if (veils.Count == 0)
        {
            body.Children.Add(new Card(Nothing.Make("Nothing is hidden here.")));
        }
        else
        {
            body.Spacing = 6;
            body.Children.Add(Caption.Make("Hidden on this site — rest on a line to see it"));
            var card = new Card();
            foreach (var veil in veils.ToList()) card.Add(Row(veil));
            body.Children.Add(new ScrollViewer
            {
                Content = card,
                MaxHeight = 320,
                VerticalScrollBarVisibility = ScrollBarVisibility.Hidden,
                Padding = new Thickness(0, 0, 0, 2),
            });
        }
        foot.Children.Add(new Pill("Hide something…", browser.ToggleHiding, filled: true));
        if (veils.Count > 0) foot.Children.Add(new Pill("Restore all", browser.RestoreAll));
    }

    private Grid Row(Veil veil)
    {
        var row = new Grid { Padding = new Thickness(14, 9, 14, 9), ColumnSpacing = 8, Background = Palette.Clear };
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        row.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        var words = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
        var label = Kit.Text(veil.Label, 13, Palette.Ink);
        label.MaxLines = 1;
        label.TextTrimming = TextTrimming.CharacterEllipsis;
        words.Children.Add(label);
        if (!string.IsNullOrEmpty(veil.Note))
        {
            var note = Kit.Text(veil.Note!, 11.5, Palette.Muted);
            note.MaxLines = 1;
            note.TextTrimming = TextTrimming.CharacterEllipsis;
            words.Children.Add(note);
        }
        row.Children.Add(words);

        var restore = new Quick("Restore", () => browser.Restore(veil)) { Opacity = 0 };
        Motion.Fades(restore);
        Grid.SetColumn(restore, 1);
        row.Children.Add(restore);

        void Hover(bool inside)
        {
            row.Background = inside ? Palette.Hover : Palette.Clear;
            restore.Opacity = inside ? 1 : 0;
            if (inside) browser.Peek(veil);
        }
        row.PointerEntered += (_, _) => Hover(true);
        row.PointerExited += (_, e) => { if (Left(row, e)) Hover(false); };
        return row;
    }

    /// Whether the pointer has really gone from an element. An exit from
    /// anything inside it bubbles up as though from the element itself —
    /// moving off the Restore button onto its own row is not leaving the row.
    private static bool Left(FrameworkElement element, PointerRoutedEventArgs e)
    {
        var at = e.GetCurrentPoint(element).Position;
        return at.X < 0 || at.Y < 0 || at.X >= element.ActualWidth || at.Y >= element.ActualHeight;
    }
}
