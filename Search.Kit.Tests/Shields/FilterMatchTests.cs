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

    // A `||` line whose name isn't one the host dictionary can hold (an
    // underscore, no dot, a wildcard) still means "starting at the host, or
    // at one of its dots" — never a literal `|` that no address contains.
    [Theory]
    [InlineData("||ad_server.example.com^", "https://ad_server.example.com/x.js", true)]
    [InlineData("||ad_server.example.com^", "https://cdn.ad_server.example.com/x.js", true)]
    [InlineData("||ad_server.example.com^", "https://bad_server.example.com/x.js", false)]
    [InlineData("||ad_server.example.com^", "https://ad_server.example.com.evil.net/x.js", false)]
    [InlineData("||ad_server.example.com^", "https://site.com/?u=ad_server.example.com/", false)]
    [InlineData("||adhost/banner", "https://adhost/banner.png", true)]
    [InlineData("||adhost/banner", "https://cdn.adhost/banner.png", true)]
    [InlineData("||adhost/banner", "https://myadhost/banner.png", false)]
    [InlineData("||*.tracker.example/px", "https://a.tracker.example/px.gif", true)]
    [InlineData("||*.tracker.example/px", "https://site.com/px.gif", false)]
    // The shape the real lists have (17 lines of EasyList + EasyPrivacy).
    [InlineData("||collector-*.luigisbox.com^", "https://collector-12.luigisbox.com/v1/t", true)]
    [InlineData("||collector-*.luigisbox.com^", "https://www.luigisbox.com/", false)]
    // A name ending in a dot is a prefix of names (125 real lines).
    [InlineData("||adservice.google.", "https://adservice.google.com/x", true)]
    [InlineData("||adservice.google.", "https://adservice.google.co.uk/x", true)]
    [InlineData("||adservice.google.", "https://notadservice.google.com/x", false)]
    [InlineData("||google.*/pagead/lvz?", "https://www.google.com/pagead/lvz?x=1", true)]
    [InlineData("||google.*/pagead/lvz?", "https://www.google.de/pagead/lvz?x=1", true)]
    [InlineData("||google.*/pagead/lvz?", "https://www.google.com/search?q=1", false)]
    [InlineData("||142.91.159.", "http://142.91.159.12/a", true)]
    [InlineData("||142.91.159.", "http://142.91.15.9/a", false)]
    public void A_double_bar_rule_with_an_odd_name_is_anchored_at_the_host(string rule, string url, bool blocked)
    {
        var list = Compile(rule);
        Assert.Equal(1, list.Stats.NetworkRules);
        Assert.Equal(blocked, list.ShouldBlock(new Uri(url), "page.com", ResourceKind.Script));

        // And the same after a save and load.
        using var stream = new MemoryStream();
        list.Save(stream);
        stream.Position = 0;
        Assert.Equal(blocked, FilterList.Load(stream).ShouldBlock(new Uri(url), "page.com", ResourceKind.Script));
    }

    [Fact]
    public void An_exception_with_an_odd_name_still_excepts()
    {
        var list = FilterList.Compile(["/banner/*", "@@||ad_server.example.com/banner/"]);
        Assert.False(list.ShouldBlock(new Uri("https://ad_server.example.com/banner/1.png"), "page.com", ResourceKind.Image));
        Assert.True(list.ShouldBlock(new Uri("https://other.example.com/banner/1.png"), "page.com", ResourceKind.Image));
    }

    // EasyList's own shape: an exception for Google's search requests on
    // any Google country site.
    [Fact]
    public void An_exception_whose_name_ends_in_a_dot_still_excepts()
    {
        var list = FilterList.Compile(["/search?$xmlhttprequest", "@@||www.google.*/search?$xmlhttprequest"]);
        Assert.False(list.ShouldBlock(new Uri("https://www.google.de/search?q=x"), "www.google.de", ResourceKind.XmlHttpRequest));
        Assert.True(list.ShouldBlock(new Uri("https://other.example/search?q=x"), "other.example", ResourceKind.XmlHttpRequest));
    }

    // MARK: - the token index

    // A rule's word is only a whole word of the address when the rule says
    // so: `banner` with nothing around it is a piece of text, and matches
    // inside "banners" just as a plain pattern test would.
    [Fact]
    public void A_bare_word_matches_inside_a_longer_one()
    {
        var list = Compile("banner");
        Assert.True(list.ShouldBlock(new Uri("https://cdn.example.com/banners/1.png"), "page.com", ResourceKind.Image));
    }

    [Fact]
    public void An_unanchored_name_matches_inside_a_longer_host()
    {
        var list = Compile("adserver");
        Assert.True(list.ShouldBlock(new Uri("https://myadserver.example/x.js"), "page.com", ResourceKind.Script));
    }

    [Fact]
    public void A_word_next_to_a_wildcard_is_not_a_whole_word()
    {
        var list = Compile("-ad-*banner");
        Assert.True(list.ShouldBlock(new Uri("https://cdn.example.com/-ad-bigbanner.png"), "page.com", ResourceKind.Image));
    }

    [Fact]
    public void A_word_between_separators_is_still_a_whole_word()
    {
        var list = Compile("/adframe/*");
        Assert.True(list.ShouldBlock(new Uri("https://cdn.example.com/adframe/x.html"), "page.com", ResourceKind.Subdocument));
        Assert.False(list.ShouldBlock(new Uri("https://cdn.example.com/myadframe/x.html"), "page.com", ResourceKind.Subdocument));
    }

    [Fact]
    public void An_exception_word_inside_a_longer_one_still_excepts()
    {
        var list = Compile("||cdn.example^", "@@banner");
        Assert.False(list.ShouldBlock(new Uri("https://cdn.example/banners/1.png"), "page.com", ResourceKind.Image));
        Assert.True(list.ShouldBlock(new Uri("https://cdn.example/other/1.png"), "page.com", ResourceKind.Image));
    }

    [Fact]
    public void A_pattern_starting_with_a_separator_is_found_anywhere()
    {
        var list = Compile("^adsbygoogle.js");
        Assert.True(list.ShouldBlock(new Uri("https://cdn.example.com/p/adsbygoogle.js"), "page.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://cdn.example.com/p/myadsbygoogle.js"), "page.com", ResourceKind.Script));
    }

    [Fact]
    public void Separator_matches_the_end_of_the_address()
    {
        var list = Compile("/track.js^", "|https://end.example/x^|");
        Assert.True(list.ShouldBlock(new Uri("https://cdn.example.com/track.js"), null, ResourceKind.Script));
        Assert.True(list.ShouldBlock(new Uri("https://end.example/x"), null, ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://end.example/xy"), null, ResourceKind.Script));
    }

    [Fact]
    public void End_anchor_after_wildcards()
    {
        var list = Compile("/ads/*.js|");
        Assert.True(list.ShouldBlock(new Uri("https://cdn.example.com/ads/a.js/b.js"), null, ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://cdn.example.com/ads/a.js?v=1"), null, ResourceKind.Script));
    }

    // MARK: - hosts

    [Fact]
    public void A_trailing_dot_on_the_host_is_the_same_host()
    {
        var list = Compile("||doubleclick.net^", "||tracker.com^$third-party", "||cdn.example^$domain=news.example");
        Assert.True(list.ShouldBlock(new Uri("https://ads.doubleclick.net/x.js"), "page.com", ResourceKind.Script));
        Assert.True(list.ShouldBlock(new Uri("https://ads.doubleclick.net./x.js"), "page.com", ResourceKind.Script));
        Assert.True(list.ShouldBlock(new Uri("https://tracker.com./x.js"), "page.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://tracker.com./x.js"), "tracker.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://tracker.com/x.js"), "tracker.com.", ResourceKind.Script));
        Assert.True(list.ShouldBlock(new Uri("https://cdn.example/x.js"), "news.example.", ResourceKind.Script));
    }

    [Fact]
    public void The_public_suffix_list_tells_sites_apart()
    {
        var psl = new PslRegistrableDomain(SearchKit.Tests.ParityTests.Data.Value.Psl);
        var withPsl = FilterList.Compile(["||bob.github.io^$third-party"], psl);
        var withGuess = FilterList.Compile(["||bob.github.io^$third-party"], SimpleRegistrableDomain.Instance);
        var url = new Uri("https://bob.github.io/track.js");
        Assert.True(withPsl.ShouldBlock(url, "alice.github.io", ResourceKind.Script));
        Assert.False(withPsl.ShouldBlock(url, "bob.github.io", ResourceKind.Script));
        Assert.False(withGuess.ShouldBlock(url, "alice.github.io", ResourceKind.Script));
    }

    // MARK: - options

    [Fact]
    public void An_exception_for_other_requests_is_kept()
    {
        var list = Compile("||x.example^", "@@||x.example/ok.js$script,other");
        Assert.False(list.ShouldBlock(new Uri("https://x.example/ok.js"), "page.com", ResourceKind.Script));
        Assert.False(list.ShouldBlock(new Uri("https://x.example/ok.js"), "page.com", ResourceKind.Other));
        Assert.True(list.ShouldBlock(new Uri("https://x.example/ok.js"), "page.com", ResourceKind.Image));
        Assert.True(list.ShouldBlock(new Uri("https://x.example/bad.js"), "page.com", ResourceKind.Script));
    }

    [Theory]
    [InlineData("other", ResourceKind.Other, ResourceKind.Script)]
    [InlineData("ping", ResourceKind.Other, ResourceKind.Script)]
    [InlineData("beacon", ResourceKind.Other, ResourceKind.Image)]
    [InlineData("websocket", ResourceKind.Other, ResourceKind.Script)]
    [InlineData("object", ResourceKind.Other, ResourceKind.Media)]
    [InlineData("object-subrequest", ResourceKind.Other, ResourceKind.Media)]
    [InlineData("xhr", ResourceKind.XmlHttpRequest, ResourceKind.Script)]
    [InlineData("css", ResourceKind.Stylesheet, ResourceKind.Script)]
    [InlineData("frame", ResourceKind.Subdocument, ResourceKind.Script)]
    public void Type_aliases_narrow_the_rule(string option, ResourceKind hit, ResourceKind miss)
    {
        var list = Compile("||x.example^$" + option);
        Assert.Equal(1, list.Stats.NetworkRules);
        Assert.True(list.ShouldBlock(new Uri("https://x.example/a"), "page.com", hit));
        Assert.False(list.ShouldBlock(new Uri("https://x.example/a"), "page.com", miss));
    }

    [Theory]
    [InlineData("3p", true)]
    [InlineData("~1p", true)]
    [InlineData("1p", false)]
    [InlineData("first-party", false)]
    [InlineData("~first-party", true)]
    public void Party_aliases(string option, bool thirdPartyOnly)
    {
        var list = Compile("||x.example/a.js$" + option);
        Assert.Equal(thirdPartyOnly, list.ShouldBlock(new Uri("https://x.example/a.js"), "page.com", ResourceKind.Script));
        Assert.Equal(!thirdPartyOnly, list.ShouldBlock(new Uri("https://x.example/a.js"), "x.example", ResourceKind.Script));
    }

    [Fact]
    public void An_exception_with_an_option_it_cannot_judge_is_kept_without_it()
    {
        var list = Compile("||x.example^", "@@||x.example/ok.js$script,match-case", "@@||x.example/fine.js$some-future-option");
        Assert.Equal(2, list.Stats.NetworkExceptions);
        Assert.False(list.ShouldBlock(new Uri("https://x.example/ok.js"), "page.com", ResourceKind.Script));
        Assert.True(list.ShouldBlock(new Uri("https://x.example/ok.js"), "page.com", ResourceKind.Image));
        Assert.False(list.ShouldBlock(new Uri("https://x.example/fine.js"), "page.com", ResourceKind.Script));
    }

    [Theory]
    [InlineData("@@||x.example^$elemhide")]
    [InlineData("@@||x.example^$csp")]
    [InlineData("@@||x.example^$document")]
    [InlineData("@@||x.example^$redirect-rule")]
    [InlineData("@@||x.example^$removeparam=utm_source")]
    [InlineData("@@||x.example^$genericblock")]
    [InlineData("@@||x.example^$badfilter")]
    public void An_exception_that_is_about_something_else_is_not_an_allow(string line)
    {
        var list = Compile("||x.example^", line);
        Assert.Equal(1, list.Stats.SkippedUnsupported);
        Assert.True(list.ShouldBlock(new Uri("https://x.example/a.js"), "page.com", ResourceKind.Script));
    }

    [Theory]
    [InlineData("/banner[0-9]+\\.gif/")]
    [InlineData("/^https?:\\/\\/ads\\./$script")]
    [InlineData("@@/^https?:\\/\\/ok\\./$image")]
    [InlineData("/ads\\.js$/$script,third-party")]
    public void Regex_rules_are_counted_as_unsupported(string line)
    {
        var stats = new FilterStats();
        var network = new List<NetworkRule>();
        FilterParser.ParseLine(line, network, [], stats);
        Assert.Empty(network);
        Assert.Equal(1, stats.SkippedUnsupported);
    }

    [Fact]
    public void A_path_that_merely_starts_with_a_slash_is_not_a_regex()
    {
        var list = Compile("/ads/*", "/banner.gif");
        Assert.Equal(2, list.Stats.NetworkRules);
        Assert.True(list.ShouldBlock(new Uri("https://cdn.example/ads/x.js"), null, ResourceKind.Script));
        Assert.True(list.ShouldBlock(new Uri("https://cdn.example/img/banner.gif"), null, ResourceKind.Image));
    }

    [Fact]
    public void A_block_rule_with_an_option_it_cannot_judge_is_still_dropped()
    {
        var list = Compile("||x.example^$match-case", "||y.example^$webrtc");
        Assert.Equal(0, list.Stats.NetworkRules);
        Assert.Equal(2, list.Stats.SkippedUnsupported);
    }
}
