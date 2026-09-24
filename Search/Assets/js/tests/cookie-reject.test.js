'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const cookies = require('../cookie-reject.js');

// ---- CMP handlers: each calls only its own documented reject API ---------

test('runCmpHandlers calls OneTrust.RejectAll and nothing else when present', () => {
  const calls = [];
  const win = { OneTrust: { RejectAll: () => calls.push('OneTrust.RejectAll') } };
  const handled = cookies.runCmpHandlers(win);
  assert.equal(handled, 'OneTrust');
  assert.deepEqual(calls, ['OneTrust.RejectAll']);
});

test('runCmpHandlers calls Cookiebot.decline', () => {
  const calls = [];
  const win = { Cookiebot: { decline: () => calls.push('Cookiebot.decline') } };
  assert.equal(cookies.runCmpHandlers(win), 'Cookiebot');
  assert.deepEqual(calls, ['Cookiebot.decline']);
});

test('runCmpHandlers calls Didomi.setUserDisagreeToAll, never setUserAgreeToAll', () => {
  const calls = [];
  const win = {
    Didomi: {
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
    OneTrust: { RejectAll: () => { throw new Error('boom'); } },
    Cookiebot: { decline: () => calls.push('Cookiebot.decline') },
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

// ---- findRejectButton: pure, given an already-collected element list -----

test('findRejectButton finds the reject button among several candidates', () => {
  const elements = [
    { textContent: 'Settings' },
    { textContent: 'Accept all' },
    { textContent: 'Reject all' },
  ];
  const found = cookies.findRejectButton(elements);
  assert.equal(found.textContent, 'Reject all');
});

test('findRejectButton reads innerText, textContent, or value, in that order', () => {
  assert.equal(cookies.findRejectButton([{ innerText: 'Decline' }]).innerText, 'Decline');
  assert.equal(cookies.findRejectButton([{ value: 'Decline' }]).value, 'Decline');
});

test('findRejectButton returns null when nothing matches', () => {
  const elements = [{ textContent: 'Accept all' }, { textContent: 'Settings' }];
  assert.equal(cookies.findRejectButton(elements), null);
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

// ---- init: bounded, never throws ------------------------------------

test('init never throws when handed nothing at all', () => {
  assert.doesNotThrow(() => cookies.init(undefined));
});

test('init runs the matching CMP handler before touching the DOM', () => {
  const calls = [];
  const win = {
    OneTrust: { RejectAll: () => calls.push('reject') },
    document: { querySelectorAll: () => [] },
  };
  assert.doesNotThrow(() => cookies.init(win));
  assert.deepEqual(calls, ['reject']);
});
