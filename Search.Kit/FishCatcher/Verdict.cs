namespace SearchKit.FishCatcher;

/// FishCatcher's own four levels. It warns and never blocks: High and
/// Critical are where the extension showed its banner.
public enum Level { Low, Elevated, High, Critical }

/// One red flag: an i18n key (the extension's message names), its
/// parameters, and how much it added to the score.
public sealed record Signal(string Key, IReadOnlyList<string> Params, int Weight)
{
    public Signal(string key, int weight, params string[] args) : this(key, (IReadOnlyList<string>)args, weight) { }

    /// The reason as a sentence, in English.
    public string Sentence => Messages.Sentence(Key, Params);
}

/// What FishCatcher says about one address (and, when the probe reported
/// them, the page's facts).
public sealed class Verdict
{
    public required string Url { get; init; }
    public required string Host { get; init; }
    /// The registrable domain (eTLD+1), or the host itself for an IP address.
    public required string Registrable { get; init; }
    /// 0 to 100.
    public required int Score { get; init; }
    public required Level Level { get; init; }
    public required IReadOnlyList<Signal> Signals { get; init; }
    /// The site is on your own trust list.
    public bool Trusted { get; init; }
    /// The brand's real domain, when the address imitates one ("Open paypal.com instead").
    public string? RealSite { get; init; }

    /// Why, as sentences (empty for a clean address). A signal with no
    /// sentence of its own is left out rather than shown as its key.
    public IReadOnlyList<string> Reasons => Signals.Select(s => s.Sentence).Where(s => s.Length > 0).ToArray();

    /// One line for the level ("Multiple phishing indicators…").
    public string Summary => Messages.ForLevel(Level);

    /// Worth a warning in the field or a bar (High or Critical); never a block.
    public bool Warns => Level >= Level.High;

    public static Level LevelFor(int score) => score switch
    {
        >= 75 => Level.Critical,
        >= 45 => Level.High,
        >= 20 => Level.Elevated,
        _ => Level.Low,
    };

    /// The brand the page imitates, as a domain the UI can offer to open
    /// instead. reasonBrand carries the brand's domain; the others carry its name.
    public static string? RealSiteFor(IEnumerable<Signal> signals, IReadOnlyList<Brand> brands)
    {
        foreach (var s in signals)
        {
            if (s.Key == "reasonBrand" && s.Params.Count > 0 && s.Params[0].Length > 0) return s.Params[0];
            if (s.Key is "reasonHomoglyph" or "reasonBrandSubdomain" or "reasonAitmMismatch" && s.Params.Count > 0)
            {
                var name = s.Params[0];
                var brand = brands.FirstOrDefault(b => b.Name == name);
                if (brand is not null && brand.Domains.Length > 0 && brand.Domains[0].Length > 0) return brand.Domains[0];
            }
        }
        return null;
    }
}

/// A brand people get phished for: its name, the domains it really uses,
/// and the words that give its name away in a host.
public sealed record Brand(string Name, string[] Domains, string[] Keywords);
