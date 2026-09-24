using SearchKit.Shields;

namespace SearchKit.Tests.Shields;

/// The browser's two halves of a site's stylesheet: `GenericCss`, the same
/// for every page, and `SiteCss`, what one site adds or can't have. Between
/// them they must hide exactly what `CssFor` says.
public class CosmeticSplitTests
{
    [Fact]
    public void An_unconditional_generic_rule_goes_in_the_shared_sheet_only()
    {
        var list = FilterList.Compile(["##.ad-slot"]);
        Assert.Equal(".ad-slot{display:none!important}\n", list.GenericCss);
        Assert.Equal("", list.SiteCss("example.com"));
    }

    [Fact]
    public void A_generic_rule_some_site_excepts_is_decided_per_site()
    {
        var list = FilterList.Compile(["##.ad-slot", "example.com#@#.ad-slot"]);
        Assert.Equal("", list.GenericCss);
        Assert.Equal("", list.SiteCss("example.com"));
        Assert.Equal(".ad-slot { display: none !important; }", list.SiteCss("other.org"));
    }

    [Fact]
    public void A_generic_rule_with_an_excluded_site_is_decided_per_site()
    {
        var list = FilterList.Compile(["~example.com##.promo"]);
        Assert.Equal("", list.GenericCss);
        Assert.Equal("", list.SiteCss("www.example.com"));
        Assert.Equal(".promo { display: none !important; }", list.SiteCss("other.org"));
    }

    [Fact]
    public void Site_rules_come_one_selector_to_a_rule()
    {
        var list = FilterList.Compile(["example.com##.one", "example.com##.two"]);
        Assert.Equal(".one { display: none !important; }\n.two { display: none !important; }", list.SiteCss("example.com"));
    }

    [Fact]
    public void A_site_rule_repeating_a_generic_one_isnt_sent_twice()
    {
        var list = FilterList.Compile(["##.ad", "example.com##.ad", "example.com##.own"]);
        Assert.Equal(".own { display: none !important; }", list.SiteCss("example.com"));
    }

    [Fact]
    public void Generichide_turns_the_shared_sheet_off_but_keeps_the_sites_own_rules()
    {
        var list = FilterList.Compile(["##.ad", "~other.org##.promo", "example.com##.own", "@@||example.com^$generichide"]);
        Assert.False(list.HidesGenerics("www.example.com"));
        Assert.True(list.HidesGenerics("other.org"));
        Assert.Contains("example.com", list.GenericHideSites);
        Assert.Equal(".own { display: none !important; }", list.SiteCss("www.example.com"));
        Assert.Equal(".own { display: none !important; }", list.CssFor("www.example.com"));
    }

    [Fact]
    public void Generichide_with_other_options_is_skipped_and_counted()
    {
        var list = FilterList.Compile(["@@||example.com^$generichide,script"]);
        Assert.True(list.HidesGenerics("example.com"));
        Assert.Equal(1, list.Stats.SkippedUnsupported);
    }

    [Theory]
    [InlineData("##.ad-box", true)]
    [InlineData("##div[id^=\"ad-\"] > .slot", true)]
    [InlineData("##.ad:has(> img)", true)]
    [InlineData("##.ad:-abp-has(.x)", false)]
    [InlineData("##.ad:has-text(Sponsored)", false)]
    [InlineData("##.x} body { background: url(//evil.example/) ", false)]
    [InlineData("##.a;b", false)]
    [InlineData("##.a/* c */", false)]
    public void Only_selectors_a_stylesheet_can_hold_are_sent(string line, bool sent)
    {
        var list = FilterList.Compile([line]);
        Assert.Equal(sent, list.GenericCss.Length > 0);
    }

    [Fact]
    public void The_shared_sheet_comes_a_hundred_selectors_to_a_rule()
    {
        var list = FilterList.Compile(Enumerable.Range(0, 250).Select(i => $"##.ad-{i:D3}"));
        var rules = list.GenericCss.Split('\n', StringSplitOptions.RemoveEmptyEntries);
        Assert.Equal(3, rules.Length);
        Assert.All(rules, r => Assert.EndsWith("{display:none!important}", r));
        Assert.Equal(100, rules[0].Split(',').Length);
        Assert.Equal(50, rules[2].Split(',').Length);
    }

    [Fact]
    public void Shared_and_site_sheets_together_hide_what_CssFor_hides()
    {
        string[] lines =
        [
            "##.a", "##.b", "~quiet.org##.c", "#@#.b", "loud.com##.d", "loud.com#@#.a", "##.e", "quiet.org##.e",
        ];
        var list = FilterList.Compile(lines);
        foreach (var host in new[] { "loud.com", "quiet.org", "www.quiet.org", "plain.net" })
        {
            var whole = Selectors(list.CssFor(host));
            var split = Selectors(list.GenericCss).Union(Selectors(list.SiteCss(host))).ToHashSet();
            Assert.True(whole.SetEquals(split), $"{host}: [{string.Join(' ', whole)}] vs [{string.Join(' ', split)}]");
        }

        static HashSet<string> Selectors(string css) =>
            css.Split(['\n', ','], StringSplitOptions.RemoveEmptyEntries)
                .Select(s => s.Split('{')[0].Trim())
                .Where(s => s.Length > 0)
                .ToHashSet();
    }

    [Fact]
    public void A_plain_domain_block_still_yields_to_an_exception()
    {
        var list = FilterList.Compile(["||ads.example^", "@@||ads.example/ok.js$script", "||sub.cdn.example^$third-party"]);
        Assert.True(list.ShouldBlock(new Uri("https://x.ads.example/banner.png"), "news.org", ResourceKind.Image));
        Assert.False(list.ShouldBlock(new Uri("https://ads.example/ok.js"), "news.org", ResourceKind.Script));
        Assert.True(list.ShouldBlock(new Uri("https://ads.example/ok.js"), "news.org", ResourceKind.Image));
        Assert.True(list.ShouldBlock(new Uri("https://sub.cdn.example/a.js"), "news.org", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://sub.cdn.example/a.js"), "www.cdn.example", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://notads.example/x.png"), "news.org", ResourceKind.Image));
    }

    [Fact]
    public void The_split_survives_save_and_load()
    {
        var list = FilterList.Compile(["##.ad", "~x.com##.p", "a.com##.own", "@@||b.com^$generichide", "||tracker.example^", "||cdn.example^$third-party"]);
        using var stream = new MemoryStream();
        list.Save(stream);
        stream.Position = 0;
        var back = FilterList.Load(stream);
        Assert.Equal(list.GenericCss, back.GenericCss);
        Assert.Equal(list.SiteCss("a.com"), back.SiteCss("a.com"));
        Assert.False(back.HidesGenerics("www.b.com"));
        Assert.True(back.ShouldBlock(new Uri("https://px.tracker.example/p.gif"), "news.org", ResourceKind.Image));
        Assert.True(back.ShouldBlock(new Uri("https://cdn.example/lib.js"), "news.org", ResourceKind.Script));
        Assert.False(back.ShouldBlock(new Uri("https://cdn.example/lib.js"), "cdn.example", ResourceKind.Script));
    }
}
