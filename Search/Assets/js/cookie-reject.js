// cookie-reject.js — Search's Shields, cookie banners (docs/brain/Ideas/Agentic
// Browser.md, Part 1: "Block cookie consent notices ... a small auto-reject
// script, never 'accept'").
//
// Runs on every page (and in frames that are themselves a consent dialog),
// main-world, before page scripts. Tries known consent management platform
// (CMP) APIs first, but only while that CMP says it is still waiting for an
// answer; falls back to clicking a button that says exactly "reject" (or
// the like, in several languages) inside what is clearly a consent banner;
// and, after a bounded wait, hides whatever banner is still on screen with
// CSS. It never clicks or calls anything that means "accept" — worst case
// is a banner stays up and the CSS fallback hides it.
//
// Same shape as the other Shields scripts: `require()`-able from Node for the
// pure logic, self-wiring when evaluated raw in a page.
//
// LIMITS:
//  - Only four CMPs have a public, stable, single-call "reject everything"
//    method we're confident enough to call blind: OneTrust, Cookiebot,
//    Didomi, Usercentrics. Quantcast Choice/the generic IAB TCF
//    (`__tcfapi`), TrustArc and Sourcepoint don't expose one consistent
//    documented call across their versions. They're handled by the
//    button-text fallback only, same as any other unrecognised banner.
//  - A CMP is only told "reject" while it says consent is still pending, so
//    an answer you gave a site yourself — "accept", even — is never
//    overridden on the next visit.
//  - The button fallback wants the button's whole text to be a reject
//    phrase ("Reject all", "Decline", "Nur notwendige"), not merely to
//    contain one, and the button to sit inside something that names itself
//    a cookie or consent banner. A headline link such as "Senate to reject
//    the bill" is neither. It will under-fire far more often than misfire.
//  - Bounded by `WAIT_TIMEOUT_MS`, and the DOM is looked at no more than
//    every `THROTTLE_MS`: never an open-ended observer, never a scan per
//    mutation on a busy page.
(function (root) {
  'use strict';

  // ---- CMP API handlers: only ones with a stable, documented, single call.
  // Each takes a window-like object and returns whether it acted. Each acts
  // only while its CMP reports that no answer has been given yet.
  function oneTrust(win) {
    var OT = win && win.OneTrust;
    if (!OT || typeof OT.RejectAll !== 'function') return false;
    if (typeof OT.IsAlertBoxClosed !== 'function' || OT.IsAlertBoxClosed()) return false;
    OT.RejectAll();
    return true;
  }

  function cookiebot(win) {
    var CB = win && win.Cookiebot;
    if (!CB || typeof CB.decline !== 'function') return false;
    if (CB.hasResponse !== false) return false;
    CB.decline();
    return true;
  }

  function didomi(win) {
    var D = win && win.Didomi;
    if (!D || typeof D.setUserDisagreeToAll !== 'function') return false;
    if (typeof D.shouldConsentBeCollected !== 'function' || !D.shouldConsentBeCollected()) return false;
    D.setUserDisagreeToAll();
    return true;
  }

  function usercentrics(win) {
    var UC = win && win.UC_UI;
    if (!UC || typeof UC.denyAllConsents !== 'function') return false;
    if (typeof UC.isConsentRequired !== 'function' || !UC.isConsentRequired()) return false;
    UC.denyAllConsents();
    return true;
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
  // What a reject button says, in full: "reject", "decline", "necessary
  // only" and their equivalents in a handful of languages. A button's text
  // must be one of these (give or take a trailing "cookies" and
  // punctuation), not merely contain one.
  // 'deny' and 'deny all' are deliberately not here: on an OAuth or
  // permissions screen ("This app wants access to your contacts") "Deny" is
  // never a cookie answer, and there is no wrapper check that rules that
  // screen out reliably enough to risk it. 'no thanks' is gone for the same
  // reason: it's a generic dismissal, not specific to cookies, and shows up
  // on plenty of banners ("no thanks" to a newsletter, an app install, an
  // insurance add-on) that have nothing to do with consent.
  var REJECT_PHRASES = [
    // English
    'reject all', 'reject', 'decline', 'decline all', 'necessary only', 'only necessary',
    'essential only', 'only essential', 'strictly necessary only', 'use necessary only',
    'use necessary cookies only', 'use essential cookies only', 'disagree', 'i disagree',
    'refuse all', 'refuse', 'reject and close', 'continue without accepting',
    'continue without agreeing',
    // German
    'ablehnen', 'alle ablehnen', 'nur notwendige', 'nur erforderliche', 'nur essenzielle',
    'weiter ohne zuzustimmen',
    // French
    'refuser', 'tout refuser', 'refuser tout', 'nécessaire uniquement',
    'uniquement nécessaire', 'continuer sans accepter', 'poursuivre sans accepter',
    // Spanish
    'rechazar', 'rechazar todo', 'rechazar todas', 'solo necesarias', 'solo esenciales',
    'continuar sin aceptar',
    // Italian
    'rifiuta', 'rifiuta tutto', 'solo necessari', 'solo essenziali', 'continua senza accettare',
    // Portuguese
    'rejeitar', 'rejeitar tudo', 'apenas necessários', 'somente necessários',
    'continuar sem aceitar',
    // Dutch
    'weigeren', 'alles weigeren', 'alleen noodzakelijk', 'alleen noodzakelijke',
    'doorgaan zonder te accepteren',
    // Polish
    'odrzuć', 'odrzuć wszystkie', 'tylko niezbędne', 'kontynuuj bez akceptacji',
    // Swedish / Danish / Norwegian
    'avvisa', 'avvisa alla', 'endast nödvändiga', 'kun nødvendige', 'afvis alle', 'avvis alle',
    'fortsätt utan att acceptera', 'fortsæt uden at acceptere', 'fortsett uten å godta',
  ];

  // Safety net: never click something that also reads as "accept", even if a
  // reject phrase matched too.
  var ACCEPT_PHRASES = [
    'accept', 'agree', 'allow all', 'akzeptieren', 'zustimmen', 'accepter',
    'accetta', 'aceptar', 'aceitar', 'accepteren', 'akceptuj', 'godkänn',
  ];

  // Read as "accept" — the veto above would otherwise catch every one of
  // these — but narrow enough ("only essential", "only necessary") that they
  // can never mean "agree to everything", let alone start a subscription or
  // a payment. Kept short and literal on purpose: a phrase that also reads as
  // subscribing or paying (a real one seen in the wild: "Refuse and
  // subscribe") must never be added here or to REJECT_PHRASES, whatever else
  // it says about cookies.
  var NARROW_ACCEPT_PHRASES = [
    // English
    'accept only essential', 'accept only necessary', 'accept essential only',
    'accept necessary only',
    // German
    'nur essenzielle akzeptieren', 'nur notwendige akzeptieren',
    // French
    'accepter uniquement les nécessaires', "n'accepter que les nécessaires",
    // Spanish
    'aceptar solo las esenciales', 'aceptar solo las necesarias',
    // Italian
    'accetta solo i necessari', 'accetta solo gli essenziali',
    // Portuguese
    'aceitar apenas os necessários',
    // Dutch
    'alleen noodzakelijke accepteren',
    // Swedish
    'acceptera endast nödvändiga',
  ];

  // A button longer than this isn't a button's worth of words.
  var MAX_BUTTON_TEXT = 40;

  function normalizeText(text) {
    return String(text == null ? '' : text).replace(/\s+/g, ' ').trim().toLowerCase();
  }

  function escapeRegExp(text) {
    return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  }

  // Word-boundary matching for the accept veto: without it, the accept word
  // "agree" would also "match" inside "disagree" and wrongly veto a
  // legitimate reject button.
  var phrasePatternCache = {};
  function containsAny(normalized, phrases) {
    for (var i = 0; i < phrases.length; i++) {
      var phrase = phrases[i];
      var pattern = phrasePatternCache[phrase];
      if (!pattern) {
        pattern = new RegExp('(^|[^\\p{L}])' + escapeRegExp(phrase) + '($|[^\\p{L}])', 'u');
        phrasePatternCache[phrase] = pattern;
      }
      if (pattern.test(normalized)) return true;
    }
    return false;
  }

  // "Reject all cookies!" → "reject all": punctuation and a trailing
  // "cookies" (in the languages above) are the only slack allowed.
  function bareText(normalized) {
    return normalized
      .replace(/[\s.!?:;,"'«»“”()\[\]✕×]+$/u, '')
      .replace(/^[\s"'«»“”()\[\]]+/u, '')
      .replace(/ (cookies?|tracking|kekse|témoins|galletas|biscotti|cookies optionnels)$/u, '')
      .trim();
  }

  /** Whether an element's whole text reads as a reject action, never an accept one. */
  function isRejectText(text) {
    var normalized = normalizeText(text);
    if (!normalized || normalized.length > MAX_BUTTON_TEXT) return false;
    var bare = bareText(normalized);
    // Checked before the accept veto, and only by an exact match against the
    // short literal list above: "accept only essential" is safe by name, but
    // nothing merely containing "accept" gets a pass.
    if (NARROW_ACCEPT_PHRASES.indexOf(bare) !== -1) return true;
    if (containsAny(normalized, ACCEPT_PHRASES)) return false;
    return REJECT_PHRASES.indexOf(bare) !== -1;
  }

  // textContent before innerText: reading innerText lays the page out, and
  // this is read for every link and button on it, while the page is still
  // arriving.
  function elementText(el) {
    if (!el) return '';
    if (typeof el.textContent === 'string' && el.textContent.length > 0) return el.textContent;
    if (typeof el.innerText === 'string' && el.innerText.length > 0) return el.innerText;
    if (typeof el.value === 'string') return el.value; // input[type=button/submit]
    return '';
  }

  // What a consent banner calls itself: "cookie" or "consent" as a whole
  // word, or a specific CMP's own container name — never the bare "cmp" or
  // "privacy" that used to be here. Adobe Experience Manager gives ordinary
  // page chrome classes like "cmp-container" and "cmp-button" to every
  // button on the page; a bare "cmp" (even word-bounded: the hyphen already
  // makes "cmp" its own word) turned every one of them into "a consent
  // banner" on any AEM site. "privacy" alone catches privacy-policy links
  // and settings pages that have nothing to do with a banner.
  var CONSENT_NAME = /\b(cookie|consent|gdpr|onetrust|didomi|usercentrics|cookiebot|truste|sp_message|qc-cmp2?|tarteaucitron|klaro|cc-window|cc_banner)\b/i;
  var MAX_ANCESTORS = 12;

  /** Whether `el` is a dialog by shape: a <dialog>, or role="dialog"/"alertdialog". */
  function isDialogLike(el) {
    try {
      if (String(el.tagName || '').toLowerCase() === 'dialog') return true;
      var role = (typeof el.getAttribute === 'function' && el.getAttribute('role')) || '';
      return /^(dialog|alertdialog)$/i.test(String(role));
    } catch (e) { return false; }
  }

  /** Whether `el` is pinned to the viewport — fixed or sticky — the way a real banner is. */
  function isFixedOrSticky(el, win) {
    try {
      var getComputed = (win && win.getComputedStyle) || (typeof getComputedStyle === 'function' ? getComputedStyle : null);
      if (!getComputed) return false;
      var position = getComputed(el).position;
      return position === 'fixed' || position === 'sticky';
    } catch (e) { return false; }
  }

  function nameOf(el) {
    var parts = [];
    try {
      if (el.id) parts.push(String(el.id));
      var cls = el.className;
      if (cls && typeof cls === 'object' && typeof cls.baseVal === 'string') cls = cls.baseVal; // SVG
      if (cls) parts.push(String(cls));
      if (typeof el.getAttribute === 'function') {
        parts.push(el.getAttribute('aria-label') || '', el.getAttribute('aria-labelledby') || '', el.getAttribute('data-testid') || '');
      }
    } catch (e) { /* an element that won't say: no name */ }
    return parts.join(' ');
  }

  /**
   * Whether `el` sits inside something that both names itself a cookie or
   * consent banner AND looks like one: a dialog, or fixed/sticky to the
   * viewport. A name match alone isn't enough — a privacy-policy paragraph
   * or an AEM "cmp-" component sits in ordinary page flow, not pinned over
   * it. Pure but for the optional `win` (for getComputedStyle); with none,
   * only the dialog-shape check applies.
   */
  function inConsentContainer(el, win) {
    var at = el;
    for (var depth = 0; at && depth <= MAX_ANCESTORS; depth++) {
      if (CONSENT_NAME.test(nameOf(at)) && (isDialogLike(at) || isFixedOrSticky(at, win))) return true;
      at = at.parentElement;
    }
    return false;
  }

  /** Whether a frame at `href` is itself a consent dialog (Sourcepoint, TrustArc…). */
  function isConsentFrame(href) {
    try { return /consent|cmp|privacy|cookie|gdpr/i.test(new URL(String(href)).hostname + new URL(String(href)).pathname); } catch (e) { return false; }
  }

  function isVisible(el) {
    try {
      if (typeof el.getClientRects === 'function') return el.getClientRects().length > 0;
    } catch (e) { return false; }
    return true; // a fixture with no layout: take its word
  }

  /**
   * Finds the first element in `elements` (an array or array-like of
   * element-like objects) whose text reads as a reject action. Pure, and
   * deliberately takes an already-collected list rather than a live DOM, so
   * it's directly unit-testable with plain fixture objects. `wholeFrame`
   * says the frame itself is the consent dialog, so no container is needed.
   */
  function findRejectButton(elements, wholeFrame, win) {
    if (!elements) return null;
    for (var i = 0; i < elements.length; i++) {
      var el = elements[i];
      if (!isRejectText(elementText(el))) continue;
      if (!wholeFrame && !inConsentContainer(el, win)) continue;
      if (!isVisible(el)) continue;
      return el;
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
      style.textContent = css;
      doc.head.appendChild(style);
      return true;
    } catch (e) {
      return false;
    }
  }

  // ---- Places this must never act at all ---------------------------------
  // An OAuth or account-permissions screen: "Deny" or "Reject" there answers
  // a login or an app's requested access, never a cookie banner, even when
  // the page also happens to use the word "consent" or "privacy" somewhere.
  var OAUTH_HOST = /(^|\.)(accounts\.google|login\.microsoftonline|appleid\.apple|login\.live|www\.facebook|github|auth0|okta|onelogin)\.com$/i;
  var OAUTH_PATH = /\/(oauth2?|authorize|authorization|sso|saml)(\/|$)/i;

  function isOAuthLike(win) {
    try {
      var loc = win && win.location;
      if (!loc) return false;
      return OAUTH_HOST.test(String(loc.hostname || '')) || OAUTH_PATH.test(String(loc.pathname || ''));
    } catch (e) { return false; }
  }

  /** A password field actually on screen — a sign-in form in use, not a hidden login-modal template. */
  function hasVisiblePasswordField(doc) {
    try {
      if (!doc || typeof doc.querySelectorAll !== 'function') return false;
      var fields = doc.querySelectorAll('input[type="password"]');
      for (var i = 0; i < fields.length; i++) if (isVisible(fields[i])) return true;
    } catch (e) { /* fall through */ }
    return false;
  }

  // ---- Orchestration: bounded, throttled, never an infinite loop ---------
  var WAIT_TIMEOUT_MS = 8000;
  var THROTTLE_MS = 250;

  function init(win) {
    win = win || (typeof window !== 'undefined' ? window : undefined);
    if (!win || !win.document) return;
    if (isOAuthLike(win)) return;
    var doc = win.document;

    // In a frame, only one that is a consent dialog of its own.
    var framed = false;
    try { framed = win.top !== win; } catch (e) { framed = true; }
    var wholeFrame = false;
    if (framed) {
      try { wholeFrame = isConsentFrame(win.location.href); } catch (e) { wholeFrame = false; }
      if (!wholeFrame) return;
    }

    var settled = false;
    function attempt() {
      if (settled) return true;
      // A sign-in form on screen: a "Deny"-shaped click here could be
      // answering that instead of a cookie banner. Leave the whole page
      // alone rather than guess which is which.
      if (hasVisiblePasswordField(doc)) { settled = true; return true; }
      try {
        if (!framed && runCmpHandlers(win)) { settled = true; return true; }
      } catch (e) { /* never throw into the page */ }
      var target = findRejectButton(collectClickable(doc), wholeFrame, win);
      if (target) {
        settled = clickElement(target);
        return settled;
      }
      return false;
    }

    try {
      var observer;
      var pending = null;
      var timer = setTimeout(function () {
        settled = true;
        if (pending) clearTimeout(pending);
        try { observer && observer.disconnect(); } catch (e) {}
        try { injectStyle(doc, LEFTOVER_BANNER_CSS); } catch (e) {}
      }, WAIT_TIMEOUT_MS);
      // In Node (tests, or any non-browser host) an unref'd timer doesn't
      // hold the process open; a real page doesn't have `unref` at all.
      if (timer && typeof timer.unref === 'function') timer.unref();
      var finish = function () {
        clearTimeout(timer);
        if (pending) clearTimeout(pending);
        try { observer && observer.disconnect(); } catch (e) {}
      };
      if (attempt()) { finish(); return; }
      observer = new MutationObserver(function () {
        if (settled || pending) return;
        pending = setTimeout(function () {
          pending = null;
          if (!settled && attempt()) finish();
        }, THROTTLE_MS);
        if (pending && typeof pending.unref === 'function') pending.unref();
      });
      observer.observe(doc, { childList: true, subtree: true });
    } catch (e) { /* no MutationObserver, or it threw: leave the page as-is */ }
  }

  var api = {
    CMP_HANDLERS: CMP_HANDLERS,
    runCmpHandlers: runCmpHandlers,
    REJECT_PHRASES: REJECT_PHRASES,
    ACCEPT_PHRASES: ACCEPT_PHRASES,
    NARROW_ACCEPT_PHRASES: NARROW_ACCEPT_PHRASES,
    isRejectText: isRejectText,
    isDialogLike: isDialogLike,
    isFixedOrSticky: isFixedOrSticky,
    inConsentContainer: inConsentContainer,
    isConsentFrame: isConsentFrame,
    isOAuthLike: isOAuthLike,
    hasVisiblePasswordField: hasVisiblePasswordField,
    findRejectButton: findRejectButton,
    clickElement: clickElement,
    LEFTOVER_BANNER_CSS: LEFTOVER_BANNER_CSS,
    injectStyle: injectStyle,
    init: init,
  };

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  } else {
    // In a real page, nothing of this is left where the page can find it —
    // no named global to check for, no fingerprint to read.
    init(root);
  }
})(typeof window !== 'undefined' ? window : this);
