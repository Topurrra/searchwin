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

    /// A `||` rule whose name the host dictionary can't hold (see
    /// BuildPattern): the pattern starts at the host or at one of its dots.
    private readonly bool hostAnchored;

    private NetworkRule(bool isException, bool important, ResourceKinds kinds, bool? thirdParty,
        string[]? domainIncludes, string[]? domainExcludes, string? anchorDomain, AbpPattern pattern, bool hostAnchored = false)
    {
        this.hostAnchored = hostAnchored;
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
    /// a line whose *shape* is a network rule but that this engine can't
    /// honour, and the rule is dropped and counted:
    /// - a `/regex/` pattern;
    /// - an option that makes the rule about something other than whether a
    ///   request may load ($redirect, $csp, $removeparam, $document,
    ///   $elemhide, $popup, $badfilter…) — as a plain block or allow it
    ///   would do something the list never asked for;
    /// - on a block, an option outside the set below: dropping the rule
    ///   under-blocks, while ignoring the option would over-block whatever
    ///   it was meant to narrow the rule to.
    ///
    /// An exception with an option it can't judge (`$match-case`, a new
    /// one) is kept without that option instead. It then allows a little
    /// more than the list meant; dropping it would block what the list
    /// says must load, and that breaks sites.
    public static bool TryParse(string line, out NetworkRule? rule, out bool unsupported)
    {
        rule = null;
        unsupported = false;

        var text = line;
        var isException = false;
        if (text.StartsWith("@@", StringComparison.Ordinal)) { isException = true; text = text[2..]; }
        if (text.Length == 0) return false;
        if (IsRegex(text)) { unsupported = true; return false; }

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

                switch (key = key.ToLowerInvariant())
                {
                    case "third-party" or "3p":
                        thirdParty = !negate;
                        break;
                    case "first-party" or "1p":
                        thirdParty = negate;
                        break;
                    case "important":
                        important = true;
                        break;
                    case "domain" or "from":
                        if (value is null or "") { unsupported = true; return false; }
                        (domainIncludes, domainExcludes) = SplitDomainOption(value);
                        break;
                    case "script": Flag(ResourceKinds.Script); break;
                    case "image": Flag(ResourceKinds.Image); break;
                    case "stylesheet" or "css": Flag(ResourceKinds.Stylesheet); break;
                    case "xmlhttprequest" or "xhr": Flag(ResourceKinds.XmlHttpRequest); break;
                    case "subdocument" or "frame": Flag(ResourceKinds.Subdocument); break;
                    case "media": Flag(ResourceKinds.Media); break;
                    case "font": Flag(ResourceKinds.Font); break;
                    // What WebView2 has no kind of its own for comes as Other
                    // (the browser's Kind maps Ping, Websocket, Manifest… there).
                    case "other" or "ping" or "beacon" or "websocket" or "object" or "object-subrequest":
                        Flag(ResourceKinds.Other);
                        break;
                    // Real kinds of request that never reach the filter: a
                    // rule for only these never fires.
                    case "webrtc" or "webbundle":
                        Flag(ResourceKinds.None);
                        break;
                    default:
                        if (!isException || OtherMeaning.Contains(key))
                        {
                            unsupported = true;
                            return false;
                        }
                        break; // an exception without the one option it can't judge
                }
                continue;

                void Flag(ResourceKinds k)
                {
                    if (negate) { negative |= k; hasNegative = true; }
                    else { positive |= k; hasPositive = true; }
                }
            }

            if (hasPositive) kinds = positive & ~negative;
            else if (hasNegative) kinds = ResourceKinds.All & ~negative;
            if (kinds == ResourceKinds.None) { unsupported = true; return false; }
        }

        var (anchorDomain, pattern, hostAnchored) = BuildPattern(patternText);
        rule = new NetworkRule(isException, important, kinds, thirdParty, domainIncludes, domainExcludes, anchorDomain, pattern, hostAnchored);
        return true;
    }

    /// Options that turn a rule into something other than "this request
    /// may (not) load": a redirect, a CSP header, a parameter taken off, a
    /// whole page or its cosmetics let off, a popup, another rule cancelled.
    /// An exception carrying one isn't an allow, so it's never kept as one.
    private static readonly HashSet<string> OtherMeaning = new(StringComparer.Ordinal)
    {
        "redirect", "redirect-rule", "rewrite", "empty", "mp4", "csp", "permissions", "removeparam", "queryprune",
        "replace", "urltransform", "uritransform", "urlskip", "header", "removeheader", "cookie", "jsonprune",
        "document", "doc", "all", "popup", "popunder", "elemhide", "ehide", "generichide", "ghide",
        "specifichide", "shide", "genericblock", "badfilter", "inline-script", "inline-font", "cname",
        "urlblock", "content", "jsinject", "extension", "stealth", "network", "app", "sitekey",
    };

    // `/…/`, with or without options after it: a regular expression, which
    // this engine doesn't run. A path pattern that happens to start with a
    // slash (`/ads/*`, `/banner.gif`) ends with something else.
    private static bool IsRegex(string text)
    {
        if (text.Length < 3 || text[0] != '/') return false;
        if (text[^1] == '/') return true;
        var dollar = text.LastIndexOf('$');
        return dollar > 1 && text[dollar - 1] == '/';
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
    /// index right after the request's `scheme://host[:port]`. A match must
    /// start within the first `limit` characters (FilterList.MatchedLength).
    public bool MatchesUrl(string lowerUrl, int afterHost, string requestHost, int limit = int.MaxValue)
    {
        if (AnchorDomain != null && !HostMatches(requestHost, AnchorDomain)) return false;
        if (hostAnchored) return MatchesAtHost(lowerUrl, afterHost, limit);
        return pattern.IsMatch(lowerUrl, AnchorDomain != null ? afterHost : 0, limit);
    }

    // At the start of the host, or just after one of its dots: where
    // `scheme://` (or `user@`) ends, up to the end of the authority.
    private bool MatchesAtHost(string lowerUrl, int afterHost, int limit)
    {
        var scheme = lowerUrl.IndexOf("://", StringComparison.Ordinal);
        if (scheme < 0 || afterHost > lowerUrl.Length) return false;
        var hostStart = scheme + 3;
        var at = lowerUrl.LastIndexOf('@', afterHost - 1, afterHost - hostStart);
        if (at >= 0) hostStart = at + 1;
        for (var i = hostStart; i < afterHost; i++)
            if ((i == hostStart || lowerUrl[i - 1] == '.') && pattern.IsMatch(lowerUrl, i, limit)) return true;
        return false;
    }

    /// Every whole word this pattern could be indexed under
    /// (`AbpPattern.IndexTokens`) — empty for a domain-anchored rule (the
    /// host dictionary already narrows those) or a pattern with no word it
    /// holds on both sides (`*ads*`, `banner`).
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
    //
    // A name the dictionary can't hold (an underscore, no dot, a wildcard:
    // `||ad_host.example^`, `||adhost/`, `||*.cdn.example/px`) keeps what
    // `||` means — the pattern starts at the host or at one of its dots —
    // and is matched as text from there. Parsed as a plain pattern instead,
    // its `||` would be a literal `|` no address holds, and it never matched.
    private static (string? AnchorDomain, AbpPattern Pattern, bool HostAnchored) BuildPattern(string text)
    {
        var lower = text.ToLowerInvariant();
        if (!lower.StartsWith("||", StringComparison.Ordinal))
            return (null, AbpPattern.Parse(lower), false);

        var rest = lower[2..];
        var end = 0;
        while (end < rest.Length && rest[end] is not ('^' or '/' or '*')) end++;
        var domain = rest[..end];
        if (domain.Length == 0 || !IsPlausibleDomain(domain))
            return (null, AbpPattern.Parse(rest, forceStartAnchor: true), true);

        return (domain, AbpPattern.Parse(rest[end..], forceStartAnchor: true), false);
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
        w.Write(hostAnchored);
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
        var hostAnchored = r.ReadBoolean();
        var pattern = AbpPattern.ReadFrom(r);
        return new NetworkRule(isException, important, kinds, thirdParty, includes, excludes, anchorDomain, pattern, hostAnchored);
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
