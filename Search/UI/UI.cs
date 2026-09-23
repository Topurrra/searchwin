using Microsoft.UI.Dispatching;

namespace Search;

/// The UI thread, from anywhere. WinUI, like AppKit, draws from one thread,
/// and everything that changes what is drawn is sent back to it.
public static class UI
{
    public static DispatcherQueue Queue { get; set; } = null!;

    public static bool OnMain => Queue?.HasThreadAccess == true;

    public static void Main(Action act)
    {
        if (OnMain) act();
        else Queue.TryEnqueue(() => act());
    }

    /// The next turn of the loop — for things that must not happen inside
    /// whatever is happening now.
    public static void Soon(Action act) => Queue.TryEnqueue(DispatcherQueuePriority.Normal, () => act());

    /// A piece of work for later, that can be called off before it runs —
    /// what DispatchWorkItem is on the Mac.
    public static Later After(double seconds, Action act)
    {
        var later = new Later(act);
        var timer = Queue.CreateTimer();
        timer.Interval = TimeSpan.FromSeconds(Math.Max(0, seconds));
        timer.IsRepeating = false;
        timer.Tick += (t, _) =>
        {
            t.Stop();
            later.Run();
        };
        later.Timer = timer;
        timer.Start();
        return later;
    }

    public static DispatcherQueueTimer Every(double seconds, Action act)
    {
        var timer = Queue.CreateTimer();
        timer.Interval = TimeSpan.FromSeconds(seconds);
        timer.IsRepeating = true;
        timer.Tick += (_, _) => act();
        timer.Start();
        return timer;
    }
}

public sealed class Later(Action act)
{
    internal DispatcherQueueTimer? Timer;
    public bool Cancelled { get; private set; }
    public bool Done { get; private set; }

    public void Cancel()
    {
        Cancelled = true;
        Timer?.Stop();
    }

    internal void Run()
    {
        if (Cancelled || Done) return;
        Done = true;
        act();
    }
}
