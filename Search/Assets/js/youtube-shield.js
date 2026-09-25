// youtube-shield.js — Search's Shields, part of Shields 2.0 (docs/brain/Ideas/Agentic Browser.md, Part 1).
//
// Runs on *.youtube.com, injected main-world, before page scripts
// (AddScriptToExecuteOnDocumentCreatedAsync). It prunes ad-carrying fields out
// of YouTube's player/page data without touching anything else, so playback
// keeps working. This is our own code — no uBlock Origin scriptlets (GPL-3.0)
// were read or copied; the *shape* of the problem (prune known ad fields from
// a JSON blob before the page reads it) is the only thing in common.
//
// Written once as a script that works two ways:
//  - required from Node (`module.exports`) so the pure functions are unit
//    tested against synthetic fixtures (tests/youtube-shield.test.js);
//  - evaluated raw in the page, where `module` doesn't exist, so it wires
//    itself to `window`/`document` and runs.
//
// LIMITS (worth writing down — this is an arms race):
//  - The field list below is what YouTube's player response happened to use
//    when this was written. YouTube reshapes this often; expect this list to
//    need updates, not a one-time fix.
//  - We prune by exact field name, never a prefix/regex over key names,
//    because a regex like /^ad/ would also eat `adaptiveFormats`
//    (the actual video streams) and break playback. Precision over recall.
//  - The property traps only catch a *later* `ytInitialPlayerResponse =`
//    assignment because we install them before the page's own inline
//    scripts run (that's what "document created" + main world buys us). If
//    YouTube ever reads the value through `Object.getOwnPropertyDescriptor`
//    tricks or a different global name, this stops working silently — by
//    design (see "never throw into the page" below) but silently.
//  - XHR interception only rewrites `responseText`/`response`; a caller that
//    reads `responseXML` or streams the body (`ReadableStream`) isn't covered.
(function (root) {
  'use strict';

  // ---- What counts as an "ad field" -------------------------------------
  // Exact keys only (see LIMITS above). `playerAds` covers the older
  // instream-ad container; `adPlacements`/`adSlots` are the current ones;
  // `adBreakHeartbeatParams` is the mid-roll heartbeat ping YouTube's player
  // polls to schedule the next break.
  var AD_FIELD_NAMES = [
    'adPlacements',
    'playerAds',
    'adSlots',
    'adBreakHeartbeatParams',
    // Similar-shaped fields seen alongside the above; harmless to remove
    // when present, and each is specific enough not to catch real data.
    'adBreakServiceRenderer',
    'adSlotRenderer',
    'playerLegacyDesktopWatchAdsRenderer',
    'adPlacementRenderer',
  ];
  var AD_FIELD_SET = {};
  for (var i = 0; i < AD_FIELD_NAMES.length; i++) AD_FIELD_SET[AD_FIELD_NAMES[i]] = true;

  // youtubei endpoints whose JSON bodies can carry the same ad fields.
  var AD_ENDPOINT_PATTERN = /\/youtubei\/v1\/(player|next)(?:[/?]|$)/;

  // A conservative depth cap: real player responses nest maybe 10-15 levels
  // deep. 40 is generous headroom against a hostile/huge payload without
  // risking a stack blowout on something pathological.
  var MAX_DEPTH = 40;

  /** True for anything we should walk into: plain objects and arrays. */
  function isWalkable(value) {
    return value !== null && typeof value === 'object';
  }

  /**
   * Removes every known ad field found anywhere inside `value`, in place.
   * Pure in the sense that it never reads globals and never throws; a
   * malformed or circular structure is handled defensively (depth cap +
   * a seen-set) rather than by letting the walk run away.
   * Returns `value` for chaining.
   */
  function pruneAdFields(value, depth, seen) {
    if (!isWalkable(value)) return value;
    depth = depth || 0;
    if (depth > MAX_DEPTH) return value;
    seen = seen || (typeof WeakSet === 'function' ? new WeakSet() : null);
    if (seen) {
      if (seen.has(value)) return value;
      seen.add(value);
    }
    if (Array.isArray(value)) {
      for (var i = 0; i < value.length; i++) pruneAdFields(value[i], depth + 1, seen);
      return value;
    }
    var keys = Object.keys(value);
    for (var k = 0; k < keys.length; k++) {
      var key = keys[k];
      if (Object.prototype.hasOwnProperty.call(AD_FIELD_SET, key)) {
        try { delete value[key]; } catch (e) { /* frozen/sealed object: leave it */ }
        continue;
      }
      pruneAdFields(value[key], depth + 1, seen);
    }
    return value;
  }

  /**
   * A cheap, conservative test for "this looks like a YouTube player
   * response (or something ad-bearing like it)". Used to decide whether a
   * generic `JSON.parse` result or network response is worth pruning at
   * all — anything that doesn't match is returned untouched, so ordinary
   * JSON on the page is never mutated.
   */
  function isPlayerResponseShape(value) {
    if (!isWalkable(value) || Array.isArray(value)) return false;
    return (
      'playabilityStatus' in value ||
      'streamingData' in value ||
      'videoDetails' in value ||
      'adPlacements' in value ||
      'playerAds' in value ||
      'adSlots' in value
    );
  }

  /**
   * Parses `text` as JSON and, only if it looks like player data, prunes it
   * and re-serializes. Anything that fails to parse, or doesn't look like
   * player data, is returned byte-for-byte unchanged.
   */
  function pruneJsonText(text) {
    if (typeof text !== 'string' || text.length === 0) return text;
    var parsed;
    try {
      parsed = JSON.parse(text);
    } catch (e) {
      return text;
    }
    if (!isPlayerResponseShape(parsed)) return text;
    pruneAdFields(parsed);
    try {
      return JSON.stringify(parsed);
    } catch (e) {
      return text;
    }
  }

  // ---- Property traps: ytInitialPlayerResponse / ytInitialData ----------
  //
  // These globals are set later by a `var x = {...};` inline script. `var`
  // at top level doesn't overwrite an existing accessor property on
  // `window` — it just leaves it, and the assignment that follows goes
  // through our setter. Installing the trap here, before any page script
  // has run, is what makes this work; it relies on nothing more exotic than
  // that ordering, which `AddScriptToExecuteOnDocumentCreatedAsync` gives us.
  //
  // `target` is `window` in the page and a plain object in tests.
  function installGlobalTrap(target, propName) {
    if (!target || typeof Object.defineProperty !== 'function') return false;
    var backing;
    var had = Object.prototype.hasOwnProperty.call(target, propName);
    if (had) {
      var existing = Object.getOwnPropertyDescriptor(target, propName);
      if (existing && existing.configurable === false) return false; // can't trap; leave it alone
      backing = target[propName];
    }
    try {
      Object.defineProperty(target, propName, {
        configurable: true,
        enumerable: true,
        get: function () { return backing; },
        set: function (value) {
          if (isWalkable(value)) pruneAdFields(value);
          backing = value;
        },
      });
    } catch (e) {
      return false; // never throw into the page; the field just stays unpruned
    }
    if (had && isWalkable(backing)) pruneAdFields(backing);
    return true;
  }

  // ---- Idempotency tracking, without marking anything page-visible --------
  // Each patch used to stamp a `__searchShield` property on the function or
  // constructor it wrapped, so a page could detect Shields by checking for
  // that name (`JSON.parse.__searchShield`, `window.fetch.__searchShield`,
  // `XMLHttpRequest.__searchShield`). A WeakSet keyed by the wrapped object
  // does the same "already patched?" check without adding anything a page
  // can see with `in`, `Object.keys`, or a property read.
  var patched = typeof WeakSet === 'function' ? new WeakSet() : null;
  function markPatched(obj) { try { if (patched) patched.add(obj); } catch (e) { /* ignore */ } }
  function isPatched(obj) { try { return !!(patched && patched.has(obj)); } catch (e) { return false; } }

  // ---- JSON.parse patch ---------------------------------------------------
  // Wraps `JSON.parse` so any result that looks like player data gets
  // pruned before the caller sees it. Anything else round-trips untouched.
  function patchJsonParse(jsonLike) {
    if (!jsonLike || typeof jsonLike.parse !== 'function' || isPatched(jsonLike.parse)) {
      return false;
    }
    var original = jsonLike.parse;
    function wrapped(text, reviver) {
      var result = original.call(jsonLike, text, reviver);
      if (isPlayerResponseShape(result)) {
        try { pruneAdFields(result); } catch (e) { /* leave result as parsed */ }
      }
      return result;
    }
    markPatched(wrapped);
    jsonLike.parse = wrapped;
    return true;
  }

  // ---- fetch patch ----------------------------------------------------------
  // Only touches the two youtubei endpoints that carry player data; every
  // other fetch passes straight through to the original, untouched.
  function patchFetch(win) {
    if (!win || typeof win.fetch !== 'function' || isPatched(win.fetch)) return false;
    var original = win.fetch;
    function wrapped(input, init) {
      var url = typeof input === 'string' ? input : (input && input.url) || '';
      var result = original.call(win, input, init);
      if (!AD_ENDPOINT_PATTERN.test(String(url))) return result;
      return result.then(function (response) {
        if (!response || typeof response.text !== 'function') return response;
        return response.text().then(function (text) {
          var pruned = pruneJsonText(text);
          try {
            return new win.Response(pruned, {
              status: response.status,
              statusText: response.statusText,
              headers: response.headers,
            });
          } catch (e) {
            return response; // Response construction failed; hand back the original
          }
        }, function () {
          return response; // reading the body failed; hand back the original
        });
      });
    }
    markPatched(wrapped);
    win.fetch = wrapped;
    return true;
  }

  // ---- XMLHttpRequest patch ---------------------------------------------
  // Records whether the requested URL matches an ad-bearing endpoint, then
  // wraps `responseText`/`response` so a matching request's body is pruned
  // the moment the page reads it. Non-matching requests are never touched.
  // Which instances matched lives in a WeakMap, not a `__searchShieldMatch`
  // property on the instance itself — a page could otherwise read that
  // property off any XHR it made to tell whether Shields had flagged it.
  var xhrMatches = typeof WeakMap === 'function' ? new WeakMap() : null;
  function markXhrMatch(xhr, matched) { try { if (xhrMatches) xhrMatches.set(xhr, matched); } catch (e) { /* ignore */ } }
  function xhrMatched(xhr) { try { return !!(xhrMatches && xhrMatches.get(xhr)); } catch (e) { return false; } }

  function patchXhr(XHRCtor) {
    if (!XHRCtor || !XHRCtor.prototype || isPatched(XHRCtor)) return false;
    var proto = XHRCtor.prototype;
    var originalOpen = proto.open;
    proto.open = function (method, url) {
      var matched;
      try { matched = AD_ENDPOINT_PATTERN.test(String(url)); } catch (e) { matched = false; }
      markXhrMatch(this, matched);
      return originalOpen.apply(this, arguments);
    };
    var textDescriptor = Object.getOwnPropertyDescriptor(proto, 'responseText');
    if (textDescriptor && typeof textDescriptor.get === 'function') {
      Object.defineProperty(proto, 'responseText', {
        configurable: true,
        get: function () {
          var value = textDescriptor.get.call(this);
          if (xhrMatched(this) && typeof value === 'string') {
            try { return pruneJsonText(value); } catch (e) { return value; }
          }
          return value;
        },
      });
    }
    var responseDescriptor = Object.getOwnPropertyDescriptor(proto, 'response');
    if (responseDescriptor && typeof responseDescriptor.get === 'function') {
      Object.defineProperty(proto, 'response', {
        configurable: true,
        get: function () {
          var value = responseDescriptor.get.call(this);
          if (!xhrMatched(this)) return value;
          if (typeof value === 'string') {
            try { return pruneJsonText(value); } catch (e) { return value; }
          }
          if (isPlayerResponseShape(value)) {
            try { pruneAdFields(value); } catch (e) { /* leave it */ }
          }
          return value;
        },
      });
    }
    markPatched(XHRCtor);
    return true;
  }

  // ---- Cosmetic CSS: ad slots that still render an empty/decorative shell.
  var AD_CSS = [
    '#masthead-ad', // homepage/search masthead banner
    'ytd-display-ad-renderer', // in-feed display ad card
    'ytd-in-feed-ad-layout-renderer',
    'ytd-ad-slot-renderer',
    'ytd-promoted-sparkles-web-renderer',
    'ytd-companion-slot-renderer', // watch-page companion banner
    '.ytp-ad-overlay-container', // in-player overlay ad
    '.ytp-ad-image-overlay',
    'ytd-action-companion-ad-renderer',
  ].map(function (selector) { return selector + '{display:none!important}'; }).join('');

  function injectStyle(doc, css) {
    if (!doc || typeof doc.createElement !== 'function') return;
    var apply = function () {
      if (!doc.head) return false;
      var style = doc.createElement('style');
      style.textContent = css;
      doc.head.appendChild(style);
      return true;
    };
    try {
      if (apply()) return;
      var observer = new (doc.defaultView ? doc.defaultView.MutationObserver : MutationObserver)(function (list, obs) {
        if (apply()) obs.disconnect();
      });
      observer.observe(doc, { childList: true, subtree: true });
    } catch (e) { /* cosmetic only; never throw into the page */ }
  }

  /** Wires every guard above onto a real window/document. Never throws. */
  function init(win) {
    win = win || (typeof window !== 'undefined' ? window : undefined);
    if (!win) return;
    try {
      var host = win.location && win.location.hostname;
      if (host && !/(^|\.)youtube\.com$/i.test(host)) return;
    } catch (e) { /* location access shouldn't ever throw, but never trust it */ }

    try { installGlobalTrap(win, 'ytInitialPlayerResponse'); } catch (e) {}
    try { installGlobalTrap(win, 'ytInitialData'); } catch (e) {}
    try { if (win.JSON) patchJsonParse(win.JSON); } catch (e) {}
    try { patchFetch(win); } catch (e) {}
    try { if (win.XMLHttpRequest) patchXhr(win.XMLHttpRequest); } catch (e) {}
    try { injectStyle(win.document, AD_CSS); } catch (e) {}
  }

  var api = {
    AD_FIELD_NAMES: AD_FIELD_NAMES,
    AD_ENDPOINT_PATTERN: AD_ENDPOINT_PATTERN,
    AD_CSS: AD_CSS,
    pruneAdFields: pruneAdFields,
    isPlayerResponseShape: isPlayerResponseShape,
    pruneJsonText: pruneJsonText,
    installGlobalTrap: installGlobalTrap,
    patchJsonParse: patchJsonParse,
    patchFetch: patchFetch,
    patchXhr: patchXhr,
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
