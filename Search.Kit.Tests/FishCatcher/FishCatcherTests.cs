using System.Diagnostics;
using System.Text.Json;
using SearchKit.FishCatcher;
using Xunit.Abstractions;
using Fish = SearchKit.FishCatcher.FishCatcher;

namespace SearchKit.Tests;

public class FishCatcherTests(ITestOutputHelper output)
{
    private static FishData Data => ParityTests.Data.Value;

    [Fact]
    public async Task Loads_lazily_and_fails_open_until_then()
    {
        // Nothing is loaded by merely touching the type; the first check starts it.
        var url = new Uri("http://paypal-login-verify.com/");
        var first = Fish.Check(url);
        if (!Fish.IsReady) Assert.Null(first);
        var verdict = await Fish.CheckAsync(url);
        Assert.True(Fish.IsReady);
        Assert.Null(Fish.LoadError);
        Assert.NotNull(verdict);
        Assert.Equal(Level.Critical, verdict.Level);
        Assert.True(verdict.Warns);
        Assert.Equal(verdict.Score, Fish.Check(url)!.Score);
        Assert.Null(Fish.Check(new Uri("ftp://example.com/")));
        Assert.Null(Fish.Check("not an address"));
    }

    [Fact]
    public void Reasons_read_as_sentences()
    {
        var v = Analyzer.Analyze("http://pyapal.com/login", Data)!;
        Assert.Equal(Level.High, v.Level);
        Assert.Equal(["Domain looks like a misspelling of paypal.com", "Connection is not encrypted (http instead of https)"], v.Reasons);
        Assert.Equal("paypal.com", v.RealSite);
        Assert.Equal("Multiple phishing indicators. Be careful before logging in", v.Summary);

        var brand = Analyzer.Analyze("https://paypa1-secure.net/", Data)!;
        Assert.Equal("Mentions PayPal but is not a PayPal domain", brand.Reasons[0]);
        Assert.Equal("Contains a word often used in phishing pages (secure)", brand.Reasons[1]);

        var clean = Analyzer.Analyze("https://www.wikipedia.org/", Data)!;
        Assert.Equal(Level.Low, clean.Level);
        Assert.Empty(clean.Reasons);
        Assert.False(clean.Warns);
    }

    [Fact]
    public void Trusted_sites_are_never_scored()
    {
        var trusting = Data.WithTrusted(["paypa1-secure.net"]);
        var v = Analyzer.Analyze("http://paypa1-secure.net/login", trusting)!;
        Assert.Equal(0, v.Score);
        Assert.True(v.Trusted);
    }

    [Fact]
    public void Page_facts_parse_from_the_probe_json()
    {
        var facts = PageFacts.Parse("""
            {"aitm":{"interactions":["password"],"identityHints":{"title":"Sign in to your Microsoft account","ogSiteName":"","logoAlts":[],"brandTokens":[]},
                     "resourceHosts":{"evil-proxy.com":10},"faviconCrossOrigin":true,"formActions":["api.telegram.org"]},
             "scam":{"cryptoSeed":false,"seedInput":false,"techScare":false,"phone":false,"fullscreen":false}}
            """);
        Assert.NotNull(facts);
        Assert.True(facts.HasPasswordForm);
        var v = Analyzer.Analyze("https://evil-proxy.com/login", Data, facts)!;
        Assert.Equal(100, v.Score);
        Assert.Contains(v.Signals, s => s.Key == "reasonAitmMismatch" && s.Params[0] == "Microsoft");
        Assert.Contains(v.Signals, s => s.Key == "reasonFormExfil");
        Assert.Null(PageFacts.Parse("not json"));
        Assert.Null(PageFacts.Parse("[]"));
    }

    [Theory]
    [InlineData("http://999.1.1.1/")]       // not an IPv4 address to WHATWG: no verdict, as in the extension
    [InlineData("http://example.1/")]
    [InlineData("http://1.2.3.4.5/")]
    [InlineData("https://xn--zz.com/")]     // bad Punycode: the extension's engine threw
    [InlineData("javascript:alert(1)")]
    [InlineData("")]
    public void No_verdict(string url) => Assert.Null(Analyzer.Analyze(url, Data));

    [Theory]
    [InlineData("http://[::ffff:1.2.3.4]/", "[::ffff:102:304]")]
    [InlineData("http://[2001:db8:0:0:1:0:0:1]/", "[2001:db8::1:0:0:1]")]
    [InlineData("http://0x7f.1/", "127.0.0.1")]
    [InlineData("http://3232235777/", "192.168.1.1")]
    [InlineData("https://paypal.com%2eevil.com/", "paypal.com.evil.com")]
    [InlineData("https://ｇｏｏｇｌｅ.com/", "google.com")]
    [InlineData("https://аpple.com/", "xn--pple-43d.com")]
    public void Hosts_read_as_a_browser_reads_them(string url, string host) =>
        Assert.Equal(host, WebAddress.From(url)!.Value.Host);

    [Theory]
    [InlineData(0.0, "0")]
    [InlineData(-0.0, "0")]
    [InlineData(1.5, "1.5")]
    [InlineData(1e21, "1e+21")]
    [InlineData(1e20, "100000000000000000000")]
    [InlineData(1e-7, "1e-7")]
    [InlineData(1e-6, "0.000001")]
    [InlineData(123e-20, "1.23e-18")]
    [InlineData(-2.5e-10, "-2.5e-10")]
    [InlineData(0.1 + 0.2, "0.30000000000000004")]
    public void Numbers_print_as_in_JavaScript(double value, string text) => Assert.Equal(text, JsJson.Number(value));

    [Fact]
    public void A_check_takes_well_under_two_milliseconds()
    {
        var golden = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "FishCatcher", "golden.json")));
        var urls = golden.RootElement.GetProperty("urls").EnumerateArray().Select(u => u.GetProperty("url").GetString()!).ToArray();

        var load = Stopwatch.StartNew();
        var data = FishData.LoadBundled();
        load.Stop();

        foreach (var url in urls) Analyzer.Analyze(url, data); // warm up (JIT)
        var times = new double[urls.Length];
        var clock = new Stopwatch();
        for (int i = 0; i < urls.Length; i++)
        {
            clock.Restart();
            Analyzer.Analyze(urls[i], data);
            times[i] = clock.Elapsed.TotalMilliseconds;
        }
        Array.Sort(times);
        double mean = times.Average(), p99 = times[(int)(times.Length * 0.99)], max = times[^1];
        output.WriteLine($"load {load.Elapsed.TotalMilliseconds:F1} ms; per address over {urls.Length}: mean {mean * 1000:F0} µs, p99 {p99 * 1000:F0} µs, max {max * 1000:F0} µs");
        Assert.True(mean < 2, $"mean {mean} ms");
        Assert.True(p99 < 2, $"p99 {p99} ms");
    }
}
