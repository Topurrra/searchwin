using System.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;

namespace Search;

/// Settings › Extensions: what is installed, and the two ways in — a Chrome
/// Web Store link, or a folder.
public sealed class ExtensionsPage : StackPanel
{
    private readonly Browser browser;
    private readonly TextBox link = Kit.Field(12.5);
    private readonly Grid addSpot = new() { VerticalAlignment = VerticalAlignment.Center };
    private readonly Pill add;
    private readonly StackPanel list = new();

    public ExtensionsPage(Browser browser)
    {
        this.browser = browser;
        Spacing = 18;

        // Add from the store: a link or an id, or the store itself.
        var top = new StackPanel { Spacing = 10, Padding = new Thickness(14) };
        var heading = new Grid();
        heading.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        heading.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        heading.Children.Add(Kit.Text("Add from the Chrome Web Store", 13));
        var store = new Pill("Open the Store", () =>
        {
            browser.Tuning = false;
            browser.Open(Browser.WebStore, foreground: true);
        });
        Grid.SetColumn(store, 1);
        heading.Children.Add(store);
        top.Children.Add(heading);

        var entry = new Grid { ColumnSpacing = 8 };
        entry.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        entry.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var well = new Grid { CornerRadius = new CornerRadius(9), Background = Palette.Wash, Padding = new Thickness(10, 7, 10, 7) };
        well.Children.Add(Kit.Placeheld(link, "Paste a link to an extension, or its id", 12.5));
        entry.Children.Add(well);
        add = new Pill("Add", Add, filled: true);
        addSpot.Children.Add(add);
        Grid.SetColumn(addSpot, 1);
        entry.Children.Add(addSpot);
        top.Children.Add(entry);
        link.TextChanged += (_, _) => DrawAdd();
        link.KeyDown += (_, e) =>
        {
            if (e.Key != Windows.System.VirtualKey.Enter) return;
            e.Handled = true;
            Add();
        };

        var note = Kit.Text("Or find it in the store and press Add to Search on its page.", 11.5, Palette.Muted);
        note.TextWrapping = TextWrapping.Wrap;
        top.Children.Add(note);
        Children.Add(new Card(top));

        Children.Add(list);

        Children.Add(new Card(new Line(
            "Load an unpacked extension",
            "A folder with a manifest.json — your own, or one exported from another browser. Reload picks up what you've changed in it since.",
            new Pill("Choose…", ChooseFolder))));

        // Drawn again whenever the list changes, for as long as the page is up.
        PropertyChangedEventHandler changed = (_, e) =>
        {
            if (e.PropertyName is nameof(Extensions.Busy)) DrawAdd();
            else Draw();
        };
        Loaded += (_, _) =>
        {
            Extensions.Shared.PropertyChanged -= changed;
            Extensions.Shared.PropertyChanged += changed;
            Draw();
        };
        Unloaded += (_, _) => Extensions.Shared.PropertyChanged -= changed;
        Draw();
    }

    private void Add()
    {
        if (Crx.Id(link.Text) == null) return;
        Extensions.Shared.Install(link.Text);
        link.Text = "";
    }

    private void DrawAdd()
    {
        addSpot.Children.Clear();
        if (Extensions.Shared.Busy != null)
        {
            addSpot.Children.Add(new Ring(12));
            return;
        }
        var ready = Crx.Id(link.Text) != null;
        add.Opacity = ready ? 1 : 0.35;
        add.IsHitTestVisible = ready;
        addSpot.Children.Add(add);
    }

    private void Draw()
    {
        DrawAdd();
        list.Children.Clear();
        var installed = Extensions.Shared.Installed;
        if (installed.Count == 0)
        {
            list.Children.Add(new Card(Nothing.Make("No extensions yet.")));
            return;
        }
        var card = new Card();
        foreach (var item in installed) card.Add(new Row(browser, item));
        list.Children.Add(card);
    }

    /// The folder, chosen in Windows' own picker.
    public static async void ChooseFolder()
    {
        if (App.Window is not { } window) return;
        var picker = new Windows.Storage.Pickers.FolderPicker
        {
            SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.DocumentsLibrary,
            CommitButtonText = "Load Extension",
        };
        picker.FileTypeFilter.Add("*");
        WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(window));
        try
        {
            if (await picker.PickSingleFolderAsync() is { } folder) Extensions.Shared.InstallFolder(folder.Path);
        }
        catch { }
    }

    /// One extension: its icon, name, where it came from; what can be done
    /// with it under the pointer; and its switch.
    private sealed class Row : Grid
    {
        public Row(Browser browser, Installed item)
        {
            var extensions = Extensions.Shared;
            Padding = new Thickness(14, 10, 14, 10);
            ColumnSpacing = 12;
            Background = Palette.Clear;
            ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
            ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });

            var icon = ExtensionIcon.Make(item, 22);
            icon.VerticalAlignment = VerticalAlignment.Center;
            Children.Add(icon);

            var words = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
            words.Children.Add(Kit.Text(item.Name, 13));
            var detail = Kit.Text(Detail(item), 11.5, Palette.Muted);
            words.Children.Add(detail);
            // Where it was loaded from, and why it isn't running, in full.
            var tip = string.Join("\n", new[] { item.Source, extensions.Failed.GetValueOrDefault(item.Id) }.Where(s => !string.IsNullOrEmpty(s)));
            if (tip.Length > 0)
            {
                words.IsHitTestVisible = true;
                words.Background = Palette.Clear;
                ToolTipService.SetToolTip(words, tip);
            }
            Grid.SetColumn(words, 1);
            Children.Add(words);

            var quick = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 6, VerticalAlignment = VerticalAlignment.Center, Visibility = Visibility.Collapsed };
            if (extensions.HasNewTabPage(item))
            {
                var on = extensions.ShowsInNewTabs(item);
                quick.Children.Add(new Quick(on ? "Stop in New Tabs" : "Show in New Tabs", () => extensions.SetShowsInNewTabs(item, !on)));
            }
            if (extensions.Manifest(item)?.HasAction == true)
                quick.Children.Add(new Quick(item.Pinned == true ? "Unpin" : "Pin", () => extensions.SetPinned(item.Id, item.Pinned != true)));
            if (item.Source != null || !item.FromStore)
                quick.Children.Add(new Quick("Reload", () => extensions.Reload(item.Id)));
            if (extensions.Manifest(item)?.OptionsPage != null)
                quick.Children.Add(new Quick("Options", () =>
                {
                    browser.Tuning = false;
                    extensions.OpenOptions(item.Id);
                }));
            quick.Children.Add(new Quick("Remove", () => ExtensionActions.ConfirmRemove(item), Palette.Brush(Tone.Red, 0.75)));
            Grid.SetColumn(quick, 2);
            Children.Add(quick);

            var toggle = new Switch(item.Enabled, on => extensions.SetEnabled(item.Id, on));
            Grid.SetColumn(toggle, 3);
            Children.Add(toggle);

            PointerEntered += (_, _) => Hover(true);
            PointerExited += (_, _) => Hover(false);
            void Hover(bool on)
            {
                Background = on ? Palette.Hover : Palette.Clear;
                quick.Visibility = on ? Visibility.Visible : Visibility.Collapsed;
            }
        }

        private static string Detail(Installed item)
        {
            var extensions = Extensions.Shared;
            var from = item.FromStore
                ? "Chrome Web Store"
                : item.Source is { } source ? $"From “{Path.GetFileName(source.TrimEnd('\\', '/'))}”" : "From a folder";
            var parts = new List<string> { $"Version {item.Version}", from };
            if (item.Enabled && extensions.Failed.ContainsKey(item.Id)) parts.Add("couldn't start");
            if (extensions.HasNewTabPage(item) && extensions.ShowsInNewTabs(item)) parts.Add("shows in new tabs");
            return string.Join(" · ", parts);
        }
    }
}
