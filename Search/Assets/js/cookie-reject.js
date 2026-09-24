// cookie-reject.js — Search's Shields, cookie banners (docs/brain/Ideas/Agentic
// Browser.md, Part 1: "Block cookie consent notices ... a small auto-reject
// script, never 'accept'").
//
// Runs on every page, main-world, before page scripts. Tries known consent
// management platform (CMP) APIs first, in order; falls back to finding and
// clicking a reject-ish button by its text, in several languages; and, after
// a bounded wait, hides whatever banner is still on screen with CSS. It never
// clicks or calls anything that means "accept" — worst case is a banner stays
// up and the CSS fallback hides it.
//
// Same shape as the other Shields scripts: `require()`-able from Node for the
// pure logic, self-wiring when evaluated raw in a page.
//
// LIMITS (see also "Document limits" in the task this shipped from):
//  - Only four CMPs have a public, stable, single-call "reject everything"
//    method we're confident enough to call blind: OneTrust, Cookiebot,
//    Didomi, Usercentrics. Quantcast Choice/the generic IAB TCF
//    (`__tcfapi`), TrustArc and Sourcepoint don't expose one consistent
//    documented call across their versions — calling a guessed method name
//    on those risks doing nothing or, worse, something we didn't intend.
//    They're handled by the button-text fallback only, same as any other
//    unrecognised banner.
//  - The button-text fallback is intentionally conservative: it matches a
//    short, curated phrase list and skips anything whose text also looks
//    like an accept action, even if a reject phrase matches too. It will
//    under-fire (miss a banner) far more often than it will click "accept".
//  - Bounded by `WAIT_TIMEOUT_MS`: this never becomes an infinite observer.
(function (root) {
  'use strict';

  // ---- CMP API handlers: only ones with a stable, documented, single call.
  // Each takes a window-like object and returns whether it acted.
  function oneTrust(win) {
    var OT = win && win.OneTrust;
    if (OT && typeof OT.RejectAll === 'function') { OT.RejectAll(); return true; }
    return false;
  }

  function cookiebot(win) {
    var CB = win && win.Cookiebot;
    if (CB && typeof CB.decline === 'function') { CB.decline(); return true; }
    return false;
  }

  function didomi(win) {
    var D = win && win.Didomi;
    if (D && typeof D.setUserDisagreeToAll === 'function') { D.setUserDisagreeToAll(); return true; }
    return false;
  }

  function usercentrics(win) {
    var UC = win && win.UC_UI;
    if (UC && typeof UC.denyAllConsents === 'function') { UC.denyAllConsents(); return true; }
    return false;
  }

  // Order doesn't matter much (a page only runs one CMP), but keep it stable
  // for predictable tests.
  var CMP_HANDLERS = [
    { name: 'OneTrust', reject: oneTrust },
    { name: 'Cookiebot', reject: cookiebot },
    { name: 'Didomi', reject: didomi },
    { name: 'Usercentrics', reject: usercentrics },
  ];

  /** Tries every known CMP handler; returns the name of the one that acted, or null. */
  function runCmpHandlers(win) {
    for (var i = 0; i < CMP_HANDLERS.length; i++) {
      try {
        if (CMP_HANDLERS[i].reject(win)) return CMP_HANDLERS[i].name;
      } catch (e) { /* that CMP's API misbehaved; try the next one */ }
    }
    return null;
  }

  // ---- Button-text fallback ----------------------------------------------
  // Short and deliberately narrow: "reject", "decline", "necessary only" and
  // their equivalents in a handful of languages. No word here is also a
  // plausible substring of an accept phrase.
  var REJECT_PHRASES = [
    // English
    'reject all', 'reject', 'decline', 'necessary only', 'only necessary',
    'essential only', 'only essential', 'deny', 'disagree', 'refuse all',
    'refuse', 'reject and close',
    // German
    'ablehnen', 'alle ablehnen', 'nur notwendige', 'nur erforderliche',
    // French
    'refuser', 'tout refuser', 'refuser tout', 'nécessaire uniquement',
    'uniquement nécessaire',
    // Spanish
    'rechazar', 'rechazar todo', 'solo necesarias', 'solo esenciales',
    // Italian
    'rifiuta', 'rifiuta tutto', 'solo necessari', 'solo essenziali',
    // Portuguese
    'rejeitar', 'rejeitar tudo', 'apenas necessários', 'somente necessários',
    // Dutch
    'weigeren', 'alles weigeren', 'alleen noodzakelijk',
    // Polish
    'odrzuć', 'odrzuć wszystkie', 'tylko niezbędne',
    // Swedish / Danish / Norwegian
    'avvisa', 'endast nödvändiga', 'kun nødvendige',
  ];

  // Safety net: never click something that also reads as "accept", even if a
  // reject phrase matched too (e.g. a mis-translated "accept and reject
  // nothing" button, however unlikely).
  var ACCEPT_PHRASES = [
    'accept', 'agree', 'allow all', 'akzeptieren', 'zustimmen', 'accepter',
    'accetta', 'aceptar', 'aceitar', 'accepteren', 'akceptuj', 'godkänn',
  ];

  function normalizeText(text) {
    return String(text == null ? '' : text).replace(/\s+/g, ' ').trim().toLowerCase();
  }

  function escapeRegExp(text) {
    return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  }

  // Word-boundary matching, not plain substring: without it, the accept
  // word "agree" would also "match" inside "disagree" and wrongly veto a
  // legitimate reject button.
  var phrasePatternCache = {};
  function containsAny(normalized, phrases) {
    for (var i = 0; i < phrases.length; i++) {
      var phrase = phrases[i];
      var pattern = phrasePatternCache[phrase];
      if (!pattern) {
        pattern = new RegExp('\\b' + escapeRegExp(phrase) + '\\b');
        phrasePatternCache[phrase] = pattern;
      }
      if (pattern.test(normalized)) return true;
    }
    return false;
  }

  /** Whether an element's visible text reads as a reject action, never an accept one. */
  function isRejectText(text) {
    var normalized = normalizeText(text);
    if (!normalized) return false;
    if (containsAny(normalized, ACCEPT_PHRASES)) return false;
    return containsAny(normalized, REJECT_PHRASES);
  }

  function elementText(el) {
    if (!el) return '';
    if (typeof el.innerText === 'string' && el.innerText.length > 0) return el.innerText;
    if (typeof el.textContent === 'string') return el.textContent;
    if (typeof el.value === 'string') return el.value; // input[type=button/submit]
    return '';
  }

  /**
   * Finds the first element in `elements` (an array or array-like of
   * element-like objects) whose text reads as a reject action. Pure, and
   * deliberately takes an already-collected list rather than a live DOM, so
   * it's directly unit-testable with plain fixture objects.
   */
  function findRejectButton(elements) {
    if (!elements) return null;
    for (var i = 0; i < elements.length; i++) {
      if (isRejectText(elementText(elements[i]))) return elements[i];
    }
    return null;
  }

  var CLICKABLE_SELECTOR = 'button, [role="button"], a, input[type="button"], input[type="submit"]';

  function collectClickable(doc) {
    if (!doc || typeof doc.querySelectorAll !== 'function') return [];
    try {
      var list = doc.querySelectorAll(CLICKABLE_SELECTOR);
      return Array.prototype.slice.call(list);
    } catch (e) {
      return [];
    }
  }

  /** Clicks `el` if it can be clicked. Never throws. Returns whether it did. */
  function clickElement(el) {
    try {
      if (el && typeof el.click === 'function') { el.click(); return true; }
    } catch (e) { /* never throw into the page */ }
    return false;
  }

  // ---- CSS fallback: hide whatever banner is still standing -------------
  // Generic selectors for the CMPs' own banner containers, plus a couple of
  // very common ad-hoc patterns. This only hides; it never simulates consent.
  var LEFTOVER_BANNER_CSS = [
    '#onetrust-banner-sdk', '#onetrust-consent-sdk',
    '#CybotCookiebotDialog',
    '#didomi-host', '.didomi-popup-container',
    '#usercentrics-root',
    '.qc-cmp2-container',
    '#truste-consent-track', '.truste_box_overlay',
    '#sp_message_container', '[id^="sp_message_container"]',
    '[id*="cookie-consent" i]', '[class*="cookie-consent" i]',
    '[id*="cookie-banner" i]', '[class*="cookie-banner" i]',
    '[aria-label*="cookie" i][role="dialog"]',
  ].map(function (selector) { return selector + '{display:none!important}'; }).join('');

  function injectStyle(doc, css) {
    if (!doc || typeof doc.createElement !== 'function') return false;
    if (!doc.head) return false;
    try {
      var style = doc.createElement('style');
      style.setAttribute('data-search-shield', 'cookie-reject');
      style.textContent = css;
      doc.head.appendChild(style);
      return true;
    } catch (e) {
      return false;
    }
  }

  // ---- Orchestration: bounded, never an infinite loop --------------------
  var WAIT_TIMEOUT_MS = 8000;

  function init(win) {
    win = win || (typeof window !== 'undefined' ? window : undefined);
    if (!win || !win.document) return;
    var doc = win.document;

    try {
      if (runCmpHandlers(win)) {
        // The CMP itself will tear down its banner; the CSS fallback below
        // still runs after the timeout in case something lingers on screen.
      }
    } catch (e) { /* never throw into the page */ }

    var settled = false;
    function attempt() {
      if (settled) return true;
      var target = findRejectButton(collectClickable(doc));
      if (target) {
        settled = clickElement(target);
        return settled;
      }
      return false;
    }

    try {
      if (attempt()) return;
      var observer;
      var timer = setTimeout(function () {
        settled = true;
        try { observer && observer.disconnect(); } catch (e) {}
        try { injectStyle(doc, LEFTOVER_BANNER_CSS); } catch (e) {}
      }, WAIT_TIMEOUT_MS);
      // In Node (tests, or any non-browser host) an unref'd timer doesn't
      // hold the process open; a real page doesn't have `unref` at all.
      if (timer && typeof timer.unref === 'function') timer.unref();
      observer = new MutationObserver(function () {
        if (settled) return;
        if (attempt()) {
          clearTimeout(timer);
          try { observer.disconnect(); } catch (e) {}
        }
      });
      observer.observe(doc, { childList: true, subtree: true });
    } catch (e) { /* no MutationObserver, or it threw: leave the page as-is */ }
  }

  var api = {
    CMP_HANDLERS: CMP_HANDLERS,
    runCmpHandlers: runCmpHandlers,
    REJECT_PHRASES: REJECT_PHRASES,
    ACCEPT_PHRASES: ACCEPT_PHRASES,
    isRejectText: isRejectText,
    findRejectButton: findRejectButton,
    clickElement: clickElement,
    LEFTOVER_BANNER_CSS: LEFTOVER_BANNER_CSS,
    injectStyle: injectStyle,
    init: init,
  };

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  } else {
    root.__searchCookieReject = api;
    init(root);
  }
})(typeof window !== 'undefined' ? window : this);
