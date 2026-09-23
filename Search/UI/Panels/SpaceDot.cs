using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;

namespace Search;

/// The space on screen, as its icon: at the column's foot, or before the
/// tabs in the row. One icon however many spaces there are; a click opens
/// the menu. When the space changes the icon turns over the way the spaces
/// went — out on one side, the next one in from the other.
///
/// It keeps itself out of sight while there are no spaces, so whoever
/// places it only has to place it.
public sealed partial class SpaceDot : Press
{
    public const double Width_ = 26;

    private readonly Browser browser;
    private readonly Border ground = Kit.Rounded(8);
    /// What is drawn now, and what it replaced, on its way out.
    private readonly FontIcon now = Icons.Make("", 12);
    private readonly FontIcon was = Icons.Make("", 12);
    private readonly TranslateTransform nowShift = new();
    private readonly TranslateTransform wasShift = new();
    private string shownKey = "";

    public SpaceDot(Browser browser)
    {
        this.browser = browser;
        Width = Width_;
        Height = 26;
        VerticalAlignment = VerticalAlignment.Center;
        // The icon on its way in or out stays inside the dot.
        Clip = new RectangleGeometry { Rect = new Windows.Foundation.Rect(0, 0, Width_, 26) };
        Children.Add(ground);
        foreach (var (icon, shift) in new[] { (was, wasShift), (now, nowShift) })
        {
            icon.FontWeight = FontWeights.Medium;
            icon.HorizontalAlignment = HorizontalAlignment.Center;
            icon.VerticalAlignment = VerticalAlignment.Center;
            icon.RenderTransform = shift;
            Children.Add(icon);
        }
        was.Opacity = 0;
        Motion.Fades(ground);

        Hovered += _ => Paint();
        Clicked += _ => SpaceMenu.Show(browser, this);

        browser.OnAny(name =>
        {
            if (name is nameof(Browser.SpaceID) or nameof(Browser.Spaces) or nameof(Browser.MakingSpace) or "") Turn();
        });
        browser.Prefs.On(nameof(Preferences.UsesSpaces), Place);
        Place();
        Turn(animated: false);
    }

    private string Glyph => browser.MakingSpace ? Icons.Plus : browser.CurrentSpace.Glyph;
    private string Key => browser.MakingSpace ? "new" : $"{browser.SpaceID}-{browser.CurrentSpace.Symbol}";

    private void Place()
    {
        Visibility = browser.Prefs.UsesSpaces ? Visibility.Visible : Visibility.Collapsed;
    }

    /// The new icon in from the side the spaces went towards, the old one out
    /// the other way, over a fifth of a second.
    private void Turn(bool animated = true)
    {
        ToolTipService.SetToolTip(this, $"{browser.CurrentSpace.Name} — Alt+1–Alt+9 or two fingers sideways to switch");
        var key = Key;
        if (key == shownKey) return;
        shownKey = key;
        var glyph = Glyph;
        if (!animated || now.Glyph.Length == 0)
        {
            now.Glyph = glyph;
            Paint();
            return;
        }
        var step = browser.SpaceStep > 0 ? 1 : -1;
        was.Glyph = now.Glyph;
        now.Glyph = glyph;
        // One storyboard for the four, each with where it starts said
        // outright: a turn that comes before the last one has finished
        // starts from the beginning rather than from wherever that one left.
        var board = new Storyboard();
        void Run(DependencyObject target, string property, double from, double to) =>
            board.Children.Add(Animate(target, property, from, to));
        Run(wasShift, "X", 0, -step * Width_);
        Run(was, "Opacity", 1, 0);
        Run(nowShift, "X", step * Width_, 0);
        Run(now, "Opacity", 0, 1);
        turning?.Stop();
        turning = board;
        board.Begin();
        Paint();
    }

    private Storyboard? turning;

    private static DoubleAnimation Animate(DependencyObject target, string property, double from, double to)
    {
        var animation = new DoubleAnimation
        {
            From = from,
            To = to,
            Duration = TimeSpan.FromMilliseconds(220),
            EasingFunction = new CubicEase { EasingMode = EasingMode.EaseOut },
            EnableDependentAnimation = true,
        };
        Storyboard.SetTarget(animation, target);
        Storyboard.SetTargetProperty(animation, property);
        return animation;
    }

    private void Paint()
    {
        var ink = IsHovering ? Palette.Ink : Palette.Muted;
        now.Foreground = ink;
        was.Foreground = ink;
        ground.Background = IsHovering ? Palette.Hover : Palette.Clear;
    }
}

/// The dot's menu: the spaces, then what can be done to the one on screen.
public static class SpaceMenu
{
    public static void Show(Browser browser, FrameworkElement at)
    {
        var menu = new MenuFlyout();

        static FontIcon Glyph(string glyph) => new() { Glyph = glyph, FontFamily = Icons.Font };

        MenuFlyoutItem Item(string text, Action act)
        {
            var item = new MenuFlyoutItem { Text = text };
            item.Click += (_, _) => act();
            return item;
        }

        for (var index = 0; index < browser.Spaces.Count; index++)
        {
            var space = browser.Spaces[index];
            var entry = new ToggleMenuFlyoutItem { Text = space.Name, IsChecked = space.Id == browser.SpaceID, Icon = Glyph(space.Glyph) };
            if (index < 9) entry.KeyboardAcceleratorTextOverride = $"Alt+{index + 1}";
            entry.Click += (_, _) => browser.SwitchSpace(space.Id);
            menu.Items.Add(entry);
        }
        menu.Items.Add(new MenuFlyoutSeparator());
        menu.Items.Add(Item("New Space…", browser.AskForSpace));
        menu.Items.Add(new MenuFlyoutSeparator());

        var here = browser.CurrentSpace;
        menu.Items.Add(Item($"Rename “{here.Name}”…", () =>
            SpaceAsk.Name("Rename Space", here.Name, here.Name, "Rename", name => browser.RenameSpace(here.Id, name))));

        var icons = new MenuFlyoutSubItem { Text = "Icon" };
        for (var i = 0; i < Space.Icons.Length; i++)
        {
            var symbol = Space.Icons[i];
            var choice = new ToggleMenuFlyoutItem { Text = Space.IconNames[i], IsChecked = here.Symbol == symbol, Icon = Glyph(Space.GlyphOf(symbol)) };
            choice.Click += (_, _) => browser.SetSpaceIcon(here.Id, symbol);
            icons.Items.Add(choice);
        }
        menu.Items.Add(icons);

        // The order is the swipe's, and Alt+1–Alt+9's.
        var place = browser.Spaces.FindIndex(s => s.Id == here.Id);
        if (place > 0) menu.Items.Add(Item("Move Left", () => browser.MoveSpace(here.Id, place - 1)));
        if (place >= 0 && place < browser.Spaces.Count - 1) menu.Items.Add(Item("Move Right", () => browser.MoveSpace(here.Id, place + 1)));

        var folder = here.Downloads is { } path ? Path.GetFileName(path.TrimEnd('\\', '/')) : null;
        menu.Items.Add(Item(folder != null ? $"Downloads to “{folder}”…" : "Downloads Folder…", () =>
            SpaceAsk.Folder(chosen => browser.SetSpaceDownloads(here.Id, chosen))));
        if (folder != null)
            menu.Items.Add(Item("Downloads to the Folder in Settings", () => browser.SetSpaceDownloads(here.Id, null)));

        if (!here.IsFirst)
        {
            menu.Items.Add(new MenuFlyoutSeparator());
            menu.Items.Add(Item($"Delete “{here.Name}”…", () =>
                SpaceAsk.Sure($"Delete “{here.Name}”?",
                    "Its tabs close, and its cookies and sign-ins are erased from this PC. History and bookmarks stay.",
                    "Delete", () => browser.DeleteSpace(here.Id))));
        }

        menu.ShowAt(at, new FlyoutShowOptions { Placement = FlyoutPlacementMode.Auto });
    }
}

/// The few questions a space's menu asks, as dialogs over the window.
public static class SpaceAsk
{
    public static async void Name(string title, string placeholder, string initial, string confirm, Action<string> then)
    {
        var field = new TextBox { PlaceholderText = placeholder, Text = initial, Width = 260 };
        field.Loaded += (_, _) =>
        {
            field.Focus(FocusState.Programmatic);
            field.SelectAll();
        };
        var dialog = new ContentDialog { Title = title, Content = field, PrimaryButtonText = confirm, CloseButtonText = "Cancel" };
        var name = (await Ask(dialog, field) ? field.Text : "").Trim();
        if (name.Length > 0) then(name);
    }

    /// A new space's name, and whether it keeps the sign-ins the others
    /// have — for when the column isn't there to hold the card.
    public static async void NewSpace(Action<string, bool> then)
    {
        var field = new TextBox { PlaceholderText = "Work", Width = 280 };
        field.Loaded += (_, _) => field.Focus(FocusState.Programmatic);
        var fresh = new CheckBox { Content = "Start signed out, with its own cookies" };
        var words = new TextBlock
        {
            Text = "Its own tabs. Signed in where your other spaces are, unless it starts afresh.",
            TextWrapping = TextWrapping.Wrap,
            Opacity = 0.7,
            Width = 280,
        };
        var box = new StackPanel { Spacing = 10, Children = { words, field, fresh } };
        var dialog = new ContentDialog { Title = "New Space", Content = box, PrimaryButtonText = "Create", CloseButtonText = "Cancel" };
        if (!await Ask(dialog, field)) return;
        var name = field.Text.Trim();
        if (name.Length > 0) then(name, fresh.IsChecked != true);
    }

    public static async void Sure(string title, string detail, string confirm, Action then)
    {
        var dialog = new ContentDialog
        {
            Title = title,
            Content = new TextBlock { Text = detail, TextWrapping = TextWrapping.Wrap, MaxWidth = 320 },
            PrimaryButtonText = confirm,
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
        };
        if (await Ask(dialog)) then();
    }

    /// A folder for this space's downloads. Cancel keeps the folder it has.
    public static async void Folder(Action<string> then)
    {
        if (App.Window is not { } window) return;
        try
        {
            var picker = new Windows.Storage.Pickers.FolderPicker
            {
                CommitButtonText = "Use for This Space",
                SuggestedStartLocation = Windows.Storage.Pickers.PickerLocationId.Downloads,
            };
            picker.FileTypeFilter.Add("*");
            WinRT.Interop.InitializeWithWindow.Initialize(picker, WinRT.Interop.WindowNative.GetWindowHandle(window));
            if (await picker.PickSingleFolderAsync() is { } folder) then(folder.Path);
        }
        catch { }
    }

    /// Only one dialog can be up at a time; asking while one is up is
    /// answered no rather than thrown. Return in the field is the confirm
    /// button, as it was on the Mac's sheet.
    private static async Task<bool> Ask(ContentDialog dialog, TextBox? field = null)
    {
        if (App.Root?.XamlRoot is not { } root) return false;
        dialog.XamlRoot = root;
        dialog.RequestedTheme = App.Root.RequestedTheme;
        var entered = false;
        if (field != null)
        {
            dialog.DefaultButton = ContentDialogButton.Primary;
            field.KeyDown += (_, e) =>
            {
                if (e.Key != Windows.System.VirtualKey.Enter) return;
                e.Handled = true;
                entered = true;
                dialog.Hide();
            };
        }
        try
        {
            var result = await dialog.ShowAsync();
            return result == ContentDialogResult.Primary || entered;
        }
        catch { return false; }
    }
}
