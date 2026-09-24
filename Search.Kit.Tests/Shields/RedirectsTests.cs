using SearchKit.Shields;

namespace SearchKit.Tests.Shields;

public class RedirectsTests
{
    [Theory]
    [InlineData("https://www.google.com/url?q=https://example.com/page&sa=U", "https://example.com/page")]
    [InlineData("https://www.google.co.uk/url?url=https://example.com/&sa=t", "https://example.com/")]
    [InlineData("https://l.facebook.com/l.php?u=https%3A%2F%2Fexample.com%2Fx&h=abc", "https://example.com/x")]
    [InlineData("https://lm.facebook.com/l.php?u=https://example.com/y", "https://example.com/y")]
    [InlineData("https://l.instagram.com/?u=https%3A%2F%2Fexample.com%2Fz", "https://example.com/z")]
    [InlineData("https://www.youtube.com/redirect?q=https%3A%2F%2Fexample.com%2Fvid", "https://example.com/vid")]
    [InlineData("https://out.reddit.com/t3_abc?url=https%3A%2F%2Fexample.com%2Fpost", "https://example.com/post")]
    [InlineData("https://steamcommunity.com/linkfilter/?url=https%3A%2F%2Fexample.com%2Fitem", "https://example.com/item")]
    [InlineData("https://example.slack.com/redir?url=https%3A%2F%2Fexample.com%2Fdoc", "https://example.com/doc")]
    [InlineData("https://disq.us/url?url=https%3A%2F%2Fexample.com%2Fcomment&key=x", "https://example.com/comment")]
    [InlineData("http://href.li/?https://example.com/raw", "https://example.com/raw")]
    public void Unwraps_known_redirectors(string input, string expected)
    {
        var unwrapped = Redirects.Unwrap(new Uri(input));
        Assert.NotNull(unwrapped);
        Assert.Equal(expected, unwrapped!.ToString());
    }

    [Theory]
    [InlineData("https://example.com/")]                                   // not a redirector at all
    [InlineData("https://google.com/search?q=cats")]                       // right host, wrong path
    [InlineData("https://notgoogle.com/url?q=https://example.com/")]       // similarly-named host, not Google's
    [InlineData("https://l.facebook.com/l.php?u=/relative/path")]          // target isn't absolute http(s)
    [InlineData("https://l.facebook.com/l.php?u=javascript:alert(1)")]     // target isn't http(s)
    public void Leaves_everything_else_alone(string input) =>
        Assert.Null(Redirects.Unwrap(new Uri(input)));

    [Fact]
    public void A_google_link_with_no_query_is_left_alone() =>
        Assert.Null(Redirects.Unwrap(new Uri("https://www.google.com/url")));
}
