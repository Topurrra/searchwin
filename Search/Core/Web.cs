using System.Text.Json;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.Web.WebView2.Core;

namespace Search;

// One WebView2 environment for the whole app: one browser process, one GPU
// process, one network service — every tab after the first gets a warm
// renderer instead of starting the engine again. That is the Windows side of
// WebKit pooling its processes by data store.
public static class Web
{
    private static Task<CoreWebView2Environment>? environment;

    /// Where WebView2 keeps its profiles: cookies, caches, site data,
    /// extensions. Beside the app's own files, never shared with a test run.
    public static string DataFolder => Path.Combine(Store.Folder, "WebView2");

    /// Started as early as possible — before the window has drawn — so the
    /// first address typed navigates instead of waiting for the engine.
    public static Task<CoreWebView2Environment> Environment => environment ??= Create();

    private static async Task<CoreWebView2Environment> Create()
    {
        // A view that has never drawn is opaque white. In a dark window that
        // is a flash of it between a link that opens a tab and the page
        // arriving, so every view starts the colour of the window's ground.
        var ground = Palette.ColorOf(Tone.Ground);
        System.Environment.SetEnvironmentVariable("WEBVIEW2_DEFAULT_BACKGROUND_COLOR", $"FF{ground.R:X2}{ground.G:X2}{ground.B:X2}");
        var options = new CoreWebView2EnvironmentOptions
        {
            AreBrowserExtensionsEnabled = true,
            // Nothing of Edge's own that talks to Microsoft about the pages
            // you visit, and no shopping or hub popups over them.
            AdditionalBrowserArguments =
                "--disable-features=msSmartScreenProtection,msEdgeShoppingUI,msWebOOUI,msPdfOOUI,msHubApps",
            // Crash dumps stay on this machine: nothing is sent anywhere.
            IsCustomCrashReportingEnabled = true,
        };
        return await CoreWebView2Environment.CreateWithOptionsAsync(null, DataFolder, options);
    }

    /// The stage every page lives on (see Stage). A page is put here the moment
    /// its view is built — WebView2 only starts once its view is in a window —
    /// and stays until the tab lets it go.
    public static Panel Stage { get; set; } = null!;
}

/// The page's side of the conversation. Every script this browser puts in a
/// page talks back through one function, `__officePost(name, body)`, which
/// posts to WebView2; the tab hands each message to whoever registered for
/// its name. The Mac's scripts said `window.webkit.messageHandlers.NAME
/// .postMessage(body)` — Port() rewrites them to this, so they can be carried
/// over as they are.
public static class Bridge
{
    public const string Prelude = """
    (function () {
      if (window.__officePost) return;
      var hook = window.chrome && window.chrome.webview;
      Object.defineProperty(window, '__officePost', {
        value: function (name, body) {
          try { if (hook) hook.postMessage({ name: name, body: body === undefined ? null : body }); } catch (e) {}
        },
        enumerable: false, configurable: false, writable: false
      });
    })();
    """;

    public static string Port(string script) =>
        System.Text.RegularExpressions.Regex.Replace(
            script,
            @"window\.webkit\.messageHandlers\.(\w+)\.postMessage\(",
            m => $"window.__officePost('{m.Groups[1].Value}', ");

    /// Runs at document start in every frame; `mainFrameOnly` scripts check
    /// for themselves and step aside in frames.
    public static string Wrap(string script, bool mainFrameOnly, bool atEnd)
    {
        var body = Port(script);
        if (atEnd)
            body = $"(function(){{ var run = function () {{ {body}\n }}; if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', run, {{ once: true }}); else run(); }})();";
        if (mainFrameOnly)
            body = $"if (window.top === window) {{ {body}\n }}";
        return body;
    }

    /// Whoever answers a given name: the scroll reporter, the forms, the
    /// picker that hides things. Registered once at launch.
    public static readonly Dictionary<string, Action<Tab, JsonElement>> Handlers = [];

    /// A string for a template literal in a script, safely.
    public static string Escape(string text) =>
        text.Replace("\\", "\\\\").Replace("`", "\\`").Replace("$", "\\$");

    /// A value as a JavaScript literal.
    public static string Literal(object? value) => JsonSerializer.Serialize(value);
}

/// A script that goes into every page, and when.
public sealed record PageScript(string Source, bool MainFrameOnly, bool AtEnd);
