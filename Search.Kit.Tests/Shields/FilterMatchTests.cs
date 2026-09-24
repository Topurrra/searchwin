using SearchKit.Shields;

namespace SearchKit.Tests.Shields;

public class FilterMatchTests
{
    private static FilterList Compile(params string[] lines) => FilterList.Compile(lines);

    [Fact]
    public void Domain_anchor_blocks_the_domain_and_its_subdomains()
    {
        var list = Compile("||ads.example.com^");
        Assert.True(list.ShouldBlock(new Uri("https://ads.example.com/x.js"), "page.com", ResourceKind.Script));
        Assert.True(list.ShouldBlock(new Uri("https://sub.ads.example.com/x.js"), "page.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://notads.example.com/x.js"), "page.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://example.com/ads.example.com"), "page.com", ResourceKind.Script));
    }

    [Fact]
    public void Domain_anchor_with_a_path_only_blocks_that_path()
    {
        var list = Compile("||example.com/ads/*");
        Assert.True(list.ShouldBlock(new Uri("https://example.com/ads/banner.js"), null, ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://example.com/content/banner.js"), null, ResourceKind.Script));
    }

    [Fact]
    public void Wildcard_and_separator_match()
    {
        var list = Compile("/ads-*.js^");
        Assert.True(list.ShouldBlock(new Uri("https://cdn.example.com/ads-1234.js?x=1"), null, ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://cdn.example.com/adsx1234.jsx"), null, ResourceKind.Script));
    }

    [Fact]
    public void Start_and_end_anchors()
    {
        var list = Compile("|http://insecure.example.com/tracker.js|");
        Assert.True(list.ShouldBlock(new Uri("http://insecure.example.com/tracker.js"), null, ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("http://insecure.example.com/tracker.js.map"), null, ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("http://cdn.com/http://insecure.example.com/tracker.js"), null, ResourceKind.Script));
    }

    [Fact]
    public void An_exception_overrides_a_block()
    {
        var list = Compile("||example.com^", "@@||example.com/ok.js^");
        Assert.True(list.ShouldBlock(new Uri("https://example.com/bad.js"), "page.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://example.com/ok.js"), "page.com", ResourceKind.Script));
    }

    [Fact]
    public void Important_overrides_an_exception()
    {
        var list = Compile("@@||example.com^", "||example.com/bad.js^$important");
        Assert.False(list.ShouldBlock(new Uri("https://example.com/fine.js"), "page.com", ResourceKind.Script));
        Assert.True(list.ShouldBlock(new Uri("https://example.com/bad.js"), "page.com", ResourceKind.Script));
    }

    [Theory]
    [InlineData(ResourceKind.Script, true)]
    [InlineData(ResourceKind.Image, false)]
    public void Type_option_narrows_the_rule(ResourceKind kind, bool expected)
    {
        var list = Compile("||example.com^$script");
        Assert.Equal(expected, list.ShouldBlock(new Uri("https://example.com/x"), "page.com", kind));
    }

    [Fact]
    public void Negated_type_option_blocks_everything_else()
    {
        var list = Compile("||example.com^$~image");
        Assert.True(list.ShouldBlock(new Uri("https://example.com/x"), "page.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://example.com/x"), "page.com", ResourceKind.Image));
    }

    [Fact]
    public void Third_party_option()
    {
        var list = Compile("||tracker.com^$third-party");
        Assert.True(list.ShouldBlock(new Uri("https://tracker.com/x"), "page.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://tracker.com/x"), "tracker.com", ResourceKind.Script));
    }

    [Fact]
    public void Not_third_party_option()
    {
        var list = Compile("||cdn.com^$~third-party");
        Assert.False(list.ShouldBlock(new Uri("https://cdn.com/x"), "page.com", ResourceKind.Script));
        Assert.True(list.ShouldBlock(new Uri("https://cdn.com/x"), "cdn.com", ResourceKind.Script));
    }

    [Fact]
    public void No_page_host_is_never_third_party()
    {
        var list = Compile("||tracker.com^$third-party");
        Assert.False(list.ShouldBlock(new Uri("https://tracker.com/x"), null, ResourceKind.Script));
    }

    [Fact]
    public void Domain_include_option_only_fires_on_listed_pages()
    {
        var list = Compile("||cdn.com^$domain=good.com");
        Assert.True(list.ShouldBlock(new Uri("https://cdn.com/x"), "good.com", ResourceKind.Script));
        Assert.True(list.ShouldBlock(new Uri("https://cdn.com/x"), "sub.good.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://cdn.com/x"), "other.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://cdn.com/x"), null, ResourceKind.Script));
    }

    [Fact]
    public void Domain_exclude_option_never_fires_on_listed_pages()
    {
        var list = Compile("||cdn.com^$domain=~safe.com");
        Assert.False(list.ShouldBlock(new Uri("https://cdn.com/x"), "safe.com", ResourceKind.Script));
        Assert.True(list.ShouldBlock(new Uri("https://cdn.com/x"), "other.com", ResourceKind.Script));
    }

    [Fact]
    public void Mixed_domain_option_includes_and_excludes()
    {
        var list = Compile("||cdn.com^$domain=good.com|~bad.sub.good.com");
        Assert.True(list.ShouldBlock(new Uri("https://cdn.com/x"), "good.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://cdn.com/x"), "bad.sub.good.com", ResourceKind.Script));
    }

    [Fact]
    public void Non_http_requests_are_never_blocked()
    {
        var list = Compile("||example.com^");
        Assert.False(list.ShouldBlock(new Uri("data:text/plain,example.com"), null, ResourceKind.Other));
    }

    [Theory]
    [InlineData("$redirect=noop.js")]
    [InlineData("$csp=default-src 'none'")]
    [InlineData("$removeparam=utm_source")]
    [InlineData("$popup")]
    public void Unsupported_options_are_skipped_and_counted(string options)
    {
        var stats = new FilterStats();
        var network = new List<NetworkRule>();
        var cosmetic = new List<CosmeticRule>();
        FilterParser.ParseLine("||example.com^" + options, network, cosmetic, stats);
        Assert.Empty(network);
        Assert.Equal(1, stats.SkippedUnsupported);
    }

    [Fact]
    public void Comments_and_blank_lines_are_counted_not_dropped_silently()
    {
        var list = FilterList.Compile(["! a comment", "", "[Adblock Plus 2.0]", "||example.com^"]);
        Assert.Equal(2, list.Stats.Comments);
        Assert.Equal(1, list.Stats.Blank);
        Assert.Equal(1, list.Stats.NetworkRules);
    }

    [Fact]
    public void Save_and_load_round_trips_matching_behaviour()
    {
        var original = FilterList.Compile([
            "||ads.example.com^$script,third-party",
            "@@||ads.example.com/ok.js^",
            "example.com##.banner-ad",
            "#@#.never-hide-this",
        ]);

        using var stream = new MemoryStream();
        original.Save(stream);
        stream.Position = 0;
        var loaded = FilterList.Load(stream);

        Assert.Equal(original.Stats.NetworkRules, loaded.Stats.NetworkRules);
        Assert.Equal(original.Stats.CosmeticRules, loaded.Stats.CosmeticRules);

        var url = new Uri("https://ads.example.com/x.js");
        Assert.Equal(original.ShouldBlock(url, "page.com", ResourceKind.Script), loaded.ShouldBlock(url, "page.com", ResourceKind.Script));
        Assert.Equal(original.CssFor("example.com"), loaded.CssFor("example.com"));
    }
}
