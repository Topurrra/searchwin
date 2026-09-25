namespace SearchKit.Shields;

/// Whether cancelling a navigation and starting a clean, unconditional GET
/// at its tidied address (Shield.Tidy, in Search/) is safe — apart from
/// WebView2 entirely, so it can be unit tested without a browser.
///
/// Shield.Tidy strips tracking parameters and unwraps redirect notices, and
/// the browser replaces the navigation with a fresh `Browser.Go`, which is
/// always a plain GET with no body. That is only ever the same navigation
/// for another plain GET: a form's POST, and a 307 or 308 redirect (the two
/// statuses that must replay the original request, body and all, rather
/// than fall back to GET) would have their method, body and referrer
/// silently dropped.
public static class TidyDecision
{
    /// `hasRequestBody`: the navigating request carries a body, or looks
    /// likely to — a Content-Type header is the signal WebView2 exposes
    /// before the request goes out, and is set for a form's POST but not for
    /// an ordinary GET. `redirectStatus`: the HTTP status of the redirect
    /// this navigation is following, or 0 when it isn't following one.
    public static bool CanTidy(bool hasRequestBody, int redirectStatus) =>
        !hasRequestBody && redirectStatus is not (307 or 308);
}
