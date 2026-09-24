namespace Search;

/// Why a page isn't there, in words a person reads — Search's own page, never
/// the engine's. Each kind says what happened and offers what's worth doing.
public enum TroubleKind
{
    /// No internet at all. Comes back by itself when the connection does.
    Offline,
    /// The name doesn't lead anywhere.
    NoSuchSite,
    /// The site was reached and turned the connection away.
    Refused,
    /// It never answered.
    TooSlow,
    /// Its certificate is wrong; Search stopped before sending anything.
    Insecure,
    /// Something on this side refused it (a policy, a blocked address).
    Blocked,
    /// FishCatcher's verdict: the site looks like a scam or a phishing page.
    /// Search stopped before loading it, or covered it once the page showed
    /// what it was asking for. Warned about, never blocked: you can go on.
    Scam,
    /// Anything else.
    Broken,
}

public sealed record PageTrouble(TroubleKind Kind, string Host)
{
    /// The address that was stopped, for "Continue anyway". The tab's own
    /// address can still be the page you were on.
    public Uri? Url { get; init; }

    /// Why, in plain sentences (a scam warning's reasons).
    public IReadOnlyList<string> Reasons { get; init; } = [];

    /// The real site the page seems to imitate ("paypal.com"), when known.
    public string? RealSite { get; init; }

    /// Strong signs, rather than several weaker ones.
    public bool Strong { get; init; }

    /// The page had already loaded when the warning went up (it was the
    /// page itself that gave it away), so Search covered it rather than
    /// stopping it.
    public bool Loaded { get; init; }

    /// "example.com" for https://www.example.com/a/b, as the page names it.
    public static string HostOf(Uri? url) =>
        url is { IsAbsoluteUri: true } && !string.IsNullOrEmpty(url.Host)
            ? Address.Pretty(new Uri(url.GetLeftPart(UriPartial.Authority)))
            : "This page";

    public string Glyph => Kind switch
    {
        TroubleKind.Offline => "",   // no network
        TroubleKind.Insecure => "",  // lock
        TroubleKind.Blocked => Icons.Shield,
        TroubleKind.NoSuchSite => Icons.Search,
        TroubleKind.Scam => Icons.Warning,
        _ => "",                    // error
    };

    public string Headline => Kind switch
    {
        TroubleKind.Offline => "You're offline",
        TroubleKind.NoSuchSite => $"{Host} can't be found",
        TroubleKind.Refused => $"{Host} turned the connection away",
        TroubleKind.TooSlow => $"{Host} took too long to answer",
        TroubleKind.Insecure => "This connection isn't private",
        TroubleKind.Blocked => $"{Host} was blocked",
        TroubleKind.Scam => Strong ? $"{Host} looks like a scam" : $"{Host} may be a scam",
        _ => $"{Host} didn't load",
    };

    public string Detail => Kind switch
    {
        TroubleKind.Offline => $"Search will open {Host} as soon as the connection is back.",
        TroubleKind.NoSuchSite => "Check the address for a typo, or search the web for it.",
        TroubleKind.Refused => "The site may be down, or not accepting connections from this network.",
        TroubleKind.TooSlow => "It may be busy or down. Try again in a moment.",
        TroubleKind.Insecure => $"{Host}'s certificate isn't valid, so someone could read or change what you send. Search stopped before loading it.",
        TroubleKind.Blocked => "Something on this computer or network isn't allowing it.",
        TroubleKind.Scam => ScamDetail,
        _ => "Something went wrong between here and the site.",
    };

    /// What a scam warning says under its headline: what such a page is
    /// after, what Search did about it, and where the real site is.
    private string ScamDetail
    {
        get
        {
            var did = Loaded
                ? "Search covered the page to warn you before you type anything into it."
                : "Search stopped before loading anything from it.";
            var real = RealSite is { Length: > 0 } site ? $" The real site is {site}." : "";
            return $"Pages like this are made to look trustworthy, to get a password, money or card details from you. {did}{real}";
        }
    }
}
