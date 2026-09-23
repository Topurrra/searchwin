using System.Runtime.CompilerServices;
using System.Text.Json;

namespace Search;

// What the page says about itself that a browser has to know: where the
// keyboard is, and whether there is a sign-in on it — and when one has just
// been sent, so the password can be offered a place in Credential Manager.
//
// Filling goes through the field's own setter and fires the events a keystroke
// would. Assigning to .value behind a framework's back leaves it thinking the
// box is still empty, which is a sign-in button that stays grey.
public static class FormRelay
{
    public const string Name = "officeForms";

    /// What the page said, handed to the tab and the browser.
    public static void Take(Browser browser, Tab tab, JsonElement body)
    {
        if (body.ValueKind != JsonValueKind.Object || !body.TryGetProperty("kind", out var kindEl)) return;
        switch (kindEl.GetString())
        {
            case "submit":
                tab.SentSignIn(Text(body, "user"), Text(body, "password"));
                break;
            case "settled":
                tab.SettleSignIn(browser, navigated: false);
                break;
            case "focus":
                tab.Typing = body.TryGetProperty("typing", out var typing) && typing.ValueKind == JsonValueKind.True;
                // Which sign-in box the caret is in, and where it sits on the
                // page — so a list of accounts can hang from it.
                if (body.TryGetProperty("rect", out var rect) && rect.ValueKind == JsonValueKind.Object
                    && Number(rect, "x") is { } x && Number(rect, "y") is { } y
                    && Number(rect, "w") is { } w && Number(rect, "h") is { } h)
                    tab.FieldFocused(browser, new Windows.Foundation.Rect(x, y, Math.Max(0, w), Math.Max(0, h)));
                else
                    tab.FieldFocused(browser, null);
                break;
            // "form" and "fullscreen" are said too. Nothing here listens for
            // the first; the second WebView2 reports for itself, and more
            // surely than the page (see Tab.Immersed).
        }
    }

    private static string Text(JsonElement body, string name) =>
        body.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() ?? "" : "";

    private static double? Number(JsonElement body, string name) =>
        body.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.Number ? v.GetDouble() : null;

    /// Whether to keep claiming passkeys are possible here.
    ///
    /// On Windows they are: WebView2 hands a passkey request to Windows Hello
    /// and to a phone nearby, as Edge does. The setting stays for the sites
    /// that offer the passkey path and strand you on it anyway — taking the
    /// object away sends them straight to the password. Set from Settings.
    public static bool PasskeysOffered { get; set; } = true;

    /// Only the passkey object goes. navigator.credentials itself stays: sites
    /// use it for stored passwords too, and that half still works.
    public const string WithoutPasskeys = """
    (function () {
      try {
        Object.defineProperty(window, 'PublicKeyCredential', {
          value: undefined, configurable: true, writable: true
        });
      } catch (e) {
        try { delete window.PublicKeyCredential; } catch (ignored) {}
      }
    })();
    """;

    public const string Script = """
    (function () {
      if (window.__officeForms) return;

      // The password box, and the last box before it that could hold a name.
      function pair() {
        var boxes = document.querySelectorAll('input[type="password"]');
        var pass = null;
        for (var p = 0; p < boxes.length; p++) {
          var b = boxes[p];
          var r = b.getBoundingClientRect();
          if (r.width > 0 && r.height > 0) { pass = b; break; }
        }
        if (!pass) return null;
        var scope = pass.form || (pass.closest && pass.closest('form')) || document;
        var all = scope.querySelectorAll('input');
        var user = null;
        for (var i = 0; i < all.length; i++) {
          if (all[i] === pass) break;
          var kind = (all[i].type || 'text').toLowerCase();
          if (kind === 'text' || kind === 'email' || kind === 'tel') user = all[i];
        }
        return { user: user, pass: pass };
      }

      function put(box, value) {
        if (!box) return;
        var setter = Object.getOwnPropertyDescriptor(
          window.HTMLInputElement.prototype, 'value'
        );
        if (setter && setter.set) { setter.set.call(box, value); } else { box.value = value; }
        box.dispatchEvent(new Event('input', { bubbles: true }));
        box.dispatchEvent(new Event('change', { bubbles: true }));
      }

      // What was typed by hand and not yet sent, box by box. A page whose
      // boxes still hold it is not put to sleep: waking it couldn't bring
      // that back. A box emptied by sending — a chat's composer — no longer
      // counts, and neither does a search box.
      var typed = [];
      document.addEventListener('input', function (e) {
        if (!e.isTrusted) return;
        var el = e.target;
        if (!el || typed.indexOf(el) >= 0) return;
        typed.push(el);
        if (typed.length > 40) typed.shift();
      }, true);
      function unsaved() {
        for (var i = 0; i < typed.length; i++) {
          var el = typed[i];
          if (!el.isConnected) continue;
          var tag = (el.tagName || '').toLowerCase();
          if (tag === 'textarea') {
            if (el.value.trim() && el.value !== el.defaultValue) return true;
          } else if (tag === 'input') {
            var kind = (el.type || 'text').toLowerCase();
            if (['text', 'email', 'url', 'tel', 'number'].indexOf(kind) < 0) continue;
            if (el.value.trim() && el.value !== el.defaultValue) return true;
          } else if (el.isContentEditable) {
            if ((el.textContent || '').trim()) return true;
          }
        }
        return false;
      }

      window.__officeForms = {
        unsaved: unsaved,
        fill: function (user, password) {
          var both = pair();
          if (!both) return false;
          if (both.user && !both.user.value) put(both.user, user);
          put(both.pass, password);
          return true;
        },
        // Whether there is still a sign-in on the page. Asked after a
        // password went out, to tell a sign-in that took from one refused.
        hasPassword: function () { return !!pair(); }
      };

      // What is in the boxes when they are sent. Said every time — a click
      // on "show password" says it too — because the browser only listens
      // once the page has moved on, and keeps the last thing it heard.
      function offer() {
        var both = pair();
        if (!both || !both.pass.value) return;
        window.webkit.messageHandlers.officeForms.postMessage({
          kind: 'submit',
          user: both.user ? both.user.value : '',
          password: both.pass.value
        });
      }

      document.addEventListener('submit', offer, true);
      document.addEventListener('keydown', function (e) {
        if (e.key !== 'Enter') return;
        var both = pair();
        if (both && (document.activeElement === both.pass || document.activeElement === both.user)) offer();
      }, true);
      // Plenty of sign-in buttons aren't in a form and never fire submit.
      document.addEventListener('click', function (e) {
        var el = e.target;
        if (!el || !el.closest) return;
        if (el.closest('button, input[type="submit"], [role="button"]')) {
          setTimeout(offer, 0);
        }
      }, true);

      var told = false;
      function tell() {
        if (told || !pair()) return;
        told = true;
        window.webkit.messageHandlers.officeForms.postMessage({ kind: 'form' });
      }
      if (document.readyState === 'complete') { tell(); }
      else { window.addEventListener('load', tell); }
      // A form the page builds for itself, a moment after it loads — or the
      // password step of a sign-in that asks for the name first.
      setTimeout(tell, 700);
      setTimeout(tell, 2200);
      // The boxes going away without a new page — a sign-in done in place —
      // is the other way a sign-in shows it took.
      var settling = null;
      new MutationObserver(function () {
        if (!told) { tell(); return; }
        if (pair()) return;
        told = false;
        clearTimeout(settling);
        settling = setTimeout(function () {
          if (pair()) return;
          window.webkit.messageHandlers.officeForms.postMessage({ kind: 'settled' });
        }, 400);
      }).observe(document.documentElement, { childList: true, subtree: true });

      // Whether the caret is somewhere on the page that takes typing.
      //
      // The browser gives Tab to its own row of tabs, which is right until you
      // are filling something in: plenty of fields offer a completion you take
      // with Tab, and stealing the key there would make them unusable.
      function editable(el) {
        if (!el) return false;
        var tag = (el.tagName || '').toLowerCase();
        if (tag === 'textarea') return true;
        if (el.isContentEditable === true) return true;
        if (tag !== 'input') return false;
        var kind = (el.type || 'text').toLowerCase();
        return ['text', 'search', 'email', 'url', 'tel', 'password', 'number',
                'date', 'datetime-local', 'month', 'week', 'time'].indexOf(kind) >= 0;
      }

      function caret() {
        var el = document.activeElement;
        var both = pair();
        var rect = null;
        if (both && el && (el === both.user || el === both.pass)) {
          var r = el.getBoundingClientRect();
          if (r.width > 0 && r.height > 0) rect = { x: r.left, y: r.top, w: r.width, h: r.height };
        }
        window.webkit.messageHandlers.officeForms.postMessage({
          kind: 'focus',
          typing: editable(el),
          rect: rect
        });
      }

      // The box moves when the page scrolls or the window changes size, and
      // whatever hangs from it has to move too. Once a frame at most.
      var moving = false;
      function moved() {
        if (moving) return;
        moving = true;
        requestAnimationFrame(function () { moving = false; caret(); });
      }
      window.addEventListener('scroll', moved, true);
      window.addEventListener('resize', moved);

      // Going full screen, announced before it happens rather than after.
      function immersed() {
        var on = !!(document.fullscreenElement || document.webkitFullscreenElement);
        window.webkit.messageHandlers.officeForms.postMessage({
          kind: 'fullscreen', on: on
        });
      }
      document.addEventListener('fullscreenchange', immersed, true);
      document.addEventListener('webkitfullscreenchange', immersed, true);

      // The asking, caught before the animation starts.
      ['requestFullscreen', 'webkitRequestFullscreen', 'webkitRequestFullScreen']
        .forEach(function (name) {
          var was = Element.prototype[name];
          if (!was) return;
          Element.prototype[name] = function () {
            window.webkit.messageHandlers.officeForms.postMessage({
              kind: 'fullscreen', on: true
            });
            return was.apply(this, arguments);
          };
        });

      document.addEventListener('focusin', caret, true);
      document.addEventListener('focusout', function () { setTimeout(caret, 0); }, true);
      document.addEventListener('mouseup', function () { setTimeout(caret, 0); }, true);
      caret();
    })();
    """;
}

/// A tab's side of a sign-in: what it has just sent, and putting a kept one
/// back. Held beside the tab rather than on it, for as long as the tab lives.
public static class SignIn
{
    /// A name and password the page has just sent — held, not yet offered.
    /// Whether the sign-in worked is only known afterwards: a page that
    /// comes back without a password box took it, one that still has the
    /// box refused it, and only the first is worth remembering.
    private sealed record Sent(string Host, string User, string Password, DateTime At);

    private sealed class Held { public Sent? Sent; }

    private static readonly ConditionalWeakTable<Tab, Held> held = new();

    public static void SentSignIn(this Tab tab, string user, string password)
    {
        // The host now, while the page is still the sign-in page: a moment
        // later it may be somewhere else entirely, and that is not where the
        // password belongs.
        if (Curtain.Host(tab.Address) is not { } host) return;
        held.GetOrCreateValue(tab).Sent = new Sent(host, user, password, DateTime.UtcNow);
    }

    /// The page has moved on — a new document has loaded, or the sign-in
    /// fields have gone. If a password went out recently and there is no
    /// longer a box for it, that is a sign-in that took.
    ///
    /// A new document is judged at once. Fields that a page removed by itself
    /// are given a moment first: a sign-in built into the page closes its
    /// form the instant you press the button and puts it back if the server
    /// says no — and offering in between is offering a password that may be
    /// wrong.
    public static async void SettleSignIn(this Tab tab, Browser browser, bool navigated = true)
    {
        if (!held.TryGetValue(tab, out var state) || state.Sent is not { } sent) return;
        if ((DateTime.UtcNow - sent.At).TotalSeconds >= 45)
        {
            state.Sent = null;
            return;
        }
        if (!navigated)
        {
            UI.After(1.5, () =>
            {
                // Only if nothing newer went out in the meantime.
                if (state.Sent?.At == sent.At) tab.SettleSignIn(browser, navigated: true);
            });
            return;
        }
        var still = await tab.Eval("!!(window.__officeForms && window.__officeForms.hasPassword())");
        if (state.Sent is not { } now) return;
        // The box is still there: a refused sign-in, or the second step of
        // one. Kept for a moment longer, in case the page is still on its way.
        if (still is { ValueKind: JsonValueKind.True }) return;
        state.Sent = null;
        browser.Credentials(tab, now.Host, now.User, now.Password);
    }

    /// From the page, in CSS pixels; passed on in the window's own. Page zoom
    /// is the only scale between the two that matters here.
    public static void FieldFocused(this Tab tab, Browser browser, Windows.Foundation.Rect? rect)
    {
        if (rect is not { } r)
        {
            browser.Field(tab, null);
            return;
        }
        var zoom = tab.Zoom > 0 ? tab.Zoom : 1;
        browser.Field(tab, new Windows.Foundation.Rect(r.X * zoom, r.Y * zoom, r.Width * zoom, r.Height * zoom));
    }

    /// Puts a remembered name and password where a person would have typed
    /// them. Nothing is echoed back and nothing is written down here. Hears
    /// back false for the one case worth saying something about: the sign-in
    /// fields that were there a moment ago, when this was offered, are gone by
    /// the time it actually runs.
    public static async Task<bool> Fill(this Tab tab, string user, string password)
    {
        var result = await tab.Eval(
            $"!!(window.__officeForms && window.__officeForms.fill({Bridge.Literal(user)}, {Bridge.Literal(password)}))");
        return result is { ValueKind: JsonValueKind.True };
    }
}
