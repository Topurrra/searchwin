namespace SearchKit.Shields;

/// One compiled filter set — every network and cosmetic rule from however
/// many source lists were fed to `Compile`, indexed so a request costs a
/// handful of dictionary lookups rather than a scan of every rule.
/// `ShouldBlock` and `CssFor` are the only two calls the browser makes per
/// request/navigation; everything else here is either building that index
/// (`Compile`) or reloading it without re-parsing text (`Save`/`Load`).
public sealed class FilterList
{
    private readonly NetworkRule[] rules;
    private readonly Dictionary<string, int[]> domainAnchored;
    private readonly Dictionary<string, int[]> tokenIndex;
    private readonly int[] genericPlain;

    private readonly List<CosmeticRule> genericCosmetic;
    private readonly List<CosmeticRule> genericCosmeticExceptions;
    private readonly Dictionary<string, List<CosmeticRule>> cosmeticByDomain;
    private readonly List<CosmeticRule> hostScopedCosmetic;

    private readonly IRegistrableDomain sites;

    public FilterStats Stats { get; }

    private FilterList(
        NetworkRule[] rules, Dictionary<string, int[]> domainAnchored, Dictionary<string, int[]> tokenIndex, int[] genericPlain,
        List<CosmeticRule> genericCosmetic, List<CosmeticRule> genericCosmeticExceptions,
        Dictionary<string, List<CosmeticRule>> cosmeticByDomain, List<CosmeticRule> hostScopedCosmetic,
        FilterStats stats, IRegistrableDomain sites)
    {
        this.rules = rules;
        this.domainAnchored = domainAnchored;
        this.tokenIndex = tokenIndex;
        this.genericPlain = genericPlain;
        this.genericCosmetic = genericCosmetic;
        this.genericCosmeticExceptions = genericCosmeticExceptions;
        this.cosmeticByDomain = cosmeticByDomain;
        this.hostScopedCosmetic = hostScopedCosmetic;
        Stats = stats;
        this.sites = sites;
    }

    public static FilterList Compile(IEnumerable<string> lines, IRegistrableDomain? sites = null) =>
        Compile([lines], sites);

    /// Several source lists (EasyList + EasyPrivacy, say) compiled into one
    /// engine in a single pass, so a request checks one index instead of one
    /// per list.
    public static FilterList Compile(IEnumerable<IEnumerable<string>> lists, IRegistrableDomain? sites = null)
    {
        var stats = new FilterStats();
        var network = new List<NetworkRule>();
        var cosmetic = new List<CosmeticRule>();
        foreach (var lines in lists)
            foreach (var line in lines)
                FilterParser.ParseLine(line, network, cosmetic, stats);
        return Build(network, cosmetic, stats, sites ?? SimpleRegistrableDomain.Instance);
    }

    private static FilterList Build(List<NetworkRule> network, List<CosmeticRule> cosmetic, FilterStats stats, IRegistrableDomain sites)
    {
        var (rules, domainAnchored, tokenIndex, genericPlain) = IndexNetwork(network);
        var (genericCosmetic, genericCosmeticExceptions, cosmeticByDomain, hostScopedCosmetic) = IndexCosmetic(cosmetic);
        return new FilterList(rules, domainAnchored, tokenIndex, genericPlain,
            genericCosmetic, genericCosmeticExceptions, cosmeticByDomain, hostScopedCosmetic, stats, sites);
    }

    private static (NetworkRule[] Rules, Dictionary<string, int[]> DomainAnchored, Dictionary<string, int[]> TokenIndex, int[] GenericPlain)
        IndexNetwork(List<NetworkRule> network)
    {
        var rules = network.ToArray();
        var domainAnchored = new Dictionary<string, List<int>>(StringComparer.Ordinal);
        var tokenIndex = new Dictionary<string, List<int>>(StringComparer.Ordinal);
        var genericPlain = new List<int>();

        // Two passes for the non-domain-anchored rules: first count how many
        // rules each candidate token would gather, then give each rule its
        // rarest one. A rule's *longest* literal word is often a plain
        // English one ("banner", "advert"…) that plenty of unrelated rules
        // also contain — indexing by that would defeat the index by putting
        // them all in one bucket a request has to scan through anyway.
        var candidates = new string[rules.Length][];
        var frequency = new Dictionary<string, int>(StringComparer.Ordinal);
        for (var i = 0; i < rules.Length; i++)
        {
            if (rules[i].AnchorDomain != null) { candidates[i] = []; continue; }
            candidates[i] = [.. rules[i].CandidateTokens().Distinct(StringComparer.Ordinal)];
            foreach (var t in candidates[i]) frequency[t] = frequency.GetValueOrDefault(t) + 1;
        }

        for (var i = 0; i < rules.Length; i++)
        {
            var rule = rules[i];
            if (rule.AnchorDomain != null) { Bucket(domainAnchored, rule.AnchorDomain).Add(i); continue; }

            string? best = null;
            var bestFrequency = int.MaxValue;
            foreach (var t in candidates[i])
            {
                var f = frequency[t];
                if (f < bestFrequency || (f == bestFrequency && (best == null || t.Length > best.Length)))
                {
                    best = t;
                    bestFrequency = f;
                }
            }
            if (best != null) Bucket(tokenIndex, best).Add(i);
            else genericPlain.Add(i);
        }

        return (rules, Freeze(domainAnchored), Freeze(tokenIndex), [.. genericPlain]);

        static List<int> Bucket(Dictionary<string, List<int>> map, string key)
        {
            if (!map.TryGetValue(key, out var list)) map[key] = list = [];
            return list;
        }

        static Dictionary<string, int[]> Freeze(Dictionary<string, List<int>> map)
        {
            var frozen = new Dictionary<string, int[]>(map.Count, StringComparer.Ordinal);
            foreach (var (key, value) in map) frozen[key] = [.. value];
            return frozen;
        }
    }

    private static (List<CosmeticRule> Generic, List<CosmeticRule> GenericExceptions,
        Dictionary<string, List<CosmeticRule>> ByDomain, List<CosmeticRule> HostScoped) IndexCosmetic(List<CosmeticRule> cosmetic)
    {
        var generic = new List<CosmeticRule>();
        var genericExceptions = new List<CosmeticRule>();
        var byDomain = new Dictionary<string, List<CosmeticRule>>(StringComparer.Ordinal);
        var hostScoped = new List<CosmeticRule>();

        foreach (var rule in cosmetic)
        {
            if (rule.Domains.Length == 0)
            {
                (rule.IsException ? genericExceptions : generic).Add(rule);
                continue;
            }
            hostScoped.Add(rule);
            foreach (var domain in rule.Domains)
            {
                if (!byDomain.TryGetValue(domain, out var list)) byDomain[domain] = list = [];
                list.Add(rule);
            }
        }

        return (generic, genericExceptions, byDomain, hostScoped);
    }

    /// Whether `request` should be blocked, given the page it's for (null
    /// for a top-level navigation) and what kind of resource it is.
    /// `$important` blocks override even a matching exception; short of
    /// that, any matching exception wins over any matching block.
    public bool ShouldBlock(Uri request, string? pageHost, ResourceKind kind)
    {
        if (request.Scheme is not ("http" or "https")) return false;

        var host = request.Host.ToLowerInvariant();
        var thirdParty = pageHost != null && !string.Equals(sites.Of(host), sites.Of(pageHost), StringComparison.Ordinal);
        var lowerUrl = request.AbsoluteUri.ToLowerInvariant();
        var afterHost = request.GetLeftPart(UriPartial.Authority).Length;

        var seen = new HashSet<int>();
        var blocked = false;
        var important = false;
        var excepted = false;

        void Consider(int idx)
        {
            if (!seen.Add(idx)) return;
            var rule = rules[idx];
            if (!rule.MatchesContext(kind, thirdParty, pageHost)) return;
            if (!rule.MatchesUrl(lowerUrl, afterHost, host)) return;
            if (rule.IsException) excepted = true;
            else { blocked = true; if (rule.Important) important = true; }
        }

        for (var at = host; ; )
        {
            if (domainAnchored.TryGetValue(at, out var bucket))
                foreach (var idx in bucket) Consider(idx);
            if (important) return true; // nothing can un-block an $important match
            var dot = at.IndexOf('.');
            if (dot < 0) break;
            at = at[(dot + 1)..];
        }

        foreach (var token in AbpPattern.AlnumRuns(lowerUrl))
        {
            if (token.Length < 3) continue;
            if (tokenIndex.TryGetValue(token, out var bucket))
                foreach (var idx in bucket) Consider(idx);
        }
        foreach (var idx in genericPlain) Consider(idx);

        return important || (blocked && !excepted);
    }

    /// The stylesheet hiding every cosmetic rule that applies to `host` and
    /// isn't cancelled by an exception — one `display: none !important`
    /// rule covering every matching selector, or "" when nothing applies.
    public string CssFor(string host)
    {
        host = host.ToLowerInvariant();
        var selectors = new HashSet<string>(StringComparer.Ordinal);
        var excepted = new HashSet<string>(StringComparer.Ordinal);

        bool HostOrSub(string domain) => host == domain || host.EndsWith("." + domain, StringComparison.Ordinal);

        foreach (var rule in genericCosmetic)
            if (rule.ExcludedDomains.Length == 0 || !Array.Exists(rule.ExcludedDomains, HostOrSub))
                selectors.Add(rule.Selector);
        foreach (var rule in genericCosmeticExceptions)
            if (rule.ExcludedDomains.Length == 0 || !Array.Exists(rule.ExcludedDomains, HostOrSub))
                excepted.Add(rule.Selector);

        for (var at = host; ; )
        {
            if (cosmeticByDomain.TryGetValue(at, out var list))
                foreach (var rule in list)
                {
                    if (rule.ExcludedDomains.Length > 0 && Array.Exists(rule.ExcludedDomains, HostOrSub)) continue;
                    (rule.IsException ? excepted : selectors).Add(rule.Selector);
                }
            var dot = at.IndexOf('.');
            if (dot < 0) break;
            at = at[(dot + 1)..];
        }

        selectors.ExceptWith(excepted);
        if (selectors.Count == 0) return "";
        return string.Join(", ", selectors.OrderBy(s => s, StringComparer.Ordinal)) + " { display: none !important; }";
    }

    private const int FormatVersion = 1;

    /// The compact binary form: every compiled rule, with no re-parsing of
    /// list text needed to use it again — `Load` rebuilds the same indices
    /// `Compile` does, straight from already-structured data.
    public void Save(Stream stream)
    {
        using var w = new BinaryWriter(stream, System.Text.Encoding.UTF8, leaveOpen: true);
        w.Write(FormatVersion);

        w.Write(rules.Length);
        foreach (var rule in rules) rule.WriteTo(w);

        w.Write(genericCosmetic.Count);
        foreach (var r in genericCosmetic) r.WriteTo(w);
        w.Write(genericCosmeticExceptions.Count);
        foreach (var r in genericCosmeticExceptions) r.WriteTo(w);
        w.Write(hostScopedCosmetic.Count);
        foreach (var r in hostScopedCosmetic) r.WriteTo(w);

        w.Write(Stats.NetworkRules);
        w.Write(Stats.NetworkExceptions);
        w.Write(Stats.CosmeticRules);
        w.Write(Stats.CosmeticExceptions);
        w.Write(Stats.SkippedUnsupported);
        w.Write(Stats.Comments);
        w.Write(Stats.Blank);
    }

    public static FilterList Load(Stream stream, IRegistrableDomain? sites = null)
    {
        using var r = new BinaryReader(stream, System.Text.Encoding.UTF8, leaveOpen: true);
        var version = r.ReadInt32();
        if (version != FormatVersion) throw new InvalidDataException($"Unknown Shields filter blob version {version}.");

        var networkCount = r.ReadInt32();
        var network = new List<NetworkRule>(networkCount);
        for (var i = 0; i < networkCount; i++) network.Add(NetworkRule.ReadFrom(r));

        var cosmetic = new List<CosmeticRule>();
        var genericCount = r.ReadInt32();
        for (var i = 0; i < genericCount; i++) cosmetic.Add(CosmeticRule.ReadFrom(r));
        var genericExceptionCount = r.ReadInt32();
        for (var i = 0; i < genericExceptionCount; i++) cosmetic.Add(CosmeticRule.ReadFrom(r));
        var hostScopedCount = r.ReadInt32();
        for (var i = 0; i < hostScopedCount; i++) cosmetic.Add(CosmeticRule.ReadFrom(r));

        var stats = new FilterStats
        {
            NetworkRules = r.ReadInt32(),
            NetworkExceptions = r.ReadInt32(),
            CosmeticRules = r.ReadInt32(),
            CosmeticExceptions = r.ReadInt32(),
            SkippedUnsupported = r.ReadInt32(),
            Comments = r.ReadInt32(),
            Blank = r.ReadInt32(),
        };

        return Build(network, cosmetic, stats, sites ?? SimpleRegistrableDomain.Instance);
    }
}
