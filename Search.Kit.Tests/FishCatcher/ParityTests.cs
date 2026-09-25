using System.Text;
using System.Text.Json;
using SearchKit.FishCatcher;

namespace SearchKit.Tests;

/// The C# port against the original JS engine. golden.json is what the
/// extension's own engine said about the parity corpus
/// (Extensions/fishcatcher/parity/golden.mjs); every verdict here must match
/// it: host, registrable domain, score, level, and each reason with its
/// parameters and weight, in order. The model's probability must agree to 1e-9.
public class ParityTests
{
    internal static readonly Lazy<FishData> Data = new(FishData.LoadBundled);
    private static readonly Lazy<JsonDocument> Golden = new(() => JsonDocument.Parse(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "FishCatcher", "golden.json"))));

    private static JsonElement G(string name) => Golden.Value.RootElement.GetProperty(name);

    [Fact]
    public void The_corpus_is_big_enough()
    {
        Assert.True(G("urls").GetArrayLength() >= 300);
        var scored = G("urls").EnumerateArray().Count(u => u.GetProperty("result").ValueKind == JsonValueKind.Object && !u.GetProperty("result").TryGetProperty("error", out _));
        Assert.True(scored >= 300, $"{scored} scored");
        // Every level shows up, so the comparison covers each branch.
        foreach (var level in new[] { "low", "elevated", "high", "critical" })
            Assert.Contains(G("urls").EnumerateArray(), u => u.GetProperty("result") is { ValueKind: JsonValueKind.Object } r && r.TryGetProperty("level", out var l) && l.GetString() == level);
    }

    [Fact]
    public void Addresses_score_as_in_the_extension()
    {
        var failures = new List<string>();
        foreach (var item in G("urls").EnumerateArray())
        {
            var url = item.GetProperty("url").GetString()!;
            var verdict = Analyzer.Analyze(url, Data.Value);
            Compare(url, item.GetProperty("result"), verdict, Data.Value, failures);

            // A Uri from the browser must give the same verdict as the text.
            if (Uri.TryCreate(url, UriKind.Absolute, out var uri))
            {
                var viaUri = Analyzer.Analyze(uri, Data.Value);
                if (Describe(viaUri) != Describe(verdict)) failures.Add($"{url}: Uri overload says {Describe(viaUri)}, text says {Describe(verdict)}");
            }
        }
        Assert.True(failures.Count == 0, Report(failures));
    }

    [Fact]
    public void A_feed_scores_as_in_the_extension()
    {
        // The one intended difference: remote.js let a feed's blocklist replace
        // the bundled one, Search adds it to the bundled one (and takes it only
        // when signed; the corpus bundle isn't). With no bundled list the two
        // are the same, so that is what's compared; FeedTests covers the rest.
        var feed = G("feed");
        var bare = Data.Value with { BlockList = [], BundledBlockList = [] };
        var fed = bare.WithFeed(FeedBundle.From(feed.GetProperty("bundle"), blockListSigned: true));
        var failures = new List<string>();
        foreach (var item in feed.GetProperty("urls").EnumerateArray())
        {
            var url = item.GetProperty("url").GetString()!;
            Compare(url, item.GetProperty("result"), Analyzer.Analyze(url, fed), fed, failures);
        }
        Assert.True(failures.Count == 0, Report(failures));
    }

    [Fact]
    public void Page_facts_score_as_in_the_extension()
    {
        var failures = new List<string>();
        foreach (var item in G("pages").EnumerateArray())
        {
            var url = item.GetProperty("url").GetString()!;
            var facts = PageFacts.From(item.GetProperty("facts"));
            Assert.NotNull(facts);
            Compare(url + " " + item.GetProperty("facts").GetRawText(), item.GetProperty("result"), Analyzer.Analyze(url, Data.Value, facts), Data.Value, failures);
        }
        Assert.True(failures.Count == 0, Report(failures));
    }

    [Fact]
    public void Page_text_matchers_agree()
    {
        var failures = new List<string>();
        foreach (var item in G("texts").EnumerateArray())
        {
            var text = item.GetProperty("text").GetString()!;
            var title = item.GetProperty("title").GetString()!;
            var kind = item.GetProperty("kind").ValueKind == JsonValueKind.Null ? null : item.GetProperty("kind").GetString();
            void Check(string what, object? expected, object? actual)
            {
                if (!Equals(expected, actual)) failures.Add($"{what}(\"{text}\"): JS {expected ?? "null"}, C# {actual ?? "null"}");
            }
            Check("kind", kind, Aitm.KindForText(text));
            Check("seed", item.GetProperty("seed").GetBoolean(), ScamPacks.CryptoSeedText(text));
            Check("tech", item.GetProperty("tech").GetBoolean(), ScamPacks.TechSupportText(text));
            Check("deviceCode", item.GetProperty("deviceCode").GetBoolean(), DeviceCode.MatchText(text, title));
        }
        Assert.True(failures.Count == 0, Report(failures));
    }

    [Fact]
    public void Link_checks_agree()
    {
        var failures = new List<string>();
        foreach (var item in G("links").EnumerateArray())
        {
            var links = item.GetProperty("links").EnumerateArray()
                .Select(l => new PageLink(l.GetProperty("href").GetString()!, l.GetProperty("text").GetString()!, l.GetProperty("download").GetString()!))
                .ToList();
            var expected = string.Join("; ", item.GetProperty("findings").EnumerateArray().Select(f =>
                $"{f.GetProperty("key").GetString()}[{string.Join(",", f.GetProperty("params").EnumerateArray().Select(p => p.GetString()))}] {f.GetProperty("href").GetString()}"));
            var actual = string.Join("; ", Links.Classify(links, Data.Value, item.GetProperty("deep").GetBoolean())
                .Select(f => $"{f.Key}[{string.Join(",", f.Params)}] {f.Href}"));
            if (expected != actual) failures.Add($"{item.GetProperty("links").GetRawText()}: JS {expected}, C# {actual}");
        }
        foreach (var item in G("downloads").EnumerateArray())
        {
            var name = item.GetProperty("name").GetString()!;
            var mime = item.GetProperty("mime").GetString()!;
            var r = item.GetProperty("result");
            var expected = r.ValueKind == JsonValueKind.Null ? "null" : $"{r.GetProperty("body").GetString()} {r.GetProperty("arg").GetString()}";
            var actual = Links.InspectDownload(name, mime) is { } w ? $"{w.Body} {w.Arg}" : "null";
            if (expected != actual) failures.Add($"download {name} ({mime}): JS {expected}, C# {actual}");
        }
        Assert.True(failures.Count == 0, Report(failures));
    }

    private static void Compare(string label, JsonElement expected, Verdict? actual, FishData data, List<string> failures)
    {
        if (expected.ValueKind == JsonValueKind.Null || expected.TryGetProperty("error", out _))
        {
            // No verdict in the extension (not web, or the engine threw): none here either.
            if (actual is not null) failures.Add($"{label}: JS gave no verdict, C# {Describe(actual)}");
            return;
        }
        if (actual is null)
        {
            failures.Add($"{label}: C# gave no verdict, JS {expected.GetRawText()}");
            return;
        }
        var want = new StringBuilder();
        want.Append(expected.GetProperty("host").GetString()).Append(' ')
            .Append(expected.GetProperty("registrable").GetString()).Append(' ')
            .Append(expected.GetProperty("score").GetInt32()).Append(' ')
            .Append(expected.GetProperty("level").GetString());
        foreach (var r in expected.GetProperty("reasons").EnumerateArray())
            want.Append(" | ").Append(r.GetProperty("key").GetString()).Append('(')
                .Append(string.Join(",", r.GetProperty("params").EnumerateArray().Select(p => p.GetString()))).Append(")=")
                .Append(r.GetProperty("weight").GetInt32());
        var realSite = expected.GetProperty("realSite");
        want.Append(" real=").Append(realSite.ValueKind == JsonValueKind.Null ? "-" : realSite.GetString());
        var got = Describe(actual);
        if (want.ToString() != got) failures.Add($"{label}:\n    JS {want}\n    C# {got}");

        if (expected.GetProperty("known").GetBoolean() != data.SafeBloom.Has(actual.Registrable))
            failures.Add($"{label}: known-legit Bloom differs for {actual.Registrable}");
        double ml = expected.GetProperty("ml").GetDouble();
        double mine = data.Ml!.Predict(actual.Host);
        if (Math.Abs(ml - mine) > 1e-9) failures.Add($"{label}: model JS {ml:R}, C# {mine:R}");
    }

    private static string Describe(Verdict? v)
    {
        if (v is null) return "null";
        var sb = new StringBuilder($"{v.Host} {v.Registrable} {v.Score} {v.Level.ToString().ToLowerInvariant()}");
        foreach (var s in v.Signals) sb.Append(" | ").Append(s.Key).Append('(').Append(string.Join(",", s.Params)).Append(")=").Append(s.Weight);
        sb.Append(" real=").Append(v.RealSite ?? "-");
        return sb.ToString();
    }

    private static string Report(List<string> failures) =>
        $"{failures.Count} differences:\n" + string.Join("\n", failures.Take(40));
}
