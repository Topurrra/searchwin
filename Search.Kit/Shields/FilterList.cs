namespace SearchKit.Shields;

/// One compiled filter set — every network and cosmetic rule from however
/// many source lists were fed to `Compile`, indexed so a request costs a
/// handful of dictionary lookups rather than a scan of every rule.
/// `ShouldBlock` is the one call the browser makes per request, and
/// `GenericCss`/`SiteCss` the ones it makes per list and per site;
/// everything else here is either building that index (`Compile`) or
/// reloading it without re-parsing text (`Save`/`Load`).
public sealed class FilterList
{
    private readonly NetworkRule[] rules;
    private readonly Dictionary<string, int[]> domainAnchored;
    private readonly Dictionary<string, int[]>.AlternateLookup<ReadOnlySpan<char>> domainAnchoredBySpan;
    private readonly Dictionary<string, int[]> tokenIndex;
    private readonly Dictionary<string, int[]>.AlternateLookup<ReadOnlySpan<char>> tokenIndexBySpan;
    private readonly int[] genericPlain;

    // Plain `||domain^` blocks as bare names (see NetworkRule.IsPlainDomain).
    private readonly HashSet<string> plainDomains;
    private readonly HashSet<string>.AlternateLookup<ReadOnlySpan<char>> plainDomainsBySpan;
    private readonly HashSet<string> plainThirdPartyDomains;
    private readonly HashSet<string>.AlternateLookup<ReadOnlySpan<char>> plainThirdPartyBySpan;

    private readonly List<CosmeticRule> genericCosmetic;
    private readonly List<CosmeticRule> genericCosmeticExceptions;
    private readonly Dictionary<string, List<CosmeticRule>> cosmeticByDomain;
    private readonly List<CosmeticRule> hostScopedCosmetic;

    // The generic hide rules split in two: those that hold on every site
    // (one stylesheet, the same for every page), and the few that some
    // exception or `~site` turns off somewhere, decided per site instead.
    private readonly string[] unconditionalSelectors;
    private readonly HashSet<string> unconditional;
    private readonly List<CosmeticRule> conditionalGeneric;
    private readonly HashSet<string> genericHide;

    // Every name any cosmetic rule mentions — scoped to it, excluding it,
    // or excepting it — and what a site none of them mention gets. Most
    // sites are such a site, and for them `SiteCss` is a lookup.
    private readonly HashSet<string> mentioned;
    private string? unmentionedCss;

    private readonly IRegistrableDomain sites;

    public FilterStats Stats { get; }

    /// Every rule that holds on every site, as one stylesheet: the same text
    /// for every page, so the browser can hand it over once per tab rather
    /// than once per navigation. Not for sites in `GenericHideSites`.
    public string GenericCss { get; }

    /// Sites that asked for no generic cosmetic rules (`$generichide`).
    public IReadOnlyCollection<string> GenericHideSites => genericHide;

    /// How the index came out, for the benchmarks: rules filed under a
    /// word, and rules with no whole word that every request tries.
    internal (int Indexed, int Words, int Everywhere) IndexShape =>
        (tokenIndex.Values.Sum(b => b.Length), tokenIndex.Count, genericPlain.Length);

    /// Every word the index is filed under, for the worst-case benchmark.
    internal IEnumerable<string> IndexWords => tokenIndex.Keys;

    private FilterList(
        NetworkRule[] rules, Dictionary<string, int[]> domainAnchored, Dictionary<string, int[]> tokenIndex, int[] genericPlain,
        HashSet<string> plainDomains, HashSet<string> plainThirdPartyDomains,
        List<CosmeticRule> genericCosmetic, List<CosmeticRule> genericCosmeticExceptions,
        Dictionary<string, List<CosmeticRule>> cosmeticByDomain, List<CosmeticRule> hostScopedCosmetic,
        HashSet<string> genericHide, FilterStats stats, IRegistrableDomain sites)
    {
        this.rules = rules;
        this.domainAnchored = domainAnchored;
        domainAnchoredBySpan = domainAnchored.GetAlternateLookup<ReadOnlySpan<char>>();
        this.tokenIndex = tokenIndex;
        tokenIndexBySpan = tokenIndex.GetAlternateLookup<ReadOnlySpan<char>>();
        this.genericPlain = genericPlain;
        this.plainDomains = plainDomains;
        plainDomainsBySpan = plainDomains.GetAlternateLookup<ReadOnlySpan<char>>();
        this.plainThirdPartyDomains = plainThirdPartyDomains;
        plainThirdPartyBySpan = plainThirdPartyDomains.GetAlternateLookup<ReadOnlySpan<char>>();
        this.genericCosmetic = genericCosmetic;
        this.genericCosmeticExceptions = genericCosmeticExceptions;
        this.cosmeticByDomain = cosmeticByDomain;
        this.hostScopedCosmetic = hostScopedCosmetic;
        this.genericHide = genericHide;
        Stats = stats;
        this.sites = sites;

        // A selector any exception names anywhere can't go in the shared
        // sheet: there'd be no taking it back on the one site that asked.
        var excepted = new HashSet<string>(StringComparer.Ordinal);
        foreach (var rule in genericCosmeticExceptions) excepted.Add(rule.Selector);
        foreach (var rule in hostScopedCosmetic) if (rule.IsException) excepted.Add(rule.Selector);
        unconditional = new HashSet<string>(StringComparer.Ordinal);
        conditionalGeneric = [];
        foreach (var rule in genericCosmetic)
        {
            if (rule.ExcludedDomains.Length == 0 && !excepted.Contains(rule.Selector)) unconditional.Add(rule.Selector);
            else conditionalGeneric.Add(rule);
        }
        unconditionalSelectors = [.. unconditional.Where(Css.IsSafe).OrderBy(s => s, StringComparer.Ordinal)];
        GenericCss = Css.Chunked(unconditionalSelectors);

        mentioned = new HashSet<string>(genericHide, StringComparer.Ordinal);
        mentioned.UnionWith(cosmeticByDomain.Keys);
        foreach (var rule in genericCosmetic) mentioned.UnionWith(rule.ExcludedDomains);
        foreach (var rule in genericCosmeticExceptions) mentioned.UnionWith(rule.ExcludedDomains);
        foreach (var rule in hostScopedCosmetic) mentioned.UnionWith(rule.ExcludedDomains);
    }

    /// `sites` decides which requests are third-party; left out, it's
    /// `LiveRegistrableDomain` — the Public Suffix List once FishCatcher has
    /// loaded it, a guess until then. The same goes for `Load`.
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
        var genericHide = new List<string>();
        foreach (var lines in lists)
            foreach (var line in lines)
                FilterParser.ParseLine(line, network, cosmetic, genericHide, stats);

        var plain = new List<string>();
        var plainThirdParty = new List<string>();
        var rest = new List<NetworkRule>(network.Count / 4);
        foreach (var rule in network)
        {
            if (!rule.IsPlainDomain) rest.Add(rule);
            else if (rule.ThirdParty == true) plainThirdParty.Add(rule.AnchorDomain!);
            else plain.Add(rule.AnchorDomain!);
        }
        return Build(rest, plain, plainThirdParty, cosmetic, genericHide, stats, sites ?? LiveRegistrableDomain.Instance);
    }

    private static FilterList Build(List<NetworkRule> network, List<string> plain, List<string> plainThirdParty,
        List<CosmeticRule> cosmetic, List<string> genericHide, FilterStats stats, IRegistrableDomain sites)
    {
        var (rules, domainAnchored, tokenIndex, genericPlain) = IndexNetwork(network);
        var (genericCosmetic, genericCosmeticExceptions, cosmeticByDomain, hostScopedCosmetic) = IndexCosmetic(cosmetic);
        return new FilterList(rules, domainAnchored, tokenIndex, genericPlain,
            new HashSet<string>(plain, StringComparer.Ordinal), new HashSet<string>(plainThirdParty, StringComparer.Ordinal),
            genericCosmetic, genericCosmeticExceptions, cosmeticByDomain, hostScopedCosmetic,
            new HashSet<string>(genericHide, StringComparer.Ordinal), stats, sites);
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
    ///
    /// Called for every request a page makes, on the thread that draws the
    /// window, so it allocates as little as it can: lookups go by span
    /// rather than by substring, and a word the address repeats has its
    /// rules tested once, not once a time — a long address made of one word
    /// would otherwise test the same rules thousands of times.
    ///
    /// `ads.example.` is `ads.example` (a name may end in the root's dot),
    /// for the request and for the page alike.
    public bool ShouldBlock(Uri request, string? pageHost, ResourceKind kind)
    {
        if (request.Scheme is not ("http" or "https")) return false;

        var host = Bare(request.Host);
        if (pageHost != null) pageHost = Bare(pageHost);
        var thirdParty = pageHost != null && !string.Equals(sites.Of(host), sites.Of(pageHost), StringComparison.Ordinal);
        var lowerUrl = request.AbsoluteUri.ToLowerInvariant();
        var afterHost = request.GetLeftPart(UriPartial.Authority).Length;
        var limit = Math.Min(lowerUrl.Length, MatchedLength);

        var blocked = false;
        var important = false;
        var excepted = false;

        void Consider(int idx)
        {
            var rule = rules[idx];
            if (!rule.MatchesContext(kind, thirdParty, pageHost)) return;
            if (!rule.MatchesUrl(lowerUrl, afterHost, host, limit)) return;
            if (rule.IsException) excepted = true;
            else { blocked = true; if (rule.Important) important = true; }
        }

        for (var at = host.AsSpan(); ; )
        {
            if (!blocked && (plainDomainsBySpan.Contains(at) || (thirdParty && plainThirdPartyBySpan.Contains(at))))
                blocked = true;
            if (domainAnchoredBySpan.TryGetValue(at, out var bucket))
                foreach (var idx in bucket) Consider(idx);
            if (important) return true; // nothing can un-block an $important match
            var dot = at.IndexOf('.');
            if (dot < 0) break;
            at = at[(dot + 1)..];
        }

        var tried = Tried();
        var words = 0;
        // The words of the part the patterns read (the last may run on past
        // it), and of the very end, where a rule held there by `|` looks.
        var tail = Math.Max(limit, lowerUrl.Length - EndRead);
        // From the start of a word, never half of one.
        while (tail > limit && tail < lowerUrl.Length && char.IsAsciiLetterOrDigit(lowerUrl[tail - 1])) tail++;
        Words(lowerUrl, 0, limit);
        Words(lowerUrl, tail, lowerUrl.Length);
        foreach (var idx in genericPlain) Consider(idx);

        return important || (blocked && !excepted);

        void Words(ReadOnlySpan<char> url, int from, int to)
        {
            for (var i = from; i < to && words < MaxWords; )
            {
                if (!char.IsAsciiLetterOrDigit(url[i])) { i++; continue; }
                var start = i;
                while (i < url.Length && char.IsAsciiLetterOrDigit(url[i])) i++;
                if (i - start < AbpPattern.MinToken) continue;
                words++;
                if (tokenIndexBySpan.TryGetValue(url[start..i], out var bucket) && tried.Add(bucket))
                    foreach (var idx in bucket) Consider(idx);
            }
        }
    }

    /// How much of the end of a long address is looked at for words, for
    /// the rules held at the end by `|`.
    private const int EndRead = 256;

    /// How much of an address the patterns read, in characters. Each rule a
    /// request tries reads it once; an address made of every word the index
    /// knows would otherwise open thousands of buckets and read 64 KB for
    /// each of their rules — 20 ms a request with EasyList + EasyPrivacy, on
    /// the thread that draws every window, and a page can ask for hundreds;
    /// at 16 KB it was still 4 ms, at 4 KB it is a third of one. Nearly
    /// every real address fits inside. Past it a match must start within
    /// it — the host and the path are there — and a rule held at the end by
    /// `|` is still checked at the address's real end (the words of its last
    /// `EndRead` characters are looked up too).
    public const int MatchedLength = 4 * 1024;

    /// At most this many words of an address are looked up in the index
    /// (µBlock Origin stops at about as many).
    public const int MaxWords = 2048;

    // The buckets one request has tried, kept per thread and emptied at the
    // start of each request rather than made anew. A set that grew large
    // (an address with thousands of different words) is let go, so the next
    // request doesn't pay to empty it.
    [ThreadStatic] private static HashSet<int[]>? tried;

    private static HashSet<int[]> Tried()
    {
        var set = tried;
        if (set == null || set.Count > 64) return tried = new HashSet<int[]>(ReferenceEqualityComparer.Instance);
        set.Clear();
        return set;
    }

    private static string Bare(string host)
    {
        host = host.ToLowerInvariant();
        return host.EndsWith('.') ? host.TrimEnd('.') : host;
    }

    /// Whether the generic rules apply on `host` at all.
    public bool HidesGenerics(string host)
    {
        for (var at = Bare(host); ; )
        {
            if (genericHide.Contains(at)) return false;
            var dot = at.IndexOf('.');
            if (dot < 0) return true;
            at = at[(dot + 1)..];
        }
    }

    /// What `host` gets on top of `GenericCss`: its own rules, and the
    /// generic ones that depend on where they are — minus every exception
    /// that applies there, and minus what `GenericCss` already hides. Each
    /// selector is a rule of its own, so one the engine can't parse takes
    /// nothing else down with it. "" when there is nothing to add.
    public string SiteCss(string host)
    {
        host = Bare(host);
        if (!Mentions(host)) return unmentionedCss ??= Compose("\u0001");
        return Compose(host);
    }

    private bool Mentions(string host)
    {
        for (var at = host; ; )
        {
            if (mentioned.Contains(at)) return true;
            var dot = at.IndexOf('.');
            if (dot < 0) return false;
            at = at[(dot + 1)..];
        }
    }

    private string Compose(string host)
    {
        var generics = HidesGenerics(host);
        var selectors = new HashSet<string>(StringComparer.Ordinal);
        var excepted = new HashSet<string>(StringComparer.Ordinal);

        if (generics)
            foreach (var rule in conditionalGeneric)
                if (!Excludes(rule, host)) selectors.Add(rule.Selector);
        foreach (var rule in genericCosmeticExceptions)
            if (!Excludes(rule, host)) excepted.Add(rule.Selector);
        Scoped(host, selectors, excepted);

        selectors.ExceptWith(excepted);
        if (generics) selectors.ExceptWith(unconditional);
        if (selectors.Count == 0) return "";
        return string.Join("\n", selectors.Where(Css.IsSafe).OrderBy(s => s, StringComparer.Ordinal).Select(s => s + " { display: none !important; }"));
    }

    /// The stylesheet hiding every cosmetic rule that applies to `host` and
    /// isn't cancelled by an exception — one `display: none !important`
    /// rule covering every matching selector, or "" when nothing applies.
    /// `GenericCss` + `SiteCss` is the same set, split for the browser, and
    /// like them it leaves out every selector `Css.IsSafe` refuses: here all
    /// of them share one rule, so a single one that closed it could write
    /// any CSS it liked into the page.
    public string CssFor(string host)
    {
        host = Bare(host);
        var selectors = new HashSet<string>(StringComparer.Ordinal);
        var excepted = new HashSet<string>(StringComparer.Ordinal);

        if (HidesGenerics(host))
            foreach (var rule in genericCosmetic)
                if (!Excludes(rule, host)) selectors.Add(rule.Selector);
        foreach (var rule in genericCosmeticExceptions)
            if (!Excludes(rule, host)) excepted.Add(rule.Selector);
        Scoped(host, selectors, excepted);

        selectors.ExceptWith(excepted);
        selectors.RemoveWhere(s => !Css.IsSafe(s));
        if (selectors.Count == 0) return "";
        return string.Join(", ", selectors.OrderBy(s => s, StringComparer.Ordinal)) + " { display: none !important; }";
    }

    private void Scoped(string host, HashSet<string> selectors, HashSet<string> excepted)
    {
        for (var at = host; ; )
        {
            if (cosmeticByDomain.TryGetValue(at, out var list))
                foreach (var rule in list)
                {
                    if (Excludes(rule, host)) continue;
                    (rule.IsException ? excepted : selectors).Add(rule.Selector);
                }
            var dot = at.IndexOf('.');
            if (dot < 0) break;
            at = at[(dot + 1)..];
        }
    }

    private static bool Excludes(CosmeticRule rule, string host)
    {
        foreach (var domain in rule.ExcludedDomains)
            if (NetworkRule.HostMatches(host, domain)) return true;
        return false;
    }

    private const int FormatVersion = 3;

    /// The compact binary form: every compiled rule, with no re-parsing of
    /// list text needed to use it again — `Load` rebuilds the same indices
    /// `Compile` does, straight from already-structured data.
    public void Save(Stream stream)
    {
        using var w = new BinaryWriter(stream, System.Text.Encoding.UTF8, leaveOpen: true);
        w.Write(FormatVersion);

        w.Write(rules.Length);
        foreach (var rule in rules) rule.WriteTo(w);
        Names(plainDomains);
        Names(plainThirdPartyDomains);
        Names(genericHide);

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

        void Names(HashSet<string> names)
        {
            w.Write(names.Count);
            foreach (var name in names) w.Write(name);
        }
    }

    public static FilterList Load(Stream stream, IRegistrableDomain? sites = null)
    {
        using var r = new BinaryReader(stream, System.Text.Encoding.UTF8, leaveOpen: true);
        var version = r.ReadInt32();
        if (version != FormatVersion) throw new InvalidDataException($"Unknown Shields filter blob version {version}.");

        var networkCount = r.ReadInt32();
        var network = new List<NetworkRule>(networkCount);
        for (var i = 0; i < networkCount; i++) network.Add(NetworkRule.ReadFrom(r));
        var plain = Names();
        var plainThirdParty = Names();
        var genericHide = Names();

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

        return Build(network, plain, plainThirdParty, cosmetic, genericHide, stats, sites ?? LiveRegistrableDomain.Instance);

        List<string> Names()
        {
            var count = r.ReadInt32();
            var names = new List<string>(count);
            for (var i = 0; i < count; i++) names.Add(r.ReadString());
            return names;
        }
    }
}

/// Turning selectors into a stylesheet a page can take.
internal static class Css
{
    /// Selectors a stylesheet can't hold, or shouldn't: the extended
    /// pseudo-classes other blockers run in script (`:-abp-has`,
    /// `:has-text`…), which a browser rejects — and with them the whole rule
    /// they sit in — and anything that could close the rule and start one of
    /// its own (`{`, `}`, `;`, a comment), which a list has no business
    /// sending.
    private static readonly string[] Refused =
    [
        "{", "}", ";", "/*", "*/", "<", ":-abp-", ":has-text(", ":contains(", ":xpath(", ":style(", ":remove(",
        ":matches-css", ":upward(", ":watch-attr(", ":min-text-length(", ":others(", ":matches-path(",
        ":matches-attr(", ":matches-prop(", ":if(", ":if-not(", ":nth-ancestor(", ":remove-attr(", ":remove-class(",
    ];

    public static bool IsSafe(string selector)
    {
        if (selector.Length == 0 || selector[0] == '@') return false;
        foreach (var bad in Refused)
            if (selector.Contains(bad, StringComparison.OrdinalIgnoreCase)) return false;
        return true;
    }

    /// One rule per hundred selectors. A browser drops a whole rule when a
    /// single selector in it doesn't parse; a hundred at a time keeps that
    /// loss small without paying for thirteen thousand separate rules.
    public static string Chunked(IReadOnlyList<string> selectors, int size = 100)
    {
        if (selectors.Count == 0) return "";
        var text = new System.Text.StringBuilder(selectors.Count * 24);
        for (var i = 0; i < selectors.Count; i += size)
        {
            var end = Math.Min(i + size, selectors.Count);
            for (var j = i; j < end; j++)
            {
                if (j > i) text.Append(',');
                text.Append(selectors[j]);
            }
            text.Append("{display:none!important}\n");
        }
        return text.ToString();
    }
}
