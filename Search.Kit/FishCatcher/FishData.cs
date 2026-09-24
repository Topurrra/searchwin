using System.Text.Json;

namespace SearchKit.FishCatcher;

/// Everything the engine scores with. Immutable: a feed update or a change to
/// your trust list makes a new one (`with`), and a check that's running keeps
/// the one it started with.
public sealed record FishData
{
    /// Big sites whose whole domain is never scored (google.com, paypal.com…).
    public required HashSet<string> SafeList { get; init; }
    /// ~100k known-legitimate domains: still scored, but brand look-alike
    /// signals are off for them (google.ca, github.io).
    public required Bloom SafeBloom { get; init; }
    public required IReadOnlyList<Brand> Brands { get; init; }
    /// High-abuse endings and their weights (tk: 15, xyz: 10…).
    public required IReadOnlyDictionary<string, int> Tlds { get; init; }
    public required IReadOnlyList<string> Keywords { get; init; }
    public required Psl Psl { get; init; }
    /// Known-bad domains: the bundled seed list, or the feed's when it has one.
    public required HashSet<string> BlockList { get; init; }
    /// Identity providers an attacker can't own (okta.com, microsoftonline.com…).
    public required HashSet<string> AitmIdp { get; init; }
    /// ZTNA/SWG wrappers that legitimately proxy other sites (zscaler.net…).
    public required HashSet<string> AitmMediation { get; init; }
    public MlModel? Ml { get; init; }
    /// The registry's public key (SPKI, base64), for the signed feed.
    public string RegistryKeySpki { get; init; } = "";
    /// Domains you chose to trust: never scored.
    public HashSet<string> TrustList { get; init; } = new(StringComparer.Ordinal);
    /// The blocklist that ships with the code, for going back from a feed's.
    public HashSet<string>? BundledBlockList { get; init; }
    /// The community feed's Bloom filter, when a verified feed is applied.
    public Bloom? FeedBloom { get; init; }

    /// Applies a verified feed. Only the blocklist and the Bloom filter may
    /// change: the safe list, brands, endings and keywords ship with the code,
    /// and a feed must never be able to widen them (remote.js applyBundle).
    public FishData WithFeed(FeedBundle bundle) => this with
    {
        BlockList = bundle.BlockList is { } list ? new HashSet<string>(list, StringComparer.Ordinal) : BlockList,
        FeedBloom = bundle.Bloom ?? FeedBloom,
    };

    /// Back to the bundled blocklist and no feed Bloom filter (the feed was turned off).
    public FishData WithoutFeed() => this with { BlockList = BundledBlockList ?? BlockList, FeedBloom = null };

    public FishData WithTrusted(IEnumerable<string> domains) => this with
    {
        TrustList = new HashSet<string>(domains, StringComparer.Ordinal),
    };

    /// Loads the tables bundled in Search.Kit. Takes a few tens of
    /// milliseconds, so it runs on a worker thread (FishCatcher does that).
    public static FishData LoadBundled()
    {
        using var brands = Json("brands.json");
        using var tlds = Json("tlds.json");
        using var keywords = Json("keywords.json");
        using var psl = Json("psl.json");
        using var block = Json("blocklist.json");
        using var safe = Json("safe-list.json");
        using var allow = Json("aitm-allow.json");
        using var key = Json("registry-key.json");

        var brandList = new List<Brand>();
        foreach (var b in brands.RootElement.GetProperty("brands").EnumerateArray())
            brandList.Add(new Brand(b.GetProperty("name").GetString()!, [.. Strings(b, "domains")], [.. Strings(b, "keywords")]));

        var tldWeights = new Dictionary<string, int>(StringComparer.Ordinal);
        foreach (var p in tlds.RootElement.GetProperty("tlds").EnumerateObject()) tldWeights[p.Name] = p.Value.GetInt32();

        var blockList = Set(Strings(block.RootElement, "domains"));
        return new FishData
        {
            SafeList = Set(Strings(safe.RootElement, "domains")),
            SafeBloom = Bloom.FromBinary(Resource("safe-bloom.bin")),
            Brands = brandList,
            Tlds = tldWeights,
            Keywords = [.. Strings(keywords.RootElement, "keywords")],
            Psl = new Psl(Strings(psl.RootElement, "suffixes")),
            BlockList = blockList,
            BundledBlockList = blockList,
            AitmIdp = Set(Strings(allow.RootElement, "idp")),
            AitmMediation = Set(Strings(allow.RootElement, "mediation")),
            Ml = MlModel.FromBinary(Resource("ml-weights.bin")),
            RegistryKeySpki = key.RootElement.GetProperty("spki").GetString() ?? "",
        };
    }

    private static HashSet<string> Set(IEnumerable<string> items) => new(items, StringComparer.Ordinal);

    private static IEnumerable<string> Strings(JsonElement e, string name)
    {
        foreach (var x in e.GetProperty(name).EnumerateArray())
            if (x.ValueKind == JsonValueKind.String) yield return x.GetString()!;
    }

    private static JsonDocument Json(string name) => JsonDocument.Parse(Resource(name));

    internal static byte[] Resource(string name)
    {
        using var stream = typeof(FishData).Assembly.GetManifestResourceStream("FishCatcher/" + name)
            ?? throw new FileNotFoundException("FishCatcher data missing", name);
        var bytes = new byte[stream.Length];
        stream.ReadExactly(bytes);
        return bytes;
    }
}
