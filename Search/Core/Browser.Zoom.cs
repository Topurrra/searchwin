namespace Search;

// How big a page is drawn. Not a magnifying glass over the rendered page — the
// page is laid out again at the new size, so text stays as sharp at 200% as at
// 100% — and remembered for the site, not for the tab: setting a paper's type
// to 125% once should be the last time you think about it.
//
// WebView2 keeps its own zoom out of reach of the app hosting it, so the page
// is asked to lay itself out bigger (CSS zoom on the root) — which is what the
// Mac's pageZoom does too. Ctrl and the wheel still do the engine's own.
public sealed partial class Browser
{
    public void Zoom(double factor)
    {
        if (Active is not { IsBlank: false } tab) return;
        Magnify(tab, tab.Zoom * factor);
    }

    /// Ctrl+0 undoes it.
    public void ResetZoom()
    {
        if (Active is not { IsBlank: false } tab) return;
        Magnify(tab, 1);
    }

    /// Between two fifths and three times, which is as far as a page is worth
    /// pushing in either direction.
    private void Magnify(Tab tab, double value)
    {
        var wanted = Math.Round(Math.Clamp(value, 0.4, 3), 2);
        if (Address.Host(tab.Address) is { } host && !tab.Shy)
        {
            if (Math.Abs(wanted - 1) < 0.01) Store.Settings.Remove("zoom." + host);
            else Store.Settings.Set("zoom." + host, wanted);
        }
        tab.Zoom = wanted;
        tab.Run(Zooming.Apply(wanted));
        Announce($"{(int)Math.Round(wanted * 100)}%");
    }
}

public static class Zooming
{
    /// Whatever you last set this site to.
    public static double For(string? host) =>
        host == null ? 1 : Store.Settings.Double("zoom." + host, 1);

    public static string Apply(double zoom) =>
        $"(function(){{ var r = document.documentElement; if (r) r.style.zoom = {(Math.Abs(zoom - 1) < 0.01 ? "''" : zoom.ToString(System.Globalization.CultureInfo.InvariantCulture))}; }})();";

    /// Before the page draws a single frame at the wrong size: the root is
    /// zoomed the moment it exists.
    public static string AtStart(double zoom)
    {
        if (Math.Abs(zoom - 1) < 0.01) return "";
        var z = zoom.ToString(System.Globalization.CultureInfo.InvariantCulture);
        return $$"""
        (function () {
          function put() { var r = document.documentElement; if (!r) return false; r.style.zoom = {{z}}; return true; }
          if (!put()) new MutationObserver(function (m, o) { if (put()) o.disconnect(); }).observe(document, { childList: true });
        })();
        """;
    }
}
