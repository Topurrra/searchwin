using SearchKit.FishCatcher;

namespace SearchKit.Shields;

/// The part of a host a site would call "its own domain" — what a
/// first-party/third-party check and a filter's `$domain=` option both
/// really mean by "domain", as opposed to the exact hostname. Kept behind an
/// interface because it isn't a solved problem without a public-suffix list:
/// `PslRegistrableDomain` answers from FishCatcher's, `SimpleRegistrableDomain`
/// guesses, and `LiveRegistrableDomain` is the first once it's there and the
/// second until then.
public interface IRegistrableDomain
{
    string? Of(string host);
}

/// The real answer, from the Public Suffix List FishCatcher ships
/// (`FishData.Psl`): `alice.github.io` and `bob.github.io` are two sites,
/// and so are `one.com.cn` and `two.com.cn`. An IP address is its own site.
public sealed class PslRegistrableDomain(Psl psl) : IRegistrableDomain
{
    public string? Of(string host) => Answer(psl, host);

    internal static string? Answer(Psl psl, string host)
    {
        if (string.IsNullOrEmpty(host)) return null;
        host = host.Trim('.').ToLowerInvariant();
        if (host.Length == 0) return null;
        if (IsAddress(host)) return host;
        return psl.RegistrableDomain(host);
    }

    // `[::1]`, `::1`, `192.168.0.1`: the last two labels of an address mean
    // nothing, and 10.0.0.1 is no more the same site as 99.0.0.1 than any
    // two names are.
    private static bool IsAddress(string host)
    {
        if (host.Contains(':')) return true;
        var last = host.LastIndexOf('.');
        return last >= 0 && last + 1 < host.Length && host.AsSpan(last + 1).IndexOfAnyExceptInRange('0', '9') < 0;
    }
}

/// What the browser asks: FishCatcher's Public Suffix List once
/// `FishCatcher.Data` has loaded (on its own worker thread — nothing here
/// loads it), and the guess until then or if the list ever can't answer.
/// Asked on every request, so it reads a field and nothing more.
public sealed class LiveRegistrableDomain(Func<Psl?> source) : IRegistrableDomain
{
    public static readonly LiveRegistrableDomain Instance = new(() => SearchKit.FishCatcher.FishCatcher.Data?.Psl);

    public string? Of(string host)
    {
        try
        {
            if (source() is { } psl) return PslRegistrableDomain.Answer(psl, host);
        }
        catch (Exception)
        {
            // Fall back to the guess, as before the list was there.
        }
        return SimpleRegistrableDomain.Instance.Of(host);
    }
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
