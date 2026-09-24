namespace SearchKit.Shields;

/// One compiled network rule — a block (`||ads.example^`) or an exception
/// (`@@||example.com/analytics.js$script`) — plus the options that narrow
/// it. Built once by `FilterParser`; `MatchesContext`/`MatchesUrl` are the
/// only two calls `FilterList.ShouldBlock` makes per candidate rule, so
/// nothing here does more work than a field read and a couple of comparisons.
public sealed class NetworkRule
{
    public readonly bool IsException;
    public readonly bool Important;
    public readonly ResourceKinds Kinds;
    public readonly bool? ThirdParty;
    public readonly string[]? DomainIncludes;
    public readonly string[]? DomainExcludes;

    /// The `||domain` this rule is anchored to, already lower-cased —
    /// matched against the request's actual host (with subdomains), never
    /// as text inside the URL. Null for a plain pattern, matched as text
    /// against the whole URL instead.
    public readonly string? AnchorDomain;

    private readonly AbpPattern pattern;

    private NetworkRule(bool isException, bool important, ResourceKinds kinds, bool? thirdParty,
        string[]? domainIncludes, string[]? domainExcludes, string? anchorDomain, AbpPattern pattern)
    {
        IsException = isException;
        Important = important;
        Kinds = kinds;
        ThirdParty = thirdParty;
        DomainIncludes = domainIncludes;
        DomainExcludes = domainExcludes;
        AnchorDomain = anchorDomain;
        this.pattern = pattern;
    }

    /// Parses one non-comment, non-cosmetic line. `unsupported` is true for
    /// a line whose *shape* is a network rule but that leans on an option
    /// this engine doesn't act on ($redirect, $csp, $removeparam, anything
    /// else outside the documented set) — the whole rule is dropped rather
    /// than applied as a plain block, which would over-block whatever the
    /// missing option was meant to narrow it to.
    public static bool TryParse(string line, out NetworkRule? rule, out bool unsupported)
    {
        rule = null;
        unsupported = false;

        var text = line;
        var isException = false;
        if (text.StartsWith("@@", StringComparison.Ordinal)) { isException = true; text = text[2..]; }
        if (text.Length == 0) return false;

        var dollarAt = text.IndexOf('$');
        var patternText = dollarAt >= 0 ? text[..dollarAt] : text;
        var optionsText = dollarAt >= 0 ? text[(dollarAt + 1)..] : null;
        if (patternText.Length == 0) return false;

        var kinds = ResourceKinds.All;
        bool? thirdParty = null;
        string[]? domainIncludes = null;
        string[]? domainExcludes = null;
        var important = false;

        if (optionsText != null)
        {
            var positive = ResourceKinds.None;
            var negative = ResourceKinds.None;
            var hasPositive = false;
            var hasNegative = false;

            foreach (var raw in optionsText.Split(','))
            {
                var option = raw.Trim();
                if (option.Length == 0) continue;
                var negate = option.StartsWith('~');
                var key = negate ? option[1..] : option;
                var eq = key.IndexOf('=');
                var value = eq >= 0 ? key[(eq + 1)..] : null;
                if (eq >= 0) key = key[..eq];

                switch (key.ToLowerInvariant())
                {
                    case "third-party":
                        thirdParty = !negate;
                        break;
                    case "important":
                        important = true;
                        break;
                    case "domain":
                        if (value is null or "") { unsupported = true; return false; }
                        (domainIncludes, domainExcludes) = SplitDomainOption(value);
                        break;
                    case "script": Flag(ResourceKinds.Script); break;
                    case "image": Flag(ResourceKinds.Image); break;
                    case "stylesheet": Flag(ResourceKinds.Stylesheet); break;
                    case "xmlhttprequest": Flag(ResourceKinds.XmlHttpRequest); break;
                    case "subdocument": Flag(ResourceKinds.Subdocument); break;
                    case "media": Flag(ResourceKinds.Media); break;
                    case "font": Flag(ResourceKinds.Font); break;
                    default:
                        // $redirect(-rule), $csp, $removeparam, $popup,
                        // $genericblock/$generichide, $ping, $websocket,
                        // $object, $document, $match-case, $badfilter… none
                        // of these change what "block" or "allow" means in a
                        // way a plain match/no-match can stand in for.
                        unsupported = true;
                        return false;
                }
                continue;

                void Flag(ResourceKinds k)
                {
                    if (negate) { negative |= k; hasNegative = true; }
                    else { positive |= k; hasPositive = true; }
                }
            }

            if (hasPositive) kinds = positive;
            else if (hasNegative) kinds = ResourceKinds.All & ~negative;
        }

        var (anchorDomain, pattern) = BuildPattern(patternText);
        rule = new NetworkRule(isException, important, kinds, thirdParty, domainIncludes, domainExcludes, anchorDomain, pattern);
        return true;
    }

    public bool MatchesContext(ResourceKind kind, bool thirdParty, string? pageHost)
    {
        if ((Kinds & ToFlag(kind)) == 0) return false;
        if (ThirdParty is { } wantThirdParty && wantThirdParty != thirdParty) return false;
        if (DomainIncludes is { Length: > 0 } inc && (pageHost == null || !AnyHostMatch(inc, pageHost))) return false;
        if (DomainExcludes is { Length: > 0 } exc && pageHost != null && AnyHostMatch(exc, pageHost)) return false;
        return true;
    }

    /// `lowerUrl` is the full request URL, already lower-cased once by the
    /// caller and shared across every rule it tries; `afterHost` is the
    /// index right after the request's `scheme://host[:port]`.
    public bool MatchesUrl(string lowerUrl, int afterHost, string requestHost)
    {
        if (AnchorDomain != null && !HostMatches(requestHost, AnchorDomain)) return false;
        return pattern.IsMatch(lowerUrl, AnchorDomain != null ? afterHost : 0);
    }

    /// Every literal token (≥3 alnum characters) this pattern could be
    /// indexed under — empty for a domain-anchored rule (the host dictionary
    /// already narrows those) or a pattern with nothing that long (`*ads*`).
    /// `FilterList` picks whichever candidate is rarest across the whole
    /// list, not just the longest one: two rules can easily share their
    /// longest word ("banner", "advert"…) while differing everywhere else,
    /// and indexing by the common word would dump both into one oversized
    /// bucket instead of narrowing anything.
    public IEnumerable<string> CandidateTokens() => AnchorDomain != null ? [] : pattern.IndexTokens();

    /// A plain `||domain^` block, optionally `$third-party`, and nothing
    /// else — most of a real list (93k of EasyList + EasyPrivacy's 110k
    /// network rules). `FilterList` keeps these as bare names in a set
    /// rather than as rule objects: about a third of the memory, and a
    /// lookup instead of a rule test.
    internal bool IsPlainDomain =>
        !IsException && !Important && Kinds == ResourceKinds.All && ThirdParty != false
        && DomainIncludes == null && DomainExcludes == null && AnchorDomain != null && pattern.IsDomainOnly;

    /// `@@||domain^$generichide` (or `$ghide`): the site asks for no generic
    /// cosmetic rules — usually because hiding every `.ad` breaks it or sets
    /// off its ad-block detector. Only the plain domain shape is understood;
    /// anything fancier is left to the caller to count as unsupported.
    internal static string? GenericHideDomain(string line)
    {
        if (!line.StartsWith("@@||", StringComparison.Ordinal)) return null;
        var dollar = line.IndexOf('$');
        if (dollar < 0) return null;
        var options = line[(dollar + 1)..].Split(',');
        if (options.Length != 1 || options[0].Trim().ToLowerInvariant() is not ("generichide" or "ghide")) return null;
        var domain = line[4..dollar].TrimEnd('^').ToLowerInvariant();
        return IsPlausibleDomain(domain) ? domain : null;
    }

    // Without building "." + domain: this runs for every `$domain=` rule a
    // request meets, and for every `~site` a stylesheet is checked against.
    internal static bool HostMatches(string host, string domain) =>
        host.Length == domain.Length
            ? host.Equals(domain, StringComparison.Ordinal)
            : host.Length > domain.Length && host[host.Length - domain.Length - 1] == '.' && host.EndsWith(domain, StringComparison.Ordinal);

    private static bool AnyHostMatch(string[] domains, string host)
    {
        foreach (var d in domains)
            if (HostMatches(host, d)) return true;
        return false;
    }

    internal static ResourceKinds ToFlag(ResourceKind kind) => kind switch
    {
        ResourceKind.Document => ResourceKinds.Document,
        ResourceKind.Subdocument => ResourceKinds.Subdocument,
        ResourceKind.Script => ResourceKinds.Script,
        ResourceKind.Image => ResourceKinds.Image,
        ResourceKind.Stylesheet => ResourceKinds.Stylesheet,
        ResourceKind.XmlHttpRequest => ResourceKinds.XmlHttpRequest,
        ResourceKind.Media => ResourceKinds.Media,
        ResourceKind.Font => ResourceKinds.Font,
        _ => ResourceKinds.Other,
    };

    private static (string[]? Includes, string[]? Excludes) SplitDomainOption(string value)
    {
        List<string>? includes = null;
        List<string>? excludes = null;
        foreach (var part in value.Split('|'))
        {
            if (part.Length == 0) continue;
            if (part[0] == '~') (excludes ??= []).Add(part[1..].ToLowerInvariant());
            else (includes ??= []).Add(part.ToLowerInvariant());
        }
        return (includes?.ToArray(), excludes?.ToArray());
    }

    // `||domain[^/*...]`: everything up to the first `^`, `/` or `*` is the
    // anchor domain, matched against the request's actual host; what's left
    // (often nothing, or just the `^`) is parsed as a pattern that has to
    // follow the domain immediately.
    private static (string? AnchorDomain, AbpPattern Pattern) BuildPattern(string text)
    {
        var lower = text.ToLowerInvariant();
        if (!lower.StartsWith("||", StringComparison.Ordinal))
            return (null, AbpPattern.Parse(lower));

        var rest = lower[2..];
        var end = 0;
        while (end < rest.Length && rest[end] is not ('^' or '/' or '*')) end++;
        var domain = rest[..end];
        if (domain.Length == 0 || !IsPlausibleDomain(domain))
            return (null, AbpPattern.Parse(lower)); // an odd `||` line — fall back to matching it as plain text

        return (domain, AbpPattern.Parse(rest[end..], forceStartAnchor: true));
    }

    private static bool IsPlausibleDomain(string domain) =>
        domain.Contains('.') && !domain.Contains("..") &&
        domain.All(c => char.IsAsciiLetterOrDigit(c) || c is '.' or '-');

    internal void WriteTo(BinaryWriter w)
    {
        w.Write(IsException);
        w.Write(Important);
        w.Write((int)Kinds);
        w.Write(ThirdParty.HasValue);
        if (ThirdParty.HasValue) w.Write(ThirdParty.Value);
        WriteStrings(w, DomainIncludes);
        WriteStrings(w, DomainExcludes);
        w.Write(AnchorDomain != null);
        if (AnchorDomain != null) w.Write(AnchorDomain);
        pattern.WriteTo(w);
    }

    internal static NetworkRule ReadFrom(BinaryReader r)
    {
        var isException = r.ReadBoolean();
        var important = r.ReadBoolean();
        var kinds = (ResourceKinds)r.ReadInt32();
        bool? thirdParty = r.ReadBoolean() ? r.ReadBoolean() : null;
        var includes = ReadStrings(r);
        var excludes = ReadStrings(r);
        var anchorDomain = r.ReadBoolean() ? r.ReadString() : null;
        var pattern = AbpPattern.ReadFrom(r);
        return new NetworkRule(isException, important, kinds, thirdParty, includes, excludes, anchorDomain, pattern);
    }

    private static void WriteStrings(BinaryWriter w, string[]? values)
    {
        w.Write(values?.Length ?? 0);
        if (values == null) return;
        foreach (var v in values) w.Write(v);
    }

    private static string[]? ReadStrings(BinaryReader r)
    {
        var count = r.ReadInt32();
        if (count == 0) return null;
        var values = new string[count];
        for (var i = 0; i < count; i++) values[i] = r.ReadString();
        return values;
    }
}
