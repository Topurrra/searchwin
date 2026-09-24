namespace SearchKit.FishCatcher;

/// The registrable domain (eTLD+1) of a host, from the bundled Public Suffix
/// List. Port of psl.js: rules use PSL syntax (plain "co.uk", wildcard "*.ck",
/// exception "!www.ck"), the longest matching rule wins, an exception makes its
/// own name the registrable domain, and a host under an unlisted ending falls
/// back to its last two labels.
public sealed class Psl
{
    private readonly HashSet<string> rules;
    private readonly HashSet<string>.AlternateLookup<ReadOnlySpan<char>> lookup;

    public Psl(IEnumerable<string> suffixes)
    {
        rules = new HashSet<string>(suffixes, StringComparer.Ordinal);
        lookup = rules.GetAlternateLookup<ReadOnlySpan<char>>();
    }

    public int Count => rules.Count;

    public string RegistrableDomain(string host)
    {
        // labels.length <= 2: nothing to strip.
        int dots = host.AsSpan().Count('.');
        if (dots <= 1) return host;

        // Candidate suffixes from the longest (the whole host) to the shortest;
        // the first hit is the prevailing rule. A candidate is the text after a
        // label boundary, which is what labels.slice(i).join('.') gives.
        Span<char> probe = stackalloc char[Math.Min(host.Length + 2, 512)];
        int previous = -1; // start of the label before the candidate
        int start = 0;
        while (true)
        {
            var candidate = host.AsSpan(start);
            if (Has('!', candidate, probe)) return candidate.ToString();

            int next = host.IndexOf('.', start);
            bool wildcard = next >= 0 && Has('*', host.AsSpan(next + 1), probe, dot: true);
            if (lookup.Contains(candidate) || wildcard)
                return previous < 0 ? host : host[previous..];

            if (next < 0) break;
            previous = start;
            start = next + 1;
        }
        // No rule: the last two labels.
        int last = host.LastIndexOf('.');
        int cut = host.LastIndexOf('.', last - 1);
        return host[(cut + 1)..];
    }

    // "!" + name or "*." + name, looked up without allocating a string.
    private bool Has(char mark, ReadOnlySpan<char> name, Span<char> probe, bool dot = false)
    {
        int extra = dot ? 2 : 1;
        if (name.Length + extra > probe.Length) return rules.Contains((dot ? "*." : "!") + name.ToString());
        probe[0] = mark;
        if (dot) probe[1] = '.';
        name.CopyTo(probe[extra..]);
        return lookup.Contains(probe[..(name.Length + extra)]);
    }

    /// First label of the registrable domain ("evil" for evil.ru, "bbc" for bbc.co.uk).
    public static string SldOf(string registrable)
    {
        int dot = registrable.IndexOf('.');
        return dot < 0 ? registrable : registrable[..dot];
    }
}
