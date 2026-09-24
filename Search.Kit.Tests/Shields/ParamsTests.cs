using SearchKit.Shields;

namespace SearchKit.Tests.Shields;

public class ParamsTests
{
    [Theory]
    [InlineData("https://example.com/?utm_source=x&utm_medium=y&id=1", "https://example.com/?id=1")]
    [InlineData("https://example.com/?fbclid=abc", "https://example.com/")]
    [InlineData("https://example.com/?gclid=1&gclsrc=aw", "https://example.com/")]
    [InlineData("https://example.com/?a=1&mc_eid=x&b=2", "https://example.com/?a=1&b=2")]
    public void Strips_known_trackers(string input, string expected)
    {
        var cleaned = Params.Clean(new Uri(input));
        Assert.NotNull(cleaned);
        Assert.Equal(expected, cleaned!.ToString());
    }

    [Theory]
    [InlineData("https://example.com/")]
    [InlineData("https://example.com/?id=1")]
    [InlineData("https://example.com/?a=1&b=2")]
    public void Leaves_urls_without_trackers_alone(string input) =>
        Assert.Null(Params.Clean(new Uri(input)));

    [Fact]
    public void A_utm_prefixed_name_of_any_kind_is_stripped()
    {
        var cleaned = Params.Clean(new Uri("https://example.com/?utm_whatever_new=1&keep=1"));
        Assert.Equal("https://example.com/?keep=1", cleaned!.ToString());
    }

    [Fact]
    public void Ref_src_is_stripped_only_on_twitter_and_x()
    {
        Assert.Equal("https://twitter.com/a/status/1", Params.Clean(new Uri("https://twitter.com/a/status/1?ref_src=twsrc"))!.ToString());
        Assert.Equal("https://x.com/a/status/1", Params.Clean(new Uri("https://x.com/a/status/1?ref_src=twsrc"))!.ToString());
        Assert.Null(Params.Clean(new Uri("https://example.com/page?ref_src=my-own-tracking")));
    }

    [Fact]
    public void Si_is_stripped_only_on_youtube()
    {
        Assert.Equal("https://youtu.be/abc123", Params.Clean(new Uri("https://youtu.be/abc123?si=xyz"))!.ToString());
        Assert.Equal("https://www.youtube.com/watch?v=abc", Params.Clean(new Uri("https://www.youtube.com/watch?v=abc&si=xyz"))!.ToString());
        // A generic site using "si" for something of its own is left alone.
        Assert.Null(Params.Clean(new Uri("https://example.com/search?si=session-id")));
    }

    [Fact]
    public void Every_rule_has_a_reason() =>
        Assert.All(Params.Rules, r => Assert.False(string.IsNullOrWhiteSpace(r.Why)));

    [Fact]
    public void A_bare_question_mark_with_no_query_is_left_alone() =>
        Assert.Null(Params.Clean(new Uri("https://example.com/?")));

    [Fact]
    public void Fragment_and_path_survive_cleaning()
    {
        var cleaned = Params.Clean(new Uri("https://example.com/page?utm_source=x#section"));
        Assert.Equal("https://example.com/page#section", cleaned!.ToString());
    }
}
