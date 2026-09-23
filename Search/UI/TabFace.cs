using System.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.System;

namespace Search;

/// What sits inside a tab, wherever the tab is drawn — across the top or down
/// the side: its mark, its name, and one slot at the right-hand end doing two
/// jobs — the cross when the pointer is on it, the ring while the page is still
/// coming, the speaker while it makes noise.
public sealed class TabFace : Grid
{
    private readonly Browser browser;
    public Tab Tab { get; }
    private readonly Mark mark = new(15);
    private readonly FontIcon robot = Icons.Make(Icons.Robot, 10);
    private readonly FontIcon shy = Icons.Make(Icons.Private, 10);
    private readonly TextBlock title = Kit.Text("", 12.5);
    private readonly Grid slot = new() { Width = 15, Height = 15 };
    private readonly Border cross;
    private readonly Ring ring = new();
    private readonly FontIcon speaker = Icons.Make(Icons.Speaker, 9);
    private readonly Press closer = new() { Width = 30, Height = 28, HorizontalAlignment = HorizontalAlignment.Right };
    private TextBox? field;
    private bool live;
    private bool hovering;

    public event Action? CloseAsked;

    public TabFace(Browser browser, Tab tab, double spacing)
    {
        this.browser = browser;
        Tab = tab;
        ColumnSpacing = spacing;
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

        Add(mark, 0);
        Add(robot, 1);
        Add(shy, 2);
        Add(title, 3);

        cross = Kit.Rounded(7.5, Palette.Brush(Tone.Ink, 0.07));
        var x = Icons.Make(Icons.Close, 7, Palette.Muted);
        x.HorizontalAlignment = HorizontalAlignment.Center;
        x.VerticalAlignment = VerticalAlignment.Center;
        cross.Child = x;
        slot.Children.Add(cross);
        slot.Children.Add(ring);
        slot.Children.Add(speaker);
        speaker.HorizontalAlignment = HorizontalAlignment.Center;
        speaker.VerticalAlignment = VerticalAlignment.Center;
        foreach (var child in slot.Children) Motion.Fades(child);
        Add(slot, 4);

        // The cross is fifteen points across because that is how big it should
        // look. What you have to hit is the whole right-hand end of the tab.
        closer.Margin = new Thickness(0, 0, -7, 0);
        closer.Clicked += _ => { if (hovering) CloseAsked?.Invoke(); };
        Grid.SetColumn(closer, 4);
        Children.Add(closer);

        tab.PropertyChanged += OnTab;
        Paint();
    }

    private void Add(FrameworkElement e, int column)
    {
        Grid.SetColumn(e, column);
        e.VerticalAlignment = VerticalAlignment.Center;
        Children.Add(e);
    }

    private void OnTab(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(Tab.Label) or nameof(Tab.Icon) or nameof(Tab.Loading) or nameof(Tab.Noisy)
            or nameof(Tab.IsBlank) or nameof(Tab.Address) or nameof(Tab.Asleep))
            Paint();
    }

    public void Detach() => Tab.PropertyChanged -= OnTab;

    public void Set(bool live, bool hovering)
    {
        this.live = live;
        this.hovering = hovering;
        Paint();
    }

    public void Paint()
    {
        var editing = field != null;
        var colour = live ? Palette.Ink : hovering ? Palette.Brush(Tone.Ink, 0.7) : Palette.Muted;
        title.Text = Tab.Label;
        title.Foreground = colour;
        title.Visibility = editing ? Visibility.Collapsed : Visibility.Visible;
        mark.Visibility = !editing && browser.Prefs.Glyph == Glyph.Icons && !Tab.IsBlank ? Visibility.Visible : Visibility.Collapsed;
        mark.Show(Tab.Icon, Tab.Monogram);
        robot.Visibility = !editing && Tab.Bench ? Visibility.Visible : Visibility.Collapsed;
        robot.Foreground = Palette.Brush(Tone.Muted, 0.7);
        shy.Visibility = !editing && Tab.Shy ? Visibility.Visible : Visibility.Collapsed;
        shy.Foreground = Palette.Brush(Tone.Muted, 0.7);

        slot.Width = editing ? 0 : 15;
        closer.Visibility = editing ? Visibility.Collapsed : Visibility.Visible;
        cross.Opacity = hovering && !editing ? 1 : 0;
        ring.Visibility = !hovering && Tab.Loading ? Visibility.Visible : Visibility.Collapsed;
        speaker.Opacity = !hovering && !Tab.Loading && Tab.Noisy ? 1 : 0;
    }

    /// The address, or the name, inside the tab itself — a field of its own,
    /// arriving selected, so typing replaces it.
    public void Edit(bool on)
    {
        if (on == (field != null)) return;
        if (!on)
        {
            Children.Remove(field);
            field = null;
            Paint();
            return;
        }
        field = Kit.Field(12.5);
        field.Text = browser.TabDraft;
        Grid.SetColumn(field, 0);
        Grid.SetColumnSpan(field, 4);
        Children.Add(field);
        var mine = field;
        field.TextChanged += (_, _) => browser.TabDraft = mine.Text;
        field.KeyDown += (_, e) =>
        {
            switch (e.Key)
            {
                case VirtualKey.Enter:
                    // Staying in the field is what lets a refused address stay
                    // on screen instead of being thrown away.
                    browser.CommitTabEdit();
                    e.Handled = true;
                    break;
                case VirtualKey.Escape:
                    browser.CancelTabEdit();
                    e.Handled = true;
                    break;
            }
        };
        // Clicking anywhere else is a way of saying never mind.
        field.LostFocus += (_, _) => UI.Soon(() => { if (field == mine) browser.CancelTabEdit(); });
        Paint();
        UI.Soon(() =>
        {
            mine.Focus(FocusState.Programmatic);
            mine.SelectAll();
        });
    }
}

/// The letter of a pinned tab, typed in the square itself.
public static class PinField
{
    public static TextBox Make(Browser browser, Tab tab, double size)
    {
        var field = Kit.Field(size);
        field.TextAlignment = TextAlignment.Center;
        field.FontWeight = Microsoft.UI.Text.FontWeights.Medium;
        field.HorizontalAlignment = HorizontalAlignment.Stretch;
        field.Text = tab.Pin ?? "";
        var typing = false;
        field.TextChanged += (_, _) =>
        {
            if (typing) return;
            typing = true;
            browser.Letter(field.Text, tab);
            // One character only, and shown as it will be worn.
            field.Text = tab.Pin ?? "";
            field.SelectAll();
            typing = false;
        };
        field.KeyDown += (_, e) =>
        {
            if (e.Key is VirtualKey.Enter or VirtualKey.Escape or VirtualKey.Tab)
            {
                browser.EndPinEdit();
                e.Handled = true;
            }
        };
        field.LostFocus += (_, _) => UI.Soon(browser.EndPinEdit);
        UI.Soon(() =>
        {
            field.Focus(FocusState.Programmatic);
            // The guessed letter arrives selected, so one keystroke replaces
            // it and doing nothing keeps it.
            field.SelectAll();
        });
        return field;
    }
}
