using System.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Windows.System;

namespace Search;

/// Everything below the strip: the page, the picture of it while it wakes, and
/// the sentence that says it never came.
///
/// Every page lives on one stage, and only the live tab's is visible — the
/// others keep their pages, hidden, which is what makes switching instant.
public sealed partial class Stage : Grid
{
    private readonly Browser browser;
    private readonly Panel pages = Web.Stage;
    private readonly Image cover = new() { Stretch = Stretch.UniformToFill, HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top, IsHitTestVisible = false };
    private readonly Grid away = new() { Background = Palette.Ground, Visibility = Visibility.Collapsed };
    private readonly Grid trouble = new() { Background = Palette.Ground, Visibility = Visibility.Collapsed };
    private readonly TextBlock troubleText = Kit.Text("", 22, semibold: true);
    private readonly TextBlock troubleDetail = Kit.Text("", 14, Palette.Muted);
    private readonly FontIcon troubleIcon = Icons.Make(Icons.Globe, 28, Palette.Muted);
    private readonly StackPanel troubleActions = new() { Orientation = Orientation.Horizontal, Spacing = 8 };
    private readonly StackPanel troubleReasons = new() { Spacing = 6 };
    private PageTrouble? shownTrouble;
    private readonly Grid blank = new() { Background = Palette.Ground };
    private Tab? watched;

    public FindBar Find { get; }

    public Stage(Browser browser)
    {
        this.browser = browser;
        Background = Palette.Ground;
        Children.Add(pages);
        // A blank tab has no page; the ground shows through, and the field
        // stands on it.
        Children.Add(blank);

        Motion.Fades(cover, TimeSpan.FromMilliseconds(200));
        Children.Add(cover);

        // The tab is not empty, its page is simply elsewhere. Saying so is
        // kinder than a white rectangle.
        var awayText = Kit.Text("This page is playing in the floating window.", 13, Palette.Muted);
        awayText.HorizontalAlignment = HorizontalAlignment.Center;
        away.Children.Add(awayText);
        Children.Add(away);

        // What there is to say when the page never came: Search's own page,
        // never the engine's. What happened, in a sentence, and what's worth
        // doing about it.
        var words = new StackPanel { Spacing = 12, MaxWidth = 460, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
        troubleIcon.HorizontalAlignment = HorizontalAlignment.Left;
        words.Children.Add(troubleIcon);
        troubleText.TextWrapping = TextWrapping.Wrap;
        words.Children.Add(troubleText);
        troubleDetail.TextWrapping = TextWrapping.Wrap;
        troubleDetail.LineHeight = 20;
        words.Children.Add(troubleDetail);
        // A scam warning's reasons, one sentence each: the difference between
        // "trust me" and something you can check for yourself.
        words.Children.Add(troubleReasons);
        troubleActions.Margin = new Thickness(0, 8, 0, 0);
        words.Children.Add(troubleActions);
        trouble.Children.Add(words);
        Motion.Fades(trouble);
        Children.Add(trouble);

        Find = new FindBar(browser) { HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Top };
        Children.Add(Find);

        var accounts = new AccountList(browser);
        Children.Add(accounts);

        browser.On(nameof(Browser.Active), Watch);
        Watch();
    }

    private void Watch()
    {
        if (watched != null) watched.PropertyChanged -= OnTab;
        watched = browser.Active;
        if (watched != null) watched.PropertyChanged += OnTab;
        Paint();
    }

    private void OnTab(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(Tab.Cover) or nameof(Tab.Failure) or nameof(Tab.Floating) or nameof(Tab.IsBlank) or nameof(Tab.Asleep) or nameof(Tab.Address))
            Paint();
    }

    private void Paint()
    {
        var tab = watched;
        blank.Visibility = tab == null || tab.IsBlank || tab.Asleep ? Visibility.Visible : Visibility.Collapsed;
        cover.Source = tab?.Cover;
        cover.Opacity = tab?.Cover != null ? 1 : 0;
        away.Visibility = tab?.Floating == true ? Visibility.Visible : Visibility.Collapsed;
        ShowTrouble(tab?.Failure);
    }

    private void ShowTrouble(PageTrouble? failure)
    {
        trouble.Visibility = failure != null ? Visibility.Visible : Visibility.Collapsed;
        if (failure == null || failure == shownTrouble) return;
        shownTrouble = failure;
        troubleIcon.Glyph = failure.Glyph;
        troubleText.Text = failure.Headline;
        troubleDetail.Text = failure.Detail;
        troubleReasons.Children.Clear();
        foreach (var reason in failure.Reasons)
        {
            // The dot hangs in its own column, so a reason that wraps lines
            // up under its own first word.
            var line = new Grid { ColumnSpacing = 8 };
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            line.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            var dot = Kit.Text("•", 13, Palette.Muted);
            dot.VerticalAlignment = VerticalAlignment.Top;
            dot.LineHeight = 19;
            var words = Kit.Text(reason, 13, Palette.Muted);
            words.TextWrapping = TextWrapping.Wrap;
            words.LineHeight = 19;
            Grid.SetColumn(words, 1);
            line.Children.Add(dot);
            line.Children.Add(words);
            troubleReasons.Children.Add(line);
        }
        troubleReasons.Visibility = failure.Reasons.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        troubleIcon.Foreground = failure.Kind == TroubleKind.Scam ? Palette.Brush(Tone.Red, 0.9) : Palette.Muted;
        troubleActions.Children.Clear();
        switch (failure.Kind)
        {
            case TroubleKind.Offline:
                // Nothing to press: it comes back by itself.
                troubleActions.Children.Add(Action("Try now", browser.TryAgain, primary: false));
                break;
            case TroubleKind.NoSuchSite:
                troubleActions.Children.Add(Action("Search the web", browser.SearchForTrouble, primary: true));
                troubleActions.Children.Add(Action("Try again", browser.TryAgain, primary: false));
                break;
            case TroubleKind.Insecure:
                troubleActions.Children.Add(Action("Go back", browser.LeaveTrouble, primary: true));
                troubleActions.Children.Add(Action($"Continue to {failure.Host} anyway", browser.TrustAnyway, primary: false));
                break;
            case TroubleKind.Scam:
                // Warned, never blocked: going on is always there, just not
                // the button that looks like the answer.
                troubleActions.Children.Add(Action("Go back", browser.LeaveScam, primary: true));
                if (failure.RealSite is { Length: > 0 } real)
                    troubleActions.Children.Add(Action($"Go to {real}", browser.OpenRealSite, primary: false));
                troubleActions.Children.Add(Action("Continue anyway", browser.ContinueToScam, primary: false));
                break;
            default:
                troubleActions.Children.Add(Action("Try again", browser.TryAgain, primary: true));
                break;
        }
    }

    /// A button on Search's trouble page: the one to press, filled; the
    /// other, just words.
    private static Press Action(string text, Action act, bool primary)
    {
        var button = new Press { HorizontalAlignment = HorizontalAlignment.Left };
        var ground = Kit.Rounded(9, primary ? Palette.Ink : null);
        var words = Kit.Text(text, 13, primary ? Palette.Ground : Palette.Muted, medium: true);
        words.Margin = new Thickness(14, 8, 14, 8);
        button.Children.Add(ground);
        button.Children.Add(words);
        button.Hovered += on =>
        {
            if (primary) ground.Opacity = on ? 0.85 : 1;
            else ground.Background = on ? Palette.Hover : null;
            if (!primary) words.Foreground = on ? Palette.Ink : Palette.Muted;
        };
        button.Clicked += _ => act();
        return button;
    }
}

/// Looking for a word on the page. A pill in the top corner, the same white and
/// hairline as everything else that floats, and gone the moment it isn't wanted.
public sealed partial class FindBar : Grid
{
    private readonly Browser browser;
    private readonly TextBox field;
    private readonly TextBlock count = Kit.Text("", 11.5, Palette.Muted);

    public FindBar(Browser browser)
    {
        this.browser = browser;
        CornerRadius = new CornerRadius(17);
        Background = Palette.Ground;
        BorderBrush = Palette.Hairline;
        BorderThickness = new Thickness(1);
        Padding = new Thickness(16, 5, 8, 5);
        Margin = new Thickness(0, 12, 14, 0);
        Kit.Lift(this, 20);

        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6 };
        field = Kit.Field(12.5);
        var held = Kit.Placeheld(field, "Find on page", 12.5);
        held.Width = 160;
        held.VerticalAlignment = VerticalAlignment.Center;
        row.Children.Add(held);
        count.Margin = new Thickness(0, 0, 4, 0);
        row.Children.Add(count);
        row.Children.Add(new Door(Icons.Up, "Previous   Shift+Enter", () => browser.Look(false), 24));
        row.Children.Add(new Door(Icons.Down, "Next   Enter", () => browser.Look(true), 24));
        row.Children.Add(new Door(Icons.Close, "Done   Esc", browser.CloseFind, 24));
        Children.Add(row);

        field.TextChanged += (_, _) => browser.Needle = field.Text;
        field.KeyDown += (_, e) =>
        {
            if (e.Key == VirtualKey.Enter)
            {
                browser.Look(!Keys.Down(Keys.Shift));
                e.Handled = true;
            }
        };

        Visibility = Visibility.Collapsed;
        Motion.Fades(this);
        browser.OnAny(name =>
        {
            switch (name)
            {
                case nameof(Browser.Finding):
                    Visibility = browser.Finding ? Visibility.Visible : Visibility.Collapsed;
                    if (!browser.Finding) field.Text = "";
                    break;
                case nameof(Browser.FindFocus):
                    UI.Soon(() =>
                    {
                        field.Focus(FocusState.Programmatic);
                        field.SelectAll();
                    });
                    break;
                case nameof(Browser.Missed):
                    BorderBrush = browser.Missed ? Palette.Brush(Tone.Red, 0.35) : Palette.Hairline;
                    break;
                case nameof(Browser.Matches):
                    count.Text = browser.Matches;
                    break;
            }
        });
    }
}
