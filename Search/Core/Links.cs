using Microsoft.Windows.AppLifecycle;

namespace Search;

// Links from elsewhere. A click in Mail, in Slack, in a PDF — Windows hands the
// address to whichever app owns http, and this is how that app takes it: on
// the command line of a new launch, or redirected from a second launch to the
// one already running. Addresses can arrive before the window has been built,
// so they wait here until the browser says it is ready for them.
public static class Links
{
    private static Browser? browser;
    private static readonly List<Uri> waiting = [];

    /// A launch's own arguments.
    public static void Take(IEnumerable<string> args)
    {
        foreach (var arg in args)
        {
            var text = arg.Trim().Trim('"');
            if (Uri.TryCreate(text, UriKind.Absolute, out var url) && (Address.IsWeb(url) || url.IsFile))
                Arrive(url);
        }
    }

    /// Another launch, redirected here.
    public static void Activated(AppActivationArguments e)
    {
        var args = e.Kind == ExtendedActivationKind.Launch && e.Data is Windows.ApplicationModel.Activation.ILaunchActivatedEventArgs launch
            ? Split(launch.Arguments)
            : [];
        UI.Main(() =>
        {
            Take(args);
            // The window comes forward either way, like clicking the app again.
            App.Window?.Activate();
            App.Window?.Foreground();
        });
    }

    private static IEnumerable<string> Split(string line)
    {
        var parts = new List<string>();
        var current = new System.Text.StringBuilder();
        var quoted = false;
        foreach (var c in line)
        {
            if (c == '"') { quoted = !quoted; continue; }
            if (c == ' ' && !quoted)
            {
                if (current.Length > 0) parts.Add(current.ToString());
                current.Clear();
                continue;
            }
            current.Append(c);
        }
        if (current.Length > 0) parts.Add(current.ToString());
        // The first is the program itself, when Windows includes it.
        return parts.Where(p => !p.EndsWith(".exe", StringComparison.OrdinalIgnoreCase));
    }

    private static void Arrive(Uri url)
    {
        if (browser == null) { waiting.Add(url); return; }
        browser.Arrive(url);
    }

    /// The browser, once it has a window. Anything that came earlier is handed
    /// over now — the first into the blank tab that is already there, the
    /// others behind it, a few frames apart, in the order they came.
    public static void Ready(Browser ready)
    {
        browser = ready;
        var early = waiting.ToList();
        waiting.Clear();
        if (early.Count == 0) return;
        UI.After(0.05, () =>
        {
            ready.Arrive(early[0]);
            for (var n = 1; n < early.Count; n++)
            {
                var url = early[n];
                UI.After(0.15 * n, () => ready.Open(url, foreground: false, atEnd: true));
            }
        });
    }

    /// The nearest thing to a crash reporter a browser with no server can
    /// have: nothing is sent anywhere, but a line is kept, so a bug report
    /// can say what went wrong.
    public static void Trouble(Exception e)
    {
        try
        {
            File.AppendAllText(Store.File("crash.log"), $"{DateTime.Now:u} — {e.GetType().Name}: {e.Message}\n{e.StackTrace}\n\n");
        }
        catch { }
    }
}
