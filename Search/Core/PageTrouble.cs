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
    /// Anything else.
    Broken,
}

public sealed record PageTrouble(TroubleKind Kind, string Host)
{
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
        _ => "Something went wrong between here and the site.",
    };
}
