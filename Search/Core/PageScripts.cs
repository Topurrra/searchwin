namespace Search;

/// Windows' spelling checker in pages' text fields, on or off.
public static class Spelling
{
    public const string Off = """
    (function () {
      function put() { var r = document.documentElement; if (!r) return false; r.setAttribute('spellcheck', 'false'); return true; }
      if (!put()) new MutationObserver(function (m, o) { if (put()) o.disconnect(); }).observe(document, { childList: true });
    })();
    """;

    /// For the pages already open, at once.
    public static string Now(bool on) =>
        on ? "document.documentElement && document.documentElement.removeAttribute('spellcheck')"
           : "document.documentElement && document.documentElement.setAttribute('spellcheck', 'false')";
}

/// Every script that goes into a page besides the bridge and the scroll
/// reporter, and when. Each belongs to the file of the feature it serves;
/// this is only the list of them, in the order the Mac's Tab.arm put them in.
public static class PageScripts
{
    public static IEnumerable<PageScript> For(Tab tab, string css)
    {
        // The pointing mode that hides things (Curtain).
        yield return new(Veiling.Picker, MainFrameOnly: true, AtEnd: false);
        // Sign-ins: where they are, what was sent (Forms).
        yield return new(FormRelay.Script, MainFrameOnly: true, AtEnd: true);
        // The Chrome Web Store's own button (StoreRelay).
        yield return new(StoreRelay.Script, MainFrameOnly: true, AtEnd: true);
        if (!FormRelay.PasskeysOffered)
            yield return new(FormRelay.WithoutPasskeys, MainFrameOnly: false, AtEnd: false);
        // Spelling off: every field inherits `spellcheck` from the root, so
        // turning it off there turns it off everywhere on the page.
        if (Browser.Shared is { Prefs.Spelling: false })
            yield return new(Spelling.Off, MainFrameOnly: false, AtEnd: false);
        // FishCatcher's probe: what the page asks for, once it has drawn
        // (Fish). It waits for the document itself.
        if (Browser.Shared is { Prefs.WarnsOfScams: true })
            yield return new(FishProbe.Script, MainFrameOnly: true, AtEnd: false);
        // This site's stylesheet of hidden things, before the body exists.
        if (css.Length > 0)
            yield return new(Veiling.Style(css), MainFrameOnly: true, AtEnd: false);
    }
}
