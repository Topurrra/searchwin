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
}
