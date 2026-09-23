namespace Search;

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
        // This site's stylesheet of hidden things, before the body exists.
        if (css.Length > 0)
            yield return new(Veiling.Style(css), MainFrameOnly: true, AtEnd: false);
    }
}
