using System.Text.Json;

namespace Search;

// The Chrome Web Store, made to work for this browser.
//
// The store sees a browser that isn't Chrome and says so: a banner asking to
// "Switch to Chrome", and an "Add to Chrome" button that stays grey. Search
// installs from the store on its own (Extensions.Install, through Crx), so on
// the store's pages the banner goes and the grey button is replaced by an
// "Add to Search" one — the button people already look for, rather than a bar
// at the bottom of the window they would have to notice. What gets installed
// is read from the tab's own address, never from anything the page says; the
// page only asks, and the usual confirmation still stands between the asking
// and the installing.
public static class StoreRelay
{
    public const string Name = "officeStore";

    /// Main frame, every page, returning at once anywhere but the store. The
    /// store's markup is generated and its class names change between
    /// releases, so nothing here leans on them: the store's own button is the
    /// disabled one that names Chrome, and the banner is the small block
    /// around the one enabled button that does — only that block, since the
    /// store puts the banner and the extension's own header, button and all,
    /// in the same section. Carried over from the Mac as it is; Bridge.Port
    /// turns its messages into WebView2's.
    public const string Script = """
    (function () {
      if (location.hostname !== 'chromewebstore.google.com' || window.__officeStore) return;
      var state = { installed: [], busy: null };

      function pageID() {
        var m = location.pathname.match(/\/detail\/(?:[^\/]+\/)?([a-p]{32})/);
        return m ? m[1] : null;
      }

      // The store's own button: grey and naming Chrome for a browser it
      // doesn't recognise (Safari, on the Mac), or — for a Chromium engine
      // like this one — a live "Add to Chrome" that would call an install
      // API only Chrome has, and do nothing.
      function theirs() {
        var buttons = document.querySelectorAll('button');
        for (var i = 0; i < buttons.length; i++) {
          var b = buttons[i];
          if (b.dataset.office) continue;
          var text = (b.textContent || '').trim();
          if ((b.disabled && /chrome/i.test(text)) || /^(add to|remove from) chrome$/i.test(text)) return b;
        }
        return null;
      }

      // From the banner's own button up, as far as it goes without taking in
      // the header beside it: short, and holding no install button.
      function bannerOf(button) {
        var box = null, up = button.parentElement;
        while (up && up !== document.body) {
          if (up.querySelector('button[disabled], button[data-office]')) break;
          if ((up.innerText || '').length > 160) break;
          box = up;
          up = up.parentElement;
        }
        return box;
      }

      function hideBanner() {
        // And the floating "Switch to Chrome?" card, known by the Chrome logo
        // it carries in any language — it sits right over the button.
        var cards = document.querySelectorAll('[role="dialog"]');
        for (var c = 0; c < cards.length; c++) {
          if (!cards[c].dataset.office && cards[c].querySelector('img[src*="productlogos/chrome"]')) {
            cards[c].style.display = 'none';
            cards[c].dataset.office = 'promo';
          }
        }
        var buttons = document.querySelectorAll('button:not([disabled])');
        for (var i = 0; i < buttons.length; i++) {
          var b = buttons[i];
          if (b.dataset.office || !/chrome/i.test(b.getAttribute('aria-label') || '')) continue;
          // The install button itself is not a banner, live as it is here.
          if (/^(add to|remove from) chrome$/i.test((b.textContent || '').trim())) continue;
          var box = bannerOf(b);
          if (box && !box.dataset.office) {
            box.style.display = 'none';
            box.dataset.office = 'banner';
          }
        }
      }

      // The words only, so the button keeps the store's own shape and colour.
      function label(button, text) {
        var walker = document.createTreeWalker(button, NodeFilter.SHOW_TEXT);
        var node, last = null;
        while ((node = walker.nextNode())) { if (node.nodeValue.trim()) last = node; }
        if (last) last.nodeValue = text; else button.textContent = text;
      }

      function render(ours) {
        var id = pageID();
        var installed = !!id && state.installed.indexOf(id) >= 0;
        var busy = !!id && state.busy === id;
        label(ours, installed ? 'Added to Search' : (busy ? 'Adding…' : 'Add to Search'));
        ours.disabled = installed || busy;
      }

      function mend() {
        hideBanner();
        if (!pageID()) return;
        var original = theirs();
        if (original && original.parentNode) {
          var ours = original.cloneNode(true);
          ['disabled', 'jsaction', 'jscontroller', 'jsname', 'jslog', 'aria-describedby'].forEach(function (name) {
            ours.removeAttribute(name);
          });
          ours.dataset.office = 'add';
          original.dataset.office = 'theirs';
          original.style.display = 'none';
          original.parentNode.insertBefore(ours, original.nextSibling);
          window.webkit.messageHandlers.officeStore.postMessage({ placed: pageID() });
        }
        renderAll();
      }

      // The store keeps the pages it has left, hidden, beside the one it shows.
      function renderAll() {
        var mine = document.querySelectorAll('button[data-office="add"]');
        for (var i = 0; i < mine.length; i++) render(mine[i]);
      }

      // Caught on the window, before the store's own handlers — which listen
      // on the document — can see the click at all.
      window.addEventListener('click', function (e) {
        var mine = e.target && e.target.closest && e.target.closest('button[data-office="add"]');
        if (!mine) return;
        e.preventDefault();
        e.stopImmediatePropagation();
        if (!mine.disabled) window.webkit.messageHandlers.officeStore.postMessage({ add: true });
      }, true);

      window.__officeStore = {
        state: function (next) {
          state = next || state;
          renderAll();
        }
      };

      // The store is one page that rewrites itself: whatever it redraws, mend
      // again. A timer rather than a frame — a tab out of sight gets no frames.
      var queued = false;
      new MutationObserver(function () {
        if (queued) return;
        queued = true;
        setTimeout(function () { queued = false; mend(); }, 60);
      }).observe(document.documentElement, { childList: true, subtree: true });
      mend();
    })();
    """;

    /// A store page, where an extension can be added from.
    public static bool IsStorePage(Uri? url)
    {
        if (url == null) return false;
        var host = url.Host.ToLowerInvariant();
        return host == "chromewebstore.google.com"
            || (host == "chrome.google.com" && url.AbsolutePath.StartsWith("/webstore"));
    }
}

public sealed partial class Browser
{
    /// Where "Chrome Web Store…" goes: its extensions, not its themes.
    public static readonly Uri WebStore = new("https://chromewebstore.google.com/category/extensions");

    /// The page's messages: its "Add to Search" pressed, or put in place.
    /// And every store page told again whenever what is installed, or on
    /// its way, changes.
    private void StartStore()
    {
        Bridge.Handlers[StoreRelay.Name] = (tab, body) =>
        {
            if (body.ValueKind != JsonValueKind.Object) return;
            if (body.TryGetProperty("add", out _)) AddFromStore(tab);
            if (body.TryGetProperty("placed", out var placed) && placed.ValueKind == JsonValueKind.String) tab.StorePlaced = placed.GetString();
        };
        Extensions.Shared.OnAny(name =>
        {
            if (name is not (nameof(Extensions.Installed) or nameof(Extensions.Busy))) return;
            foreach (var tab in Tabs) TellStore(tab);
        });
    }

    /// The page's "Add to Search" was pressed: the extension this tab is
    /// showing, by its address.
    public void AddFromStore(Tab tab)
    {
        if (tab.Address is not { } url || !StoreRelay.IsStorePage(url)) return;
        Extensions.Shared.Install(url.AbsoluteUri);
    }

    /// Tells a store page what is installed and what is on its way, so its
    /// button can say "Added to Search" or "Adding…". Only a page already
    /// running is told — never one woken up to hear it.
    private void TellStore(Tab tab)
    {
        if (tab.Core is not { } core || !StoreRelay.IsStorePage(tab.Address)) return;
        var state = JsonSerializer.Serialize(new
        {
            installed = Extensions.Shared.Installed.Select(i => i.Id).ToArray(),
            busy = Extensions.Shared.Busy,
        });
        _ = core.ExecuteScriptAsync($"window.__officeStore && window.__officeStore.state({state})");
    }
}
