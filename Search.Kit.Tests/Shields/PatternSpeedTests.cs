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
        var url = new Uri(address);
        Assert.True(url.AbsoluteUri.Length >= 16 * 1024);
        Assert.True(NetworkRule.TryParse(rule, out var parsed, out _));
        var lower = url.AbsoluteUri.ToLowerInvariant();
        var afterHost = url.GetLeftPart(UriPartial.Authority).Length;

        // The matcher itself, reading the whole address. A backtracking one
        // doesn't finish at all here; don't wait for it.
        var first = Task.Run(() => parsed!.MatchesUrl(lower, afterHost, url.Host));
        Assert.True(await Finishes(first), $"{rule} still running after 5 s");
        Assert.Equal(blocked, await first);

        const int runs = 50;
        var clock = Stopwatch.StartNew();
        for (var i = 0; i < runs; i++) parsed!.MatchesUrl(lower, afterHost, url.Host);
        clock.Stop();
        var each = clock.Elapsed.TotalMilliseconds / runs;
        output.WriteLine($"{rule}: {each * 1000:N1} µs a match ({url.AbsoluteUri.Length:N0} characters)");
        Assert.True(each < 1.0, $"{rule} took {each:N3} ms a match; budget is 1 ms.");

        // The list reads only the first FilterList.MatchedLength characters
        // of an address, so a match that only starts past them — every one
        // here — isn't looked for.
        var list = FilterList.Compile([rule]);
        Assert.False(list.ShouldBlock(url, "page.com", ResourceKind.Script));
    }

    // An address made of every word the index is filed under: each word
    // opens a bucket, and each rule in it is tested against the whole
    // address. Six thousand words of a 64 KB address was 20+ ms a request
    // on the thread that draws every window; a page can ask for hundreds.
    internal static string AllWords(IEnumerable<string> words, int length)
    {
        var text = new System.Text.StringBuilder("https://cdn.example.com/", length + 64);
        var list = words.ToArray();
        for (var i = 0; text.Length < length; i++) text.Append(list[i % list.Length]).Append('/');
        return text.ToString();
    }

    // The best of a few rounds: other tests run alongside and take the CPU
    // now and then; what's measured is the request's own cost.
    internal static double Each(FilterList list, Uri url, int runs)
    {
        list.ShouldBlock(url, "page.com", ResourceKind.Image);
        var best = double.MaxValue;
        for (var round = 0; round < 5; round++)
        {
            var clock = Stopwatch.StartNew();
            for (var i = 0; i < runs; i++) list.ShouldBlock(url, "page.com", ResourceKind.Image);
            best = Math.Min(best, clock.Elapsed.TotalMilliseconds / runs);
        }
        return best;
    }

    [Fact]
    public void An_address_of_every_indexed_word_costs_no_more_at_64_KB_than_at_4_KB()
    {
        // Every rule is its own word, found early, then has a piece that is
        // nowhere, so it reads on to the end of what it reads. Harsher than
        // the real lists (one rule a word, 6,000 words): unbounded, the 64 KB
        // address costs 6,000 reads of 64 KB. Bounded, it costs what the
        // first 4 KB do.
        var words = Enumerable.Range(0, 6_000).Select(i => $"w{i}q").ToArray();
        var list = FilterList.Compile(words.Select(w => $"/{w}/*zzz^"));
        Assert.Equal(6_000, list.IndexShape.Words);
        var shorter = new Uri(AllWords(words, FilterList.MatchedLength));
        var url = new Uri(AllWords(words, 65_000));
        Assert.True(url.AbsoluteUri.Length >= 65_000);

        var bounded = Each(list, shorter, 20);
        var each = Each(list, url, 20);
        output.WriteLine($"6,000 words: {bounded:N3} ms a request at {shorter.AbsoluteUri.Length:N0} characters, {each:N3} ms at {url.AbsoluteUri.Length:N0}");
        Assert.True(each < 2 * bounded + 0.25, $"took {each:N3} ms a request at 64 KB against {bounded:N3} ms at 4 KB.");
        Assert.True(each < 5.0, $"took {each:N3} ms a request; budget is 5 ms for this list, even unoptimised.");
    }

    // Past the part of an address the list reads, what it holds still
    // counts where it can be checked cheaply: at the start (the host, the
    // path) and at the very end.
    [Fact]
    public void A_very_long_address_is_still_judged_by_its_start_and_its_end()
    {
        var list = FilterList.Compile(["/ads/banner.", ".exe|", "@@/ads/banner.$image"]);
        var tail = new string('a', 60_000);
        var start = new Uri($"https://cdn.example.com/ads/banner.js?{tail}");
        Assert.True(list.ShouldBlock(start, "page.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(start, "page.com", ResourceKind.Image));
        var end = new Uri($"https://cdn.example.com/x?{tail}/setup.exe");
        Assert.True(list.ShouldBlock(end, "page.com", ResourceKind.Other));
        // An end anchor is the address's real end, never where reading stopped.
        var cut = new Uri($"https://cdn.example.com/x?{new string('a', FilterList.MatchedLength - 30)}.exe{tail}");
        Assert.False(list.ShouldBlock(cut, "page.com", ResourceKind.Other));
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
