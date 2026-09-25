using SearchKit.Shields;

namespace SearchKit.Tests.Shields;

public class WorkerRequestTests
{
    [Theory]
    [InlineData(true, null, null, true)]
    [InlineData(false, "navigate", null, true)]
    [InlineData(false, " Navigate ", null, true)]
    [InlineData(false, null, "document", true)]
    [InlineData(false, "no-cors", "image", false)]
    [InlineData(false, "cors", "empty", false)]
    [InlineData(false, null, null, false)]
    public void A_navigation_a_worker_fetches_is_told_apart(bool document, string? mode, string? dest, bool navigation) =>
        Assert.Equal(navigation, WorkerRequest.IsNavigation(document, mode, dest));

    // What WebView2 153 really sends for a service worker's fetches (seen in
    // a test world): XmlHttpRequest context every time, no Sec-Fetch, no
    // Origin. So a navigation is told apart only by being the address a tab
    // is waiting for.
    private const string ImageAccept = "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8";

    [Fact]
    public void A_navigation_a_worker_fetches_is_the_address_a_tab_is_waiting_for()
    {
        var awaited = new AwaitedNavigations();
        var page = new Uri("https://www.site.example/2026/9/article");
        Assert.False(WorkerRequest.IsNavigation(false, null, null, page, awaited));
        awaited.Expect(new Uri("https://WWW.site.example/2026/9/article#top"));
        Assert.True(WorkerRequest.IsNavigation(false, null, null, page, awaited));
        Assert.False(WorkerRequest.IsNavigation(false, null, null, new Uri("https://www.site.example/2026/9/other"), awaited));
    }

    // A page can put an HTML Accept and Upgrade-Insecure-Requests on any
    // fetch it makes (Accept is even CORS-safelisted): those headers must
    // never be what spares a tracker's request.
    [Fact]
    public void Headers_a_page_can_set_do_not_make_a_navigation()
    {
        var awaited = new AwaitedNavigations();
        awaited.Expect(new Uri("https://www.site.example/"));
        Assert.False(WorkerRequest.IsNavigation(false, null, null, new Uri("https://tracker.example/px"), awaited));
    }

    [Fact]
    public void A_page_is_awaited_for_a_while_not_for_ever()
    {
        var awaited = new AwaitedNavigations(keep: TimeSpan.FromSeconds(30));
        var at = new DateTime(2026, 9, 25, 12, 0, 0, DateTimeKind.Utc);
        var page = new Uri("https://site.example/a");
        awaited.Expect(page, at);
        Assert.True(awaited.IsAwaited(page, at.AddSeconds(29)));
        Assert.False(awaited.IsAwaited(page, at.AddSeconds(31)));
    }

    [Fact]
    public void Only_so_many_pages_are_remembered()
    {
        var awaited = new AwaitedNavigations(capacity: 3);
        var at = new DateTime(2026, 9, 25, 12, 0, 0, DateTimeKind.Utc);
        for (var i = 0; i < 5; i++) awaited.Expect(new Uri($"https://site.example/{i}"), at.AddSeconds(i));
        Assert.False(awaited.IsAwaited(new Uri("https://site.example/0"), at.AddSeconds(5)));
        Assert.True(awaited.IsAwaited(new Uri("https://site.example/4"), at.AddSeconds(5)));
    }

    [Theory]
    [InlineData(ImageAccept, ResourceKind.Image)]
    [InlineData("text/css,*/*;q=0.1", ResourceKind.Stylesheet)]
    [InlineData("*/*", ResourceKind.XmlHttpRequest)]
    [InlineData(null, ResourceKind.XmlHttpRequest)]
    public void What_a_worker_fetches_is_judged_as_what_the_page_asked_for(string? accept, ResourceKind kind) =>
        Assert.Equal(kind, WorkerRequest.Kind(ResourceKind.XmlHttpRequest, accept));

    [Fact]
    public void A_known_context_is_kept() =>
        Assert.Equal(ResourceKind.Script, WorkerRequest.Kind(ResourceKind.Script, ImageAccept));

    // EasyList's `/adverts/*$~xmlhttprequest`: an image a worker fetches is
    // an image, not a fetch the rule leaves alone.
    [Fact]
    public void An_image_rule_holds_for_an_image_a_worker_fetches()
    {
        var list = FilterList.Compile(["/adverts/*$~xmlhttprequest"]);
        var image = new Uri("http://localhost:8791/adverts/banner.gif");
        var page = WorkerRequest.Page(null, "http://localhost:8791/adverts/")?.Host;
        Assert.False(list.ShouldBlock(image, page, ResourceKind.XmlHttpRequest));
        Assert.True(list.ShouldBlock(image, page, WorkerRequest.Kind(ResourceKind.XmlHttpRequest, ImageAccept)));
    }

    // An article a PWA's worker fetches for the page, with words in its
    // address a list blocks elsewhere: the list alone would refuse it (a
    // 403 in place of the page); as a navigation it is never asked.
    [Theory]
    [InlineData("https://www.theverge.com/2026/9/1/ad-tracking-banner-ads-explained")]
    [InlineData("https://www.example.com/adverts/")]
    public void A_page_its_worker_serves_is_not_refused(string address)
    {
        var list = FilterList.Compile(["-banner-ads-", "/adverts/*"]);
        var url = new Uri(address);
        Assert.True(list.ShouldBlock(url, null, ResourceKind.XmlHttpRequest));
        var awaited = new AwaitedNavigations();
        awaited.Expect(url);
        Assert.True(WorkerRequest.IsNavigation(false, null, null, url, awaited));
    }

    [Theory]
    [InlineData("https://site.example", null, "site.example")]
    [InlineData(null, "https://www.site.example/sw.js", "www.site.example")]
    [InlineData("https://site.example", "https://other.example/", "site.example")]
    [InlineData("null", "https://site.example/", "site.example")]
    [InlineData("chrome-extension://abc", null, null)]
    [InlineData("", "", null)]
    [InlineData(null, "not an address", null)]
    public void The_site_comes_from_the_origin_then_the_referer(string? origin, string? referer, string? host) =>
        Assert.Equal(host, WorkerRequest.Page(origin, referer)?.Host);

    // With the site known, `$domain=` and first-party hold for a worker's
    // requests as they do for the page's own.
    [Fact]
    public void A_domain_exception_holds_for_the_site_the_worker_serves()
    {
        var list = FilterList.Compile(["||cdn.example^$image", "@@||cdn.example^$image,domain=site.example"]);
        var image = new Uri("https://cdn.example/pic.png");
        Assert.True(list.ShouldBlock(image, null, ResourceKind.Image));
        var page = WorkerRequest.Page(null, "https://site.example/sw.js");
        Assert.False(list.ShouldBlock(image, page?.Host, ResourceKind.Image));
    }

    [Fact]
    public void A_site_s_own_requests_through_its_worker_are_first_party()
    {
        var list = FilterList.Compile(["||tracker.example^$third-party,script"]);
        var script = new Uri("https://tracker.example/t.js");
        Assert.True(list.ShouldBlock(script, WorkerRequest.Page("https://site.example", null)?.Host, ResourceKind.Script));
        Assert.False(list.ShouldBlock(script, WorkerRequest.Page("https://tracker.example", null)?.Host, ResourceKind.Script));
    }
}
