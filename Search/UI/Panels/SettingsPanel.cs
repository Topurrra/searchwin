using System.ComponentModel;
using System.Numerics;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace Search;

/// Everything there is to set. Pages down the left, one page at a time on
/// the right, each a short list of lines with a hairline between them —
/// nothing to scroll through, nothing to hunt for. The same white and
/// hairline as the rest of the app; the same pill for the page you are on
/// as for the tab you are on.
public sealed partial class SettingsPanel : Grid
{
    private enum Page { General, Tabs, Extensions, Passwords, Downloads, Privacy, About }

    private static readonly (Page page, string raw, string title, string icon)[] Pages =
    [
        (Page.General, "general", "General", Icons.Window),
        (Page.Tabs, "tabs", "Tabs", Icons.Tabs),
        (Page.Extensions, "extensions", "Extensions", Icons.Puzzle),
        (Page.Passwords, "passwords", "Passwords", Icons.Key),
        (Page.Downloads, "downloads", "Downloads", Icons.Download),
        (Page.Privacy, "privacy", "Privacy", Icons.Shield),
        (Page.About, "about", "About", Icons.Info),
    ];

    private const double Rail = 168, Wide = 660, High = 500;

    private readonly Browser browser;
    private readonly Preferences prefs;
    private readonly Shield shield = Shield.Shared;
    private readonly Dictionary<Page, PageRow> rows = [];
    private readonly TextBlock heading = Kit.Text("", 17, semibold: true);
    private readonly ScrollViewer scroller = Parts.Scroller(null!, double.PositiveInfinity);
    private Page page;
    private bool isDefault = DefaultBrowser.IsDefault;
    private DispatcherQueueTimer? watching;

    /// Lines that come and go with a setting, rather than the page being
    /// drawn again under a switch that is still sliding.
    private Action? sideHides;
    private Action? hereLine;

    public SettingsPanel(Browser browser)
    {
        this.browser = browser;
        prefs = browser.Prefs;
        var saved = Store.Settings.String("settings.page");
        var found = Pages.FirstOrDefault(p => p.raw == saved);
        page = found.raw == null ? Page.General : found.page;

        Width = Wide;
        Height = High;
        CornerRadius = new CornerRadius(16);
        Background = Palette.Ground;
        BorderBrush = Palette.Hairline;
        BorderThickness = new Thickness(1);
        HorizontalAlignment = HorizontalAlignment.Center;
        VerticalAlignment = VerticalAlignment.Center;
        Kit.Lift(this, 48);

        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Rail) });
        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1) });
        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });

        Children.Add(MakeRail());
        var line = new Grid { Background = Palette.Hairline };
        Grid.SetColumn(line, 1);
        Children.Add(line);
        var right = MakeContent();
        Grid.SetColumn(right, 2);
        Children.Add(right);

        Loaded += (_, _) => Listen(true);
        Unloaded += (_, _) =>
        {
            Listen(false);
            watching?.Stop();
        };
        Show();
    }

    // MARK: - the rail

    private FrameworkElement MakeRail()
    {
        // Its own left corners, so the wash stays inside the plate's curve.
        var rail = new Grid
        {
            Background = Palette.Brush(Tone.Wash, 0.45),
            Padding = new Thickness(8),
            CornerRadius = new CornerRadius(15, 0, 0, 15),
        };
        var stack = new StackPanel { Spacing = 2 };
        var title = Kit.Text("Settings", 13, semibold: true);
        title.Margin = new Thickness(10, 14, 10, 12);
        stack.Children.Add(title);
        foreach (var (item, _, name, icon) in Pages)
        {
            var row = new PageRow(icon, name, () => Go(item));
            rows[item] = row;
            stack.Children.Add(row);
        }
        rail.Children.Add(stack);
        return rail;
    }

    private void Go(Page to)
    {
        if (page == to) return;
        page = to;
        Store.Settings.Set("settings.page", Pages.First(p => p.page == to).raw);
        Show();
    }

    /// One page on the rail: the icon and the name, lifted out in white when
    /// it is the page you are on.
    private sealed partial class PageRow : Press
    {
        private readonly Border ground = Kit.Rounded(8);
        private readonly FontIcon icon;
        private readonly TextBlock label;
        private bool on;

        public PageRow(string glyph, string title, Action act)
        {
            Height = 30;
            var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 9, Padding = new Thickness(10, 0, 10, 0) };
            icon = Icons.Make(glyph, 12);
            icon.Width = 16;
            icon.VerticalAlignment = VerticalAlignment.Center;
            label = Kit.Text(title, 13);
            row.Children.Add(icon);
            row.Children.Add(label);
            ground.BackgroundTransition = new BrushTransition { Duration = Motion.Quick };
            Children.Add(ground);
            Children.Add(row);
            Hovered += _ => Paint();
            Clicked += _ => act();
            Paint();
        }

        public bool On { get => on; set { on = value; Paint(); } }

        private void Paint()
        {
            var ink = on ? Palette.Ink : IsHovering ? Palette.Brush(Tone.Ink, 0.75) : Palette.Muted;
            icon.Foreground = ink;
            label.Foreground = ink;
            label.FontWeight = on ? FontWeights.Medium : FontWeights.Normal;
            ground.Background = on ? Palette.Ground : IsHovering ? Palette.Hover : Palette.Clear;
            // A breath of shadow under the page you are on, as the Mac lifts it.
            if (on == (ground.Shadow != null)) return;
            if (on) Kit.Lift(ground, 4);
            else
            {
                ground.Shadow = null;
                ground.Translation = Vector3.Zero;
            }
        }
    }

    // MARK: - the page

    private FrameworkElement MakeContent()
    {
        var content = new Grid { Padding = new Thickness(22, 18, 22, 18) };
        content.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        content.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        var head = Parts.Ends(heading, new Door(Icons.Close, "Done   Esc", () => browser.Tuning = false));
        head.Margin = new Thickness(0, 0, 0, 16);
        content.Children.Add(head);
        Grid.SetRow(scroller, 1);
        scroller.MaxHeight = double.PositiveInfinity;
        content.Children.Add(scroller);
        return content;
    }

    private void Show()
    {
        foreach (var (item, row) in rows) row.On = item == page;
        heading.Text = Pages.First(p => p.page == page).title;
        sideHides = null;
        hereLine = null;
        var body = new StackPanel { Spacing = 18, Padding = new Thickness(0, 0, 0, 4) };
        switch (page)
        {
            case Page.General: body.Children.Add(General()); break;
            case Page.Tabs: body.Children.Add(Tabs()); break;
            case Page.Extensions: body.Children.Add(new ExtensionsPage(browser)); break;
            case Page.Passwords: Passwords(body); break;
            case Page.Downloads: body.Children.Add(Downloads()); break;
            case Page.Privacy: Privacy(body); break;
            case Page.About: About(body); break;
        }
        scroller.Content = body;
        scroller.ChangeView(null, 0, null, true);
    }

    // MARK: - keeping up

    private void Listen(bool on)
    {
        if (on)
        {
            Listen(false);
            prefs.PropertyChanged += Prefs;
            shield.PropertyChanged += Shielding;
            browser.PropertyChanged += Browsing;
        }
        else
        {
            prefs.PropertyChanged -= Prefs;
            shield.PropertyChanged -= Shielding;
            browser.PropertyChanged -= Browsing;
        }
    }

    private void Prefs(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(Preferences.Sidebar)) sideHides?.Invoke();
        if (e.PropertyName == nameof(Preferences.Shielded)) hereLine?.Invoke();
    }

    private void Shielding(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(Shield.Trouble) && page == Page.Privacy) UI.Do(Show);
    }

    private void Browsing(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(Browser.Active) && page == Page.Privacy) UI.Do(Show);
    }

    // MARK: - general

    private UIElement General()
    {
        FrameworkElement control;
        if (isDefault)
        {
            var check = Icons.Make(Icons.Check, 12, Palette.Ink);
            check.Width = 24;
            control = check;
        }
        else control = new Pill("Make default", MakeDefault, filled: true);

        return Parts.Card(
            new Line("Open links from other apps",
                isDefault ? "Search is the default browser on this PC" : "Mail, Slack and the rest still send links elsewhere",
                control),
            new Line("Appearance", "Light, dark, or whatever Windows is doing — pages follow it too",
                new Segmented<Look>(Enum.GetValues<Look>().Select(l => (l, l.Title())), prefs.Look, v => prefs.Look = v)),
            new Line("Check spelling as you type", "Misspelt words underlined in pages' text fields, with the fix on a right-click",
                new Switch(prefs.Spelling, on => prefs.Spelling = on)),
            new Line("Let a script drive Search", "A local pipe for testing. Its tabs open beside yours with a robot on them and never take over — see bench.ps1",
                new Switch(prefs.Bench, on => prefs.Bench = on)));
    }

    /// Windows keeps the choice for itself: Search offers itself, Settings
    /// opens on its page, and the line changes whenever the person picks it.
    private async void MakeDefault()
    {
        if (!await DefaultBrowser.Become())
        {
            browser.Announce("Windows didn't change it");
            return;
        }
        watching?.Stop();
        watching = DefaultBrowser.Watch(() =>
        {
            isDefault = true;
            browser.Announce("Links now open here");
            if (page == Page.General) Show();
        });
    }

    // MARK: - tabs

    private UIElement Tabs()
    {
        var card = Parts.Card(
            new Line("Tabs in a sidebar", "Down the left instead of across the top. Pull its edge to make it wider; double-click the edge to reset.",
                new Switch(prefs.Sidebar, on => prefs.Sidebar = on)));
        card.Add(new Line("Hide the sidebar until the pointer reaches the edge", "The page takes the whole window; push against its left edge for the tabs. Ctrl+S keeps them out.",
            new Switch(prefs.SideHides, on => prefs.SideHides = on)));
        var hides = card.Lines.Children[^1];
        var rule = card.Lines.Children[^2];
        sideHides = () =>
        {
            var shown = prefs.Sidebar ? Visibility.Visible : Visibility.Collapsed;
            hides.Visibility = shown;
            rule.Visibility = shown;
        };
        sideHides();
        card.Add(new Line("Tabs show", "Beside the title, and on a pinned square",
            new Segmented<Glyph>(Enum.GetValues<Glyph>().Select(g => (g, g.Title())), prefs.Glyph, v => prefs.Glyph = v)));
        card.Add(new Line("Sleep tabs you aren't using", "After half an hour away they come back where you left them. Pinned tabs, sound, calls and anything typed stay awake.",
            new Switch(prefs.SleepsTabs, on => prefs.SleepsTabs = on)));
        card.Add(new Line("Spaces", "Separate sets of tabs, signed in where the others are or starting afresh, switched with Alt+1–Alt+9 or the space's icon.",
            new Switch(prefs.UsesSpaces, on => prefs.UsesSpaces = on)));
        return card;
    }

    // MARK: - passwords

    /// Says so when a password manager extension has taken the saving over.
    private static string SavingDetail =>
        Extensions.Shared.PasswordSavingTakenBy is { } name
            ? $"{name} does the saving — it asked Search not to offer"
            : "Asked once per site, never again for a site you refuse";

    private void Passwords(StackPanel body)
    {
        void OpenPasswords()
        {
            browser.Tuning = false;
            browser.Managing = true;
        }
        var card = Parts.Card(
            new Line("Your passwords", "In Windows' Credential Manager, under Search", new Pill("Open…", OpenPasswords)),
            new Line("Offer to save passwords", SavingDetail, new Switch(prefs.SavesPasswords, on => prefs.SavesPasswords = on)),
            new Line("Fill in sign-ins", "Click a sign-in box and the accounts kept for the site hang from it",
                new Switch(prefs.FillsPasswords, on => prefs.FillsPasswords = on)),
            new Line("Offer passkeys",
                prefs.PasskeysPossible
                    ? "Windows Hello, or a passkey on your phone, on sites that offer one"
                    : "This build can't offer them — off keeps sites to the password",
                new Switch(prefs.Passkeys, on => prefs.Passkeys = on)));
        if (Vault.Never.Count > 0)
            card.Add(new Line("Sites never asked", $"{Vault.Never.Count} sites told to stop offering", new Pill("Forget", () =>
            {
                Vault.Never = [];
                browser.Announce("Every site can ask again");
                Show();
            })));
        body.Children.Add(card);
        body.Children.Add(Parts.Card(
            new Line("Bring yours in", "From Chrome, Edge, Brave or Arc on this PC — nothing leaves it", new Pill("Import…", OpenPasswords))));
    }

    // MARK: - downloads

    private UIElement Downloads() => Parts.Card(
        new Line("Save to", prefs.Downloads, new Pill("Change…", ChooseFolder)),
        new Line("Ask where to save each file", null, new Switch(prefs.AsksWhereToSave, on => prefs.AsksWhereToSave = on)));

    private async void ChooseFolder()
    {
        try
        {
            var picker = new Windows.Storage.Pickers.FolderPicker
            {
                SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.Downloads,
                CommitButtonText = "Use this folder",
            };
            picker.FileTypeFilter.Add("*");
            if (App.Window is { } window)
                WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(window));
            var folder = await picker.PickSingleFolderAsync();
            if (folder == null) return;
            prefs.Downloads = folder.Path;
            if (page == Page.Downloads) Show();
        }
        catch { }
    }

    // MARK: - privacy

    private void Privacy(StackPanel body)
    {
        var card = Parts.Card(
            new Line("Block ads and trackers", shield.Trouble ?? "Third parties whose only job is to watch",
                new Switch(prefs.Shielded, on => prefs.Shielded = on)));
        if (shield.Trouble is { } trouble)
            card.Add(new Line(trouble, "Nothing is being blocked until this clears — try again, or restart Search",
                new Pill("Try again", shield.Compile)));
        if (browser.HereHost is { } host && shield.Trouble == null)
        {
            card.Add(new Line($"Block on {host}", "Turn off here if the site breaks — the page reloads",
                new Switch(!shield.IsPaused(host), on =>
                {
                    shield.Pause(host, !on);
                    browser.Reload();
                })));
            var here = card.Lines.Children[^1];
            var rule = card.Lines.Children[^2];
            hereLine = () =>
            {
                var shown = prefs.Shielded ? Visibility.Visible : Visibility.Collapsed;
                here.Visibility = shown;
                rule.Visibility = shown;
            };
            hereLine();
        }
        card.Add(new Line("Camera and microphone", "What each site was allowed or refused",
            new Pill("Forget choices", browser.ForgetCaptureChoices)));
        body.Children.Add(card);

        // FishCatcher. The address check is all on this computer, so it's on;
        // the daily list is a download, so it's yours to ask for.
        body.Children.Add(Parts.Card(
            new Line("Warn about scam and phishing sites", "Look-alike addresses and fake sign-in pages, checked on this computer before they load",
                new Switch(prefs.WarnsOfScams, on => prefs.WarnsOfScams = on)),
            new Line("Daily list of reported scam sites", FishFeed.Said() ?? "Downloaded once a day from FishCatcher's registry, and only used if its signature checks out",
                new Switch(prefs.ScamFeed, on => prefs.ScamFeed = on))));

        body.Children.Add(Parts.Card(
            new Line("History", "Every address you have been to", new Pill("Clear", browser.ClearHistory)),
            new Line("Cookies and sign-ins", "Signs you out of every site", new Pill("Sign out of everything", browser.ClearSites)),
            new Line("Cache", "Only what was fetched to draw pages", new Pill("Clear", browser.ClearCache))));
    }

    // MARK: - about

    private void About(StackPanel body)
    {
        var who = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 14, Margin = new Thickness(0, 0, 0, 2) };
        var mark = Logomark.Make(34 * Logomark.Width / Logomark.Height);
        mark.VerticalAlignment = VerticalAlignment.Center;
        who.Children.Add(mark);
        var words = new StackPanel { Spacing = 3, VerticalAlignment = VerticalAlignment.Center };
        words.Children.Add(Kit.Text("Search for Windows", 15, semibold: true));
        words.Children.Add(Kit.Text($"by Office Commun · version {Updater.Version}", 12, Palette.Muted));
        who.Children.Add(words);
        body.Children.Add(who);

        // No update line: there is no server for Windows builds to ask.
        body.Children.Add(Parts.Card(
            new Line("Found something wrong?", "Opens a draft with the version already in it", new Pill("Send Feedback", WriteFeedback))));

        body.Children.Add(Parts.Card(
            Shortcut("Ctrl+L", "Address"),
            Shortcut("Ctrl+K", "Switch tab"),
            Shortcut("Ctrl+T  Ctrl+W  Ctrl+Shift+T", "New, close, reopen tab"),
            Shortcut("Ctrl+Tab  Ctrl+1–9", "Next tab, a tab by its place"),
            Shortcut("Ctrl+Shift+S", "Tabs in a sidebar"),
            Shortcut("Ctrl+S", "Fold the sidebar away"),
            Shortcut("Ctrl+Shift+R", "Reading mode"),
            Shortcut("Ctrl+Shift+H", "Hide something on this site"),
            Shortcut("Ctrl+Shift+P", "Float the video")));
    }

    /// A draft in whatever mail app Windows opens mailto: with, the version
    /// already in it.
    private static async void WriteFeedback()
    {
        var version = Updater.Version;
        var system = Environment.OSVersion.Version;
        var subject = Uri.EscapeDataString($"Search feedback — {version} (Windows)");
        var text = Uri.EscapeDataString($"\n\n—\nSearch {version} for Windows, Windows {system.Major}.{system.Minor}.{system.Build}");
        try { await Windows.System.Launcher.LaunchUriAsync(new Uri($"mailto:hello@officecommun.com?subject={subject}&body={text}")); }
        catch { }
    }

    /// A keystroke and what it does.
    private static Grid Shortcut(string keys, string does)
    {
        var row = Parts.Ends(Kit.Text(does, 13), Kit.Text(keys, 12, Palette.Muted));
        row.Padding = new Thickness(14, 9, 14, 9);
        return row;
    }
}
