namespace Search;

// Hide anything, for good. Ctrl+Shift+H, then click a cookie banner, a
// newsletter overlay, a rail of "related" nonsense — it goes, and it is still
// gone on that site next time, before the page has drawn a single frame.
//
// PORT: see Sources/Search/Curtain.swift, Hidden.swift and the curtain part of
// Browser.swift.
public static class Curtain
{
    public static string? Host(Uri? url) => Address.Host(url) is { } h ? (h.StartsWith("www.") ? h[4..] : h) : null;

    public static string Css(string? host) => "";
}

public static class Veiling
{
    public const string Picker = "";
    public static string Style(string css) => "";
}

public sealed partial class Browser
{
    /// The site on screen, as the curtain and the blocker name it.
    public string? HereHost => Curtain.Host(Active?.Address);

    private bool veiling, reviewing;
    /// True while the pointer is picking things to hide.
    public bool Veiling { get => veiling; private set => Set(ref veiling, value); }
    /// True while the list of what is hidden here is up.
    public bool Reviewing { get => reviewing; set => Set(ref reviewing, value); }

    public void ToggleHiding() { }
    public void UndoHiding() { }
}
