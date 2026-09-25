namespace SearchKit.FishCatcher;

/// S20, where a sign-in form sends what you type. Port of formaction.js: a
/// password form that posts to another site is the classic harvesting tell,
/// and one that posts to a known collection service is worse.
public static class FormAction
{
    public const int Weight = 40;      // cross-domain post: high alone
    public const int ExfilWeight = 60; // a known credential-collection sink

    // Hosts (and their subdomains) scammers use to collect what a victim types.
    private static readonly string[] ExfilHosts =
    [
        "api.telegram.org", "discord.com", "discordapp.com", "formspree.io", "getform.io",
        "formsubmit.co", "formcarry.com", "docs.google.com", "script.google.com",
        "webhook.site", "pipedream.net", "ngrok.io", "ngrok-free.app", "ngrok.app",
    ];

    public static bool IsExfil(string host)
    {
        if (Signals.IsIpAddress(host)) return true;
        // /(^|\.)requestbin\./
        int at = host.IndexOf("requestbin.", StringComparison.Ordinal);
        while (at >= 0)
        {
            if (at == 0 || host[at - 1] == '.') return true;
            at = host.IndexOf("requestbin.", at + 1, StringComparison.Ordinal);
        }
        return ExfilHosts.Any(d => host == d || host.EndsWith("." + d, StringComparison.Ordinal));
    }

    /// Known-legitimate sites post to SSO hosts across domains all the time, so they're never flagged.
    internal static int Run(string registrable, bool knownLegit, AitmFacts aitm, FishData data, List<Signal> reasons)
    {
        if (knownLegit || aitm.FormActions.Count == 0) return 0;
        string? cross = null;
        foreach (var raw in aitm.FormActions)
        {
            var host = JsText.Lower(raw);
            // The destination is named on the warning, and the page supplied
            // it: anything that isn't a host name is the page's own text.
            if (!PageFacts.IsHost(host)) continue;
            var dest = Signals.IsIpAddress(host) ? host : data.Psl.RegistrableDomain(host);
            if (dest == registrable) continue;
            if (IsExfil(host))
            {
                reasons.Add(new Signal("reasonFormExfil", ExfilWeight, dest));
                return ExfilWeight;
            }
            if (data.SafeList.Contains(dest) || data.AitmIdp.Contains(dest) || data.AitmMediation.Contains(dest)) continue;
            // Same-brand SSO (accounts.brand.com and brand-mail.com) is not a mismatch.
            if (data.Brands.Any(b => b.Domains.Contains(registrable) && b.Domains.Contains(dest))) continue;
            cross ??= dest;
        }
        if (cross is null) return 0;
        reasons.Add(new Signal("reasonFormAction", Weight, cross));
        return Weight;
    }
}
