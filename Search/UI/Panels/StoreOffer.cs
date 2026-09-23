using System.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace Search;

/// On an extension's page in the Chrome Web Store, the offer to add it —
/// only where the page's own "Add to Search" isn't in place, so a store that
/// has changed its markup still gets a way in.
public sealed partial class StoreOffer : Grid
{
    private readonly Browser browser;
    private Tab? watched;

    public StoreOffer(Browser browser)
    {
        this.browser = browser;
        HorizontalAlignment = HorizontalAlignment.Center;
        browser.OnAny(name => { if (name is nameof(Browser.Active) or nameof(Browser.Tabs) or "") Watch(); });
        Extensions.Shared.OnAny(_ => Draw());
        Watch();
    }

    /// The tab on screen, followed for its address and for the page's own
    /// button arriving.
    private void Watch()
    {
        var tab = browser.Active;
        if (tab != watched)
        {
            if (watched != null) watched.PropertyChanged -= Changed;
            watched = tab;
            if (tab != null) tab.PropertyChanged += Changed;
        }
        Draw();
    }

    private void Changed(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(Tab.Address) or nameof(Tab.StorePlaced) or "") Draw();
    }

    private string? drawn;

    private void Draw()
    {
        var extensions = Extensions.Shared;
        var tab = watched;
        var id = tab?.Address is { } url && StoreRelay.IsStorePage(url) ? Crx.Id(url.AbsoluteUri) : null;
        var wanted = id != null && tab!.StorePlaced != id && extensions.Find(id) == null;
        if (!wanted)
        {
            Visibility = Visibility.Collapsed;
            drawn = null;
            return;
        }
        var busy = extensions.Busy == id;
        var key = $"{id}|{busy}";
        Visibility = Visibility.Visible;
        if (drawn == key) return;
        drawn = key;

        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        row.Children.Add(Icons.Make(Icons.Puzzle, 11));
        row.Children.Add(Kit.Text(busy ? "Adding…" : "Add this extension to Search", 12.5));
        if (busy) row.Children.Add(new Ring(10));
        else
        {
            var add = new Press { VerticalAlignment = VerticalAlignment.Center };
            var ground = Kit.Rounded(11, Palette.Ink);
            var label = Kit.Text("Add", 12, Palette.Ground);
            label.Margin = new Thickness(11, 5, 11, 5);
            ground.Child = label;
            add.Children.Add(ground);
            add.Hovered += on => ground.Background = on ? Palette.Brush(Tone.Ink, 0.8) : Palette.Ink;
            var which = id!;
            add.Clicked += _ => extensions.Install(which);
            row.Children.Add(add);
        }
        var capsule = new Border
        {
            CornerRadius = new CornerRadius(18),
            Background = Palette.Ground,
            BorderBrush = Palette.Hairline,
            BorderThickness = new Thickness(1),
            Padding = new Thickness(16, 9, 10, 9),
            HorizontalAlignment = HorizontalAlignment.Center,
            Child = row,
        };
        Kit.Lift(capsule, 24);
        Children.Clear();
        Children.Add(capsule);
    }
}
