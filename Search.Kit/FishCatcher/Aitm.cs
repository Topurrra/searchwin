using System.Text.RegularExpressions;
using static SearchKit.FishCatcher.JsText;

namespace SearchKit.FishCatcher;

/// M8, adversary-in-the-middle (relay) sign-in pages. Port of aitm.js.
///
/// The primary flag needs all three: an identity interaction on the page, a
/// brand the page claims to be, and an origin that isn't that brand's. A
/// proxy, a keyword or a host name alone never scores; the soft corroborators
/// (resource graph, icon drift) are only added once the primary has fired.
public static partial class Aitm
{
    public const int PrimaryWeight = 55; // the composite alone is 'high'
    public const int GraphWeight = 15;
    public const int DriftWeight = 5;
    private const double GraphDominance = 0.8; // share of resources under one origin
    private const int GraphMinResources = 5;   // don't judge tiny pages
    private const int GraphMinAnomalies = 2;   // both anomalies needed to corroborate

    public static readonly string[] InteractionKinds =
        ["password", "otp", "mfaApproval", "deviceCode", "passkey", "mfaMigration", "ssoSetup", "qrAuth", "helpdeskUpgrade"];

    private const RegexOptions I = RegexOptions.IgnoreCase | RegexOptions.CultureInvariant;

    [GeneratedRegex($@"{B}(one[{S}-]?time (code|password|passcode)|verification code|security code|6[{S}-]?digit code|enter the code){B}|ერთჯერად|код подтвержд", I)]
    private static partial Regex Otp();

    [GeneratedRegex($@"{B}((approve|deny|verify) (this )?(sign[{S}-]?in|request|login)|open your authenticator|check your (phone|authenticator)|number matching|(enter|tap|select|match) the number( shown)?){B}", I)]
    private static partial Regex MfaApproval();

    [GeneratedRegex($@"{B}((migrate|re[{S}-]?enroll|re[{S}-]?register|update|move) your (mfa|authenticator|2fa|multi[{S}-]?factor)|authenticator migration){B}", I)]
    private static partial Regex MfaMigration();

    [GeneratedRegex($@"{B}(set ?up|create|add|register|enroll|use|sign in with) (a )?(passkey|security key|fido2? key|face ?id|touch ?id|windows hello){B}", I)]
    private static partial Regex Passkey();

    [GeneratedRegex($@"{B}(set ?up|configure|enable) (single sign[{S}-]?on|sso|federation|saml|oidc){B}", I)]
    private static partial Regex SsoSetup();

    [GeneratedRegex($@"{B}scan (this|the) qr( code)?{B}{Dot}{{0,40}}{B}(sign in|log ?in|authenticate|link|pair){B}", I)]
    private static partial Regex QrAuth();

    [GeneratedRegex($@"{B}((security|account|mfa|password) (upgrade|migration|re[{S}-]?validation|re[{S}-]?verification) (required|needed)|verify your account to continue){B}", I)]
    private static partial Regex HelpdeskUpgrade();

    [GeneratedRegex($@"{B}(pass(word|phrase)){B}|პაროლ|парол", I)]
    private static partial Regex Password();

    // Narrower prompts first, so "one-time password" is otp, not password.
    private static readonly (string Kind, Func<Regex> Pattern)[] KindPatterns =
    [
        ("otp", Otp), ("mfaApproval", MfaApproval), ("mfaMigration", MfaMigration), ("passkey", Passkey),
        ("ssoSetup", SsoSetup), ("qrAuth", QrAuth), ("helpdeskUpgrade", HelpdeskUpgrade), ("password", Password),
    ];

    /// One control's accessible text → the interaction kind it asks for, or null.
    public static string? KindForText(string? text)
    {
        if (string.IsNullOrEmpty(text)) return null;
        foreach (var (kind, pattern) in KindPatterns)
            if (pattern().IsMatch(text)) return kind;
        return null;
    }

    // Naive on purpose (aitm.js): keeps the PSL from mangling IP resource hosts.
    private static bool IsIp(string host)
    {
        if (host.Contains(':') || host.StartsWith('[')) return true;
        int parts = 0;
        foreach (var range in host.AsSpan().Split('.'))
        {
            var p = host.AsSpan(range);
            if (++parts > 4 || p.Length is < 1 or > 3) return false;
            foreach (char c in p) if (c is < '0' or > '9') return false;
        }
        return parts == 4;
    }

    /// The brands the page says it is, from its own title, og:site_name and
    /// logo alt texts only. Form labels ("Sign in with Google") don't count,
    /// or every federated sign-in page would claim to be Google.
    public static List<string> InferBrands(IdentityHints hints, FishData data)
    {
        var hay = Lower(string.Join(' ', new[] { hints.Title, hints.OgSiteName }.Concat(hints.LogoAlts)));
        var names = new List<string>();
        if (IsBlank(hay)) return names;
        foreach (var brand in data.Brands)
        {
            if (hay.Contains(Lower(brand.Name), StringComparison.Ordinal) ||
                brand.Keywords.Any(k => hay.Contains(Lower(k), StringComparison.Ordinal)))
            {
                if (!names.Contains(brand.Name)) names.Add(brand.Name);
            }
        }
        return names;
    }

    private static Brand? Named(FishData data, string name) => data.Brands.FirstOrDefault(b => b.Name == name);

    /// The first claimed brand whose domains don't include this site, or null.
    public static string? ClaimedMismatch(IEnumerable<string> claimed, string registrable, FishData data)
    {
        foreach (var name in claimed)
        {
            var brand = Named(data, name);
            if (brand is not null && !brand.Domains.Contains(registrable)) return name;
        }
        return null;
    }

    /// Resource host counts folded to registrable domains (IP hosts as they are).
    public static Dictionary<string, long> ReduceGraph(IReadOnlyDictionary<string, int> resourceHosts, FishData data)
    {
        var graph = new Dictionary<string, long>(StringComparer.Ordinal);
        foreach (var (host, count) in resourceHosts)
        {
            var key = IsIp(host) ? host : data.Psl.RegistrableDomain(host);
            graph[key] = graph.GetValueOrDefault(key) + count;
        }
        return graph;
    }

    /// Reverse-proxy anomalies (0 to 2): the page's own origin serves nearly
    /// everything, and none of the claimed brand's (or any IdP's) origins appear.
    public static int ResourceAnomalies(Dictionary<string, long> graph, IEnumerable<string> claimed, string registrable, FishData data)
    {
        long total = graph.Values.Sum();
        int anomalies = 0;
        if (total >= GraphMinResources && (double)graph.GetValueOrDefault(registrable) / total >= GraphDominance) anomalies++;

        var expected = new HashSet<string>(StringComparer.Ordinal);
        foreach (var name in claimed)
            if (Named(data, name) is { } brand) expected.UnionWith(brand.Domains);
        expected.UnionWith(data.AitmIdp);
        if (expected.Count > 0 && !expected.Any(graph.ContainsKey)) anomalies++;
        return anomalies;
    }

    internal static int Run(string registrable, bool knownLegit, AitmFacts aitm, FishData data, List<Signal> reasons)
    {
        if (aitm.Interactions.Count == 0) return 0;
        // Never flagged: known-legitimate sites, identity providers an attacker
        // can't own, and ZTNA/SWG wrappers that proxy other sites for a living.
        if (knownLegit || data.AitmIdp.Contains(registrable) || data.AitmMediation.Contains(registrable)) return 0;

        var claimed = InferBrands(aitm.IdentityHints, data);
        if (claimed.Count == 0) return 0;
        var mismatch = ClaimedMismatch(claimed, registrable, data);
        if (mismatch is null) return 0;

        int score = PrimaryWeight;
        reasons.Add(new Signal("reasonAitmMismatch", PrimaryWeight, mismatch, registrable));

        var graph = ReduceGraph(aitm.ResourceHosts, data);
        if (ResourceAnomalies(graph, claimed, registrable, data) >= GraphMinAnomalies)
        {
            score += GraphWeight;
            reasons.Add(new Signal("reasonAitmResourceGraph", GraphWeight));
        }
        if (aitm.FaviconCrossOrigin)
        {
            score += DriftWeight;
            reasons.Add(new Signal("reasonAitmDrift", DriftWeight));
        }
        return score;
    }
}
