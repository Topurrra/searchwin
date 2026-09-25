using System.Text.Json;

namespace SearchKit.FishCatcher;

/// What the page probe (probe.js) found on a page: derived facts only, never
/// the page's text or what you typed. Shapes match the extension's messages,
/// so the same probe script can feed either.
public sealed class PageFacts
{
    /// The "aitm-scan" payload: sent only when the page has an active identity
    /// interaction (a password field, a code prompt, a passkey button…).
    public AitmFacts? Aitm { get; init; }
    /// The "scam-scan" facts: seed-phrase requests and tech-support scares.
    public ScamFacts? Scam { get; init; }
    /// The page's text matches the device-code scam ("devicecode-scam").
    public bool DeviceCode { get; init; }
    /// Opt-in RDAP check: the domain is this many days old (only set when young).
    public int? YoungDomainDays { get; init; }
    /// Opt-in Google Safe Browsing: the reason key it mapped to (reasonGsbMalware…).
    public string? GsbThreat { get; init; }

    /// A password field is on the page (background.js reads it from the aitm interactions).
    public bool HasPasswordForm => Aitm?.Interactions.Contains("password") == true;

    /// Reads facts as JSON: { aitm?, scam?, deviceCode?, youngDomainDays?, gsbThreat? },
    /// where aitm and scam are the payloads of the extension's messages (see From). Null
    /// for text that isn't a JSON object (fail open: no facts, no extra score).
    public static PageFacts? Parse(string json)
    {
        try
        {
            using var doc = JsonDocument.Parse(json);
            return From(doc.RootElement);
        }
        catch (JsonException)
        {
            return null;
        }
    }

    /// The extension's shapes, all of them: for facts Search worked out
    /// itself (and the parity corpus). Never for what a page sent: see FromProbe.
    public static PageFacts? From(JsonElement e)
    {
        if (e.ValueKind != JsonValueKind.Object) return null;
        return new PageFacts
        {
            Aitm = e.TryGetProperty("aitm", out var a) ? AitmFacts.From(a) : null,
            Scam = e.TryGetProperty("scam", out var s) ? ScamFacts.From(s) : null,
            DeviceCode = e.TryGetProperty("deviceCode", out var d) && d.ValueKind == JsonValueKind.True,
            YoungDomainDays = e.TryGetProperty("youngDomainDays", out var y) && y.ValueKind == JsonValueKind.Number && y.TryGetInt32(out var days) ? days : null,
            GsbThreat = e.TryGetProperty("gsbThreat", out var g) && g.ValueKind == JsonValueKind.String && g.GetString() is { Length: > 0 } t ? t : null,
        };
    }

    // The probe's own limits (fish-probe.js), held to here as well.
    public const int MaxResourceHosts = 40;
    public const int MaxFormActions = 10;
    public const int MaxLogoAlts = 10;
    public const int MaxBrandTokens = 40;
    public const int MaxTitle = 150;
    public const int MaxHint = 80;
    internal const int MaxCount = 100_000;

    /// What the page probe sent (fish.facts). The page itself can post that
    /// message, so everything in it is the page's word: only what the probe
    /// sends is read (aitm, scam), within the probe's own limits, with host
    /// names that are host names. Never Safe Browsing's verdict or the
    /// domain's age: those are Search's to find out, not the page's to say.
    public static PageFacts? FromProbe(JsonElement e)
    {
        if (e.ValueKind != JsonValueKind.Object) return null;
        return new PageFacts
        {
            Aitm = e.TryGetProperty("aitm", out var a) ? AitmFacts.FromProbe(a) : null,
            Scam = e.TryGetProperty("scam", out var s) ? ScamFacts.From(s) : null,
        };
    }

    /// A host name as `new URL().hostname` gives it: dot-separated labels of
    /// ASCII letters, digits, hyphens and underscores, with at least one dot
    /// (a trailing one is allowed), or a bracketed IPv6 address. Anything
    /// else is text, not a place.
    public static bool IsHost(string host)
    {
        if (host.Length is 0 or > 253) return false;
        if (host[0] == '[')
            return host.Length >= 4 && host[^1] == ']' && host.AsSpan(1, host.Length - 2).IndexOfAnyExcept("0123456789abcdefABCDEF:.") < 0;
        int label = 0;
        bool dotted = false;
        for (int i = 0; i < host.Length; i++)
        {
            char c = host[i];
            if (c == '.')
            {
                if (label == 0) return false;
                if (i < host.Length - 1) dotted = true;
                label = 0;
                continue;
            }
            if (!char.IsAsciiLetterOrDigit(c) && c is not ('-' or '_')) return false;
            if (++label > 63) return false;
        }
        return dotted;
    }

    internal static string[] Strings(JsonElement e, string name)
    {
        if (!e.TryGetProperty(name, out var a) || a.ValueKind != JsonValueKind.Array) return [];
        var list = new List<string>(a.GetArrayLength());
        foreach (var x in a.EnumerateArray())
            if (x.ValueKind == JsonValueKind.String) list.Add(x.GetString()!);
        return [.. list];
    }

    /// At most `max` distinct strings from the first `max` items, each cut to
    /// `length`, and only those `keep` accepts.
    internal static string[] Strings(JsonElement e, string name, int max, int length, Func<string, bool>? keep = null)
    {
        if (!e.TryGetProperty(name, out var a) || a.ValueKind != JsonValueKind.Array) return [];
        var list = new List<string>();
        int seen = 0;
        foreach (var x in a.EnumerateArray())
        {
            if (++seen > max) break;
            if (x.ValueKind != JsonValueKind.String) continue;
            var s = Clip(x.GetString()!, length);
            if ((keep is null || keep(s)) && !list.Contains(s)) list.Add(s);
        }
        return [.. list];
    }

    internal static string Text(JsonElement e, string name) =>
        e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString()! : "";

    internal static string Text(JsonElement e, string name, int length) => Clip(Text(e, name), length);

    private static string Clip(string s, int length)
    {
        if (s.Length <= length) return s;
        // Never half a surrogate pair.
        return s[..(char.IsHighSurrogate(s[length - 1]) ? length - 1 : length)];
    }
}

/// How often one document's fish.facts are heard. The probe looks at most
/// six times a page (and speaks only when something changed); a page that
/// posts the message itself, over and over, would otherwise have each one
/// parsed and scored on the UI thread. A handful at once, then one every
/// RefillMs: a hard cap of six would let the page's own scripts, which run
/// before the probe has looked, spend all of it and leave the probe unheard
/// for the rest of the document. One per tab, from the UI thread only.
public sealed class ProbeBudget(Func<long>? clock = null)
{
    public const int PerDocument = 6;
    public const long RefillMs = 2_000;

    private readonly Func<long> clock = clock ?? (() => Environment.TickCount64);
    private int left = PerDocument;
    private long since;

    /// Whether one more may be heard; counts it.
    public bool Take()
    {
        var now = clock();
        if (left >= PerDocument) since = now; // a full budget saves nothing up
        else
        {
            var earned = (int)Math.Min(PerDocument - left, Math.Max(0, now - since) / RefillMs);
            left += earned;
            since = left >= PerDocument ? now : since + earned * RefillMs;
        }
        if (left == 0) return false;
        left--;
        return true;
    }

    /// A new document starts with a full budget.
    public void NewDocument()
    {
        left = PerDocument;
        since = clock();
    }
}

/// The AiTM probe's facts (M8): which identity interactions the page has, the
/// page's own claims about who it is, and where its resources come from.
public sealed class AitmFacts
{
    /// Interaction kinds: password, otp, mfaApproval, deviceCode, passkey,
    /// mfaMigration, ssoSetup, qrAuth, helpdeskUpgrade.
    public IReadOnlyList<string> Interactions { get; init; } = [];
    public IdentityHints IdentityHints { get; init; } = new();
    /// Host name → how many of the page's resources came from it.
    public IReadOnlyDictionary<string, int> ResourceHosts { get; init; } = new Dictionary<string, int>();
    /// The site icon or manifest is served from another host.
    public bool FaviconCrossOrigin { get; init; }
    /// Hosts that a visible password or code form posts to.
    public IReadOnlyList<string> FormActions { get; init; } = [];

    public static AitmFacts? From(JsonElement e)
    {
        if (e.ValueKind != JsonValueKind.Object) return null;
        var hosts = new Dictionary<string, int>(StringComparer.Ordinal);
        if (e.TryGetProperty("resourceHosts", out var r) && r.ValueKind == JsonValueKind.Object)
            foreach (var p in r.EnumerateObject())
                if (p.Value.ValueKind == JsonValueKind.Number && p.Value.TryGetInt32(out var n)) hosts[p.Name] = n;
        var hints = e.TryGetProperty("identityHints", out var h) && h.ValueKind == JsonValueKind.Object
            ? new IdentityHints
            {
                Title = PageFacts.Text(h, "title"),
                OgSiteName = PageFacts.Text(h, "ogSiteName"),
                LogoAlts = PageFacts.Strings(h, "logoAlts"),
                BrandTokens = PageFacts.Strings(h, "brandTokens"),
            }
            : new IdentityHints();
        return new AitmFacts
        {
            Interactions = PageFacts.Strings(e, "interactions"),
            IdentityHints = hints,
            ResourceHosts = hosts,
            FaviconCrossOrigin = e.TryGetProperty("faviconCrossOrigin", out var f) && f.ValueKind == JsonValueKind.True,
            FormActions = PageFacts.Strings(e, "formActions"),
        };
    }

    /// The probe's aitm payload, within its limits: known interaction kinds,
    /// short hints, and hosts that are host names (lower case, as URL gives them).
    public static AitmFacts? FromProbe(JsonElement e)
    {
        if (e.ValueKind != JsonValueKind.Object) return null;
        var hosts = new Dictionary<string, int>(StringComparer.Ordinal);
        if (e.TryGetProperty("resourceHosts", out var r) && r.ValueKind == JsonValueKind.Object)
        {
            int seen = 0;
            foreach (var p in r.EnumerateObject())
            {
                if (++seen > PageFacts.MaxResourceHosts) break;
                if (!PageFacts.IsHost(p.Name) || p.Value.ValueKind != JsonValueKind.Number || !p.Value.TryGetInt32(out var n) || n < 1) continue;
                var host = p.Name.ToLowerInvariant();
                hosts[host] = Math.Min(hosts.GetValueOrDefault(host) + n, PageFacts.MaxCount);
            }
        }
        var hints = e.TryGetProperty("identityHints", out var h) && h.ValueKind == JsonValueKind.Object
            ? new IdentityHints
            {
                Title = PageFacts.Text(h, "title", PageFacts.MaxTitle),
                OgSiteName = PageFacts.Text(h, "ogSiteName", PageFacts.MaxHint),
                LogoAlts = PageFacts.Strings(h, "logoAlts", PageFacts.MaxLogoAlts, PageFacts.MaxHint),
                BrandTokens = PageFacts.Strings(h, "brandTokens", PageFacts.MaxBrandTokens, PageFacts.MaxHint),
            }
            : new IdentityHints();
        return new AitmFacts
        {
            Interactions = PageFacts.Strings(e, "interactions", Aitm.InteractionKinds.Length, 16, k => Aitm.InteractionKinds.Contains(k)),
            IdentityHints = hints,
            ResourceHosts = hosts,
            FaviconCrossOrigin = e.TryGetProperty("faviconCrossOrigin", out var f) && f.ValueKind == JsonValueKind.True,
            FormActions = [.. PageFacts.Strings(e, "formActions", PageFacts.MaxFormActions, 254, PageFacts.IsHost).Select(s => s.ToLowerInvariant()).Distinct()],
        };
    }
}

/// The page's own claims about who it is: title, og:site_name and logo alt
/// texts (brandTokens, from form labels, are collected but never used to infer
/// the claimed brand: "Sign in with Google" doesn't make a page Google).
public sealed class IdentityHints
{
    public string Title { get; init; } = "";
    public string OgSiteName { get; init; } = "";
    public IReadOnlyList<string> LogoAlts { get; init; } = [];
    public IReadOnlyList<string> BrandTokens { get; init; } = [];
}

/// The scam packs' facts: a wallet seed-phrase request, or a tech-support
/// scare with a phone number or a full-screen lock.
public sealed class ScamFacts
{
    public bool CryptoSeed { get; init; }
    public bool SeedInput { get; init; }
    public bool TechScare { get; init; }
    public bool Phone { get; init; }
    public bool Fullscreen { get; init; }

    public static ScamFacts? From(JsonElement e)
    {
        if (e.ValueKind != JsonValueKind.Object) return null;
        bool B(string name) => e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.True;
        return new ScamFacts
        {
            CryptoSeed = B("cryptoSeed"),
            SeedInput = B("seedInput"),
            TechScare = B("techScare"),
            Phone = B("phone"),
            Fullscreen = B("fullscreen"),
        };
    }
}
