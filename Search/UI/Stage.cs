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
public sealed class Stage : Grid
{
    private readonly Browser browser;
    private readonly Panel pages = Web.Stage;
    private readonly Image cover = new() { Stretch = Stretch.UniformToFill, HorizontalAlignment = HorizontalAlignment.Left, VerticalAlignment = VerticalAlignment.Top, IsHitTestVisible = false };
    private readonly Grid away = new() { Background = Palette.Ground, Visibility = Visibility.Collapsed };
    private readonly Grid trouble = new() { Background = Palette.Ground, Visibility = Visibility.Collapsed };
    private readonly TextBlock troubleText = Kit.Text("", 14);
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

        // What there is to say when the page never came. One line, and the
        // only thing worth offering — another go.
        var words = new StackPanel { Spacing = 10, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
        troubleText.HorizontalAlignment = HorizontalAlignment.Center;
        words.Children.Add(troubleText);
        var again = new Press { HorizontalAlignment = HorizontalAlignment.Center };
        var againText = Kit.Text("Try again", 12, Palette.Muted);
        again.Children.Add(againText);
        again.Hovered += on => againText.Foreground = on ? Palette.Ink : Palette.Muted;
        again.Clicked += _ => browser.Active?.Reload();
        words.Children.Add(again);
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
        troubleText.Text = tab?.Failure ?? "";
        trouble.Visibility = tab?.Failure != null ? Visibility.Visible : Visibility.Collapsed;
    }
}

/// Looking for a word on the page. A pill in the top corner, the same white and
/// hairline as everything else that floats, and gone the moment it isn't wanted.
public sealed class FindBar : Grid
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
