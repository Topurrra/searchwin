using Microsoft.Web.WebView2.Core;

namespace Search;

/// Chrome extensions. PORT: Extensions.swift, Crx.swift, ExtensionsUI.swift.
public sealed class Extensions
{
    public static readonly Extensions Shared = new();
    /// An extension's new tab page, if one asked and you said yes.
    public Uri? NewTabPage => null;
    /// An extension's OAuth sign-in coming back.
    public bool Intercept(Uri url) => false;
    /// A shortcut an extension registered.
    public bool Take(Windows.System.VirtualKey key, bool ctrl, bool shift, bool alt) => false;
    /// What extensions add to a page's right-click menu.
    public void AddMenuItems(Tab tab, CoreWebView2 core, CoreWebView2ContextMenuRequestedEventArgs e) { }
}
