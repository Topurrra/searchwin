namespace Search;

// What you type has to be a place. This either hands back an address or hands
// back nothing — and nothing is worth saying out loud, because the alternative
// is a browser that silently does something else with your keystrokes.
public static class Address
{
    /// Schemes the window can show itself. Anything else typed with a scheme —
    /// mailto:, a custom app link — is somebody else's job and gets refused here
    /// rather than opening a blank tab.
    private static readonly HashSet<string> Ours = ["http", "https", "file", "about", "data"];

    public static Uri? Url(string typed)
    {
        var text = typed.Trim();
        if (text.Length == 0 || text.Contains(' ')) return null;

        // Written with a scheme, it is taken at its word.
        var split = text.IndexOf("://", StringComparison.Ordinal);
        if (split >= 0)
        {
            var scheme = text[..split].ToLowerInvariant();
            // search:// is Search's own: the tool pages (see ToolsHost).
            if (scheme == "search") return ToolsHost.Resolve(text);
            if (!Ours.Contains(scheme)) return null;
            return Uri.TryCreate(text, UriKind.Absolute, out var u) ? u : null;
        }
        var lower = text.ToLowerInvariant();
        if (lower.StartsWith("about:") || lower.StartsWith("data:"))
            return Uri.TryCreate(text, UriKind.Absolute, out var u) ? u : null;

        // Everything else has to look like a host before it gets a scheme put
        // in front of it. "hello world" is not a website, and neither is "todo".
        var end = text.IndexOfAny(['/', '?', '#']);
        var head = end < 0 ? text : text[..end];
        if (head.Contains('@')) return null; // an email address
        var host = head.Split(':')[0];
        if (!LooksLikeHost(host)) return null;

        // A local server almost never has a certificate, so https there is a
        // connection failure rather than a page.
        var local = host == "localhost" || host.EndsWith(".localhost") || host == "127.0.0.1"
            || host == "0.0.0.0" || host.StartsWith("192.168.") || host.StartsWith("10.");
        return Uri.TryCreate((local ? "http://" : "https://") + text, UriKind.Absolute, out var url) ? url : null;
    }

    private static bool LooksLikeHost(string host)
    {
        if (host == "localhost") return true;

        // Four numbers is an address on the local network as often as not.
        var numbers = host.Split('.');
        if (numbers.Length == 4 && numbers.All(n => byte.TryParse(n, out _))) return true;

        if (numbers.Length < 2) return false;
        foreach (var label in numbers)
        {
            if (label.Length == 0 || label.StartsWith('-') || label.EndsWith('-')) return false;
            if (!label.All(c => char.IsLetterOrDigit(c) || c == '-')) return false;
        }
        // The last label carries the weight: a dotted thing ending in letters is
        // a domain, a dotted thing ending in digits is a version number.
        var tld = numbers[^1];
        return tld.Length >= 2 && tld.All(char.IsLetter);
    }

    /// The host, lower-cased, or null for an address that has none.
    public static string? Host(Uri? url) =>
        url is { IsAbsoluteUri: true } && !string.IsNullOrEmpty(url.Host) ? url.IdnHost.ToLowerInvariant() : null;

    /// What the tab says before the page has told us its title: the address,
    /// with the parts nobody reads taken off.
    public static string Pretty(Uri url)
    {
        if (ToolsHost.IsTools(url)) return ToolsHost.Pretty(url);
        if (!url.IsAbsoluteUri || string.IsNullOrEmpty(url.Host)) return url.OriginalString;
        var host = url.Host;
        var bare = host.StartsWith("www.") ? host[4..] : host;
        var path = Uri.UnescapeDataString(url.AbsolutePath);
        return path.Length == 0 || path == "/" ? bare : bare + path;
    }

    public static bool IsWeb(Uri? url) => url is { IsAbsoluteUri: true } && (url.Scheme == "http" || url.Scheme == "https");
}

// What to do with words that aren't a place: ask Google.
//
// The field still tells an address from a phrase — typing a domain goes
// straight there, without a round trip through anyone's results page. Only
// what can't be a place gets searched.
public static class Google
{
    public const string Name = "Google";

    /// A place if it can be one, a search if it can't.
    public static Uri? Destination(string typed) => Address.Url(typed) ?? Url(typed);

    public static Uri? Url(string text)
    {
        var words = text.Trim();
        if (words.Length == 0) return null;
        // Everything a query string can't carry raw, including the plus sign,
        // which would otherwise come out the far end as a space.
        return new Uri("https://www.google.com/search?q=" + Uri.EscapeDataString(words));
    }
}
