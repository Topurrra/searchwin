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

    /// Reads the probe's JSON: { aitm?, scam?, deviceCode?, youngDomainDays?, gsbThreat? },
    /// where aitm and scam are the payloads of the extension's messages. Null
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

    internal static string[] Strings(JsonElement e, string name)
    {
        if (!e.TryGetProperty(name, out var a) || a.ValueKind != JsonValueKind.Array) return [];
        var list = new List<string>(a.GetArrayLength());
        foreach (var x in a.EnumerateArray())
            if (x.ValueKind == JsonValueKind.String) list.Add(x.GetString()!);
        return [.. list];
    }

    internal static string Text(JsonElement e, string name) =>
        e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString()! : "";
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
