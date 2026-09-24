namespace SearchKit.Shields;

/// The part of a host a site would call "its own domain" — what a
/// first-party/third-party check and a filter's `$domain=` option both
/// really mean by "domain", as opposed to the exact hostname. Kept behind an
/// interface because it isn't a solved problem without a public-suffix list:
/// FishCatcher's own PSL (`docs/brain/Ideas/Built-in Tools.md` §2.6) is the
/// candidate for a real one later.
public interface IRegistrableDomain
{
    string? Of(string host);
}

/// A guess that costs nothing to ship: the last two labels, or three under a
/// short table of the common country-code second levels (`bbc.co.uk`,
/// `example.com.au`). Right for the sites a filter list actually names;
/// wrong only at the long tail a real PSL exists to cover.
public sealed class SimpleRegistrableDomain : IRegistrableDomain
{
    public static readonly SimpleRegistrableDomain Instance = new();

    private static readonly HashSet<string> TwoLabelSuffixes = new(StringComparer.OrdinalIgnoreCase)
    {
        "co.uk", "org.uk", "me.uk", "ac.uk", "gov.uk", "co.jp", "co.kr", "co.nz",
        "co.za", "co.in", "com.au", "net.au", "org.au", "com.br", "com.mx",
        "com.tr", "com.tw", "com.hk", "com.sg", "co.il", "co.id", "com.ar",
    };

    public string? Of(string host)
    {
        if (string.IsNullOrEmpty(host)) return null;
        host = host.Trim('.');
        if (host.Length == 0) return null;
        var labels = host.Split('.');
        if (labels.Length <= 2) return host.ToLowerInvariant();
        var lastTwo = labels[^2] + "." + labels[^1];
        var take = TwoLabelSuffixes.Contains(lastTwo) ? 3 : 2;
        take = Math.Min(take, labels.Length);
        return string.Join('.', labels[^take..]).ToLowerInvariant();
    }
}
