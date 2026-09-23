using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Search;

/// What you have downloaded, newest first. A file is a click from opening,
/// and one more from Explorer; the list forgets, the files stay.
public sealed class DownloadsPanel : Plate
{
    private const string Document = "";

    private readonly Browser browser;
    private readonly Grid content;
    private readonly Grid foot;

    public DownloadsPanel(Browser browser) : this(browser, new Grid(), new Grid()) { }

    private DownloadsPanel(Browser browser, Grid content, Grid foot)
        : base("Downloads", content, () => browser.Hoarding = false, foot, width: 560)
    {
        this.browser = browser;
        this.content = content;
        this.foot = foot;

        // A download that finishes while the list is open joins it.
        Action changed = () => UI.Do(Fill);
        Loaded += (_, _) =>
        {
            browser.Loot.Changed -= changed;
            browser.Loot.Changed += changed;
        };
        Unloaded += (_, _) => browser.Loot.Changed -= changed;
        Fill();
    }

    private void Fill()
    {
        var loot = browser.Loot;
        content.Children.Clear();
        if (loot.Kept.Count == 0)
            content.Children.Add(Parts.Card(Nothing.Make("Nothing downloaded yet.")));
        else
        {
            var card = Parts.Card();
            foreach (var keep in loot.Kept.ToList())
                card.Add(new KeepRow(keep,
                    open: () => Loot.Open(keep),
                    reveal: () => Loot.Reveal(keep),
                    forget: () => loot.Forget(keep)));
            var holder = new Grid { Padding = new Thickness(0, 0, 0, 2) };
            holder.Children.Add(card);
            content.Children.Add(Parts.Scroller(holder, 420));
        }

        foot.Children.Clear();
        var folder = Path.GetFileName(browser.DownloadsFolder.TrimEnd('\\', '/'));
        foot.Children.Add(Parts.Ends(
            Parts.Note(loot.Kept.Count == 0 ? $"Files land in {folder}" : "Clearing the list leaves the files where they are"),
            loot.Kept.Count == 0 ? null : new Pill("Clear list", loot.ForgetAll)));
    }

    private sealed class KeepRow : Press
    {
        public KeepRow(Keep keep, Action open, Action reveal, Action forget)
        {
            var there = keep.StillThere;
            Padding = new Thickness(14, 9, 14, 9);
            ColumnSpacing = 12;
            BackgroundTransition = new BrushTransition { Duration = Motion.Quick };
            ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

            // A file moved or deleted since stays in the list, faded, so you
            // can see it went — but there is nothing left to open.
            var icon = Icons.Make(Document, 13, there ? Palette.Muted : Palette.Faint);
            icon.Width = 18;
            icon.VerticalAlignment = VerticalAlignment.Center;
            Children.Add(icon);

            var words = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
            words.Children.Add(Kit.Text(keep.Name, 13, there ? Palette.Ink : Palette.Faint));
            words.Children.Add(Kit.Text(keep.From.Length == 0 ? When.Said(keep.Date) : $"{keep.From} · {When.Said(keep.Date)}", 11.5, Palette.Muted));
            Grid.SetColumn(words, 1);
            Children.Add(words);

            var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, Visibility = Visibility.Collapsed, Margin = new Thickness(8, 0, 0, 0) };
            if (there) actions.Children.Add(new Quick("Show in Explorer", reveal));
            actions.Children.Add(new Quick("Remove", forget, Palette.Brush(Tone.Red, 0.75)));
            Grid.SetColumn(actions, 2);
            Children.Add(actions);

            Hovered += on =>
            {
                Background = on ? Palette.Hover : Palette.Clear;
                actions.Visibility = on ? Visibility.Visible : Visibility.Collapsed;
            };
            Clicked += _ => { if (keep.StillThere) open(); };
        }
    }
}
