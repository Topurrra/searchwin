using Microsoft.Web.WebView2.Core;

namespace Search;

// The pieces still to be carried over from the Mac app, each an empty shell
// with the shape the rest of the browser already calls. Each moves to a file
// of its own as it is filled in.

/// The ad blocker. PORT: Sources/Search/Shield.swift.
public sealed class Shield
{
    public static readonly Shield Shared = new();
    public bool Enabled { get; set; } = true;
    public void Start() { }
    public void Protect(CoreWebView2 core) { }
}

/// Reading mode. PORT: Sources/Search/Reader.swift.
public static class Reader
{
    public static void Toggle(Tab tab, Action<bool> done) => done(false);
}

/// The Chrome Web Store's own "Add to Search" button. PORT: StoreRelay.swift.
public static class StoreRelay
{
    public const string Script = "";
    public static void Start(Browser browser) { }
}

public sealed partial class Browser
{
    /// The store page's button, told what is installed. PORT: StoreRelay.swift.
    private void TellStore(Tab tab) { }
}

/// Chrome extensions. PORT: Extensions.swift, Crx.swift, ExtensionsUI.swift.
public sealed class Extensions
{
    public static readonly Extensions Shared = new();
    public Uri? NewTabPage => null;
    public void Start(Browser browser) { }
    public bool Intercept(Uri url) => false;
    /// A shortcut an extension registered.
    public bool Take(Windows.System.VirtualKey key, bool ctrl, bool shift, bool alt) => false;
    public void AddMenuItems(Tab tab, CoreWebView2 core, CoreWebView2ContextMenuRequestedEventArgs e) { }
}

/// A local socket a script can drive the browser through. PORT: Bench.swift.
public sealed class Bench
{
    public static readonly Bench Shared = new();
    public void Start(Browser browser) { }
    public void Stop() { }
}

/// Updates. The Mac app fetches signed builds from Office Commun; there is no
/// such server for Windows builds yet, so this only knows its own version.
public sealed class Updater
{
    public static readonly Updater Shared = new();
    public static string Version => typeof(Updater).Assembly.GetName().Version?.ToString(3) ?? "1.0.0";
    public void CheckIfDue(Action<string> say) { }
}
