using System.Diagnostics;
using SearchKit.Shields;
using Xunit.Abstractions;

namespace SearchKit.Tests.Shields;

/// A rule with several wildcards against a very long address: a matcher
/// that backtracks tries every way of splitting the address between the
/// wildcards, which grows with the address to the power of their number.
/// `ShouldBlock` runs on the thread that draws every window, so a page that
/// asks for one such address would freeze them all.
public class PatternSpeedTests
{
    private readonly ITestOutputHelper output;
    public PatternSpeedTests(ITestOutputHelper output) => this.output = output;

    private static string Long(string start, string unit, string end)
    {
        var text = new System.Text.StringBuilder(start, 16_500);
        while (text.Length < 16 * 1024) text.Append(unit);
        return text.Append(end).ToString();
    }

    private static async Task<bool> Finishes(Task task) =>
        await Task.WhenAny(task, Task.Delay(TimeSpan.FromSeconds(5))) == task;

    public static TheoryData<string, string, bool> Cases => new()
    {
        // Every piece but the last is found everywhere; the last is nowhere.
        { "/ads/*/*/*/*.js", Long("https://cdn.example.com/ads/", "ads/", "x"), false },
        { "/ad*a*a*a*a*zz", Long("https://cdn.example.com/ad", "a", ""), false },
        { "-ad-*x*y*z*q^", Long("https://cdn.example.com/-ad-", "xyz", ""), false },
        { "||cdn.example.com/*a*b*c*d|", Long("https://cdn.example.com/", "ab", "c"), false },
        // And the same shapes where the address does match, late.
        { "/ads/*/*/*/*.js", Long("https://cdn.example.com/ads/", "ads/", "x.js"), true },
        { "/ad*a*a*a*a*zz", Long("https://cdn.example.com/ad", "a", "zz"), true },
    };

    [Theory]
    [MemberData(nameof(Cases))]
    public async Task Interior_wildcards_against_a_16_KB_address_take_under_a_millisecond(string rule, string address, bool blocked)
    {
        var list = FilterList.Compile([rule]);
        var url = new Uri(address);
        Assert.True(url.AbsoluteUri.Length >= 16 * 1024);

        // A backtracking matcher doesn't finish at all here; don't wait for it.
        var first = Task.Run(() => list.ShouldBlock(url, "page.com", ResourceKind.Script));
        Assert.True(await Finishes(first), $"{rule} still running after 5 s");
        Assert.Equal(blocked, await first);

        const int runs = 50;
        var clock = Stopwatch.StartNew();
        for (var i = 0; i < runs; i++) list.ShouldBlock(url, "page.com", ResourceKind.Script);
        clock.Stop();
        var each = clock.Elapsed.TotalMilliseconds / runs;
        output.WriteLine($"{rule}: {each * 1000:N1} µs a request ({url.AbsoluteUri.Length:N0} characters)");
        Assert.True(each < 1.0, $"{rule} took {each:N3} ms a request; budget is 1 ms.");
    }

    // A word that comes up thousands of times in one address costs its
    // rules one test, not one per time it comes up.
    [Fact]
    public async Task A_repeated_word_tests_its_rules_once()
    {
        var rules = Enumerable.Range(0, 200).Select(i => $"/ads/*/*/*/r{i}.js").ToArray();
        var list = FilterList.Compile(rules);
        var url = new Uri(Long("https://cdn.example.com/ads/", "ads/", "x"));

        var first = Task.Run(() => list.ShouldBlock(url, "page.com", ResourceKind.Script));
        Assert.True(await Finishes(first), "still running after 5 s");
        Assert.False(await first);

        var clock = Stopwatch.StartNew();
        for (var i = 0; i < 10; i++) list.ShouldBlock(url, "page.com", ResourceKind.Script);
        clock.Stop();
        var each = clock.Elapsed.TotalMilliseconds / 10;
        output.WriteLine($"200 rules, word repeated 4,000 times: {each:N2} ms a request");
        Assert.True(each < 5.0, $"took {each:N2} ms a request");
    }
}
