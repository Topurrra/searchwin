using SearchKit.FishCatcher;
using SearchKit.Shields;

namespace SearchKit.Tests.Shields;

public class RegistrableDomainTests
{
    private static Psl Psl => SearchKit.Tests.ParityTests.Data.Value.Psl;

    [Theory]
    [InlineData("www.bbc.co.uk", "bbc.co.uk")]
    [InlineData("shop.example.com.cn", "example.com.cn")]
    [InlineData("news.site.com.vn", "site.com.vn")]
    [InlineData("a.b.example.co.th", "example.co.th")]
    [InlineData("alice.github.io", "alice.github.io")]
    [InlineData("ads.example.com", "example.com")]
    [InlineData("Ads.Example.COM.", "example.com")]
    [InlineData("example.com", "example.com")]
    [InlineData("localhost", "localhost")]
    [InlineData("192.168.0.1", "192.168.0.1")]
    [InlineData("[::1]", "[::1]")]
    public void The_public_suffix_list_answers(string host, string site) =>
        Assert.Equal(site, new PslRegistrableDomain(Psl).Of(host));

    [Theory]
    [InlineData("")]
    [InlineData(".")]
    public void Nothing_for_no_host(string host) => Assert.Null(new PslRegistrableDomain(Psl).Of(host));

    [Fact]
    public void Two_people_on_one_hosting_suffix_are_different_sites()
    {
        var psl = new PslRegistrableDomain(Psl);
        Assert.NotEqual(psl.Of("alice.github.io"), psl.Of("bob.github.io"));
        Assert.NotEqual(psl.Of("one.com.cn"), psl.Of("two.com.cn"));
        // What the guess got wrong, and why the list is worth having.
        Assert.Equal(SimpleRegistrableDomain.Instance.Of("one.com.cn"), SimpleRegistrableDomain.Instance.Of("two.com.cn"));
    }

    [Fact]
    public void The_live_answer_is_the_guess_until_the_list_is_there()
    {
        Psl? loaded = null;
        var live = new LiveRegistrableDomain(() => loaded);
        Assert.Equal("com.cn", live.Of("one.com.cn"));
        loaded = Psl;
        Assert.Equal("one.com.cn", live.Of("one.com.cn"));
    }

    [Fact]
    public void The_live_answer_falls_back_when_the_list_fails()
    {
        var live = new LiveRegistrableDomain(() => throw new InvalidOperationException("no list"));
        Assert.Equal("example.com", live.Of("ads.example.com"));
    }

    [Fact]
    public void A_filter_list_uses_the_live_answer_by_default()
    {
        FishCatcherWarm();
        var list = FilterList.Compile(["||bob.github.io^$third-party"]);
        Assert.True(list.ShouldBlock(new Uri("https://bob.github.io/t.js"), "alice.github.io", ResourceKind.Script));
    }

    private static void FishCatcherWarm() => SearchKit.FishCatcher.FishCatcher.Warm().Wait(TimeSpan.FromSeconds(30));
}
