using SearchKit.Web;

namespace SearchKit.Tests;

public class PageMessageTests
{
    [Fact]
    public void A_message_over_256_KB_is_never_read()
    {
        Assert.True(PageMessage.Fits("{\"name\":\"fish.facts\"}"));
        Assert.True(PageMessage.Fits(new string(' ', PageMessage.MaxLength)));
        Assert.False(PageMessage.Fits(new string(' ', PageMessage.MaxLength + 1)));
        Assert.False(PageMessage.Fits(""));
        Assert.False(PageMessage.Fits(null));
    }

    // The engine's Source, not an address the page wrote into the body: a
    // page that posts "href: https://bank.example/" just before it goes
    // there is still the page that sent it.
    [Theory]
    [InlineData("https://scam.example/login", "https://scam.example/other", true)]
    [InlineData("https://SCAM.example/login", "https://scam.example/", true)]
    [InlineData("https://scam.example/login", "https://bank.example/", false)]
    [InlineData("https://www.bank.example/", "https://bank.example/", false)]
    public void Facts_are_heard_only_from_the_host_on_screen(string source, string showing, bool heard) =>
        Assert.Equal(heard, PageMessage.SameHost(new Uri(source), new Uri(showing)));

    [Fact]
    public void No_sender_is_never_the_page_on_screen()
    {
        Assert.False(PageMessage.SameHost(null, new Uri("https://bank.example/")));
        Assert.False(PageMessage.SameDocument(null, new Uri("https://bank.example/")));
        Assert.False(PageMessage.SameHost(new Uri("https://bank.example/"), null));
    }

    [Theory]
    [InlineData("https://news.example/amp/story", "https://news.example/amp/story", true)]
    [InlineData("https://news.example/amp/story#top", "https://news.example/amp/story", true)]
    [InlineData("https://news.example/amp/story", "https://news.example/amp/story?x=1", false)]
    [InlineData("https://news.example/amp/story", "https://news.example/amp/Story", false)]
    [InlineData("https://news.example/amp/story", "https://other.example/amp/story", false)]
    public void An_amp_page_may_only_move_the_document_that_sent_it(string source, string showing, bool same) =>
        Assert.Equal(same, PageMessage.SameDocument(new Uri(source), new Uri(showing)));
}
