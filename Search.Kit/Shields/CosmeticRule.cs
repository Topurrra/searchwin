namespace SearchKit.Shields;

/// One cosmetic rule: `##.ad-slot` (generic, every site), `example.com##.ad`
/// (only there and its subdomains), or `#@#.ad` / `example.com#@#.ad` (an
/// exception cancelling a matching selector). `Domains` empty means generic;
/// `ExcludedDomains` (from `~domain` entries) applies either way — a generic
/// rule can still say "except on this one site".
public sealed class CosmeticRule
{
    public readonly string Selector;
    public readonly bool IsException;
    public readonly string[] Domains;
    public readonly string[] ExcludedDomains;

    public CosmeticRule(string selector, bool isException, string[] domains, string[] excludedDomains)
    {
        Selector = selector;
        IsException = isException;
        Domains = domains;
        ExcludedDomains = excludedDomains;
    }

    internal void WriteTo(BinaryWriter w)
    {
        w.Write(Selector);
        w.Write(IsException);
        w.Write(Domains.Length);
        foreach (var d in Domains) w.Write(d);
        w.Write(ExcludedDomains.Length);
        foreach (var d in ExcludedDomains) w.Write(d);
    }

    internal static CosmeticRule ReadFrom(BinaryReader r)
    {
        var selector = r.ReadString();
        var isException = r.ReadBoolean();
        var domains = new string[r.ReadInt32()];
        for (var i = 0; i < domains.Length; i++) domains[i] = r.ReadString();
        var excluded = new string[r.ReadInt32()];
        for (var i = 0; i < excluded.Length; i++) excluded[i] = r.ReadString();
        return new CosmeticRule(selector, isException, domains, excluded);
    }
}
