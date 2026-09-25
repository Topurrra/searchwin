using SearchKit.Shields;

namespace SearchKit.Tests.Shields;

public class CosmeticTests
{
    [Fact]
    public void A_generic_rule_applies_to_every_host()
    {
        var list = FilterList.Compile(["##.ad-slot"]);
        Assert.Equal(".ad-slot { display: none !important; }", list.CssFor("example.com"));
        Assert.Equal(".ad-slot { display: none !important; }", list.CssFor("other.org"));
    }

    [Fact]
    public void A_host_scoped_rule_only_applies_there_and_its_subdomains()
    {
        var list = FilterList.Compile(["example.com##.banner"]);
        Assert.Equal(".banner { display: none !important; }", list.CssFor("example.com"));
        Assert.Equal(".banner { display: none !important; }", list.CssFor("shop.example.com"));
        Assert.Equal("", list.CssFor("other.org"));
    }

    [Fact]
    public void A_generic_exception_cancels_a_generic_selector_everywhere()
    {
        var list = FilterList.Compile(["##.ad-slot", "#@#.ad-slot"]);
        Assert.Equal("", list.CssFor("example.com"));
        Assert.Equal("", list.CssFor("other.org"));
    }

    [Fact]
    public void A_host_scoped_exception_only_cancels_it_there()
    {
        var list = FilterList.Compile(["##.ad-slot", "example.com#@#.ad-slot"]);
        Assert.Equal("", list.CssFor("example.com"));
        Assert.Equal(".ad-slot { display: none !important; }", list.CssFor("other.org"));
    }

    [Fact]
    public void A_negated_domain_excludes_one_site_from_a_multi_site_rule()
    {
        var list = FilterList.Compile(["a.com,~b.a.com##.thing"]);
        Assert.Equal(".thing { display: none !important; }", list.CssFor("a.com"));
        Assert.Equal("", list.CssFor("b.a.com"));
    }

    [Fact]
    public void Multiple_selectors_combine_into_one_stylesheet()
    {
        var list = FilterList.Compile(["example.com##.one", "example.com##.two"]);
        Assert.Equal(".one, .two { display: none !important; }", list.CssFor("example.com"));
    }

    [Fact]
    public void No_matching_rule_is_an_empty_stylesheet() =>
        Assert.Equal("", FilterList.Compile(["example.com##.x"]).CssFor("unrelated.org"));

    // CssFor is the whole set in one rule, so one selector that closes the
    // rule would let a list write any CSS it likes into the page.
    [Theory]
    [InlineData("##.x}body{display:none")]
    [InlineData("example.com##.x{}html{visibility:hidden}")]
    [InlineData("##.x;color:red")]
    [InlineData("##.x/*")]
    [InlineData("##div:has-text(Sponsored)")]
    [InlineData("##@import url(x)")]
    public void Unsafe_selectors_never_reach_the_stylesheet(string line)
    {
        var list = FilterList.Compile(["##.ad-slot", line]);
        Assert.Equal(".ad-slot { display: none !important; }", list.CssFor("example.com"));
        Assert.Equal("", FilterList.Compile([line]).CssFor("example.com"));
    }

    [Fact]
    public void A_trailing_dot_on_the_host_is_the_same_site()
    {
        var list = FilterList.Compile(["example.com##.banner", "@@||quiet.example^$generichide", "##.ad"]);
        Assert.Equal(list.CssFor("example.com"), list.CssFor("example.com."));
        Assert.Equal(list.SiteCss("example.com"), list.SiteCss("example.com."));
        Assert.False(list.HidesGenerics("quiet.example."));
    }

    [Theory]
    [InlineData("example.com##+js(set-constant, x, true)")]
    [InlineData("example.com#?#.ad:has(> img)")]
    [InlineData("example.com#$#hide-if-contains 'Advertisement' div")]
    public void Scriptlets_and_procedural_filters_are_skipped_and_counted(string line)
    {
        var stats = new FilterStats();
        var network = new List<NetworkRule>();
        var cosmetic = new List<CosmeticRule>();
        FilterParser.ParseLine(line, network, cosmetic, stats);
        Assert.Empty(cosmetic);
        Assert.Equal(1, stats.SkippedUnsupported);
    }
}
