namespace SearchKit.FishCatcher;

/// The S1–S17 red flags on an address. Port of signals.js: each match adds
/// a weight and a reason, in the same order as the extension, so the two give
/// the same score and the same list of reasons.
public static class Signals
{
    /// Edit distance, over UTF-16 units like the JS.
    public static int Levenshtein(string a, string b)
    {
        if (a == b) return 0;
        int m = a.Length, n = b.Length;
        if (m == 0 || n == 0) return m == 0 ? n : m;
        Span<int> prev = n < 256 ? stackalloc int[n + 1] : new int[n + 1];
        Span<int> cur = n < 256 ? stackalloc int[n + 1] : new int[n + 1];
        for (int j = 0; j <= n; j++) prev[j] = j;
        for (int i = 1; i <= m; i++)
        {
            cur[0] = i;
            for (int j = 1; j <= n; j++)
                cur[j] = Math.Min(Math.Min(prev[j] + 1, cur[j - 1] + 1), prev[j - 1] + (a[i - 1] == b[j - 1] ? 0 : 1));
            var swap = prev;
            prev = cur;
            cur = swap;
        }
        return prev[n];
    }

    /// Levenshtein(a, b) <= 2, without the work when the lengths alone rule it out.
    public static bool Within2(string a, string b) => Math.Abs(a.Length - b.Length) <= 2 && Levenshtein(a, b) <= 2;

    public static bool IsIpAddress(string host)
    {
        if (host.StartsWith('[')) return true; // IPv6 literal
        int parts = 0;
        foreach (var range in host.AsSpan().Split('.'))
        {
            var p = host.AsSpan(range);
            if (++parts > 4 || p.Length is < 1 or > 3) return false;
            int value = 0;
            foreach (char c in p)
            {
                if (c is < '0' or > '9') return false;
                value = value * 10 + (c - '0');
            }
            if (value > 255) return false;
        }
        return parts == 4;
    }

    /// localhost, loopback, private-LAN and link-local addresses: developer and
    /// home-network hosts, never a phishing target (plain http is normal there too).
    public static bool IsLocalHost(string host)
    {
        if (host == "localhost" || host.EndsWith(".localhost", StringComparison.Ordinal)) return true;
        var h6 = host.AsSpan();
        if (h6.StartsWith('[')) h6 = h6[1..];
        if (h6.EndsWith(']')) h6 = h6[..^1];
        var lower = h6.ToString().ToLowerInvariant();
        if (lower is "::1" or "::") return true;
        if (UniqueLocal(lower) || LinkLocal(lower)) return true;

        // ^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.\d{1,3}$
        Span<int> octets = stackalloc int[4];
        int count = 0;
        foreach (var range in host.AsSpan().Split('.'))
        {
            var p = host.AsSpan(range);
            if (count == 4 || p.Length is < 1 or > 3) return false;
            int value = 0;
            foreach (char c in p)
            {
                if (c is < '0' or > '9') return false;
                value = value * 10 + (c - '0');
            }
            octets[count++] = value;
        }
        if (count != 4) return false;
        int a = octets[0], b = octets[1];
        return a == 127 || a == 0 || a == 10 ||
            (a == 192 && b == 168) ||
            (a == 172 && b >= 16 && b <= 31) ||
            (a == 169 && b == 254);
    }

    private static bool IsHex(char c) => c is >= '0' and <= '9' or >= 'a' and <= 'f';

    // /^f[cd][0-9a-f]{0,2}:/ — fc00::/7
    private static bool UniqueLocal(string h)
    {
        if (h.Length < 3 || h[0] != 'f' || h[1] is not ('c' or 'd')) return false;
        // ':' isn't a hex digit, so backtracking can't help: take up to two, then need ':'.
        int i = 2;
        while (i < 4 && i < h.Length && IsHex(h[i])) i++;
        return i < h.Length && h[i] == ':';
    }

    // /^fe[89ab][0-9a-f]:/ — fe80::/10
    private static bool LinkLocal(string h) =>
        h.Length >= 5 && h[0] == 'f' && h[1] == 'e' && (h[2] is '8' or '9' or 'a' or 'b') && IsHex(h[3]) && h[4] == ':';

    private static readonly Dictionary<char, char> DigitFold = new()
    {
        ['0'] = 'o', ['1'] = 'l', ['3'] = 'e', ['4'] = 'a', ['5'] = 's', ['7'] = 't', ['8'] = 'b', ['@'] = 'a',
    };

    internal readonly record struct Context(WebAddress Address, string Host, string Registrable, bool KnownLegit);

    internal static int Run(in Context ctx, FishData data, List<Signal> reasons)
    {
        int score = 0;
        void Add(int weight, string key, params string[] args)
        {
            score += weight;
            reasons.Add(new Signal(key, weight, args));
        }

        string host = ctx.Host, registrable = ctx.Registrable;
        string decoded = Punycode.DecodeHost(host);
        string folded = Punycode.AsciiFold(decoded);
        string sld = Psl.SldOf(registrable);

        // S12: known-bad domain on the blocklist (exact, or a parent of the host).
        if (Blocked(host, registrable, data.BlockList)) Add(60, "reasonBlocklist");
        else if (data.FeedBloom is { } bloom && (bloom.Has(registrable) || bloom.Has(host))) Add(45, "reasonBloom"); // S16: community feed (probabilistic)

        // Brand-impersonation signals (S1–S3) are off for known-legitimate
        // domains, and S1 for very short names (edit distance 2 collides with real
        // brands by chance: ft.com, go.com, box.com). Without this, google.ca,
        // github.io, goo.gl and redhat.com would all fire.
        if (!ctx.KnownLegit)
        {
            // S1: a typo of a brand's domain. A brand's own domain is never an
            // impersonation of that brand (vanguard.ca is within 2 of vanguard.com).
            if (sld.Length >= 5)
            {
                foreach (var brand in data.Brands)
                {
                    if (brand.Domains.Contains(registrable)) continue;
                    var hit = brand.Domains.FirstOrDefault(d => d != registrable && Within2(registrable, d));
                    if (hit is not null)
                    {
                        Add(45, "reasonBrand", hit, registrable);
                        break;
                    }
                }
            }

            // S2: homoglyph / IDN attack.
            if (decoded != host)
            {
                string foldedRegistrable = data.Psl.RegistrableDomain(folded);
                var brandHit = data.Brands.FirstOrDefault(b => b.Domains.Any(d => Within2(foldedRegistrable, d)));
                if (brandHit is not null || Punycode.HasMixedScripts(decoded))
                    Add(45, "reasonHomoglyph", brandHit?.Name ?? "");
            }

            // S3: a brand's name in the part of the host the owner controls
            // (the public suffix is dropped, so github.io tenants don't hit
            // GitHub). Digit look-alikes (amaz0n, paypa1) are folded too, only
            // for names of 4+ letters so short ones (bog, tbc, aws) don't match noise.
            string owned = JsSlice(host, host.Length - (registrable.Length - sld.Length));
            string ownedFold = FoldDigits(owned);
            foreach (var brand in data.Brands)
            {
                if (brand.Domains.Contains(registrable)) continue;
                var kw = brand.Keywords.FirstOrDefault(k =>
                    owned.Contains(k, StringComparison.Ordinal) || (k.Length >= 4 && ownedFold.Contains(k, StringComparison.Ordinal)));
                if (kw is not null)
                {
                    Add(40, "reasonBrandSubdomain", brand.Name);
                    break;
                }
            }
        }

        // S4: a raw IP address; the domain-shape signals (S8, S10, S11) don't apply to one.
        bool isIp = IsIpAddress(host);
        if (isIp) Add(35, "reasonIp");

        // S5: the user@ trick (the browser ignores everything before @).
        if (ctx.Address.UserName.Contains('.')) Add(35, "reasonAtSign");

        // S6: high-abuse ending.
        string tld = host[(host.LastIndexOf('.') + 1)..];
        if (data.Tlds.TryGetValue(tld, out var tldWeight) && tldWeight != 0) Add(tldWeight, "reasonTld", tld);

        // S7: a phishy word in the host. Not on known-legitimate domains, where
        // login./secure./account. names are normal.
        var keyword = ctx.KnownLegit ? null : data.Keywords.FirstOrDefault(k => host.Contains(k, StringComparison.Ordinal));
        if (keyword is not null) Add(15, "reasonKeyword", keyword);

        // S8: two or more labels below the registrable domain
        // (login.microsoft.com.evil.xyz fires, www.google.co.uk does not).
        int labelCount = host.AsSpan().Count('.') + 1;
        int extraLabels = labelCount - (registrable.AsSpan().Count('.') + 1);
        if (!isIp && extraLabels >= 2) Add(10, "reasonSubdomains", labelCount.ToString(System.Globalization.CultureInfo.InvariantCulture));

        // S9: not encrypted.
        if (ctx.Address.IsHttp) Add(15, "reasonHttp");

        // S10: an unusual digit/hyphen mix.
        int hyphens = host.AsSpan().Count('-');
        int sldDigits = 0;
        foreach (char c in sld) if (c is >= '0' and <= '9') sldDigits++;
        double digitRatio = (double)sldDigits / sld.Length; // 0/0 is NaN, as in JS, and NaN > 0.4 is false
        if (!isIp && (hyphens >= 3 || digitRatio > 0.4)) Add(10, "reasonDigits");

        // S11: a short, random-looking name.
        if (!isIp && sld.Length <= 6 && sldDigits > 0) Add(5, "reasonShort");

        // S17: the on-device n-gram model over the whole host (minus www.):
        // phishing-feed address patterns and generated (DGA) names.
        if (!isIp && data.Ml is { } ml && ml.Predict(host) >= ml.Threshold) Add(20, "reasonMl");

        return score;
    }

    // registrable === b || host === b || host.endsWith('.' + b), for every b in
    // the list, as set lookups: the feed's list can be large.
    private static bool Blocked(string host, string registrable, HashSet<string> list)
    {
        if (list.Count == 0) return false;
        if (list.Contains(registrable) || list.Contains(host)) return true;
        var lookup = list.GetAlternateLookup<ReadOnlySpan<char>>();
        for (int i = host.IndexOf('.'); i >= 0; i = host.IndexOf('.', i + 1))
            if (lookup.Contains(host.AsSpan(i + 1))) return true;
        return false;
    }

    private static string FoldDigits(string s)
    {
        char[]? copy = null;
        for (int i = 0; i < s.Length; i++)
        {
            if (!DigitFold.TryGetValue(s[i], out var letter)) continue;
            copy ??= s.ToCharArray();
            copy[i] = letter;
        }
        return copy is null ? s : new string(copy);
    }

    // String.prototype.slice(0, end): a negative end counts from the end.
    private static string JsSlice(string s, int end)
    {
        if (end < 0) end = Math.Max(0, s.Length + end);
        return s[..Math.Min(end, s.Length)];
    }
}
