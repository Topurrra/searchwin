using System.Runtime.InteropServices;
using Microsoft.Web.WebView2.Core;

namespace Search;

// What pages ask of the window: somewhere to open a link, somewhere to put a
// file, permission to see or hear you, a way to say they didn't load.
public sealed partial class Browser : IPageHost
{
    public Tab? TabFor(CoreWebView2 core) => Tabs.FirstOrDefault(t => t.Core == core) ?? ParkedTabs.FirstOrDefault(t => t.Core == core);

    public void Attach(Tab tab, CoreWebView2 core)
    {
        core.Profile.PreferredColorScheme = Palette.Dark ? CoreWebView2PreferredColorScheme.Dark : CoreWebView2PreferredColorScheme.Light;

        // A page just built for the tab on screen comes onto the stage the
        // moment it exists.
        if (tab.Id == ActiveID && !tab.IsBlank && !tab.Asleep && !tab.Floating) tab.Show(true);

        core.NavigationStarting += (sender, e) =>
        {
            if (!Uri.TryCreate(e.Uri, UriKind.Absolute, out var url)) return;
            var scheme = url.Scheme.ToLowerInvariant();

            // An extension's OAuth sign-in coming back: the address is the
            // answer, handed to the extension, and never loaded.
            if (Extensions.Shared.Intercept(url))
            {
                e.Cancel = true;
                return;
            }

            // The next document gets this site's stylesheet of hidden things,
            // decided here because here is the last moment before it loads.
            var host = Curtain.Host(url);
            tab.Arm(Curtain.Css(host), Zooming.For(Address.Host(url)));

            // Links the window has no business showing — mail, calls, an app's
            // own scheme — are handed to whoever does own them.
            if (!Known.Contains(scheme))
            {
                e.Cancel = true;
                _ = Windows.System.Launcher.LaunchUriAsync(url);
            }
        };

        core.LaunchingExternalUriScheme += (sender, e) =>
        {
            // Handed over without Edge's own dialog in between, as the Mac
            // hands such links to macOS.
            e.Cancel = true;
            if (Uri.TryCreate(e.Uri, UriKind.Absolute, out var url)) _ = Windows.System.Launcher.LaunchUriAsync(url);
        };

        core.NavigationCompleted += (_, e) =>
        {
            if (!e.IsSuccess)
            {
                Fail(tab, core, e.WebErrorStatus);
                return;
            }
            tab.Failure = null;
            tab.Uncover();
            TellStore(tab);
            // A page that arrived after a password went out: did the sign-in
            // take?
            SettleSignIn(tab);
            if (tab.Shy || tab.Bench || tab.Address is not { } url) return;
            History.Record(url, tab.Title);
        };

        core.NewWindowRequested += (_, e) => OpenWindow(tab, e);

        core.WindowCloseRequested += (_, _) =>
        {
            // A page asking to close itself. Signing in with Google — or with
            // anything using OAuth — happens in a window the page opens, and
            // that window calls close() when it is done. Back to whoever
            // opened it, so you land where you started the sign-in.
            if (tab.Opener is { } opener && Tabs.FirstOrDefault(t => t.Id == opener) is { } home) Select(home);
            tab.Pin = null;
            Close(tab);
        };

        core.DownloadStarting += (_, e) => Keep(tab, e);
        core.PermissionRequested += (_, e) => AskPermission(tab, e);

        core.ProcessFailed += (_, e) =>
        {
            if (e.ProcessFailedKind is not (CoreWebView2ProcessFailedKind.RenderProcessExited
                or CoreWebView2ProcessFailedKind.RenderProcessUnresponsive
                or CoreWebView2ProcessFailedKind.FrameRenderProcessExited)) return;
            // On screen it comes straight back; elsewhere it comes back when it
            // is next looked at.
            if (tab.Id == ActiveID) tab.RecoverFromCrash();
            else tab.Stale = true;
        };

        core.ContextMenuRequested += (_, e) => Extensions.Shared.AddMenuItems(tab, core, e);
        AttachOwnPages(tab, core);

        // Next and previous land a moment after they are asked for; the
        // count beside the find field follows the engine, not the asking.
        try
        {
            void Count(object? sender, object e)
            {
                if (tab.Id != ActiveID || !Finding) return;
                var find = core.Find;
                Missed = Needle.Length > 0 && find.MatchCount == 0;
                Matches = find.MatchCount > 0 ? $"{Math.Max(1, find.ActiveMatchIndex)} of {find.MatchCount}" : "";
            }
            core.Find.ActiveMatchIndexChanged += Count;
            core.Find.MatchCountChanged += Count;
        }
        catch { }
        AttachFeatures(tab, core);
    }

    /// chrome-extension: an extension's own pages — options, a side panel, a
    /// tab it opened. The engine serves them; nothing else here does.
    private static readonly HashSet<string> Known =
        ["http", "https", "file", "about", "data", "blob", "chrome-extension", "edge", "view-source"];

    /// A link that asks for a new window gets a new tab. The new tab's engine
    /// has to be the one the page is handed, or the opener and the opened
    /// can't talk to each other — which is what every sign-in popup needs.
    private void OpenWindow(Tab from, CoreWebView2NewWindowRequestedEventArgs e)
    {
        var deferral = e.GetDeferral();
        // Ctrl-click opens beside this tab and leaves you where you are;
        // Ctrl+Shift-click takes you with it. Middle-click does what
        // Ctrl-click does.
        var apart = Keys.Down(Keys.Control) || Keys.Down(Keys.MiddleButton);
        var background = apart && !Keys.Down(Keys.Shift) && e.IsUserInitiated;

        var tab = new Tab(shy: from.Shy, profile: from.Profile) { Opener = from.Id };
        Prepare(tab);
        var here = Tabs.IndexOf(from);
        Tabs.Insert(here >= 0 ? here + 1 : Tabs.Count, tab);
        TellTabs();
        if (Uri.TryCreate(e.Uri, UriKind.Absolute, out var url)) tab.SetAddressOptimistically(url);
        if (!background)
        {
            Leaving();
            ActiveID = tab.Id;
            Editing = false;
        }
        tab.WhenReady(core =>
        {
            try
            {
                e.NewWindow = core;
                e.Handled = true;
            }
            catch { }
            deferral.Complete();
        });
        RememberSession();
    }

    // MARK: - the camera and the microphone

    /// A page asking to see or hear you, waiting for an answer. WebView2 hands
    /// over a deferral and holds the page until it is completed — so this keeps
    /// the deferral and the question together, and never drops either.
    public sealed record CaptureAsk(string Host, string Wants);

    private CaptureAsk? asking;
    public CaptureAsk? Asking { get => asking; private set => Set(ref asking, value); }
    private (CoreWebView2PermissionRequestedEventArgs args, Windows.Foundation.Deferral deferral)? decide;
    private string askedAbout = "";

    public void AllowCapture() => AnswerCapture(true);
    public void DenyCapture() => AnswerCapture(false);

    private void AnswerCapture(bool grant)
    {
        if (decide is not { } d) return;
        // Remembered per site, so a call you take every week asks once.
        Store.Settings.Set("capture." + askedAbout, grant);
        d.args.State = grant ? CoreWebView2PermissionState.Allow : CoreWebView2PermissionState.Deny;
        d.deferral.Complete();
        decide = null;
        askedAbout = "";
        Asking = null;
    }

    private void AskPermission(Tab tab, CoreWebView2PermissionRequestedEventArgs e)
    {
        var wants = e.PermissionKind switch
        {
            CoreWebView2PermissionKind.Camera => "camera",
            CoreWebView2PermissionKind.Microphone => "microphone",
            _ => null,
        };
        // Only the camera and microphone are asked about in the window's own
        // words. Notifications are refused, as the Mac app has none to give;
        // the rest keep WebView2's own question.
        if (e.PermissionKind == CoreWebView2PermissionKind.Notifications)
        {
            e.State = CoreWebView2PermissionState.Deny;
            return;
        }
        if (wants == null) return;
        var host = Uri.TryCreate(e.Uri, UriKind.Absolute, out var origin) && origin.Host.Length > 0
            ? origin.Host : Address.Host(tab.Address) ?? "This page";
        var key = $"{host}|{wants}";
        if (Store.Settings.OptionalBool("capture." + key) is { } remembered)
        {
            e.State = remembered ? CoreWebView2PermissionState.Allow : CoreWebView2PermissionState.Deny;
            return;
        }
        // One question at a time. A second page asking while the first is
        // still waiting is refused rather than queued behind it.
        if (decide != null)
        {
            e.State = CoreWebView2PermissionState.Deny;
            return;
        }
        e.SavesInProfile = false;
        decide = (e, e.GetDeferral());
        askedAbout = key;
        Asking = new CaptureAsk(host, wants);
    }

    // MARK: - keeping files

    public Loot Loot { get; } = new();

    /// Downloads still under way, so a tab still sending one to disk is never
    /// put to sleep.
    public readonly List<(Tab tab, CoreWebView2DownloadOperation op)> Downloading = [];

    /// The names extensions asked their downloads to be saved under.
    public readonly Dictionary<string, string> NamedDownloads = [];

    public string DownloadsFolder => SpaceDownloads ?? Prefs.Downloads;

    private void Keep(Tab tab, CoreWebView2DownloadStartingEventArgs e)
    {
        var op = e.DownloadOperation;
        var asked = NamedDownloads.Remove(op.Uri, out var n) ? n : null;
        var name = asked ?? Path.GetFileName(e.ResultFilePath);
        if (string.IsNullOrWhiteSpace(name)) name = "download";

        if (Prefs.AsksWhereToSave)
        {
            // WebView2's own Save As, which is Windows' own.
            e.Handled = false;
        }
        else
        {
            e.ResultFilePath = Free(name, DownloadsFolder);
            // Nothing of Edge's download bubble: the line at the bottom says
            // it, and Downloads (Ctrl+Shift+J) keeps it.
            e.Handled = true;
            Announce($"Downloading {Path.GetFileName(e.ResultFilePath)}");
        }

        var entry = (tab, op);
        Downloading.Add(entry);
        var from = Uri.TryCreate(op.Uri, UriKind.Absolute, out var source) ? source.Host : "";
        op.StateChanged += (_, _) =>
        {
            switch (op.State)
            {
                case CoreWebView2DownloadState.Completed:
                    Downloading.Remove(entry);
                    Loot.Add(new Keep { Name = Path.GetFileName(op.ResultFilePath), From = from, Path = op.ResultFilePath, Date = DateTime.Now });
                    Announce($"Saved {Path.GetFileName(op.ResultFilePath)}");
                    break;
                case CoreWebView2DownloadState.Interrupted:
                    Downloading.Remove(entry);
                    if (op.InterruptReason != CoreWebView2DownloadInterruptReason.UserCanceled) Announce("Download failed");
                    break;
            }
        };
    }

    /// Never over a file that is already there: the name gains a number rather
    /// than the download quietly replacing something.
    private static string Free(string name, string folder)
    {
        Directory.CreateDirectory(folder);
        var stem = Path.GetFileNameWithoutExtension(name);
        var ext = Path.GetExtension(name);
        var candidate = Path.Combine(folder, name);
        for (var n = 2; File.Exists(candidate); n++) candidate = Path.Combine(folder, $"{stem} {n}{ext}");
        return candidate;
    }
}

/// Which keys and buttons are down right now, asked of Windows directly — the
/// Mac reads the same from the event that caused a navigation.
public static class Keys
{
    public const int Shift = 0x10, Control = 0x11, Menu = 0x12, MiddleButton = 0x04, LeftWin = 0x5B;

    [DllImport("user32.dll")]
    private static extern short GetAsyncKeyState(int key);

    public static bool Down(int key) => (GetAsyncKeyState(key) & 0x8000) != 0;
}
