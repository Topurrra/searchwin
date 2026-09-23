using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Windows.System;

namespace Search;

/// A new space, made where the next one would have been: its name, its
/// icon, and on its way. Escape, Cancel or two fingers back leave it.
///
/// It stands over the column's rows while Browser.MakingSpace is set, on
/// the column's own ground, and is out of sight the rest of the time.
public sealed class NewSpaceCard : Grid
{
    private readonly Browser browser;
    private readonly Press iconButton = new() { Width = 44, Height = 40, HorizontalAlignment = HorizontalAlignment.Center };
    private readonly Border iconGround = Kit.Rounded(10);
    private readonly FontIcon iconGlyph = Icons.Make("", 20, Palette.Ink);
    private readonly TextBox field = Kit.Field(13);
    private readonly Segmented<bool> signIns;
    private readonly TextBlock signInsWords = Kit.Text("", 11, Palette.Muted);
    private readonly Flyout chooser = new() { Placement = FlyoutPlacementMode.Bottom };
    private readonly Dictionary<string, (Border ground, FontIcon glyph)> cells = [];
    private string icon = "briefcase";
    /// Signed in where the other spaces are, or starting afresh.
    private bool shared = true;

    public NewSpaceCard(Browser browser)
    {
        this.browser = browser;
        Background = Palette.Ground;
        Visibility = Visibility.Collapsed;
        Motion.Fades(this);

        var stack = new StackPanel { Spacing = 12, Padding = new Thickness(16), VerticalAlignment = VerticalAlignment.Center };

        // The space's icon, and a click on it for the others: they aren't all
        // laid out on the card.
        iconGlyph.HorizontalAlignment = HorizontalAlignment.Center;
        iconGlyph.VerticalAlignment = VerticalAlignment.Center;
        iconButton.Children.Add(iconGround);
        iconButton.Children.Add(iconGlyph);
        ToolTipService.SetToolTip(iconButton, "Choose an icon");
        iconButton.Hovered += _ => PaintIcon();
        iconButton.Clicked += _ => chooser.ShowAt(iconButton);
        chooser.Content = IconGrid();
        chooser.Closed += (_, _) => { PaintIcon(); Type(); };
        stack.Children.Add(iconButton);

        var heading = new StackPanel { Spacing = 4, HorizontalAlignment = HorizontalAlignment.Center };
        var title = Kit.Text("New space", 13, semibold: true);
        title.HorizontalAlignment = HorizontalAlignment.Center;
        heading.Children.Add(title);
        var sub = Kit.Text("Its own tabs.", 11, Palette.Muted);
        sub.HorizontalAlignment = HorizontalAlignment.Center;
        heading.Children.Add(sub);
        stack.Children.Add(heading);

        var plate = new Grid { Height = 30, CornerRadius = new CornerRadius(8), Background = Palette.Wash, Padding = new Thickness(10, 0, 10, 0) };
        plate.Children.Add(Kit.Placeheld(field, "Name", 13));
        field.KeyDown += OnKey;
        stack.Children.Add(plate);

        // Most people want Google and the rest to know them here too; some
        // want a clean slate.
        var choice = new StackPanel { Spacing = 6, HorizontalAlignment = HorizontalAlignment.Center };
        signIns = new Segmented<bool>([(true, "Same sign-ins"), (false, "Signed out")], true, on => { shared = on; Explain(); })
        {
            HorizontalAlignment = HorizontalAlignment.Center,
        };
        choice.Children.Add(signIns);
        signInsWords.TextWrapping = TextWrapping.Wrap;
        signInsWords.TextAlignment = TextAlignment.Center;
        signInsWords.HorizontalAlignment = HorizontalAlignment.Center;
        choice.Children.Add(signInsWords);
        stack.Children.Add(choice);

        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, HorizontalAlignment = HorizontalAlignment.Center };
        buttons.Children.Add(new Pill("Cancel", Cancel));
        buttons.Children.Add(new Pill("Create", Create, filled: true));
        stack.Children.Add(buttons);

        Children.Add(stack);

        browser.On(nameof(Browser.MakingSpace), Place);
        browser.Prefs.On(nameof(Preferences.Sidebar), Place);
    }

    private void Place()
    {
        var showing = browser.MakingSpace && browser.Prefs.Sidebar;
        if (showing == (Visibility == Visibility.Visible)) return;
        Visibility = showing ? Visibility.Visible : Visibility.Collapsed;
        if (!showing) return;
        // A new card each time: no name, the first icon nobody wears, and
        // signed in with the others.
        field.Text = "";
        icon = browser.FreeIcon;
        shared = true;
        signIns.Selected = true;
        Explain();
        PaintIcon();
        Type();
    }

    private void Type() => UI.Soon(() => { if (Visibility == Visibility.Visible) field.Focus(FocusState.Programmatic); });

    private void Explain() => signInsWords.Text = shared
        ? "Signed in wherever your other spaces are."
        : "Its own cookies and sign-ins, starting from none.";

    private void PaintIcon()
    {
        iconGlyph.Glyph = Space.GlyphOf(icon);
        iconGround.Background = iconButton.IsHovering || chooser.IsOpen ? Palette.Hover : Palette.Clear;
        foreach (var (symbol, (ground, glyph)) in cells)
        {
            var chosen = symbol == icon;
            ground.Background = chosen ? Palette.Wash : Palette.Clear;
            glyph.Foreground = chosen ? Palette.Ink : Palette.Muted;
        }
    }

    /// Every icon, a few to a row, the chosen one on a grey of its own;
    /// picking one puts the list away.
    private Grid IconGrid()
    {
        var grid = new Grid { RowSpacing = 4, ColumnSpacing = 4 };
        const int across = 6;
        for (var c = 0; c < across; c++) grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(28) });
        for (var r = 0; r < (Space.Icons.Length + across - 1) / across; r++) grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(28) });
        for (var i = 0; i < Space.Icons.Length; i++)
        {
            var symbol = Space.Icons[i];
            var cell = new Press { Width = 28, Height = 28 };
            var ground = Kit.Rounded(7);
            var glyph = Icons.Make(Space.GlyphOf(symbol), 13);
            glyph.HorizontalAlignment = HorizontalAlignment.Center;
            glyph.VerticalAlignment = VerticalAlignment.Center;
            cell.Children.Add(ground);
            cell.Children.Add(glyph);
            ToolTipService.SetToolTip(cell, Space.IconNames[i]);
            cell.Hovered += on => { if (symbol != icon) ground.Background = on ? Palette.Hover : Palette.Clear; };
            cell.Clicked += _ =>
            {
                icon = symbol;
                chooser.Hide();
                PaintIcon();
            };
            SetColumn(cell, i % across);
            SetRow(cell, i / across);
            grid.Children.Add(cell);
            cells[symbol] = (ground, glyph);
        }
        return grid;
    }

    private void OnKey(object sender, KeyRoutedEventArgs e)
    {
        switch (e.Key)
        {
            case VirtualKey.Enter:
                e.Handled = true;
                Create();
                break;
            case VirtualKey.Escape:
                e.Handled = true;
                Cancel();
                break;
        }
    }

    private void Create()
    {
        var named = field.Text.Trim();
        if (named.Length == 0) { Type(); return; }
        browser.AddSpace(named, icon, shared);
    }

    private void Cancel() => browser.MakingSpace = false;
}
