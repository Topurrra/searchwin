using System.Runtime.InteropServices;
using Microsoft.Win32;

namespace Search;

// Being where links from other apps go.
//
// macOS lets a browser ask to be the default and puts up its own dialog.
// Windows doesn't let an app set itself: it lets it say, in the registry, that
// it *can* open web links, and then the person picks it in Settings. So the
// button writes the one set of keys every browser writes — for this user
// only, nothing that needs an administrator — and opens Settings at Search's
// own page. Nothing is written until the button is pressed.
public static class DefaultBrowser
{
    private const string Name = "Search";
    private const string ProgId = "SearchURL";
    private const string Client = @"Software\Clients\StartMenuInternet\" + Name;

    /// True when https links open here. Windows 11 keeps the choice under
    /// UserChoiceLatest on newer builds and UserChoice on older ones; either
    /// naming Search is enough.
    public static bool IsDefault
    {
        get
        {
            try
            {
                const string root = @"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\";
                foreach (var path in new[] { root + @"UserChoiceLatest\ProgId", root + "UserChoiceLatest", root + "UserChoice" })
                {
                    using var key = Registry.CurrentUser.OpenSubKey(path);
                    if (key?.GetValue("ProgId") is string id) return id == ProgId;
                }
            }
            catch { }
            return false;
        }
    }

    /// Search, offered to Windows as something that opens http and https.
    public static void Register()
    {
        var exe = Environment.ProcessPath ?? Path.Combine(AppContext.BaseDirectory, "Search.exe");
        var icon = $"\"{exe}\",0";

        using (var type = Registry.CurrentUser.CreateSubKey($@"Software\Classes\{ProgId}"))
        {
            type.SetValue("", "Search URL");
            type.SetValue("URL Protocol", "");
            using (var app = type.CreateSubKey("Application"))
            {
                app.SetValue("ApplicationName", Name);
                app.SetValue("ApplicationIcon", icon);
                app.SetValue("ApplicationCompany", "Office Commun");
            }
            using (var picture = type.CreateSubKey("DefaultIcon")) picture.SetValue("", icon);
            using (var command = type.CreateSubKey(@"shell\open\command")) command.SetValue("", $"\"{exe}\" \"%1\"");
        }

        using (var client = Registry.CurrentUser.CreateSubKey(Client))
        {
            client.SetValue("", Name);
            using (var picture = client.CreateSubKey("DefaultIcon")) picture.SetValue("", icon);
            using (var command = client.CreateSubKey(@"shell\open\command")) command.SetValue("", $"\"{exe}\"");
            using var able = client.CreateSubKey("Capabilities");
            able.SetValue("ApplicationName", Name);
            able.SetValue("ApplicationDescription", "A browser with nothing in the way.");
            able.SetValue("ApplicationIcon", icon);
            using (var start = able.CreateSubKey("StartMenu")) start.SetValue("StartMenuInternet", Name);
            using var urls = able.CreateSubKey("URLAssociations");
            urls.SetValue("http", ProgId);
            urls.SetValue("https", ProgId);
        }

        using (var registered = Registry.CurrentUser.CreateSubKey(@"Software\RegisteredApplications"))
            registered.SetValue(Name, Client + @"\Capabilities");

        // Explorer and Settings read the associations once and keep them;
        // this tells them to look again.
        SHChangeNotify(0x08000000, 0, IntPtr.Zero, IntPtr.Zero);
    }

    /// Registers, then opens Settings on Search's page, where the choice is
    /// the person's to make. False if either step failed.
    public static async Task<bool> Become()
    {
        try { Register(); }
        catch { return false; }
        try { return await Windows.System.Launcher.LaunchUriAsync(new Uri($"ms-settings:defaultapps?registeredAppUser={Name}")); }
        catch { return false; }
    }

    /// Settings is another window; the answer comes back whenever the person
    /// gets round to it. Polled once a second for a few minutes while
    /// whatever asked still wants to know, and `done` is told once if it
    /// happens.
    public static Microsoft.UI.Dispatching.DispatcherQueueTimer Watch(Action done)
    {
        var ticks = 0;
        Microsoft.UI.Dispatching.DispatcherQueueTimer? timer = null;
        timer = UI.Every(1, () =>
        {
            if (++ticks > 180) { timer?.Stop(); return; }
            if (!IsDefault) return;
            timer?.Stop();
            done();
        });
        return timer;
    }

    [DllImport("shell32.dll")]
    private static extern void SHChangeNotify(int eventId, uint flags, IntPtr item1, IntPtr item2);
}
