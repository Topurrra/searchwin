using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;

namespace Search;

/// The first-launch walk-through. PORT: Welcome.swift.
public sealed class WelcomePanel : Grid
{
    public WelcomePanel(Browser browser)
    {
        Background = Palette.Ground;
        var stack = new StackPanel { Spacing = 18, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
        stack.Children.Add(Logomark.Make(96));
        stack.Children.Add(new Pill("Start", () => { browser.Prefs.Welcomed = true; browser.Welcoming = false; }, filled: true) { HorizontalAlignment = HorizontalAlignment.Center });
        Children.Add(stack);
    }
}
