// amp-canonical.js — Search's Shields, De-AMP (docs/brain/Ideas/Agentic Browser.md, Part 1).
//
// Runs on every page, main-world, before page scripts. If the page is an AMP
// document (`<html amp>` / `<html ⚡>`) and carries a `<link rel="canonical">`
// to a *different* http(s) URL, it posts that URL to the browser and lets the
// browser decide what to do (navigate there, offer it, etc.) — this script
// never navigates on its own.
//
// Same two-faced file as the other Shields scripts: `require()`-able from
// Node for the pure logic, self-wiring when evaluated raw in a page.
//
// LIMIT: only the two AMP boilerplate attribute spellings in the spec
// (`amp`, `⚡`) are recognised, not chained AMP variants like `amp4ads`.
(function (root) {
  'use strict';

  var AMP_ATTR_NAMES = ['amp', '⚡']; // 'amp', '⚡'

  /** True when `root` (the <html> element, or an element-like fixture) carries an AMP boilerplate attribute. */
  function isAmpDocument(htmlElement) {
    if (!htmlElement || typeof htmlElement.hasAttribute !== 'function') return false;
    for (var i = 0; i < AMP_ATTR_NAMES.length; i++) {
      try { if (htmlElement.hasAttribute(AMP_ATTR_NAMES[i])) return true; } catch (e) { /* fall through */ }
    }
    return false;
  }

  /**
   * Finds a `<link rel="canonical">` href among a doc-like object's `<link>`
   * elements. Accepts anything with `querySelectorAll('link')` returning
   * elements that expose `getAttribute`, which covers both a real Document
   * and a plain synthetic fixture.
   */
  function findCanonicalHref(doc) {
    if (!doc || typeof doc.querySelectorAll !== 'function') return null;
    var links;
    try { links = doc.querySelectorAll('link'); } catch (e) { return null; }
    if (!links) return null;
    for (var i = 0; i < links.length; i++) {
      var link = links[i];
      var rel = (link.getAttribute && link.getAttribute('rel')) || link.rel || '';
      var words = String(rel).toLowerCase().split(/\s+/);
      if (words.indexOf('canonical') !== -1) {
        return (link.getAttribute && link.getAttribute('href')) || link.href || null;
      }
    }
    return null;
  }

  /** Resolves `href` against `baseUrl`; returns an absolute http(s) URL string, or null. */
  function resolveCanonical(href, baseUrl) {
    if (!href) return null;
    var resolved;
    try {
      resolved = new URL(href, baseUrl).href;
    } catch (e) {
      return null;
    }
    if (!/^https?:/i.test(resolved)) return null;
    return resolved;
  }

  function sameUrl(a, b) {
    try { return new URL(a).href === new URL(b).href; } catch (e) { return a === b; }
  }

  /**
   * The whole decision, with no side effects: given a doc-like object and
   * the page's current URL, returns the canonical URL to report, or null if
   * this isn't an AMP page, has no usable canonical link, or the canonical
   * is the page itself.
   */
  function shouldNotify(doc, currentUrl) {
    if (!doc) return null;
    var root = doc.documentElement;
    if (!isAmpDocument(root)) return null;
    var href = findCanonicalHref(doc);
    if (!href) return null;
    var canonical = resolveCanonical(href, currentUrl);
    if (!canonical) return null;
    if (sameUrl(canonical, currentUrl)) return null;
    return canonical;
  }

  function buildMessage(canonicalUrl) {
    return { name: 'shield.amp', body: { canonical: canonicalUrl } };
  }

  /** Posts the message to the browser; returns whether it could. Never throws. */
  function postCanonical(win, canonicalUrl) {
    try {
      var bridge = win && win.chrome && win.chrome.webview;
      if (bridge && typeof bridge.postMessage === 'function') {
        bridge.postMessage(buildMessage(canonicalUrl));
        return true;
      }
    } catch (e) { /* never throw into the page */ }
    return false;
  }

  // A bounded wait: this script runs at document-created, before <html> or
  // <head> necessarily exist yet, so we poll for a little while and then
  // give up — most pages aren't AMP, so this must not linger.
  var WAIT_TIMEOUT_MS = 4000;

  function waitThenNotify(win) {
    var doc = win.document;
    var settled = false;

    function attempt() {
      if (settled) return true;
      if (!doc.documentElement) return false; // too early even to know if this is AMP
      if (!isAmpDocument(doc.documentElement)) return true; // not AMP: nothing to do, stop watching
      var canonical = shouldNotify(doc, win.location.href);
      if (canonical) {
        settled = true;
        postCanonical(win, canonical);
        return true;
      }
      // AMP, but no (usable) canonical link yet — <head> may still be
      // parsing in. Keep watching until the timeout.
      return false;
    }

    if (attempt()) return;

    var observer;
    var timer = setTimeout(function () {
      settled = true;
      try { observer && observer.disconnect(); } catch (e) {}
    }, WAIT_TIMEOUT_MS);
    // In Node (tests, or any non-browser host) an unref'd timer doesn't hold
    // the process open; a real page doesn't have `unref` at all.
    if (timer && typeof timer.unref === 'function') timer.unref();

    try {
      observer = new MutationObserver(function () {
        if (settled) return;
        if (attempt()) {
          clearTimeout(timer);
          try { observer.disconnect(); } catch (e) {}
        }
      });
      observer.observe(doc, { childList: true, subtree: true, attributes: true });
    } catch (e) { /* no MutationObserver available; the one-shot attempt above is all we get */ }
  }

  function init(win) {
    win = win || (typeof window !== 'undefined' ? window : undefined);
    if (!win || !win.document) return;
    try { waitThenNotify(win); } catch (e) { /* never throw into the page */ }
  }

  var api = {
    AMP_ATTR_NAMES: AMP_ATTR_NAMES,
    isAmpDocument: isAmpDocument,
    findCanonicalHref: findCanonicalHref,
    resolveCanonical: resolveCanonical,
    shouldNotify: shouldNotify,
    buildMessage: buildMessage,
    postCanonical: postCanonical,
    init: init,
  };

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  } else {
    root.__searchAmpCanonical = api;
    init(root);
  }
})(typeof window !== 'undefined' ? window : this);
