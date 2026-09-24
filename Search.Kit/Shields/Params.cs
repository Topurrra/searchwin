namespace SearchKit.Shields;

/// One tracking parameter this strips, and why. `Hosts` null means it's
/// stripped everywhere, because the name is specific enough that no
/// legitimate page reads it for anything else; a non-null list means the
/// same short name is used for something a page actually needs elsewhere
/// (`si`, `ref_src`…), so it's only stripped on the sites known to send it
/// as a tracker.
public sealed record TrackingParam(string Name, bool IsPrefix, string Why, string[]? Hosts = null);

/// Strips tracking parameters from a URL's query string — the same idea as
/// Brave/Firefox's "query parameter stripping", kept conservative on
/// purpose: a parameter only goes on this list once it's confirmed to carry
/// nothing but where-you-came-from, never something a destination page
/// actually reads to work.
public static class Params
{
    public static readonly IReadOnlyList<TrackingParam> Rules =
    [
        // Google/Google Ads campaign and click tagging. Every reader of a
        // URL with these already has whatever they tag; nothing downstream
        // reads them back.
        new("utm_", true, "Google Analytics campaign tags (source/medium/campaign/…)."),
        new("gclid", false, "Google Ads click id."),
        new("gclsrc", false, "Google Ads click source, sent alongside gclid."),
        new("dclid", false, "Google Marketing Platform (DoubleClick) click id."),
        new("gbraid", false, "Google Ads click id, iOS app-to-web."),
        new("wbraid", false, "Google Ads click id, web-to-app."),

        // Other ad networks' own click ids.
        new("fbclid", false, "Meta (Facebook/Instagram) click id."),
        new("msclkid", false, "Microsoft/Bing Ads click id."),
        new("yclid", false, "Yandex Direct click id."),
        new("twclid", false, "X (Twitter) Ads click id."),
        new("ttclid", false, "TikTok Ads click id."),
        new("igshid", false, "Instagram share id, added when a post link is shared out of the app."),

        // Email marketing platforms.
        new("mc_eid", false, "Mailchimp per-recipient id."),
        new("mc_cid", false, "Mailchimp campaign id."),
        new("_hsenc", false, "HubSpot email open/click tracking."),
        new("_hsmi", false, "HubSpot email send id, sent alongside _hsenc."),
        new("mkt_tok", false, "Marketo/Adobe Campaign email tracking token."),
        new("vero_id", false, "Vero email tracking id."),
        new("oly_anon_id", false, "Omeda (publisher email/CRM) anonymous reader id."),
        new("oly_enc_id", false, "Omeda encoded subscriber id."),

        // Analytics suites.
        new("s_cid", false, "Adobe Analytics (SiteCatalyst) campaign id."),
        new("icid", false, "Generic internal-campaign id used by several news/media analytics setups."),

        // Affiliate/redirect trackers.
        new("rb_clickid", false, "Rebrandly/affiliate click id."),

        // Host-scoped: the name is common enough that a blanket strip would
        // break a page that uses it for something real, so it's only
        // stripped on the sites confirmed to send it as a tracker.
        new("ref_src", false, "X (Twitter)'s \"where this tweet link was posted\" tag.", ["twitter.com", "x.com"]),
        new("si", false, "YouTube's share-button id, appended to youtu.be/youtube.com links.", ["youtube.com", "youtu.be", "music.youtube.com"]),
    ];

    /// The cleaned URL, or null when nothing on it needed cleaning — a
    /// caller re-navigating only on a real change is how this avoids ever
    /// looping a load back on itself.
    public static Uri? Clean(Uri url)
    {
        if (url.Query.Length <= 1) return null; // "" or a bare "?"
        var text = url.Query[1..]; // drop the leading '?'
        var parts = text.Split('&');
        var kept = new List<string>(parts.Length);
        var changed = false;

        foreach (var part in parts)
        {
            if (part.Length == 0) { kept.Add(part); continue; }
            var eq = part.IndexOf('=');
            var rawName = eq < 0 ? part : part[..eq];
            string name;
            try { name = Uri.UnescapeDataString(rawName); }
            catch { name = rawName; }

            if (Strips(name, url.Host)) { changed = true; continue; }
            kept.Add(part);
        }

        if (!changed) return null;
        var builder = new UriBuilder(url) { Query = kept.Count == 0 ? "" : string.Join('&', kept) };
        return builder.Uri;
    }

    private static bool Strips(string name, string host)
    {
        foreach (var rule in Rules)
        {
            var match = rule.IsPrefix
                ? name.StartsWith(rule.Name, StringComparison.OrdinalIgnoreCase)
                : name.Equals(rule.Name, StringComparison.OrdinalIgnoreCase);
            if (!match) continue;
            if (rule.Hosts == null) return true;
            foreach (var h in rule.Hosts)
                if (host.Equals(h, StringComparison.OrdinalIgnoreCase) || host.EndsWith("." + h, StringComparison.OrdinalIgnoreCase))
                    return true;
        }
        return false;
    }
}
