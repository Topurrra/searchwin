using Microsoft.Web.WebView2.Core;

namespace Search;

// Tabs you aren't using, put to sleep.
//
// A page open in a tab keeps its whole renderer — a hundred to three hundred
// megabytes, running its timers, holding its sockets — for as long as the tab
// exists. Twenty tabs is two or three gigabytes spent on the nineteen nobody is
// looking at. So a tab left alone for half an hour is suspended: WebView2
// freezes its page where it is and hands most of its memory back, and showing
// it again thaws it exactly as it was — history, scroll, half-typed form.
//
// Some tabs never sleep, because freezing them would stop what they were
// doing: the one on screen, pinned tabs (those are put down by hand, with
// Ctrl+W), a tab playing sound, on a call, sending a download, holding its
// video out in the little window, or holding something typed and not sent.
//
// When Windows says memory is short, the half hour shrinks: to five minutes.
public sealed partial class Browser
{
    /// How long a tab has to go without being looked at. Half an hour, or
    /// `sleep.after` in seconds — for the bench and the measurements.
    public static double SleepAfter
    {
        get
        {
            var set = Store.Settings.Double("sleep.after");
            return set > 0 ? set : 30 * 60;
        }
    }

    private Microsoft.UI.Dispatching.DispatcherQueueTimer? dozing;

    /// Started once, at launch.
    private void WatchForSleep()
    {
        var every = Math.Min(60, Math.Max(5, SleepAfter / 4));
        dozing = UI.Every(every, () =>
        {
            // Short of memory: the wait shrinks.
            var load = MemoryLoad();
            SleepIdle(load >= 90 ? 5 * 60 : null);
        });
    }

    /// Every tab that has gone long enough without being looked at, the one
    /// left longest first.
    public void SleepIdle(double? within = null)
    {
        if (!Prefs.SleepsTabs) return;
        var wait = within ?? SleepAfter;
        var now = DateTime.UtcNow;
        // The rows of the other spaces too: parked is not the same as used.
        var idle = Tabs.Concat(ParkedTabs)
            .Where(t => (now - t.Touched).TotalSeconds >= wait && AwakeBecause(t) == null)
            .OrderBy(t => t.Touched)
            .ToList();
        foreach (var tab in idle) _ = Sleep(tab);
    }

    /// Why a tab has to stay awake — null when nothing keeps it. The clock is
    /// the caller's business; this is everything else.
    public string? AwakeBecause(Tab tab)
    {
        if (tab.Id == ActiveID) return "on screen";
        if (tab.Pin != null) return "pinned";
        if (tab.Bench) return "a bench tab";
        if (tab.IsBlank) return "blank";
        if (tab.Asleep || tab.Dozing) return "already asleep";
        if (tab.Core == null) return "no page";
        if (tab.Loading) return "still loading";
        if (tab.Noisy) return "playing sound";
        if (tab.Floating || Floating == tab.Id) return "its video is out";
        if (Downloading.Any(d => d.tab == tab)) return "downloading";
        // A sign-in window hands its answer back to the page that opened it.
        if (Active?.Opener == tab.Id) return "the page on screen came from it";
        return null;
    }

    /// Asks the page whether it holds anything typed, then freezes it —
    /// looking again at each step, since each takes a moment and you may have
    /// gone back to the tab in the meantime.
    public async Task<string> Sleep(Tab tab)
    {
        if (AwakeBecause(tab) is { } reason) return reason;
        if (await tab.Unsaved()) return "holding something typed";
        if (AwakeBecause(tab) is { } again) return again;
        // A call: the page is using the camera or the microphone.
        if (await OnCall(tab)) return "on a call";
        return await tab.Doze() ? "asleep" : "wouldn't sleep";
    }

    private static async Task<bool> OnCall(Tab tab)
    {
        var answer = await tab.Eval("""
            (function () {
              try {
                var els = document.querySelectorAll('video, audio');
                for (var i = 0; i < els.length; i++) {
                  var s = els[i].srcObject;
                  if (s && s.getTracks && s.getTracks().some(function (t) { return t.readyState === 'live'; })) return true;
                }
              } catch (e) {}
              return false;
            })()
            """);
        return answer is { ValueKind: System.Text.Json.JsonValueKind.True };
    }

    [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
    private struct MemoryStatus
    {
        public uint Length, Load;
        public ulong TotalPhys, AvailPhys, TotalPageFile, AvailPageFile, TotalVirtual, AvailVirtual, AvailExtendedVirtual;
    }

    [System.Runtime.InteropServices.DllImport("kernel32.dll")]
    private static extern bool GlobalMemoryStatusEx(ref MemoryStatus status);

    /// How much of the machine's memory is in use, in percent.
    private static uint MemoryLoad()
    {
        var status = new MemoryStatus { Length = (uint)System.Runtime.InteropServices.Marshal.SizeOf<MemoryStatus>() };
        return GlobalMemoryStatusEx(ref status) ? status.Load : 0;
    }
}
