'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const cookies = require('../cookie-reject.js');

// ---- CMP handlers: each calls only its own documented reject API ---------

test('runCmpHandlers calls OneTrust.RejectAll and nothing else when present', () => {
  const calls = [];
  const win = { OneTrust: { IsAlertBoxClosed: () => false, RejectAll: () => calls.push('OneTrust.RejectAll') } };
  const handled = cookies.runCmpHandlers(win);
  assert.equal(handled, 'OneTrust');
  assert.deepEqual(calls, ['OneTrust.RejectAll']);
});

test('runCmpHandlers calls Cookiebot.decline', () => {
  const calls = [];
  const win = { Cookiebot: { hasResponse: false, decline: () => calls.push('Cookiebot.decline') } };
  assert.equal(cookies.runCmpHandlers(win), 'Cookiebot');
  assert.deepEqual(calls, ['Cookiebot.decline']);
});

test('runCmpHandlers calls Didomi.setUserDisagreeToAll, never setUserAgreeToAll', () => {
  const calls = [];
  const win = {
    Didomi: {
      shouldConsentBeCollected: () => true,
      setUserAgreeToAll: () => calls.push('Didomi.setUserAgreeToAll'),
      setUserDisagreeToAll: () => calls.push('Didomi.setUserDisagreeToAll'),
    },
  };
  assert.equal(cookies.runCmpHandlers(win), 'Didomi');
  assert.deepEqual(calls, ['Didomi.setUserDisagreeToAll']);
});

test('runCmpHandlers calls UC_UI.denyAllConsents, never acceptAllConsents', () => {
  const calls = [];
  const win = {
    UC_UI: {
      isConsentRequired: () => true,
      acceptAllConsents: () => calls.push('UC_UI.acceptAllConsents'),
      denyAllConsents: () => calls.push('UC_UI.denyAllConsents'),
    },
  };
  assert.equal(cookies.runCmpHandlers(win), 'Usercentrics');
  assert.deepEqual(calls, ['UC_UI.denyAllConsents']);
});

test('runCmpHandlers returns null when no known CMP is present', () => {
  assert.equal(cookies.runCmpHandlers({}), null);
});

test('runCmpHandlers keeps trying other CMPs if one throws', () => {
  const calls = [];
  const win = {
    OneTrust: { IsAlertBoxClosed: () => false, RejectAll: () => { throw new Error('boom'); } },
    Cookiebot: { hasResponse: false, decline: () => calls.push('Cookiebot.decline') },
  };
  assert.equal(cookies.runCmpHandlers(win), 'Cookiebot');
  assert.deepEqual(calls, ['Cookiebot.decline']);
});

// ---- isRejectText: never treats an accept phrase as a reject -------------

test('isRejectText matches plain English reject phrases', () => {
  assert.equal(cookies.isRejectText('Reject All'), true);
  assert.equal(cookies.isRejectText('  decline  '), true);
  assert.equal(cookies.isRejectText('Necessary only'), true);
  assert.equal(cookies.isRejectText('Only necessary cookies'), true);
  assert.equal(cookies.isRejectText('Reject all cookies!'), true);
});

test('isRejectText wants the whole text to be a reject phrase, not to contain one', () => {
  assert.equal(cookies.isRejectText('Senate to reject the bill'), false);
  assert.equal(cookies.isRejectText('Refuse collection changes'), false);
  assert.equal(cookies.isRejectText('Why people decline invitations'), false);
});

test('isRejectText no longer treats "Deny" or "No thanks" as a cookie answer', () => {
  // "Deny" belongs to an OAuth/permissions screen as often as a cookie
  // banner, and "no thanks" is a generic dismissal (a newsletter, an app
  // install) that has nothing to do with consent either way.
  assert.equal(cookies.isRejectText('Deny'), false);
  assert.equal(cookies.isRejectText('Deny all'), false);
  assert.equal(cookies.isRejectText('No thanks'), false);
});

test('isRejectText matches non-English reject phrases', () => {
  assert.equal(cookies.isRejectText('Alle ablehnen'), true); // German
  assert.equal(cookies.isRejectText('Tout refuser'), true); // French
  assert.equal(cookies.isRejectText('Rechazar todo'), true); // Spanish
  assert.equal(cookies.isRejectText('Rifiuta tutto'), true); // Italian
});

test('isRejectText never matches an accept phrase', () => {
  assert.equal(cookies.isRejectText('Accept All'), false);
  assert.equal(cookies.isRejectText('I Agree'), false);
  assert.equal(cookies.isRejectText('Allow all'), false);
  assert.equal(cookies.isRejectText('Akzeptieren'), false);
});

test('isRejectText does not fire on unrelated text', () => {
  assert.equal(cookies.isRejectText('Learn more'), false);
  assert.equal(cookies.isRejectText('Settings'), false);
  assert.equal(cookies.isRejectText(''), false);
  assert.equal(cookies.isRejectText(null), false);
});

test('isRejectText does not let "agree" inside "disagree" get vetoed as an accept', () => {
  assert.equal(cookies.isRejectText('I disagree'), true);
});

// ---- real-world phrases the generic fallback used to miss (verify1) ------

test('isRejectText matches phrases real 2026 EU sites use that the old list missed', () => {
  assert.equal(cookies.isRejectText('Continue without agreeing'), true); // Euronews
  assert.equal(cookies.isRejectText('Accept only essential cookies'), true); // europa.eu
  assert.equal(cookies.isRejectText('Use necessary cookies only'), true);
});

test('a phrase that also reads as subscribing or paying is never treated as safe', () => {
  // 20minutes.fr's actual reject-equivalent button: "Refuser et s'abonner"
  // ("Refuse and subscribe") bundles the ad-free choice with a paid
  // subscription. Clicking it would start a subscription, so it must stay
  // unmatched — this is the one case that must never be "fixed".
  assert.equal(cookies.isRejectText("Refuser et s'abonner"), false);
  assert.equal(cookies.isRejectText('Reject ads and subscribe'), false);
});

test('isRejectText allows an "accept only essential/necessary" phrase past the accept veto', () => {
  assert.equal(cookies.isRejectText('Accept only essential'), true);
  assert.equal(cookies.isRejectText('Accept only necessary'), true);
  assert.equal(cookies.isRejectText('Nur essenzielle akzeptieren'), true); // German
});

test('an "accept" phrase outside the short narrow list is still vetoed', () => {
  // Guards against a loose match: "accept" plus arbitrary words must not
  // slip through just because *some* narrow phrase also starts with "accept".
  assert.equal(cookies.isRejectText('Accept only essential and upgrade'), false);
  assert.equal(cookies.isRejectText('Accept only essential, then subscribe'), false);
});

// ---- findRejectButton: pure, given an already-collected element list -----

// A consent banner to hang fixture buttons in — marked as a dialog, since a
// name match alone is no longer enough (see the AEM fixture below).
const banner = { id: 'onetrust-banner-sdk', getAttribute: (n) => (n === 'role' ? 'dialog' : null), parentElement: null };
function button(text, parent) { return { textContent: text, parentElement: parent === undefined ? banner : parent }; }

test('findRejectButton finds the reject button among several candidates', () => {
  const elements = [button('Settings'), button('Accept all'), button('Reject all')];
  const found = cookies.findRejectButton(elements);
  assert.equal(found.textContent, 'Reject all');
});

test('findRejectButton reads textContent, innerText, or value, in that order', () => {
  assert.equal(cookies.findRejectButton([{ innerText: 'Decline', parentElement: banner }]).innerText, 'Decline');
  assert.equal(cookies.findRejectButton([{ value: 'Decline', parentElement: banner }]).value, 'Decline');
});

test('findRejectButton returns null when nothing matches', () => {
  const elements = [button('Accept all'), button('Settings')];
  assert.equal(cookies.findRejectButton(elements), null);
});

test('findRejectButton leaves a reject button alone outside a consent banner', () => {
  const article = { className: 'story-body', parentElement: { tagName: 'MAIN', parentElement: null } };
  assert.equal(cookies.findRejectButton([button('Reject', article)]), null);
  assert.equal(cookies.findRejectButton([button('Reject', article)], true).textContent, 'Reject');
});

test('inConsentContainer looks up through the ancestors, by id, class or label, when it is a dialog', () => {
  const byClass = { className: 'fc-consent-root', getAttribute: (n) => (n === 'role' ? 'dialog' : null), parentElement: null };
  const byLabel = {
    getAttribute: (n) => (n === 'aria-label' ? 'Cookie preferences' : n === 'role' ? 'dialog' : null),
    parentElement: null,
  };
  const deep = { parentElement: { parentElement: { parentElement: byClass } } };
  assert.equal(cookies.inConsentContainer(deep), true);
  assert.equal(cookies.inConsentContainer({ parentElement: byLabel }), true);
  assert.equal(cookies.inConsentContainer({ className: 'nav', parentElement: null }), false);
});

test('inConsentContainer ignores a name match that is neither a dialog nor fixed/sticky', () => {
  const notPinned = { className: 'fc-consent-root', parentElement: null };
  assert.equal(cookies.inConsentContainer({ parentElement: notPinned }), false);
});

test('inConsentContainer accepts a fixed or sticky container even without a dialog role', () => {
  const win = { getComputedStyle: (el) => ({ position: el.pinned ? 'fixed' : 'static' }) };
  const pinnedBanner = { className: 'cookie-banner', pinned: true, parentElement: null };
  const unpinned = { className: 'cookie-banner', pinned: false, parentElement: null };
  assert.equal(cookies.inConsentContainer({ parentElement: pinnedBanner }, win), true);
  assert.equal(cookies.inConsentContainer({ parentElement: unpinned }, win), false);
});

test('inConsentContainer never matches Adobe Experience Manager\'s own "cmp-" component classes', () => {
  // AEM's core components give ordinary page chrome — a header, a button —
  // classes like "cmp-container" and "cmp-button", nothing to do with
  // cookies. The old bare "cmp" match, even dialog-shaped, must not fire.
  const aemHeader = { className: 'cmp-container', getAttribute: (n) => (n === 'role' ? 'dialog' : null), parentElement: null };
  assert.equal(cookies.inConsentContainer({ className: 'cmp-button', parentElement: aemHeader }), false);
});

test('inConsentContainer no longer matches a bare "privacy" name', () => {
  // A privacy-policy link or settings page, not a banner.
  const privacyLink = { className: 'privacy-policy-link', getAttribute: (n) => (n === 'role' ? 'dialog' : null), parentElement: null };
  assert.equal(cookies.inConsentContainer({ parentElement: privacyLink }), false);
});

test('findRejectButton skips a button with no layout box', () => {
  const hidden = { textContent: 'Reject all', parentElement: banner, getClientRects: () => [] };
  assert.equal(cookies.findRejectButton([hidden]), null);
});

test('isConsentFrame recognises a consent dialog frame by its address', () => {
  assert.equal(cookies.isConsentFrame('https://cdn.privacy-mgmt.com/index.html?message_id=1'), true);
  assert.equal(cookies.isConsentFrame('https://consent.example.net/banner'), true);
  assert.equal(cookies.isConsentFrame('https://ads.example.com/frame.html'), false);
  assert.equal(cookies.isConsentFrame('not a url'), false);
});

test('findRejectButton returns null for an empty or missing list', () => {
  assert.equal(cookies.findRejectButton([]), null);
  assert.equal(cookies.findRejectButton(null), null);
});

// ---- clickElement: never throws --------------------------------------

test('clickElement calls .click() on the element', () => {
  let clicked = false;
  cookies.clickElement({ click: () => { clicked = true; } });
  assert.equal(clicked, true);
});

test('clickElement never throws when .click() throws, or is missing', () => {
  assert.doesNotThrow(() => cookies.clickElement({ click: () => { throw new Error('boom'); } }));
  assert.doesNotThrow(() => cookies.clickElement({}));
  assert.doesNotThrow(() => cookies.clickElement(null));
});

// ---- injectStyle ---------------------------------------------------------

test('injectStyle appends a <style> element to <head>', () => {
  const appended = [];
  const doc = {
    head: { appendChild: (el) => appended.push(el) },
    createElement: () => ({ attrs: {}, setAttribute(name, value) { this.attrs[name] = value; } }),
  };
  const ok = cookies.injectStyle(doc, '#banner{display:none!important}');
  assert.equal(ok, true);
  assert.equal(appended.length, 1);
  assert.equal(appended[0].textContent, '#banner{display:none!important}');
});

test('injectStyle returns false rather than throwing when there is no <head>', () => {
  assert.equal(cookies.injectStyle({ createElement: () => ({}) }, 'css'), false);
});

test('injectStyle marks the <style> with nothing a page could search for', () => {
  const appended = [];
  const doc = {
    head: { appendChild: (el) => appended.push(el) },
    createElement: () => ({ attrs: {}, setAttribute(name, value) { this.attrs[name] = value; } }),
  };
  cookies.injectStyle(doc, '#banner{display:none!important}');
  assert.deepEqual(appended[0].attrs, {});
});

// ---- init: bounded, never throws ------------------------------------

test('init never throws when handed nothing at all', () => {
  assert.doesNotThrow(() => cookies.init(undefined));
});

test('init runs the matching CMP handler before touching the DOM', () => {
  const calls = [];
  const win = {
    OneTrust: { IsAlertBoxClosed: () => false, RejectAll: () => calls.push('reject') },
    document: { querySelectorAll: () => [] },
  };
  win.top = win;
  assert.doesNotThrow(() => cookies.init(win));
  assert.deepEqual(calls, ['reject']);
});

test('a CMP that already has an answer is left alone', () => {
  const calls = [];
  const win = {
    OneTrust: { IsAlertBoxClosed: () => true, RejectAll: () => calls.push('OneTrust') },
    Cookiebot: { hasResponse: true, decline: () => calls.push('Cookiebot') },
    Didomi: { shouldConsentBeCollected: () => false, setUserDisagreeToAll: () => calls.push('Didomi') },
    UC_UI: { isConsentRequired: () => false, denyAllConsents: () => calls.push('Usercentrics') },
  };
  assert.equal(cookies.runCmpHandlers(win), null);
  assert.deepEqual(calls, []);
});

test('a CMP that cannot say whether it has an answer is left alone', () => {
  const calls = [];
  const win = { OneTrust: { RejectAll: () => calls.push('OneTrust') }, Cookiebot: { decline: () => calls.push('Cookiebot') } };
  assert.equal(cookies.runCmpHandlers(win), null);
  assert.deepEqual(calls, []);
});

// ---- isOAuthLike / hasVisiblePasswordField --------------------------------

test('isOAuthLike recognises known OAuth/account hosts', () => {
  assert.equal(cookies.isOAuthLike({ location: { hostname: 'accounts.google.com', pathname: '/signin' } }), true);
  assert.equal(cookies.isOAuthLike({ location: { hostname: 'login.microsoftonline.com', pathname: '/' } }), true);
});

test('isOAuthLike recognises an authorize/oauth path on any host', () => {
  assert.equal(cookies.isOAuthLike({ location: { hostname: 'example.com', pathname: '/oauth/authorize' } }), true);
  assert.equal(cookies.isOAuthLike({ location: { hostname: 'example.com', pathname: '/sso/login' } }), true);
});

test('isOAuthLike is false for an ordinary page, and never throws', () => {
  assert.equal(cookies.isOAuthLike({ location: { hostname: 'news.example.com', pathname: '/article' } }), false);
  assert.doesNotThrow(() => cookies.isOAuthLike(undefined));
  assert.equal(cookies.isOAuthLike(undefined), false);
});

test('hasVisiblePasswordField finds a password field that has layout', () => {
  const doc = { querySelectorAll: () => [{ getClientRects: () => [{}] }] };
  assert.equal(cookies.hasVisiblePasswordField(doc), true);
});

test('hasVisiblePasswordField ignores a hidden password field (a login-modal template)', () => {
  const doc = { querySelectorAll: () => [{ getClientRects: () => [] }] };
  assert.equal(cookies.hasVisiblePasswordField(doc), false);
});

test('hasVisiblePasswordField is false with no password field, and never throws', () => {
  assert.equal(cookies.hasVisiblePasswordField({ querySelectorAll: () => [] }), false);
  assert.doesNotThrow(() => cookies.hasVisiblePasswordField(null));
});

test('init never acts on an OAuth/account-permissions screen', () => {
  const calls = [];
  const win = {
    top: null,
    location: { hostname: 'accounts.google.com', pathname: '/o/oauth2/auth' },
    OneTrust: { IsAlertBoxClosed: () => false, RejectAll: () => calls.push('reject') },
    document: { querySelectorAll: () => { calls.push('scan'); return []; } },
  };
  win.top = win;
  assert.doesNotThrow(() => cookies.init(win));
  assert.deepEqual(calls, []);
});

test('init never acts on a page with a visible sign-in form', () => {
  const calls = [];
  const win = {
    top: null,
    location: { hostname: 'example.com', pathname: '/account' },
    OneTrust: { IsAlertBoxClosed: () => false, RejectAll: () => calls.push('reject') },
    document: { querySelectorAll: (sel) => (sel === 'input[type="password"]' ? [{ getClientRects: () => [{}] }] : []) },
  };
  win.top = win;
  assert.doesNotThrow(() => cookies.init(win));
  assert.deepEqual(calls, []);
});

test('init leaves an ordinary frame alone', () => {
  const calls = [];
  const top = {};
  const win = {
    top,
    location: { href: 'https://ads.example.com/frame.html' },
    OneTrust: { IsAlertBoxClosed: () => false, RejectAll: () => calls.push('reject') },
    document: { querySelectorAll: () => { calls.push('scan'); return []; } },
  };
  assert.doesNotThrow(() => cookies.init(win));
  assert.deepEqual(calls, []);
});

// ---- run as a real page script: nothing left on the global for a page to find ---

test('evaluated in a page (no module), it puts no __search* global on window', () => {
  const src = fs.readFileSync(path.join(__dirname, '..', 'cookie-reject.js'), 'utf8');
  const sandbox = {};
  vm.createContext(sandbox);
  assert.doesNotThrow(() => vm.runInContext(src, sandbox));
  assert.equal('__searchCookieReject' in sandbox, false);
  assert.equal(Object.getOwnPropertyNames(sandbox).some((n) => n.indexOf('__search') !== -1), false);
});
