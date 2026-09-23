using System.Text.Json;
using Microsoft.Web.WebView2.Core;

namespace Search;

// A video that keeps playing after you have gone somewhere else, in a small
// window that stays above everything — other tabs, and other apps.
//
// The Mac could not ask its engine for this: WebKit will not hand a video to
// the system's picture-in-picture without a real click on the page, and
// nothing the app does counts as one. So there the whole page is moved into a
// little window of the app's own, everything but the video made invisible.
//
// Here the engine is Chromium, which is looser — the Mac's own notes say so —
// and a page can't be moved between windows anyway: a WebView2 belongs to the
// window it was made in. So this is the other way round from the Mac, and the
// way every Chromium browser does it: the page's own video is asked into the
// engine's picture-in-picture window. That window is always on top, sizes and
// moves the way Windows' own do, and has the controls a small window of video
// needs — play and pause, back to the tab, close — drawn by the engine that is
// playing it, with no second copy of the frames going through the app.
//
// The app asks through the DevTools protocol rather than an ordinary script,
// because a script run from outside the page carries no user activation, and
// requestPictureInPicture() refuses without one. Runtime.evaluate with
// userGesture set is the app's key press counting as the press it is.
//
// Only the video leaves. The page stays in its tab with the engine's own
// "playing in picture-in-picture" plate where the video was, so the tab never
// shows the Mac's "This page is playing in the floating window" — Tab.Floating
// stays false, because it isn't. Floating here says only which tab's video is
// out, so that tab is kept awake and brought home when you go back to it.
public sealed partial class Browser
{
    private Guid? floating;
    /// The tab whose video is currently out in the little window. Nothing
    /// floating means no window: the page tells the app when the window goes,
    /// whoever closed it.
    public Guid? Floating { get => floating; private set => Set(ref floating, value); }

    /// Asked for and not yet answered — a second tab switch while the first
    /// is still being asked must not open a second window over it.
    private bool lifting;

    partial void StartFloat()
    {
        // The little window closed from its own buttons, or by the page. The
        // engine's "back to tab" leaves the video playing and its close button
        // pauses it, which is the only difference the page can see between
        // them — so a video still running is someone asking to go back to it.
        Bridge.Handlers[FloatRelay.Name] = (tab, body) =>
        {
            if (Floating != tab.Id) return;
            Floating = null;
            var playing = body.ValueKind == JsonValueKind.Object
                && body.TryGetProperty("playing", out var p) && p.ValueKind == JsonValueKind.True;
            if (!playing || tab.Id == ActiveID || !Tabs.Contains(tab)) return;
            Select(tab);
            App.Window?.Activate();
            App.Window?.Foreground();
        };
    }

    partial void AttachFloat(Tab tab, CoreWebView2 core)
    {
        // A new document takes the old one's video with it, and the engine
        // closes the window without the old page having a chance to say so.
        core.ContentLoading += (_, _) => { if (Floating == tab.Id) Floating = null; };
        core.ProcessFailed += (_, _) => { if (Floating == tab.Id) Floating = null; };
    }

    /// Ctrl+Shift+P, for lifting one out by hand.
    public void ToggleFloat()
    {
        if (Floating != null)
        {
            Land();
            return;
        }
        Lift(Active, quietly: false);
    }

    /// The video goes into the engine's little window that stays above
    /// everything; the page it lives in stays where it is.
    private async void Lift(Tab? tab, bool quietly)
    {
        // A tab just put down with Ctrl+W has no page to lift a video out of,
        // and asking it would only build an empty view to ask. Nor has a page
        // that never started.
        if (tab is not { IsBlank: false, Asleep: false } || Floating != null || lifting) return;
        if (tab.Core is not { } core) return;
        // On its own, only from a site whose video is the point of the site.
        // A hero background on a studio's home page is a video too, and it
        // followed people around the desktop. Ctrl+Shift+P still lifts from
        // anywhere.
        if (quietly && !Players.Knows(tab.Address)) return;

        lifting = true;
        string answer;
        try { answer = await Gesture(core, Isolate.On); }
        finally { lifting = false; }

        if (answer != "floating")
        {
            if (quietly) return;
            Announce(answer == "refused" ? "This video can't be lifted out" : "Nothing is playing here");
            return;
        }
        if (!Tabs.Contains(tab) || Floating != null)
        {
            _ = core.ExecuteScriptAsync(Isolate.Off);
            return;
        }
        Floating = tab.Id;
        // Stepped back to the tab while the window was on its way: it comes
        // home at once rather than hanging over the page it belongs to.
        if (quietly && tab.Id == ActiveID) Land();
    }

    /// Back into its tab. The window closes whatever else is true — tying
    /// that to the bookkeeping is how a little window outlives the thing that
    /// opened it.
    public void Land()
    {
        if (Floating is not { } id) return;
        Floating = null;
        var tab = Tabs.Concat(ParkedTabs).FirstOrDefault(t => t.Id == id);
        if (tab?.Core is { } core) _ = core.ExecuteScriptAsync(Isolate.Off);
    }

    /// Runs a script in the page as though the person had just pressed
    /// something on it, and hands back the string it settled on.
    private static async Task<string> Gesture(CoreWebView2 core, string expression)
    {
        try
        {
            var ask = new System.Text.Json.Nodes.JsonObject
            {
                ["expression"] = expression,
                ["userGesture"] = true,
                ["awaitPromise"] = true,
                ["returnByValue"] = true,
            }.ToJsonString();
            var json = await core.CallDevToolsProtocolMethodAsync("Runtime.evaluate", ask);
            using var doc = JsonDocument.Parse(json);
            var root = doc.RootElement;
            if (root.TryGetProperty("exceptionDetails", out _)) return "refused";
            return root.TryGetProperty("result", out var result)
                && result.TryGetProperty("value", out var value)
                && value.ValueKind == JsonValueKind.String
                ? value.GetString() ?? "none" : "none";
        }
        catch { return "none"; }
    }
}

/// Word from the page that its video has come back out of the little window.
public static class FloatRelay
{
    public const string Name = "officeFloat";
}

/// Sites with a player worth following into the little window.
///
/// Anywhere else, a playing video is as likely to be a background as a film,
/// and the difference isn't something a script can tell from the outside. So
/// the list is of places people go to watch, and the shortcut covers the rest.
public static class Players
{
    /// A host suffix, and for a few shops that also stream, the path that
    /// separates the film from the product page.
    private static readonly (string Host, string? Path)[] Known =
    [
        ("youtube.com", null), ("youtu.be", null), ("netflix.com", null),
        ("primevideo.com", null), ("amazon.com", "/gp/video"), ("amazon.fr", "/gp/video"),
        ("amazon.co.uk", "/gp/video"), ("amazon.de", "/gp/video"),
        ("disneyplus.com", null), ("tv.apple.com", null), ("twitch.tv", null),
        ("vimeo.com", null), ("dailymotion.com", null), ("max.com", null), ("hbomax.com", null),
        ("canalplus.com", null), ("mycanal.fr", null), ("arte.tv", null), ("france.tv", null),
        ("tf1.fr", null), ("6play.fr", null), ("crunchyroll.com", null), ("plex.tv", null),
        ("peacocktv.com", null), ("hulu.com", null), ("paramountplus.com", null),
        ("molotov.tv", null), ("ocs.fr", null), ("mubi.com", null), ("criterionchannel.com", null),
        ("ted.com", null), ("nebula.tv", null), ("curiositystream.com", null),
    ];

    public static bool Knows(Uri? url)
    {
        if (Address.Host(url) is not { } host) return false;
        var path = url!.AbsolutePath.ToLowerInvariant();
        return Known.Any(entry =>
            (host == entry.Host || host.EndsWith("." + entry.Host, StringComparison.Ordinal))
            && (entry.Path is not { } needle || path.StartsWith(needle, StringComparison.Ordinal)));
    }
}

public static class Isolate
{
    /// The biggest video that is actually playing, into the engine's little
    /// window. Answers "floating", "none" when nothing is playing, or
    /// "refused" when the engine or the page wouldn't have it.
    public const string On = """
    (async function () {
      var videos = document.querySelectorAll('video');
      var best = null, area = 0;
      for (var i = 0; i < videos.length; i++) {
        var v = videos[i];
        if (v.paused || v.ended || v.readyState < 2) continue;
        var box = v.getBoundingClientRect();
        if (box.width * box.height >= area) { area = box.width * box.height; best = v; }
      }
      if (!best) return 'none';
      // Already out, because the player put it there itself: the app only
      // starts listening for it coming back.
      if (document.pictureInPictureElement !== best) {
        if (!document.pictureInPictureEnabled || !best.requestPictureInPicture) return 'refused';
        // A player that asked for no picture-in-picture is overruled, as the
        // Mac overrules every page it lifts from: the person asked, not the
        // page.
        best.disablePictureInPicture = false;
        try { await best.requestPictureInPicture(); } catch (e) { return 'refused'; }
      }

      best.setAttribute('data-office-float', '');
      best.addEventListener('leavepictureinpicture', function () {
        best.removeAttribute('data-office-float');
        // A beat later: the engine's close button pauses the video on its
        // way out, and "back to tab" doesn't, and that is how the app tells
        // them apart.
        setTimeout(function () {
          window.__officePost('officeFloat', { playing: !best.paused && !best.ended });
        }, 150);
      }, { once: true });
      return 'floating';
    })()
    """;

    public const string Off = """
    (function () {
      var video = document.querySelector('[data-office-float]');
      if (video) video.removeAttribute('data-office-float');
      if (document.pictureInPictureElement && document.exitPictureInPicture) {
        document.exitPictureInPicture().catch(function () {});
      }
      return 'landed';
    })();
    """;
}
