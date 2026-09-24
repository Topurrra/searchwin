namespace SearchKit.Shields;

/// Reads one Adblock-Plus-syntax list, line by line. A line this doesn't
/// understand is dropped rather than guessed at, and counted (`FilterStats`)
/// so the list's real coverage stays visible.
internal static class FilterParser
{
    // Checked longest-marker-first isn't needed here since none of these is
    // a substring of another, but order still matters for which one a line
    // is classified under when more than one text happens to appear.
    private static readonly string[] CosmeticMarkers = ["#@#", "#?#", "#$#", "#%#", "##"];

    public static void ParseLine(string rawLine, List<NetworkRule> network, List<CosmeticRule> cosmetic, FilterStats stats)
    {
        var line = rawLine.Trim();
        if (line.Length == 0) { stats.Blank++; return; }
        if (line[0] == '!' || line.StartsWith("[Adblock", StringComparison.OrdinalIgnoreCase))
        {
            stats.Comments++;
            return;
        }

        if (TryFindCosmeticMarker(line, out var markerAt, out var marker))
        {
            var prefix = line[..markerAt];
            var body = line[(markerAt + marker.Length)..];

            // `#?#` (procedural cosmetics), `#$#` (snippet injection) and
            // `#%#` (JS injection, AdGuard's syntax) are recognised shapes
            // this engine doesn't execute — counted, not guessed at.
            if (marker is "#?#" or "#$#" or "#%#" || body.StartsWith("+js(", StringComparison.Ordinal))
            {
                stats.SkippedUnsupported++;
                return;
            }
            if (body.Length == 0) { stats.SkippedUnsupported++; return; }

            var (domains, excluded) = SplitDomains(prefix);
            var rule = new CosmeticRule(body, marker == "#@#", domains, excluded);
            cosmetic.Add(rule);
            if (rule.IsException) stats.CosmeticExceptions++; else stats.CosmeticRules++;
            return;
        }

        if (NetworkRule.TryParse(line, out var networkRule, out var unsupported))
        {
            network.Add(networkRule!);
            if (networkRule!.IsException) stats.NetworkExceptions++; else stats.NetworkRules++;
            return;
        }

        // Either NetworkRule.TryParse explicitly refused an option it
        // doesn't act on (unsupported == true), or the line simply isn't a
        // shape this parser recognises at all — both are dropped and counted
        // the same way, since a caller can't act on either.
        stats.SkippedUnsupported++;
    }

    private static bool TryFindCosmeticMarker(string line, out int at, out string marker)
    {
        at = -1;
        marker = "";
        foreach (var m in CosmeticMarkers)
        {
            var found = line.IndexOf(m, StringComparison.Ordinal);
            if (found < 0) continue;
            if (at < 0 || found < at) { at = found; marker = m; }
        }
        if (at < 0) return false;
        // The text before the marker has to be a plausible domain list (or
        // empty) — otherwise this is a network rule that happens to contain
        // one of these short substrings somewhere in its pattern or options.
        return IsDomainList(line[..at]);
    }

    private static bool IsDomainList(string text) =>
        text.Length == 0 || text.All(c => char.IsAsciiLetterOrDigit(c) || c is '.' or ',' or '~' or '-' or '_');

    private static (string[] Domains, string[] Excluded) SplitDomains(string prefix)
    {
        if (prefix.Length == 0) return ([], []);
        List<string>? domains = null;
        List<string>? excluded = null;
        foreach (var part in prefix.Split(','))
        {
            var d = part.Trim();
            if (d.Length == 0) continue;
            if (d[0] == '~') (excluded ??= []).Add(d[1..].ToLowerInvariant());
            else (domains ??= []).Add(d.ToLowerInvariant());
        }
        return (domains?.ToArray() ?? [], excluded?.ToArray() ?? []);
    }
}
