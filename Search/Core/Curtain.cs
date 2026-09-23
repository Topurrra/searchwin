using System.Text.Json;
using System.Text.Json.Serialization;
using Microsoft.Web.WebView2.Core;

namespace Search;

// Taking things off a page and keeping them off.
//
// Point at a cookie bar, a newsletter overlay, a sidebar of related nonsense —
// it goes, and it is still gone next time. Everything is remembered by site, as
// a list of selectors, and put back on by a stylesheet injected before the page
// has drawn a single frame, so nothing is ever seen appearing and vanishing.

public sealed class Veil
{
    public string Selector { get; set; } = "";
    /// What it was, in words, so the list of what you have hidden reads like
    /// something rather than like a stylesheet.
    public string Label { get; set; } = "";
    /// How big it was and where it sat — measured when you hid it, because a
    /// hidden thing has no size to measure later. Two elements can easily read
    /// the same; they rarely have the same shape in the same corner.
    public string? Note { get; set; }
    public DateTime Date { get; set; }

    [JsonIgnore] public string Id => Selector;
}

public static class Curtain
{
    private static Dictionary<string, List<Veil>> byHost = Load();
    private static bool saving;

    /// Anything hidden or put back, anywhere — for the list of what is hidden
    /// here, which redraws from it.
    public static event Action? Changed;

    public static string? Host(Uri? url) => Address.Host(url) is { } h ? (h.StartsWith("www.") ? h[4..] : h) : null;

    public static IReadOnlyList<Veil> Veils(string? host) =>
        host != null && byHost.TryGetValue(host, out var list) ? list : [];

    public static void Hide(string selector, string label, string note, string host)
    {
        if (!byHost.TryGetValue(host, out var list)) byHost[host] = list = [];
        if (list.Any(v => v.Selector == selector)) return;
        list.Add(new Veil { Selector = selector, Label = label, Note = note, Date = DateTime.Now });
        Save();
    }

    /// Put one back.
    public static void Restore(Veil veil, string host)
    {
        if (!byHost.TryGetValue(host, out var list)) return;
        list.RemoveAll(v => v.Selector == veil.Selector);
        if (list.Count == 0) byHost.Remove(host);
        Save();
    }

    /// Put the last one back — Ctrl+Z, while you are still pointing at things.
    public static Veil? Undo(string host)
    {
        if (!byHost.TryGetValue(host, out var list) || list.Count == 0) return null;
        var last = list[^1];
        list.RemoveAt(list.Count - 1);
        if (list.Count == 0) byHost.Remove(host);
        Save();
        return last;
    }

    public static void RestoreAll(string host)
    {
        if (!byHost.Remove(host)) return;
        Save();
    }

    /// The stylesheet for a site. Each selector stands alone in its own rule:
    /// one selector the browser can't parse would otherwise take the whole
    /// list down with it.
    /// One selector may be left out — that is how a row in the list shows you
    /// what it is offering to bring back, without bringing it back.
    public static string Css(string? host, string? without = null) =>
        string.Join("\n", Veils(host)
            .Where(v => v.Selector != without)
            .Select(v => $"{v.Selector} {{ display: none !important; }}"));

    // MARK: - the file

    private const string File = "hidden.json";

    private static Dictionary<string, List<Veil>> Load() =>
        Store.Read<Dictionary<string, List<Veil>>>(File) ?? [];

    /// A moment after the last change, and off the UI thread — hiding five
    /// things in a row writes the file once.
    private static void Save()
    {
        Changed?.Invoke();
        if (saving) return;
        saving = true;
        UI.After(0.4, () =>
        {
            saving = false;
            // A copy: the file is written on another thread while this one
            // goes on hiding things.
            var snapshot = byHost.ToDictionary(p => p.Key, p => p.Value.ToList());
            Store.Write(File, snapshot);
        });
    }
}

/// Carries a chosen element back from the page — or word that the pointing
/// stopped, or that it went wrong.
public static class VeilRelay
{
    public const string Name = "officeVeil";
}

public static class Veiling
{
    /// A stylesheet put in before the document has a body, so nothing is ever
    /// seen arriving and then leaving.
    public static string Style(string css) => $$"""
    (function () {
      var sheet = document.getElementById('office-veil');
      if (!sheet) {
        sheet = document.createElement('style');
        sheet.id = 'office-veil';
        (document.head || document.documentElement).appendChild(sheet);
      }
      sheet.textContent = `{{Bridge.Escape(css)}}`;
    })();
    """;

    /// The pointing mode. Loaded on every page but asleep: it costs one closure
    /// and a few functions until somebody actually asks for it.
    public const string Picker = """
    (function () {
      if (window.__officeVeil) return;
      var frame = null, tag = null, target = null, live = false;

      function sheet(id) {
        var s = document.getElementById(id);
        if (!s) {
          s = document.createElement('style');
          s.id = id;
          (document.head || document.documentElement).appendChild(s);
        }
        return s;
      }

      function chrome() {
        if (frame) return frame;
        frame = document.createElement('div');
        frame.style.cssText = 'position:fixed;z-index:2147483646;pointer-events:none;' +
          'border:2px solid rgba(23,23,23,.9);background:rgba(23,23,23,.07);' +
          'border-radius:4px;transition:all .07s ease-out;display:none';
        tag = document.createElement('div');
        tag.style.cssText = 'position:absolute;font:500 11px "Segoe UI Variable Text",' +
          '"Segoe UI",system-ui,sans-serif;color:#fff;background:#171717;padding:2px 7px;' +
          'border-radius:5px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis';
        frame.appendChild(tag);
        document.documentElement.appendChild(frame);
        return frame;
      }

      function place(el) {
        var box = chrome(), r = el.getBoundingClientRect();
        box.style.display = 'block';
        box.style.left = r.left + 'px';
        box.style.top = r.top + 'px';
        box.style.width = r.width + 'px';
        box.style.height = r.height + 'px';
        tag.textContent = name(el);
        // Above the element if there is sky above it, tucked inside its top
        // edge if there isn't — an element flush with the top of the window
        // would otherwise have its name cut off by the window.
        tag.style.top = r.top >= 26 ? '-21px' : '3px';
        // And never off the left or right edge either.
        tag.style.left = Math.max(2, -r.left + 4) + 'px';
        tag.style.maxWidth = Math.max(80, window.innerWidth - Math.max(0, r.left) - 16) + 'px';
      }

      var known = {
        nav: 'Navigation', header: 'Header', footer: 'Footer', aside: 'Sidebar',
        form: 'Form', dialog: 'Dialog', video: 'Video', img: 'Image',
        button: 'Button', iframe: 'Embed', figure: 'Figure', table: 'Table'
      };

      // What it is, in the order a person would answer the question: what it
      // calls itself, then what kind of thing it is, then what it says.
      function name(el) {
        var said = el.getAttribute && (el.getAttribute('aria-label') || el.getAttribute('title'));
        if (said && said.trim()) return clip(said.trim(), 40);
        var tagName = el.tagName.toLowerCase();
        if (known[tagName]) return known[tagName];
        var role = el.getAttribute && el.getAttribute('role');
        if (role) return role.charAt(0).toUpperCase() + role.slice(1);
        var text = (el.innerText || '').trim().replace(/\s+/g, ' ');
        return text ? clip(text, 40) : tagName;
      }

      /// How big, and which corner. Two sidebars read alike; they are rarely
      /// the same shape in the same place.
      function shape(el) {
        var r = el.getBoundingClientRect();
        var cx = r.left + r.width / 2, cy = r.top + r.height / 2;
        var side = cx < window.innerWidth / 3 ? 'left'
                 : (cx > window.innerWidth * 2 / 3 ? 'right' : 'centre');
        var band = cy < window.innerHeight / 3 ? 'top'
                 : (cy > window.innerHeight * 2 / 3 ? 'bottom' : 'middle');
        return Math.round(r.width) + '×' + Math.round(r.height) + ' · ' + band + ' ' + side;
      }

      function clip(text, n) { return text.length > n ? text.slice(0, n) + '…' : text; }

      // A class worth hanging a rule on: a word, not a build artefact.
      function steady(c) {
        return /^[a-zA-Z][\w-]{2,29}$/.test(c) && !/\d{3,}/.test(c) &&
               !/^(css|sc|jsx|emotion|svelte|styles?)-/.test(c);
      }

      function unique(sel) {
        try { return document.querySelectorAll(sel).length === 1; } catch (e) { return false; }
      }

      function selectorFor(el) {
        if (el.id && unique('#' + CSS.escape(el.id))) return '#' + CSS.escape(el.id);

        var hooks = ['data-testid', 'data-test', 'data-qa', 'data-cy', 'aria-label', 'name', 'role'];
        for (var i = 0; i < hooks.length; i++) {
          var v = el.getAttribute && el.getAttribute(hooks[i]);
          if (v) {
            var s = el.tagName.toLowerCase() + '[' + hooks[i] + '="' + CSS.escape(v) + '"]';
            if (unique(s)) return s;
          }
        }

        var classes = (el.className && typeof el.className === 'string')
          ? el.className.trim().split(/\s+/).filter(steady) : [];
        if (classes.length) {
          var byClass = el.tagName.toLowerCase() + '.' + classes.map(CSS.escape).join('.');
          if (unique(byClass)) return byClass;
        }

        // Last resort: a path, anchored on the nearest thing with a name.
        var parts = [], node = el;
        while (node && node.nodeType === 1 && node !== document.documentElement) {
          if (node.id && unique('#' + CSS.escape(node.id))) {
            parts.unshift('#' + CSS.escape(node.id));
            break;
          }
          var tagName = node.tagName.toLowerCase();
          var parent = node.parentElement;
          if (!parent) { parts.unshift(tagName); break; }
          var kin = Array.prototype.filter.call(parent.children, function (c) {
            return c.tagName === node.tagName;
          });
          parts.unshift(kin.length > 1
            ? tagName + ':nth-of-type(' + (kin.indexOf(node) + 1) + ')'
            : tagName);
          node = parent;
        }
        return parts.join(' > ');
      }

      function onMove(e) {
        if (!live) return;
        var el = document.elementFromPoint(e.clientX, e.clientY);
        if (!el || el === frame || el === document.documentElement || el === document.body) return;
        target = el;
        place(el);
      }

      // Everything a press can be, swallowed. Real pages act on pointerdown or
      // mousedown and are gone before a click ever completes — which looked
      // exactly like nothing happening.
      function swallow(e) {
        if (!live) return;
        e.preventDefault();
        e.stopPropagation();
        e.stopImmediatePropagation();
      }

      function onPress(e) {
        if (!live) return;
        swallow(e);
        // The pointer may have arrived without ever moving — a click on the
        // very first element under it, or a tap on a touchpad.
        var el = target || document.elementFromPoint(e.clientX, e.clientY);
        if (!el || el === frame || el === document.documentElement || el === document.body) return;
        try {
          window.webkit.messageHandlers.officeVeil.postMessage({
            selector: selectorFor(el),
            label: name(el),
            note: shape(el)
          });
        } catch (err) {
          window.webkit.messageHandlers.officeVeil.postMessage({ trouble: String(err) });
        }
        target = null;
        if (frame) frame.style.display = 'none';
      }

      var presses = ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click',
                     'dblclick', 'contextmenu', 'touchstart'];

      window.__officeVeil = {
        on: function () {
          if (live) return;
          live = true;
          chrome();
          document.documentElement.style.cursor = 'crosshair';
          document.addEventListener('mousemove', onMove, true);
          document.addEventListener('pointermove', onMove, true);
          presses.forEach(function (kind) {
            document.addEventListener(kind, kind === 'pointerdown' ? onPress : swallow, true);
          });
        },
        off: function () {
          if (!live) return;
          live = false;
          target = null;
          if (frame) frame.style.display = 'none';
          document.documentElement.style.cursor = '';
          document.removeEventListener('mousemove', onMove, true);
          document.removeEventListener('pointermove', onMove, true);
          presses.forEach(function (kind) {
            document.removeEventListener(kind, kind === 'pointerdown' ? onPress : swallow, true);
          });
          window.webkit.messageHandlers.officeVeil.postMessage({ off: true });
        },
        // Show one hidden thing for as long as the pointer rests on its row.
        // The stylesheet is rebuilt without that one selector rather than
        // fighting it with another rule, so the element comes back with the
        // layout it actually had.
        peek: function (css, sel) {
          sheet('office-veil').textContent = css;
          sheet('office-peek').textContent = sel +
            ' { outline: 2px solid rgba(23,23,23,.9) !important; outline-offset: 2px !important; }';
          try {
            var el = document.querySelector(sel);
            if (el) el.scrollIntoView({ block: 'center', behavior: 'smooth' });
          } catch (e) {}
        },
        unpeek: function (css) {
          sheet('office-veil').textContent = css;
          sheet('office-peek').textContent = '';
        }
      };
    })();
    """;
}

public sealed partial class Browser
{
    /// The site on screen, as the curtain and the blocker name it.
    public string? HereHost => Curtain.Host(Active?.Address);
    public IReadOnlyList<Veil> HereVeils => Curtain.Veils(HereHost);

    private bool veiling, reviewing;
    /// True while the pointer is picking things to hide.
    public bool Veiling { get => veiling; private set => Set(ref veiling, value); }
    /// True while the list of what is hidden here is up.
    public bool Reviewing
    {
        get => reviewing;
        set { if (Set(ref reviewing, value) && !value) StopPeeking(); }
    }

    /// The tab the pointer is picking in, so leaving it stops the picking
    /// there rather than leaving a page waiting to swallow the next click.
    private Tab? picking;

    partial void StartCurtain()
    {
        Bridge.Handlers[VeilRelay.Name] = (tab, body) =>
        {
            if (body.ValueKind != JsonValueKind.Object) return;
            if (body.TryGetProperty("trouble", out var trouble))
            {
                Log.Write($"picking failed: {trouble}");
                return;
            }
            if (body.TryGetProperty("off", out var off) && off.ValueKind == JsonValueKind.True)
            {
                if (picking == tab) picking = null;
                if (tab == Active) Veiling = false;
                return;
            }
            if (Said(body, "selector") is not { Length: > 0 } selector) return;
            var label = Said(body, "label") ?? selector;
            var note = Said(body, "note") ?? "";
            if (Curtain.Host(tab.Address) is not { } host) return;
            Curtain.Hide(selector, label, note, host);
            Redress(tab, host);
            Announce("Hidden — Ctrl+Z puts it back");
        };

        // Another tab in front while pointing: the one left behind stops
        // swallowing clicks, and the hint goes with it.
        On(nameof(Active), () =>
        {
            if (picking == null || picking == Active) return;
            StopPicking(picking);
            picking = null;
            Veiling = false;
        });
    }

    partial void AttachCurtain(Tab tab, CoreWebView2 core)
    {
        // A new document is never pointing, whatever the last one was doing.
        core.NavigationStarting += (_, _) =>
        {
            if (picking != tab) return;
            picking = null;
            Veiling = false;
        };
    }

    /// Ctrl+Shift+H. Point at anything on the page and it goes, for good, on
    /// this site.
    public void ToggleHiding()
    {
        if (Active is not { IsBlank: false } tab) return;
        if (Veiling)
        {
            Veiling = false;
            picking = null;
            StopPicking(tab);
        }
        else
        {
            Reviewing = false;
            Veiling = true;
            picking = tab;
            tab.Run("window.__officeVeil && window.__officeVeil.on()");
        }
    }

    /// Ctrl+Z, while pointing: the last thing you took off comes back.
    public void UndoHiding()
    {
        if (HereHost is not { } host || Curtain.Undo(host) is not { } back) return;
        if (Active is { } tab) Redress(tab, host);
        Announce($"{back.Label} is back");
    }

    /// The pointer resting on a row in the list brings that one thing back,
    /// outlined, and scrolls the page to it.
    public void Peek(Veil veil)
    {
        if (Active?.Core is not { } core) return;
        var css = Curtain.Css(HereHost, without: veil.Selector);
        _ = core.ExecuteScriptAsync(
            $"window.__officeVeil && window.__officeVeil.peek(`{Bridge.Escape(css)}`, `{Bridge.Escape(veil.Selector)}`)");
    }

    public void StopPeeking()
    {
        if (Active?.Core is not { } core) return;
        _ = core.ExecuteScriptAsync(
            $"window.__officeVeil && window.__officeVeil.unpeek(`{Bridge.Escape(Curtain.Css(HereHost))}`)");
    }

    public void Restore(Veil veil)
    {
        if (HereHost is not { } host || Active is not { } tab) return;
        Curtain.Restore(veil, host);
        Redress(tab, host);
    }

    public void RestoreAll()
    {
        if (HereHost is not { } host || Active is not { } tab) return;
        Curtain.RestoreAll(host);
        Redress(tab, host);
        Reviewing = false;
        Announce("Everything is back");
    }

    /// Both the page in front of you and the one that loads next time.
    private static void Redress(Tab tab, string host)
    {
        var css = Curtain.Css(host);
        tab.Arm(css);
        // Only a page that is running: one not yet built gets the stylesheet
        // from Arm, before its first frame.
        _ = tab.Core?.ExecuteScriptAsync(Search.Veiling.Style(css));
    }

    private static string? Said(JsonElement body, string key) =>
        body.TryGetProperty(key, out var value) && value.ValueKind == JsonValueKind.String ? value.GetString() : null;

    private static void StopPicking(Tab tab) =>
        _ = tab.Core?.ExecuteScriptAsync("window.__officeVeil && window.__officeVeil.off()");
}
