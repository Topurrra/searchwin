namespace Search;

// Where you have been, so the field can finish the address for you. Kept in one
// small file next to the app's own settings, written a moment after a visit
// rather than on every keystroke.

public enum SuggestionKind
{
    /// A page that is open right now.
    Open,
    /// Somewhere you have actually been.
    Visited,
    /// One of the well-known addresses the field knows from the start.
    Known,
    /// Not a place at all — words, and an engine to ask.
    Search,
    /// Something Search does (`>…`), from the command registry.
    Command,
    /// Found by the engine: a file, an app, an answer (see Browser.Field).
    Found,
}

public sealed record Suggestion(string Key, string Title, Uri Url, SuggestionKind Kind, Guid? Tab = null)
{
    public string Id => Key;

    /// The engine's row, for `Found`; its `Url` is only a stand-in.
    public SearchKit.Field.FieldRow? Row { get; init; }

    public static readonly Uri Nowhere = new("search://field/");
}

public sealed class Visit
{
    public string Url { get; set; } = "";
    public string Key { get; set; } = "";
    public string Title { get; set; } = "";
    public int Count { get; set; }
    public DateTime Last { get; set; }
}

/// Everywhere you have been, newest first, for the window that shows it.
public sealed record Trace(string Key, string Title, Uri Url, DateTime Last, int Count);

public sealed class History
{
    private Dictionary<string, Visit> visits = [];
    private bool saving;

    /// Anything changed: the History menu and panel listen.
    public event Action? Changed;

    public History() => Load();

    // MARK: - writing

    public void Record(Uri url, string title)
    {
        if (!Address.IsWeb(url)) return;
        var key = Address.Pretty(url).ToLowerInvariant();
        if (key.Length == 0) return;

        // Reading a deep page is also, in the way that matters here, another
        // visit to the site. Without this, typing three letters offers the
        // article you happened to open last week rather than the front page —
        // and nobody types a domain meaning to land halfway down it.
        if (key.Contains('/') && !string.IsNullOrEmpty(url.Host))
        {
            var host = url.Host;
            var root = (host.StartsWith("www.") ? host[4..] : host).ToLowerInvariant();
            if (!visits.TryGetValue(root, out var home))
                home = visits[root] = new Visit { Url = "https://" + root + "/", Key = root, Title = "", Count = 0 };
            home.Count += 1;
            home.Last = DateTime.UtcNow;
        }

        if (visits.TryGetValue(key, out var seen))
        {
            seen.Count += 1;
            seen.Last = DateTime.UtcNow;
            seen.Url = url.AbsoluteUri;
            if (title.Length > 0) seen.Title = title;
        }
        else
        {
            visits[key] = new Visit { Url = url.AbsoluteUri, Key = key, Title = title, Count = 1, Last = DateTime.UtcNow };
        }
        Save();
    }

    /// Somewhere another browser has been. Counted as it was counted there,
    /// so a site visited daily for a year outranks one seen once — the day you
    /// switch, the field already knows you.
    public void Take(Uri url, string title, int count, DateTime last)
    {
        if (!Address.IsWeb(url)) return;
        var key = Address.Pretty(url).ToLowerInvariant();
        if (key.Length == 0) return;
        if (visits.TryGetValue(key, out var seen))
        {
            seen.Count += count;
            if (last > seen.Last) seen.Last = last;
            if (seen.Title.Length == 0) seen.Title = title;
        }
        else
        {
            visits[key] = new Visit { Url = url.AbsoluteUri, Key = key, Title = title, Count = count, Last = last };
        }
    }

    /// After a batch of Takes.
    public void Settle() => Save();

    /// A page's title usually lands a beat after the page does.
    public void Retitle(Uri url, string title)
    {
        var key = Address.Pretty(url).ToLowerInvariant();
        if (title.Length == 0 || !visits.TryGetValue(key, out var seen) || seen.Title == title) return;
        seen.Title = title;
        Save();
    }

    public void Forget()
    {
        visits = [];
        Save();
    }

    public void Forget(string key)
    {
        if (visits.Remove(key)) Save();
    }

    public List<Trace> Everything(string typed = "")
    {
        var needle = typed.Trim().ToLowerInvariant();
        var list = new List<Trace>();
        // Every visit to a page also credits its domain, so the address field
        // can offer the front door. Those credits have no title of their own,
        // and in a list of where you have been they are a second copy of every
        // line.
        foreach (var v in visits.Values.OrderByDescending(v => v.Last))
        {
            if (v.Title.Length == 0 && !v.Key.Contains('/')) continue;
            if (needle.Length > 0 && !v.Key.Contains(needle) && !v.Title.ToLowerInvariant().Contains(needle)) continue;
            if (Uri.TryCreate(v.Url, UriKind.Absolute, out var url))
                list.Add(new Trace(v.Key, v.Title, url, v.Last, v.Count));
        }
        return list;
    }

    // MARK: - reading

    /// Best matches first. A place you have been always beats a place the app
    /// merely knows the name of, and among places you have been, one you go to
    /// often and recently beats one you saw once in March.
    public List<Suggestion> Suggestions(string typed, int limit = 5)
    {
        var needle = Strip(typed);
        // An empty field proposes nothing. A list of guesses in front of
        // someone who has not yet said what they want is noise.
        if (needle.Length == 0) return [];

        var now = DateTime.UtcNow;
        var scored = new List<(Suggestion, double)>();

        foreach (var visit in visits.Values)
        {
            if (Rank(visit.Key, needle) is not { } rank) continue;
            if (!Uri.TryCreate(visit.Url, UriKind.Absolute, out var url)) continue;
            // The front door before the room inside it: a bare domain is what
            // a bare domain typed into a field means.
            scored.Add((new Suggestion(visit.Key, visit.Title, url, SuggestionKind.Visited),
                rank + 4 + Frecency(visit, now) + (visit.Key.Contains('/') ? 0 : 1.5)));
        }

        // Only where memory has nothing to offer. A list of famous websites is
        // a poor substitute for knowing where someone actually goes.
        foreach (var (key, title) in Known)
        {
            if (visits.ContainsKey(key)) continue;
            if (Rank(key, needle) is not { } rank) continue;
            scored.Add((new Suggestion(key, title, new Uri("https://" + key), SuggestionKind.Known), rank));
        }

        return scored
            .OrderByDescending(s => s.Item2)
            .ThenBy(s => s.Item1.Key.Length)
            .Take(limit)
            .Select(s => s.Item1)
            .ToList();
    }

    /// What the field should draw after the caret: the rest of the best match,
    /// or nothing if it doesn't carry on from what was typed.
    public string? Completion(string typed, IEnumerable<Suggestion> options)
    {
        var lower = typed.ToLowerInvariant();
        if (lower.Length < 2) return null;
        var hit = options.FirstOrDefault(o => o.Key.StartsWith(lower, StringComparison.Ordinal));
        if (hit == null) return null;
        var rest = hit.Key[lower.Length..];
        return rest.Length == 0 ? null : rest;
    }

    /// Where the match falls decides most of the ordering: the start of the
    /// host is what people mean, the middle of a path almost never is.
    private static double? Rank(string key, string needle)
    {
        if (key.StartsWith(needle, StringComparison.Ordinal)) return 6;
        var host = key.Split('/')[0];
        // "hub" finding github.com, once the "git" has been skipped.
        var dot = host.IndexOf('.');
        if (dot >= 0 && host[(dot + 1)..].StartsWith(needle, StringComparison.Ordinal)) return 3;
        // Only from two letters up. A single letter matching anywhere inside a
        // name turns "x" into example.com and netflix.com.
        if (needle.Length >= 2 && host.Contains(needle)) return 2;
        // Deliberately no match on the path.
        return null;
    }

    /// Often, and lately. A month-old visit counts for about a third of a
    /// fresh one, which is roughly how long a habit takes to stop being one.
    private static double Frecency(Visit visit, DateTime now)
    {
        var days = Math.Max(0, (now - visit.Last).TotalDays);
        return visit.Count * Math.Exp(-days / 30);
    }

    private static string Strip(string typed)
    {
        var text = typed.Trim().ToLowerInvariant();
        foreach (var scheme in new[] { "https://", "http://" })
            if (text.StartsWith(scheme)) text = text[scheme.Length..];
        if (text.StartsWith("www.")) text = text[4..];
        return text;
    }

    // MARK: - the file

    private const string FileName = "history.json";

    private void Load()
    {
        var list = Store.Read<List<Visit>>(FileName);
        if (list == null) return;
        visits = [];
        foreach (var v in list) visits[v.Key] = v;
    }

    /// Coalesced: a busy minute of browsing writes the file once, not thirty
    /// times, and never on the UI thread.
    private void Save()
    {
        Changed?.Invoke();
        if (saving) return;
        saving = true;
        UI.After(1.5, () =>
        {
            saving = false;
            var now = DateTime.UtcNow;
            // A cap, so the file can't grow without end. What goes is what has
            // been visited least and longest ago.
            var list = visits.Values.OrderByDescending(v => Frecency(v, now)).Take(2000)
                .Select(v => new Visit { Url = v.Url, Key = v.Key, Title = v.Title, Count = v.Count, Last = v.Last })
                .ToList();
            Store.Write(FileName, list);
        });
    }

    /// Somewhere to start on the first day, before there is any history to go
    /// on. Ranked below anything actually visited.
    private static readonly (string, string)[] Known =
    [
        ("google.com", "Google"), ("mail.google.com", "Gmail"),
        ("drive.google.com", "Google Drive"), ("calendar.google.com", "Google Calendar"),
        ("maps.google.com", "Google Maps"), ("youtube.com", "YouTube"),
        ("github.com", "GitHub"), ("figma.com", "Figma"), ("vercel.com", "Vercel"),
        ("notion.so", "Notion"), ("linear.app", "Linear"), ("slack.com", "Slack"),
        ("discord.com", "Discord"), ("x.com", "X"), ("linkedin.com", "LinkedIn"),
        ("instagram.com", "Instagram"), ("reddit.com", "Reddit"),
        ("news.ycombinator.com", "Hacker News"), ("stackoverflow.com", "Stack Overflow"),
        ("claude.ai", "Claude"), ("chatgpt.com", "ChatGPT"),
        ("dribbble.com", "Dribbble"), ("behance.net", "Behance"),
        ("awwwards.com", "Awwwards"), ("mobbin.com", "Mobbin"),
        ("siteinspire.com", "SiteInspire"), ("are.na", "Are.na"),
        ("pinterest.com", "Pinterest"), ("framer.com", "Framer"),
        ("webflow.com", "Webflow"), ("learn.microsoft.com", "Microsoft Learn"),
        ("npmjs.com", "npm"), ("supabase.com", "Supabase"),
        ("stripe.com", "Stripe"), ("shopify.com", "Shopify"),
        ("cloudflare.com", "Cloudflare"), ("netlify.com", "Netlify"),
        ("apple.com", "Apple"), ("spotify.com", "Spotify"), ("netflix.com", "Netflix"),
        ("wikipedia.org", "Wikipedia"), ("deepl.com", "DeepL"), ("loom.com", "Loom"),
        ("amazon.fr", "Amazon"), ("leboncoin.fr", "leboncoin"), ("lemonde.fr", "Le Monde"),
    ];
}
