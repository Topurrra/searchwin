using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace Search;

/// The accounts kept for a site, hanging from the sign-in box the caret is
/// in. The same white and hairline as everything else that floats over a
/// page; one line per account, the name in ink and the site under it in grey;
/// a click puts both into the form. It follows the box when the page scrolls,
/// and goes when the caret does.
///
/// It covers the whole stage but takes clicks only where the list is: the
/// ground around it has no background, so the page under it still gets them.
public sealed partial class AccountList : Grid
{
    private readonly Browser browser;
    private readonly Grid card = new();
    private readonly StackPanel rows = new();

    public AccountList(Browser browser)
    {
        this.browser = browser;
        Background = null;

        card.HorizontalAlignment = HorizontalAlignment.Left;
        card.VerticalAlignment = VerticalAlignment.Top;
        card.CornerRadius = new CornerRadius(10);
        card.Background = Palette.Ground;
        card.BorderBrush = Palette.Hairline;
        card.BorderThickness = new Thickness(1);
        card.Visibility = Visibility.Collapsed;
        Kit.Lift(card, 32);
        card.Children.Add(rows);
        // Whatever the page says about its caret while a row is held down,
        // the list waits for the press to finish.
        card.AddHandler(PointerPressedEvent, new PointerEventHandler((_, _) => browser.HoldChoice()), true);
        Children.Add(card);

        browser.On(nameof(Browser.Suggesting), Paint);
        browser.On(nameof(Browser.Active), Paint);
    }

    private Browser.Accounts? drawn;

    private void Paint()
    {
        if (browser.Suggesting is not { } asked || asked.Tab != browser.ActiveID)
        {
            card.Visibility = Visibility.Collapsed;
            drawn = null;
            return;
        }
        // Just under the box, left edges lined up. The offset is from the
        // stage's top-left, which is also the page's.
        var spot = asked.Spot;
        card.Width = Math.Max(240, Math.Min(360, spot.Width));
        card.Margin = new Thickness(spot.X, spot.Y + spot.Height + 6, 0, 0);
        card.Visibility = Visibility.Visible;
        // A box that only moved keeps the rows it has.
        if (drawn != null && ReferenceEquals(drawn.Logins, asked.Logins)) { drawn = asked; return; }
        drawn = asked;

        rows.Children.Clear();
        for (var i = 0; i < asked.Logins.Count; i++)
            rows.Children.Add(Row(asked.Logins[i], first: i == 0));

        var foot = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 6,
            Padding = new Thickness(12, 7, 12, 7),
            Background = Palette.Brush(Tone.Wash, 0.5),
            CornerRadius = new CornerRadius(0, 0, 9, 9),
        };
        foot.Children.Add(Icons.Make(Icons.Key, 9, Palette.Faint));
        foot.Children.Add(Kit.Text("From Credential Manager", 10.5, Palette.Faint));
        rows.Children.Add(foot);
    }

    private Press Row(Browser.Login login, bool first)
    {
        var press = new Press
        {
            Padding = new Thickness(10, 7, 10, 7),
            CornerRadius = first ? new CornerRadius(9, 9, 0, 0) : new CornerRadius(0),
        };
        var line = new Grid { ColumnSpacing = 10 };
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        var letter = Kit.Text(login.User.Length > 0 ? char.ToUpperInvariant(login.User[0]).ToString() : "•", 11, medium: true);
        letter.HorizontalAlignment = HorizontalAlignment.Center;
        letter.TextAlignment = TextAlignment.Center;
        var square = new Grid { Width = 22, Height = 22, CornerRadius = new CornerRadius(6), Background = Palette.Wash, IsHitTestVisible = false };
        square.Children.Add(letter);
        line.Children.Add(square);
        var words = new StackPanel { Spacing = 1, VerticalAlignment = VerticalAlignment.Center };
        words.Children.Add(Kit.Text(login.User.Length == 0 ? "No name" : login.User, 12.5));
        words.Children.Add(Kit.Text(login.Host, 10.5, Palette.Muted));
        Grid.SetColumn(words, 1);
        line.Children.Add(words);
        press.Children.Add(line);
        press.Hovered += on => press.Background = on ? Palette.Hover : Palette.Clear;
        press.Clicked += _ => browser.Choose(login);
        return press;
    }
}
