namespace SearchKit.Shields;

/// Unwraps known link-shim/redirector URLs to the address they actually go
/// to — "debouncing", the same idea as Brave's. These are first-party pages
/// doing the redirecting (a search result, a share button, a comment link),
/// not a tracking *domain* to block outright, so this only ever rewrites the
/// navigation; `Params` and the network filter list are the other two legs
/// of Shields.
public static class Redirects
{
    private readonly record struct Rule(Func<string, bool> HostMatches, string? Path, string[] Params, string Why);

    private static readonly Rule[] Table =
    [
        new(IsGoogle, "/url", ["q", "url"], "Search-result and AMP-viewer click-through links."),
        new(h => Sub(h, "l.facebook.com"), "/l.php", ["u"], "Facebook's outbound link shim."),
        new(h => Sub(h, "lm.facebook.com"), "/l.php", ["u"], "Same shim, Messenger's subdomain."),
        new(h => Sub(h, "l.instagram.com"), null, ["u"], "Instagram's bio-link shim."),
        new(h => Sub(h, "youtube.com"), "/redirect", ["q"], "YouTube description/comment links."),
        new(h => Sub(h, "out.reddit.com"), null, ["url"], "Reddit's outbound link shim."),
        new(h => Sub(h, "steamcommunity.com"), "/linkfilter", ["url"], "Steam's outbound-link warning page."),
        new(h => Sub(h, "slack.com"), "/redir", ["url"], "Slack's link-unfurl redirector."),
        new(h => Sub(h, "disq.us"), "/url", ["url"], "Disqus comment links."),
    ];

    /// The target, or null when `url` isn't one of the known redirectors, or
    /// the target it carries isn't itself a plain http(s) address (a relative
    /// path, a `javascript:`/`data:` URL, or nothing usable at all).
    public static Uri? Unwrap(Uri url)
    {
        if (url.Scheme is not ("http" or "https")) return null;
        var host = url.Host;

        // href.li never encodes the target as a query parameter at all: the
        // whole rest of the address, after the `?`, *is* the target
        // (`href.li/?https://example.com`), so it needs its own case rather
        // than a Params entry.
        if (Sub(host, "href.li"))
            return url.Query.Length > 1 ? AsHttpTarget(url.Query[1..]) : null;

        if (url.Query.Length <= 1) return null;

        foreach (var rule in Table)
        {
            if (!rule.HostMatches(host)) continue;
            if (rule.Path != null && !string.Equals(url.AbsolutePath.TrimEnd('/'), rule.Path, StringComparison.OrdinalIgnoreCase)) continue;
            foreach (var name in rule.Params)
            {
                var raw = Find(url.Query, name);
                if (raw == null) continue;
                string decoded;
                try { decoded = Uri.UnescapeDataString(raw); }
                catch { continue; }
                if (AsHttpTarget(decoded) is { } target) return target;
            }
        }
        return null;
    }

    private static Uri? AsHttpTarget(string text) =>
        Uri.TryCreate(text, UriKind.Absolute, out var target) && target.Scheme is "http" or "https" ? target : null;

    private static bool Sub(string host, string domain) =>
        host.Equals(domain, StringComparison.OrdinalIgnoreCase) || host.EndsWith("." + domain, StringComparison.OrdinalIgnoreCase);

    // google.com, google.co.uk, google.de… any of Google's country domains,
    // via the same registrable-domain guess the filter matcher uses — never
    // a bare `host.StartsWith("google.")`, which "google.evil.example"
    // would also pass.
    private static bool IsGoogle(string host) =>
        SimpleRegistrableDomain.Instance.Of(host) is { } site && site.StartsWith("google.", StringComparison.OrdinalIgnoreCase);

    private static string? Find(string query, string name)
    {
        var text = query.Length > 0 && query[0] == '?' ? query[1..] : query;
        foreach (var part in text.Split('&'))
        {
            if (part.Length == 0) continue;
            var eq = part.IndexOf('=');
            var key = eq < 0 ? part : part[..eq];
            string decodedKey;
            try { decodedKey = Uri.UnescapeDataString(key); }
            catch { decodedKey = key; }
            if (!decodedKey.Equals(name, StringComparison.OrdinalIgnoreCase)) continue;
            return eq < 0 ? "" : part[(eq + 1)..];
        }
        return null;
    }
}
