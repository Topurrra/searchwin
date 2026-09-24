namespace SearchKit.FishCatcher;

/// Address (and page facts) in, verdict out. Port of analyzer.js, plus the
/// device-code override from the extension's background worker.
///
/// Pure and thread-safe: it reads a FishData and changes nothing, so any
/// number of checks can run at once. It fails open: an address it can't read
/// gets no verdict (null), never an exception and never a warning.
public static class Analyzer
{
    public static Verdict? Analyze(string url, FishData data, PageFacts? facts = null)
    {
        var address = WebAddress.From(url);
        return address is null ? null : Analyze(url, address.Value, data, facts);
    }

    public static Verdict? Analyze(Uri url, FishData data, PageFacts? facts = null)
    {
        var address = WebAddress.From(url);
        return address is null ? null : Analyze(url.OriginalString, address.Value, data, facts);
    }

    private static Verdict? Analyze(string url, WebAddress address, FishData data, PageFacts? facts)
    {
        try
        {
            var verdict = Score(url, address, data, facts);
            // background.js flagDeviceCode: page text in the device-code scam
            // shape makes it critical, scored on the address alone. Never on a
            // safe-listed, trusted or known-legitimate site: those are the docs
            // and lessons that explain the scam.
            if (facts is { DeviceCode: true })
            {
                var basis = facts.Aitm is null && facts.Scam is null && facts.YoungDomainDays is null && facts.GsbThreat is null
                    ? verdict
                    : Score(url, address, data, null);
                var r = basis.Registrable;
                if (!data.SafeList.Contains(r) && !data.TrustList.Contains(r) && !data.SafeBloom.Has(r))
                {
                    return new Verdict
                    {
                        Url = url,
                        Host = basis.Host,
                        Registrable = r,
                        Score = 100,
                        Level = Level.Critical,
                        Signals = [.. basis.Signals, new Signal("reasonDeviceCode", DeviceCode.Weight)],
                        Trusted = false,
                    };
                }
            }
            return verdict;
        }
        catch (FormatException)
        {
            // A host the engine can't decode (bad Punycode). The extension threw
            // here and showed nothing; so do we.
            return null;
        }
    }

    private static Verdict Score(string url, WebAddress address, FishData data, PageFacts? facts)
    {
        // hostname, lower case, without a trailing dot
        var host = address.Host.EndsWith('.') ? address.Host[..^1] : address.Host;
        var registrable = Signals.IsIpAddress(host) ? host : data.Psl.RegistrableDomain(host);
        Verdict Clean() => new()
        {
            Url = url, Host = host, Registrable = registrable, Score = 0, Level = Level.Low, Signals = [],
            Trusted = data.TrustList.Contains(registrable),
        };

        if (data.SafeList.Contains(registrable) || data.TrustList.Contains(registrable)) return Clean();

        // localhost, loopback and private-LAN addresses are developer and
        // home-network hosts, never phishing targets, so they stay quiet (plain http too).
        if (Signals.IsLocalHost(host)) return Clean();

        // Softer allow-list: known-legitimate domains are still scored, but the
        // brand look-alike signals are off (google.ca, github.io). A blocklist,
        // model or ending hit can still fire.
        bool knownLegit = data.SafeBloom.Has(registrable);
        var reasons = new List<Signal>();
        int score = Signals.Run(new Signals.Context(address, host, registrable, knownLegit), data, reasons);

        if (facts is not null)
        {
            // S13: a password form on a site that isn't safe-listed or trusted.
            // Half weight on a known-legitimate domain, where a sign-in form is normal.
            if (facts.HasPasswordForm)
            {
                int w = knownLegit ? 10 : 20;
                score += w;
                reasons.Add(new Signal("reasonPasswordForm", w));
            }

            // S15: a freshly registered domain (opt-in RDAP check).
            if (facts.YoungDomainDays is { } days)
            {
                score += 25;
                reasons.Add(new Signal("reasonYoungDomain", 25, days.ToString(System.Globalization.CultureInfo.InvariantCulture)));
            }

            // S18: Google Safe Browsing (opt-in), an authoritative third-party verdict.
            if (!string.IsNullOrEmpty(facts.GsbThreat))
            {
                score += 60;
                reasons.Add(new Signal(facts.GsbThreat, 60));
            }

            // M8: AiTM composite, then S20 (the form's destination rides in the same payload).
            if (facts.Aitm is { } aitm)
            {
                score += Aitm.Run(registrable, knownLegit, aitm, data, reasons);
                score += FormAction.Run(registrable, knownLegit, aitm, data, reasons);
            }

            // Scam packs: a seed-phrase request, or a tech-support locker scare.
            if (facts.Scam is { } scam) score += ScamPacks.Run(scam, reasons);
        }

        score = Math.Min(100, score);
        return new Verdict
        {
            Url = url,
            Host = host,
            Registrable = registrable,
            Score = score,
            Level = Verdict.LevelFor(score),
            Signals = reasons,
            Trusted = data.TrustList.Contains(registrable),
            RealSite = Verdict.RealSiteFor(reasons, data.Brands),
        };
    }
}
