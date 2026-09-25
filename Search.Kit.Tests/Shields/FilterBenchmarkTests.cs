using System.Diagnostics;
using SearchKit.Shields;
using Xunit.Abstractions;

namespace SearchKit.Tests.Shields;

/// The two budgets in the task: a synthetic 60k-rule list compiles in under
/// 1.5 s, and matching a request against it averages under 20 µs. The list
/// mixes plain `||domain^` rules (most of a real list, EasyList included),
/// domain rules with options, and a few thousand plain wildcard patterns —
/// the two shapes `FilterList` indexes differently — so the benchmark
/// exercises both paths, not just the fast one.
public class FilterBenchmarkTests
{
    private readonly ITestOutputHelper output;
    public FilterBenchmarkTests(ITestOutputHelper output) => this.output = output;

    private static IEnumerable<string> SyntheticList(int count)
    {
        for (var i = 0; i < count; i++)
        {
            switch (i % 4)
            {
                case 0: yield return $"||tracker{i}.example{i % 500}.com^"; break;
                case 1: yield return $"||tracker{i}.example{i % 500}.com^$third-party"; break;
                case 2: yield return $"||tracker{i}.example{i % 500}.com/ads/*$script,image"; break;
                default: yield return $"/ads-{i}-banner-*.js^"; break;
            }
        }
    }

    [Fact]
    public void Compiles_sixty_thousand_rules_in_under_1_5_seconds()
    {
        var lines = SyntheticList(60_000).ToList(); // built outside the timed section
        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();
        var before = GC.GetTotalMemory(true);

        var clock = Stopwatch.StartNew();
        var list = FilterList.Compile(lines);
        clock.Stop();

        var after = GC.GetTotalMemory(false);
        output.WriteLine($"Compiled {list.Stats.NetworkRules:N0} network rules in {clock.Elapsed.TotalMilliseconds:N1} ms.");
        output.WriteLine($"Approximate managed memory retained: {(after - before) / 1024.0 / 1024.0:N1} MB.");

        Assert.Equal(60_000, list.Stats.NetworkRules);
        Assert.True(clock.Elapsed.TotalSeconds < 1.5, $"Compile took {clock.Elapsed.TotalSeconds:N2} s; budget is 1.5 s.");
    }

    [Fact]
    public void Matches_a_request_in_under_20_microseconds_on_average()
    {
        var list = FilterList.Compile(SyntheticList(60_000));

        // A mix of hits (domain-anchored and plain-wildcard) and misses —
        // an all-hit or all-miss set would only measure one path.
        var requests = new (Uri Url, string? PageHost, ResourceKind Kind)[]
        {
            (new Uri("https://tracker123.example123.com/x.js"), "page.com", ResourceKind.Script),
            (new Uri("https://tracker4998.example498.com/ads/banner.png"), "page.com", ResourceKind.Image),
            (new Uri("https://cdn.example.com/ads-777-banner-1.js"), "page.com", ResourceKind.Script),
            (new Uri("https://example.com/perfectly/ordinary/page.html"), "page.com", ResourceKind.Document),
            (new Uri("https://static.example.com/app.js?v=2"), "page.com", ResourceKind.Script),
            (new Uri("https://cdn.example.com/logo.png"), "page.com", ResourceKind.Image),
        };

        for (var i = 0; i < 5_000; i++) // warm up the JIT before timing
        {
            var r = requests[i % requests.Length];
            list.ShouldBlock(r.Url, r.PageHost, r.Kind);
        }

        const int iterations = 200_000;
        var clock = Stopwatch.StartNew();
        for (var i = 0; i < iterations; i++)
        {
            var r = requests[i % requests.Length];
            list.ShouldBlock(r.Url, r.PageHost, r.Kind);
        }
        clock.Stop();

        var perMatchMicroseconds = clock.Elapsed.TotalMilliseconds * 1000.0 / iterations;
        output.WriteLine($"{iterations:N0} matches in {clock.Elapsed.TotalMilliseconds:N1} ms — {perMatchMicroseconds:N2} µs/match average.");
        Assert.True(perMatchMicroseconds < 20.0, $"Average match took {perMatchMicroseconds:N2} µs; budget is 20 µs.");
    }

    /// The same two budgets with the real lists, which a synthetic one only
    /// imitates: set SEARCH_REAL_LISTS to a folder holding easylist.txt and
    /// easyprivacy.txt (a test world's `Shields` folder has them). Without
    /// it there is nothing to measure, and the test says so.
    [Fact]
    public void Real_lists_compile_and_match_within_budget()
    {
        var folder = Environment.GetEnvironmentVariable("SEARCH_REAL_LISTS");
        var paths = new[] { "easylist.txt", "easyprivacy.txt" }.Select(n => Path.Combine(folder ?? "", n)).ToArray();
        if (folder is null || !paths.All(File.Exists))
        {
            output.WriteLine("SEARCH_REAL_LISTS isn't set to a folder with easylist.txt and easyprivacy.txt; nothing measured.");
            return;
        }

        var lists = paths.Select(p => File.ReadAllLines(p)).ToArray();
        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();
        var before = GC.GetTotalMemory(true);
        var clock = Stopwatch.StartNew();
        var list = FilterList.Compile(lists);
        clock.Stop();
        var after = GC.GetTotalMemory(true);
        GC.KeepAlive(list);

        var s = list.Stats;
        var (indexed, words, everywhere) = list.IndexShape;
        output.WriteLine($"Compiled in {clock.Elapsed.TotalMilliseconds:N0} ms, ~{(after - before) / 1024.0 / 1024.0:N1} MB.");
        output.WriteLine($"Network {s.NetworkRules:N0} + {s.NetworkExceptions:N0} exceptions, cosmetic {s.CosmeticRules:N0} + {s.CosmeticExceptions:N0}, skipped {s.SkippedUnsupported:N0}.");
        output.WriteLine($"Index: {indexed:N0} rules under {words:N0} words, {everywhere:N0} tried by every request.");

        var requests = RealisticRequests().ToArray();
        foreach (var r in requests) list.ShouldBlock(r.Url, r.PageHost, r.Kind);
        const int rounds = 2_000;
        clock.Restart();
        for (var i = 0; i < rounds; i++)
            foreach (var r in requests) list.ShouldBlock(r.Url, r.PageHost, r.Kind);
        clock.Stop();
        var each = clock.Elapsed.TotalMilliseconds * 1000.0 / (rounds * requests.Length);
        var blocked = requests.Count(r => list.ShouldBlock(r.Url, r.PageHost, r.Kind));
        output.WriteLine($"{requests.Length} requests ({blocked} blocked): {each:N2} µs a request on average.");

        Assert.True(each < 20.0, $"Average match took {each:N2} µs; budget is 20 µs.");
    }

    // What a news page asks for: its own files, a CDN, and the usual ad and
    // analytics hosts, in the shapes they really use.
    private static IEnumerable<(Uri Url, string? PageHost, ResourceKind Kind)> RealisticRequests()
    {
        const string page = "www.theverge.com";
        string[] scripts =
        [
            "https://www.theverge.com/_next/static/chunks/main-4f1c2a.js",
            "https://securepubads.g.doubleclick.net/tag/js/gpt.js",
            "https://c.amazon-adsystem.com/aax2/apstag.js",
            "https://www.googletagmanager.com/gtag/js?id=G-XXXX",
            "https://cdn.concert.io/lib/concert-ads/v2-latest/concert_ads.js",
            "https://static.chartbeat.com/js/chartbeat.js",
            "https://sb.scorecardresearch.com/cs/1234/beacon.js",
            "https://cdn.cookielaw.org/scripttemplates/otSDKStub.js",
            "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js",
            "https://www.google-analytics.com/analytics.js",
            "https://platform.twitter.com/widgets.js",
            "https://cdn.vox-cdn.com/packs/js/concert_ads-8c1e.js",
            "https://ads.pubmatic.com/AdServer/js/pwt/123/456/pwt.js",
            "https://js-sec.indexww.com/ht/p/123-456.js",
            "https://code.jquery.com/jquery-3.7.1.min.js",
        ];
        foreach (var s in scripts) yield return (new Uri(s), page, ResourceKind.Script);
        string[] images =
        [
            "https://duet-cdn.vox-cdn.com/thumbor/abc=/0x0:2040x1360/2400x1600/filters:focal(1020x680:1021x681)/cdn.vox-cdn.com/uploads/chorus_asset/file/123/photo.jpg",
            "https://www.facebook.com/tr?id=123&ev=PageView&noscript=1",
            "https://pixel.quantserve.com/pixel/p-abc.gif?labels=_fp.event.Default",
            "https://www.theverge.com/icons/logo.svg",
            "https://secure.gravatar.com/avatar/abc?s=96&d=mm",
            "https://example-cdn.com/banners/728x90_banner_ad.gif",
        ];
        foreach (var s in images) yield return (new Uri(s), page, ResourceKind.Image);
        string[] xhr =
        [
            "https://www.theverge.com/api/graphql?operationName=Story&variables=%7B%7D",
            "https://prebid.adnxs.com/pbs/v1/openrtb2/auction",
            "https://bidder.criteo.com/cdb?ptv=65&profileId=207",
            "https://api.segment.io/v1/t",
            "https://c.amazon-adsystem.com/e/dtb/bid?src=3055&u=https%3A%2F%2Fwww.theverge.com%2F",
        ];
        foreach (var s in xhr) yield return (new Uri(s), page, ResourceKind.XmlHttpRequest);
        yield return (new Uri("https://www.youtube.com/embed/abc123?rel=0"), page, ResourceKind.Subdocument);
        yield return (new Uri("https://tpc.googlesyndication.com/safeframe/1-0-40/html/container.html"), page, ResourceKind.Subdocument);
        yield return (new Uri("https://fonts.gstatic.com/s/inter/v12/abc.woff2"), page, ResourceKind.Font);
        yield return (new Uri("https://www.theverge.com/styles/app.css"), page, ResourceKind.Stylesheet);
    }
}
