using Microsoft.UI.Xaml.Controls;
using SearchKit.Commands;

namespace Search;

/// Everything Search can do, in the one list the field (`>…`), and later
/// voice, the agent and scripts, all run from (SearchKit.Commands). And the
/// bangs — `!yt cats` — built in and your own (bangs.json beside settings).
public static class Commands
{
    public static readonly CommandRegistry Registry = new();
    public static readonly Bangs Bangs = new(Store.File("bangs.json"));

    private static bool attached;

    public static void Attach(Browser b)
    {
        if (attached) return;
        attached = true;

        void add(string id, string title, Tier tier, string[] words, Action<CommandCall> run, string? keys = null, string? group = null) =>
            Registry.Add(new Command(id, title, tier, words, Group: group, Keys: keys), run);

        // Tabs
        add("tabs.new", "New Tab", Tier.Act, ["new tab"], _ => b.NewTab(), "Ctrl+T", "Tabs");
        add("tabs.private", "New Private Tab", Tier.Act, ["private", "new private tab", "incognito"], _ => b.NewShyTab(), "Ctrl+Shift+N", "Tabs");
        add("tabs.reopen", "Reopen Closed Tab", Tier.Act, ["reopen", "reopen closed tab", "undo close"], _ => b.Reopen(), "Ctrl+Shift+T", "Tabs");
        add("tabs.duplicate", "Duplicate Tab", Tier.Act, ["duplicate", "duplicate tab"], _ => b.Duplicate(), "Ctrl+D", "Tabs");
        add("tabs.close-others", "Close Other Tabs", Tier.Act, ["close others", "close other tabs"],
            _ => { if (b.Active is { } here) b.CloseOthers(here); }, group: "Tabs");
        add("tabs.pin", "Pin Tab", Tier.Act, ["pin", "pin tab"], _ => { if (b.Active is { } here) b.PinTab(here); }, group: "Tabs");

        // View
        add("view.sidebar", "Tabs in the Sidebar", Tier.Act, ["sidebar", "tabs in sidebar", "vertical tabs"], _ => b.ToggleSidebar(), "Ctrl+Shift+S", "View");
        add("view.light", "Light", Tier.Act, ["light", "light mode"], _ => b.Prefs.Look = Look.Light, group: "View");
        add("view.dark", "Dark", Tier.Act, ["dark", "dark mode"], _ => b.Prefs.Look = Look.Dark, group: "View");
        add("view.system", "Look Like Windows", Tier.Act, ["system look", "match windows"], _ => b.Prefs.Look = Look.System, group: "View");

        // The page
        add("page.find", "Find on Page", Tier.Read, ["find"], _ => b.OpenFind(), "Ctrl+F", "Page");
        add("page.reader", "Reading Mode", Tier.Act, ["reader", "reading mode"], _ => b.ToggleReader(), "Ctrl+Shift+R", "Page");
        add("page.float", "Float the Video", Tier.Act, ["float", "picture in picture", "pip"], _ => b.ToggleFloat(), "Ctrl+Shift+P", "Page");
        add("page.copy-address", "Copy Address", Tier.Read, ["copy address", "copy link", "copy url"], _ => b.CopyAddress(), "Ctrl+Shift+C", "Page");
        add("page.print", "Print", Tier.Act, ["print"], _ => b.PrintPage(), "Ctrl+P", "Page");
        add("page.bookmark", "Bookmark This Page", Tier.Act, ["bookmark"], _ => b.BookmarkCurrent(), "Ctrl+Shift+B", "Page");

        // Panels
        add("open.history", "History", Tier.Read, ["history"], _ => b.Recalling = true, "Ctrl+Y", "Open");
        add("open.downloads", "Downloads", Tier.Read, ["downloads"], _ => b.Hoarding = true, "Ctrl+Shift+J", "Open");
        add("open.bookmarks", "Bookmarks", Tier.Read, ["bookmarks"], _ => b.Bookmarking = true, group: "Open");
        add("open.passwords", "Passwords", Tier.Read, ["passwords"], _ => b.Managing = true, "Ctrl+Alt+L", "Open");
        add("open.settings", "Settings", Tier.Read, ["settings", "preferences"], _ => b.Tuning = true, "Ctrl+,", "Open");
        add("open.tools", "Tools", Tier.Read, ["tools", "all tools"], call => OpenTools(b, call.Argument), group: "Open");

        // Spaces
        add("spaces.new", "New Space", Tier.Act, ["new space"], _ => b.AskForSpace(), group: "Spaces");

        // Getting rid of things: always asked.
        add("history.clear", "Clear History", Tier.AlwaysAsks, ["clear history"], _ => b.ClearHistory(), group: "Privacy");
        add("sites.clear", "Clear Cookies and Site Data", Tier.AlwaysAsks, ["clear cookies", "clear site data"], _ => b.ClearSites(), group: "Privacy");
    }

    /// `>tools` opens the list; `>tools hash` opens a tool whose id starts with it.
    private static void OpenTools(Browser b, string argument)
    {
        var id = argument.Trim().ToLowerInvariant().Replace(' ', '-');
        if (ToolsHost.Resolve(id.Length == 0 ? "search://tools" : $"search://tools/{id}") is { } url && b.Active is { } tab)
            b.Go(tab, url);
    }

    /// Runs a command typed in the field. Always-asks commands ask here.
    public static async void Run(Command command, string argument)
    {
        try
        {
            await Registry.RunAsync(command.Id, argument, Source.Field, Confirm);
        }
        catch (Exception error)
        {
            Links.Trouble(error);
        }
    }

    /// The question an always-asks command puts to a person.
    public static async Task<bool> Confirm(Command command)
    {
        if (App.Root?.XamlRoot is not { } root) return false;
        var dialog = new ContentDialog
        {
            XamlRoot = root,
            RequestedTheme = App.Root.RequestedTheme,
            Title = $"{command.Title}?",
            Content = "This can't be undone.",
            PrimaryButtonText = command.Title,
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
        };
        try { return await dialog.ShowAsync() == ContentDialogResult.Primary; }
        catch { return false; }
    }

    /// A command, carried in a suggestion row's address so the field's list
    /// (which holds addresses) can hold it too.
    public static Uri Address(Command command, string argument) =>
        new($"search://command/{command.Id}#{Uri.EscapeDataString(argument)}");

    public static (Command Command, string Argument)? From(Uri url)
    {
        if (url.Scheme != "search" || url.Host != "command") return null;
        return Registry.Get(url.AbsolutePath.Trim('/')) is { } command
            ? (command, Uri.UnescapeDataString(url.Fragment.TrimStart('#')))
            : null;
    }
}
