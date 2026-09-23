using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Search;

/// Everything that rises from the bottom edge to say one thing: a line that
/// says it and leaves, a page asking for the camera, a password offered a
/// place, the one mode this browser has.
public sealed partial class Bars : StackPanel
{
    private readonly Browser browser;
    private readonly Border announcement;
    private readonly TextBlock announcementText = Kit.Text("", 12);
    private readonly Border capture = new();
    private readonly Border offer = new();
    private readonly Border hint;

    public Bars(Browser browser)
    {
        this.browser = browser;
        Spacing = 8;
        VerticalAlignment = VerticalAlignment.Bottom;
        HorizontalAlignment = HorizontalAlignment.Center;
        Margin = new Thickness(0, 0, 0, 30);

        announcement = Capsule(announcementText, new Thickness(15, 9, 15, 9));
        Children.Add(announcement);
        Children.Add(capture);
        Children.Add(offer);
        Children.Add(new StoreOffer(browser));
        hint = Hint("Click anything to hide it   Ctrl+Z undo   Esc done");
        Children.Add(hint);

        foreach (var child in Children.OfType<UIElement>()) { Motion.Fades(child, Motion.Settle); child.Visibility = Visibility.Collapsed; }

        browser.OnAny(name =>
        {
            switch (name)
            {
                case nameof(Browser.Announcement): Announce(); break;
                case nameof(Browser.Asking): Ask(); break;
                case nameof(Browser.Offering): Offer(); break;
                case nameof(Browser.Veiling): hint.Visibility = browser.Veiling ? Visibility.Visible : Visibility.Collapsed; break;
            }
        });
    }

    /// The same white capsule, hairline and shadow for everything here.
    private static Border Capsule(UIElement content, Thickness padding)
    {
        var capsule = new Border
        {
            CornerRadius = new CornerRadius(18),
            Background = Palette.Ground,
            BorderBrush = Palette.Hairline,
            BorderThickness = new Thickness(1),
            Padding = padding,
            HorizontalAlignment = HorizontalAlignment.Center,
            Child = content,
        };
        Kit.Lift(capsule, 24);
        return capsule;
    }

    /// A dark pill, for the one mode this browser has. It stays up for as long
    /// as the mode does, which is how you know you are still in it.
    private static Border Hint(string text)
    {
        var label = Kit.Text(text, 11.5, Palette.Brush(Tone.Ground, 0.92));
        var pill = new Border
        {
            CornerRadius = new CornerRadius(18),
            Background = Palette.Brush(Tone.Ink, 0.92),
            Padding = new Thickness(15, 9, 15, 9),
            HorizontalAlignment = HorizontalAlignment.Center,
            Child = label,
        };
        Kit.Lift(pill, 24);
        return pill;
    }

    private void Announce()
    {
        if (browser.Announcement is { } text) announcementText.Text = text;
        announcement.Visibility = browser.Announcement != null ? Visibility.Visible : Visibility.Collapsed;
    }

    private static Press Filled(string title, Action act)
    {
        var press = new Press();
        var ground = Kit.Rounded(11, Palette.Ink);
        var label = Kit.Text(title, 12, Palette.Ground);
        label.Margin = new Thickness(11, 5, 11, 5);
        ground.Child = label;
        press.Children.Add(ground);
        press.Hovered += on => ground.Background = on ? Palette.Brush(Tone.Ink, 0.8) : Palette.Ink;
        press.Clicked += _ => act();
        press.VerticalAlignment = VerticalAlignment.Center;
        return press;
    }

    private static Press Plain(string title, Action act)
    {
        var press = new Press { VerticalAlignment = VerticalAlignment.Center };
        var label = Kit.Text(title, 12, Palette.Muted);
        press.Children.Add(label);
        press.Hovered += on => label.Foreground = on ? Palette.Ink : Palette.Muted;
        press.Clicked += _ => act();
        return press;
    }

    /// A page asking to see or hear you. Named by the site, in its own words,
    /// with the answer remembered so it is asked once and not every call.
    private void Ask()
    {
        if (browser.Asking is not { } ask)
        {
            capture.Visibility = Visibility.Collapsed;
            return;
        }
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        row.Children.Add(Icons.Make(ask.Wants == "microphone" ? Icons.Microphone : Icons.Video, 12));
        row.Children.Add(Kit.Text($"{ask.Host} wants to use your {ask.Wants}", 12.5));
        row.Children.Add(Filled("Allow", browser.AllowCapture));
        row.Children.Add(Plain("Don't allow", browser.DenyCapture));
        var capsule = Capsule(row, new Thickness(16, 9, 10, 9));
        capture.Child = capsule;
        capture.Visibility = Visibility.Visible;
    }

    /// Offered once, answered once. The password is never shown back to you —
    /// there is nothing to be learned from reading your own password.
    private void Offer()
    {
        if (browser.Offering is not { } offering)
        {
            offer.Visibility = Visibility.Collapsed;
            return;
        }
        var login = offering.Login;
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        var text = offering.Changed
            ? $"Update the password for {login.User} on {login.Host}?"
            : login.User.Length == 0
                ? $"Save this password for {login.Host}?"
                : $"Save the password for {login.User} on {login.Host}?";
        row.Children.Add(Kit.Text(text, 12.5));
        row.Children.Add(Filled(offering.Changed ? "Update" : "Save", browser.KeepOffer));
        row.Children.Add(Plain("Not now", browser.DropOffer));
        if (!offering.Changed) row.Children.Add(Plain("Never here", browser.NeverOffer));
        offer.Child = Capsule(row, new Thickness(16, 9, 12, 9));
        offer.Visibility = Visibility.Visible;
    }
}
