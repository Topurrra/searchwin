// fish-probe.js — FishCatcher's page probe, for Search (docs/brain/FishCatcher Port.md).
//
// Adapted from FishCatcher's own probe (Extensions/fishcatcher/src/probe/probe.js,
// MIT, the same authors) and the three text matchers it was bundled with
// (engine/aitm.js, engine/devicecode.js, engine/scampacks.js). The extension
// handed its facts to a service worker; here they go to the browser over the
// WebView2 bridge as { name: 'fish.facts', body }, and Search.Kit's C# port of
// the engine (SearchKit.FishCatcher) turns them into a verdict.
//
// What leaves the page is DERIVED FACTS only — which kinds of sign-in the
// page asks for, the page's own claims about who it is (title, og:site_name,
// logo alt text, form labels), which hosts its resources come from, and a few
// booleans. Never what you typed, never the page's text.
//
// The extension's panel pieces (link scan, QR lookup, strict-mode banner,
// the device-code notice) aren't here: Search draws its own warnings.
//
// Same two-faced file as the Shields scripts: `require()`-able from Node for
// the pure logic, self-wiring when evaluated raw in a page.
(function (root) {
  'use strict';

  var NAME = 'fish.facts';

  // MARK: - text matchers (engine/aitm.js, devicecode.js, scampacks.js)

  // One control's accessible text → interaction kind. First match wins.
  // Narrower prompts before the bare "password" fallback, so "one-time
  // password" reads as otp.
  var AITM_KIND_PATTERNS = [
    ['otp', /\b(one[\s-]?time (code|password|passcode)|verification code|security code|6[\s-]?digit code|enter the code)\b|ერთჯერად|код подтвержд/i],
    ['mfaApproval', /\b((approve|deny|verify) (this )?(sign[\s-]?in|request|login)|open your authenticator|check your (phone|authenticator)|number matching|(enter|tap|select|match) the number( shown)?)\b/i],
    ['mfaMigration', /\b((migrate|re[\s-]?enroll|re[\s-]?register|update|move) your (mfa|authenticator|2fa|multi[\s-]?factor)|authenticator migration)\b/i],
    ['passkey', /\b(set ?up|create|add|register|enroll|use|sign in with) (a )?(passkey|security key|fido2? key|face ?id|touch ?id|windows hello)\b/i],
    ['ssoSetup', /\b(set ?up|configure|enable) (single sign[\s-]?on|sso|federation|saml|oidc)\b/i],
    ['qrAuth', /\bscan (this|the) qr( code)?\b.{0,40}\b(sign in|log ?in|authenticate|link|pair)\b/i],
    ['helpdeskUpgrade', /\b((security|account|mfa|password) (upgrade|migration|re[\s-]?validation|re[\s-]?verification) (required|needed)|verify your account to continue)\b/i],
    ['password', /\b(pass(word|phrase))\b|პაროლ|парол/i]
  ];

  function aitmKindForText(text) {
    if (!text) return null;
    for (var i = 0; i < AITM_KIND_PATTERNS.length; i++) {
      if (AITM_KIND_PATTERNS[i][1].test(text)) return AITM_KIND_PATTERNS[i][0];
    }
    return null;
  }

  // Device-code phishing: a device-login address, an instruction to type a
  // code, and a code itself — all three, so pages explaining the trick stay
  // quiet. A title that says it's about phishing is a lesson, not a lure.
  var DC_ENTRY = /(microsoft\.com\/link|devicelogin|google\.com\/device|amazon\.com\/code)/;
  var DC_INSTRUCT = /(enter|input|type|use|შეიყვან|введи|введите).{0,60}(code|კოდი|код)/;
  var DC_CODE_NEAR = /code[^.\n]{0,40}\b([A-Z0-9]{4}-[A-Z0-9]{4,5}|[A-Z0-9]{8,9})\b|\b([A-Z0-9]{4}-[A-Z0-9]{4,5}|[A-Z0-9]{8,9})\b[^.\n]{0,40}code/;
  var DC_EDUCATIONAL_TITLE = /phish|scam|fraud|attack|security|how to spot|awareness/i;

  function matchDeviceCodeScam(text, title) {
    if (DC_EDUCATIONAL_TITLE.test(title || '')) return false;
    var t = String(text).toLowerCase();
    if (!DC_ENTRY.test(t) || !DC_INSTRUCT.test(t)) return false;
    return DC_CODE_NEAR.test(text);
  }

  // A wallet secret asked for: an imperative verb next to the noun, in one
  // sentence, and no negation just before it ("we will never ask you to
  // enter your seed phrase" is what real wallets say).
  var SCAM_SEED_RE =
    /(enter|input|type|paste|import|confirm|validate|verify|provide|re-?enter)[^.!?\n]{0,30}(secret recovery phrase|seed phrase|seed words|recovery phrase|secret phrase|mnemonic( phrase)?|private key)/g;
  var SCAM_SEED_NEG = /\b(never|not|avoid|nobody|no one|don'?t|won'?t|can'?t|will not|should not|shouldn'?t)\b/;

  function scamCryptoSeedText(text) {
    if (!text) return false;
    var t = String(text).toLowerCase();
    SCAM_SEED_RE.lastIndex = 0;
    var m;
    while ((m = SCAM_SEED_RE.exec(t))) {
      if (!SCAM_SEED_NEG.test(t.slice(Math.max(0, m.index - 24), m.index))) return true;
    }
    return false;
  }

  // A fake "your computer is locked" page: a scare AND a call to action.
  var SCAM_TECH_SCARE =
    /(your |this )?(computer|pc|laptop|windows|mac|device|system)\b[^.!?\n]{0,40}\b(infected|locked|blocked|compromised|hacked|disabled|suspended|at risk)\b|(virus|trojan|spyware|malware|ransomware)[^.!?\n]{0,20}\b(detected|found|infection)\b|security (alert|warning|breach)|suspicious (sign[\s-]?in|activity|login)|windows defender|do ?n'?t (restart|close|shut|turn off|power off)|do not (restart|close|shut|turn off|power off)|ვირუს|კომპიუტერი (დაბლოკ|ვირუს)|заблокирован|заражен|вирус/;
  var SCAM_TECH_CTA =
    /\bcall\b[^.!?\n]{0,30}\b(support|technician|help ?line|number|toll|microsoft|apple|windows|now|immediately)\b|\b(contact|dial|phone)\b[^.!?\n]{0,25}\b(support|technician|help ?line|number|toll|microsoft|apple)\b|toll[\s-]?free|\b1[\s.\-]?\(?8(00|88|77|66|55|44|33)\)?[\s.\-]?\d{3}[\s.\-]?\d{4}\b|დარეკ|позвони/;

  function scamTechSupportText(text) {
    if (!text) return false;
    var t = String(text).toLowerCase();
    return SCAM_TECH_SCARE.test(t) && SCAM_TECH_CTA.test(t);
  }

  // MARK: - what the page shows (probe.js collectAitm / collectScam)

  var MAX_TEXT = 200000;

  function visible(el) {
    return el.offsetParent !== null && !el.disabled && el.type !== 'hidden';
  }

  function accText(el) {
    return String((el.getAttribute && el.getAttribute('aria-label')) || el.value || el.textContent ||
      (el.getAttribute && el.getAttribute('placeholder')) || '').trim();
  }

  function hostOf(url, base) {
    try {
      var u = new URL(url, base);
      return /^https?:$/.test(u.protocol) ? u.hostname : null;
    } catch (e) {
      return null;
    }
  }

  function all(doc, selector) {
    try { return Array.prototype.slice.call(doc.querySelectorAll(selector)); } catch (e) { return []; }
  }

  // One read of innerText: each read lays the page out and walks it again.
  function bodyText(doc) {
    var body = doc.body;
    var text = body ? body.innerText : '';
    return typeof text === 'string' ? text : '';
  }

  /**
   * The AiTM facts, or null when the page asks for no identity interaction at
   * all — ordinary pages (and pages that merely mention passwords) say nothing.
   * `text` is the page's text when the caller has read it already ('' to
   * leave the text alone); left out, it is read here.
   */
  function collectAitm(doc, win, text) {
    var here = win.location.href;
    var kinds = [];
    function add(k) { if (k && kinds.indexOf(k) === -1) kinds.push(k); }

    // Password: structural, active input only.
    var passwords = all(doc, 'input[type="password"]');
    for (var i = 0; i < passwords.length; i++) {
      if (visible(passwords[i])) { add('password'); break; }
    }

    // One-time codes: autocomplete=one-time-code is the strong tell; other
    // short numeric fields are read by their text.
    var codes = all(doc, 'input[autocomplete="one-time-code"], input[inputmode="numeric"][maxlength], input[name*="otp" i], input[id*="otp" i], input[name*="code" i][maxlength]');
    for (var j = 0; j < codes.length; j++) {
      var inp = codes[j];
      if (!visible(inp)) continue;
      if (inp.getAttribute('autocomplete') === 'one-time-code') { add('otp'); continue; }
      add(aitmKindForText(accText(inp)));
    }

    // Actionable controls only, so prose that mentions a flow doesn't count.
    var controls = all(doc, 'form button, input[type="submit"], [role="button"], form legend, form label, form h1, form h2, form h3');
    for (var k = 0; k < controls.length; k++) {
      if (!visible(controls[k])) continue;
      add(aitmKindForText(accText(controls[k])));
    }

    if (text === undefined) text = bodyText(doc);
    if (text && text.length < MAX_TEXT && matchDeviceCodeScam(text, doc.title)) add('deviceCode');

    if (!kinds.length) return null;

    // Who the page says it is — never the whole text.
    var title = String(doc.title || '').slice(0, 150);
    var og = doc.querySelector && doc.querySelector('meta[property="og:site_name"], meta[name="og:site_name"]');
    var ogSiteName = String((og && og.content) || '').slice(0, 80);
    var logoAlts = [];
    var logos = all(doc, 'header img[alt], [class*="logo" i] img[alt], img[class*="logo" i][alt], img[id*="logo" i][alt]');
    for (var l = 0; l < logos.length && logoAlts.length < 10; l++) {
      var alt = String(logos[l].getAttribute('alt') || '').trim().slice(0, 80);
      if (alt && logoAlts.indexOf(alt) === -1) logoAlts.push(alt);
    }
    var brandTokens = [];
    var labels = all(doc, 'form legend, form label, form h1, form h2, form h3');
    for (var b = 0; b < labels.length && brandTokens.length < 40; b++) {
      var token = String(labels[b].textContent || '').trim().toLowerCase().slice(0, 80);
      if (token && brandTokens.indexOf(token) === -1) brandTokens.push(token);
    }

    // Where the page's pieces come from: host → count, at most 40 hosts.
    var resourceHosts = {};
    var hostCount = 0;
    function addHost(h) {
      if (!h) return;
      if (!Object.prototype.hasOwnProperty.call(resourceHosts, h)) {
        if (hostCount >= 40) return;
        hostCount++;
        resourceHosts[h] = 0;
      }
      resourceHosts[h]++;
    }
    all(doc, 'script[src], img[src], iframe[src]').forEach(function (el) { addHost(hostOf(el.src, here)); });
    all(doc, 'link[href]').forEach(function (el) { addHost(hostOf(el.href, here)); });
    all(doc, 'form[action]').forEach(function (el) { addHost(hostOf(el.getAttribute('action'), here)); });
    try {
      var perf = win.performance && win.performance.getEntriesByType ? win.performance.getEntriesByType('resource') : [];
      for (var p = 0; p < perf.length; p++) addHost(hostOf(perf[p].name, here));
    } catch (e) { /* no performance API */ }

    // The site icon or manifest from somewhere other than the page.
    var ownHost = win.location.hostname;
    var faviconCrossOrigin = false;
    var icons = all(doc, 'link[rel~="icon"], link[rel="manifest"]');
    for (var f = 0; f < icons.length; f++) {
      var ih = hostOf(icons[f].href, here);
      if (ih && ih !== ownHost) { faviconCrossOrigin = true; break; }
    }

    // Where a visible password or code form sends what's typed.
    // getAttribute, not form.action: a field named "action" shadows it.
    var formActions = [];
    var forms = all(doc, 'form[action]');
    for (var q = 0; q < forms.length && formActions.length < 10; q++) {
      var field = forms[q].querySelector('input[type="password"], input[autocomplete="one-time-code"], input[name*="otp" i], input[id*="otp" i]');
      if (!field || !visible(field)) continue;
      var fh = hostOf(forms[q].getAttribute('action'), here);
      if (fh && fh !== ownHost && formActions.indexOf(fh) === -1) formActions.push(fh);
    }

    return {
      interactions: kinds,
      identityHints: { title: title, ogSiteName: ogSiteName, logoAlts: logoAlts, brandTokens: brandTokens },
      resourceHosts: resourceHosts,
      faviconCrossOrigin: faviconCrossOrigin,
      formActions: formActions
    };
  }

  var SECRET_RE = /seed phrase|recovery phrase|mnemonic|private key|secret phrase/i;
  var TOLL_FREE = /\b1[\s.\-]?\(?8(?:00|88|77|66|55|44|33)\)?[\s.\-]?\d{3}[\s.\-]?\d{4}\b/;

  /** The scam-pack booleans, or null on an ordinary page. `text` as for collectAitm. */
  function collectScam(doc, win, whole) {
    if (whole === undefined) whole = bodyText(doc);
    var text = whole.length < MAX_TEXT ? whole : whole.slice(0, MAX_TEXT);
    var cryptoSeed = scamCryptoSeedText(text);
    var techScare = scamTechSupportText(text);

    // A visible field named for a wallet secret. The phrase, not a bare
    // "seed": game world seeds and database seeders use that word too.
    var seedInput = false;
    var fields = all(doc, 'input:not([type=hidden]), textarea');
    for (var i = 0; i < fields.length; i++) {
      var el = fields[i];
      if (el.offsetParent === null || el.disabled) continue;
      var label = (el.labels && el.labels[0] && el.labels[0].textContent) ||
        (el.closest && el.closest('label') && el.closest('label').textContent) || '';
      var near = (el.getAttribute('aria-label') || '') + ' ' + (el.placeholder || '') + ' ' + (el.name || '') + ' ' + (el.id || '') + ' ' + label;
      if (SECRET_RE.test(near)) { seedInput = true; break; }
    }

    if (!(cryptoSeed || seedInput || techScare)) return null;

    // The heavier look at overlays only once the scare has tripped.
    var phone = false;
    var fullscreen = false;
    if (techScare) {
      phone = TOLL_FREE.test(text);
      fullscreen = !!doc.fullscreenElement || coveredByOverlay(doc, win);
    }
    return { cryptoSeed: cryptoSeed, seedInput: seedInput, techScare: techScare, phone: phone, fullscreen: fullscreen };
  }

  /**
   * A fixed or absolute box over nearly the whole window. Anything that
   * covers 90% of the window, and is in it, covers its middle: so only the
   * handful of elements stacked at that one point are looked at, not every
   * div on the page (probe.js styled and measured each of them).
   */
  function coveredByOverlay(doc, win) {
    if (typeof win.getComputedStyle !== 'function' || typeof doc.elementsFromPoint !== 'function') return false;
    var vw = win.innerWidth;
    var vh = win.innerHeight;
    var stack;
    try { stack = doc.elementsFromPoint(vw / 2, vh / 2) || []; } catch (e) { return false; }
    for (var j = 0; j < stack.length && j < 64; j++) {
      var s = win.getComputedStyle(stack[j]);
      if (s.position !== 'fixed' && s.position !== 'absolute') continue;
      if (s.display === 'none' || s.visibility === 'hidden') continue;
      var r = stack[j].getBoundingClientRect();
      if (r.width >= vw * 0.9 && r.height >= vh * 0.9) return true;
    }
    return false;
  }

  /**
   * Everything worth telling the browser about this page, or null when
   * there is nothing: most pages. `href` lets the browser drop facts that
   * arrive after the tab has moved on.
   *
   * The page's text is read once, for both (innerText lays the page out, so
   * it is the costly part), and only when `readText` isn't false: the first
   * look, while the page is still arriving, is structural only.
   */
  function collect(doc, win, readText) {
    var aitm = null;
    var scam = null;
    var text = '';
    if (readText !== false) {
      try { text = bodyText(doc); } catch (e) { text = ''; }
    }
    try { aitm = collectAitm(doc, win, text); } catch (e) { aitm = null; }
    try { scam = collectScam(doc, win, text); } catch (e) { scam = null; }
    if (!aitm && !scam) return null;
    var facts = { href: String(win.location.href) };
    if (aitm) facts.aitm = aitm;
    if (scam) facts.scam = scam;
    return facts;
  }

  function buildMessage(facts) {
    return { name: NAME, body: facts };
  }

  /**
   * Captures `chrome.webview.postMessage`, bound to its bridge object, right
   * now — meant to be called the moment this script runs (document created,
   * before any page script), so the reference can't be muted later. A page
   * can still overwrite `window.chrome.webview.postMessage` after we've run
   * (it's a plain writable/configurable property), but that only replaces
   * what `win.chrome.webview.postMessage` resolves to *afterwards*; the
   * function value captured here is unaffected. Returns null when there is
   * no bridge to capture.
   */
  function captureSender(win) {
    try {
      var bridge = win && win.chrome && win.chrome.webview;
      var send = bridge && bridge.postMessage;
      if (typeof send !== 'function') return null;
      // Bound now with the original bind: calling it later mustn't reach for
      // Function.prototype.call, which a page can replace.
      var bound = BIND.call(send, bridge);
      return function (message) { bound(message); };
    } catch (e) { /* never throw into the page */ }
    return null;
  }

  /**
   * Posts to the browser; returns whether it could. Never throws. `sender`,
   * when given, is a value from `captureSender` and is used instead of
   * looking `chrome.webview.postMessage` up again on `win` — the whole point
   * being that a page can't mute an already-captured sender by reassigning
   * that property later.
   */
  function post(win, facts, sender) {
    try {
      if (typeof sender === 'function') {
        sender(buildMessage(facts));
        return true;
      }
      var bridge = win && win.chrome && win.chrome.webview;
      if (bridge && typeof bridge.postMessage === 'function') {
        bridge.postMessage(buildMessage(facts));
        return true;
      }
    } catch (e) { /* never throw into the page */ }
    return false;
  }

  // MARK: - when to look

  // A look when the document is parsed, again a moment after it has loaded
  // (sign-in pages often draw their form late), and when a password or code
  // field is first focused. Each posts only if it found something new, and
  // there are never more than a handful of looks per page.
  //
  // Reading the page's text lays the whole page out, so the first look
  // leaves the text alone (fields and forms only: a sign-in page is caught
  // there); the text is read once the page has loaded and settled, when the
  // browser is idle (or a few seconds after it was parsed, for a page that
  // never finishes loading), and on that first focus.
  // The built-ins the probe leans on after the page's own scripts have run,
  // taken while it still has them: a page can overwrite any of these on
  // the page's globals to mute the probe.
  var BIND = Function.prototype.bind;
  var STRINGIFY = JSON.stringify;

  var SETTLE_MS = 1200;
  var STUCK_MS = 5000;
  var IDLE_MS = 1000;
  var MAX_LOOKS = 6;

  function init(win) {
    try { start(win || (typeof window !== 'undefined' ? window : undefined)); } catch (e) { /* never throw into the page */ }
  }

  function start(win) {
    if (!win || !win.document || !win.location) return;
    if (!/^https?:$/.test(win.location.protocol)) return;
    var doc = win.document;
    var said = '';
    var looks = 0;
    // Captured now, at document-created time, before any page script has had
    // a chance to run — see captureSender's own doc comment.
    var sender = captureSender(win);

    function look(readText) {
      if (looks >= MAX_LOOKS) return;
      looks++;
      try {
        var facts = collect(doc, win, readText);
        if (!facts) return;
        var text = STRINGIFY(facts);
        if (text === said) return;
        said = text;
        post(win, facts, sender);
      } catch (e) { /* never throw into the page */ }
    }

    // The timers too, taken now: a page replacing setTimeout or
    // requestIdleCallback later can't stop the settled look.
    var timer = typeof win.setTimeout === 'function' ? BIND.call(win.setTimeout, win) : null;
    var idle = typeof win.requestIdleCallback === 'function' ? BIND.call(win.requestIdleCallback, win) : null;
    function later(fn, ms) {
      if (timer) timer(fn, ms);
      else setTimeout(fn, ms);
    }
    function whenIdle() {
      if (idle) idle(function () { look(true); }, { timeout: IDLE_MS });
      else look(true);
    }
    // The text look, once: after load has settled, or when load is stuck.
    var settling = false;
    function settle(ms) {
      if (settling) return;
      settling = true;
      later(whenIdle, ms);
    }
    function parsed() {
      look(false);
      later(function () { settle(0); }, STUCK_MS);
    }

    try {
      if (doc.readyState === 'loading') doc.addEventListener('DOMContentLoaded', parsed, { once: true });
      else parsed();
      if (doc.readyState === 'complete') settle(SETTLE_MS);
      else win.addEventListener('load', function () { settle(SETTLE_MS); }, { once: true });
      var focused = false;
      doc.addEventListener('focusin', function (e) {
        if (focused) return;
        var t = e.target;
        if (!t || t.tagName !== 'INPUT') return;
        if (t.type !== 'password' && t.getAttribute('autocomplete') !== 'one-time-code') return;
        focused = true;
        look(true);
      }, true);
    } catch (e) { /* never throw into the page */ }
  }

  var api = {
    NAME: NAME,
    aitmKindForText: aitmKindForText,
    matchDeviceCodeScam: matchDeviceCodeScam,
    scamCryptoSeedText: scamCryptoSeedText,
    scamTechSupportText: scamTechSupportText,
    collectAitm: collectAitm,
    collectScam: collectScam,
    collect: collect,
    buildMessage: buildMessage,
    captureSender: captureSender,
    post: post,
    init: init
  };

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  } else {
    // Nothing is left on the page's window: a page has no business reading
    // what the probe thinks of it.
    init(root);
  }
})(typeof window !== 'undefined' ? window : this);
