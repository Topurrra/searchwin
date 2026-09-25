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
    // Origin — the navigation told apart by its Accept and its
    // Upgrade-Insecure-Requests.
    private const string NavigationAccept = "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7";
    private const string ImageAccept = "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8";

    [Fact]
    public void A_navigation_a_worker_fetches_is_told_apart_by_what_webview2_really_sends()
    {
        Assert.True(WorkerRequest.IsNavigation(false, null, null, NavigationAccept, "1"));
        Assert.True(WorkerRequest.IsNavigation(false, null, null, NavigationAccept, null));
        Assert.True(WorkerRequest.IsNavigation(false, null, null, null, "1"));
        Assert.False(WorkerRequest.IsNavigation(false, null, null, ImageAccept, null));
        Assert.False(WorkerRequest.IsNavigation(false, null, null, "*/*", null));
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
        Assert.True(WorkerRequest.IsNavigation(false, null, null, NavigationAccept, "1"));
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
