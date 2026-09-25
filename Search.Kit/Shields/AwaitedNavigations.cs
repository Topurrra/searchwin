namespace SearchKit.Shields;

/// The pages tabs are on their way to, for a little while.
///
/// A service worker that serves its site's pages fetches each navigation
/// itself, and WebView2 hands that fetch over like any other of the worker's
/// requests — no Document context, no Sec-Fetch headers. What a request
/// *says* about itself (an HTML Accept, Upgrade-Insecure-Requests) can't be
/// what spares it: a page can send those headers on any fetch it likes. What
/// can: it's the very address a tab has just started to load. So every
/// top-level navigation is noted here, and a worker's request for exactly
/// that address is the page itself, never refused.
public sealed class AwaitedNavigations(TimeSpan? keep = null, int capacity = 64)
{
    private readonly TimeSpan keep = keep ?? TimeSpan.FromSeconds(30);
    private readonly Dictionary<string, DateTime> awaited = new(StringComparer.Ordinal);
    private readonly Lock gate = new();

    /// A tab has started for `url`.
    public void Expect(Uri url, DateTime? now = null)
    {
        if (Key(url) is not { } key) return;
        var at = now ?? DateTime.UtcNow;
        lock (gate)
        {
            Prune(at);
            // Bounded: the oldest goes first when many tabs start at once.
            if (awaited.Count >= capacity && !awaited.ContainsKey(key))
                awaited.Remove(awaited.MinBy(pair => pair.Value).Key);
            awaited[key] = at;
        }
    }

    /// Whether `url` is a page a tab is waiting for.
    public bool IsAwaited(Uri url, DateTime? now = null)
    {
        if (Key(url) is not { } key) return false;
        var at = now ?? DateTime.UtcNow;
        lock (gate)
        {
            return awaited.TryGetValue(key, out var since) && at - since <= keep;
        }
    }

    private void Prune(DateTime at)
    {
        foreach (var stale in awaited.Where(pair => at - pair.Value > keep).Select(pair => pair.Key).ToList())
            awaited.Remove(stale);
    }

    /// The address without its fragment (never sent), host lower-cased.
    private static string? Key(Uri url) =>
        url.IsAbsoluteUri && url.Scheme is "http" or "https"
            ? url.GetComponents(UriComponents.HttpRequestUrl, UriFormat.UriEscaped).ToLowerInvariant()
            : null;
}
