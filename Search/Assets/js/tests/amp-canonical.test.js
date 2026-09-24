'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const amp = require('../amp-canonical.js');

function makeElement(attrs) {
  return { hasAttribute: (name) => Object.prototype.hasOwnProperty.call(attrs, name) };
}

function makeLink(attributes) {
  return { getAttribute: (name) => (Object.prototype.hasOwnProperty.call(attributes, name) ? attributes[name] : null) };
}

function makeDoc(htmlAttrs, links) {
  return {
    documentElement: makeElement(htmlAttrs || {}),
    querySelectorAll: (selector) => (selector === 'link' ? links || [] : []),
  };
}

// ---- isAmpDocument --------------------------------------------------------

test('isAmpDocument recognises the "amp" attribute', () => {
  assert.equal(amp.isAmpDocument(makeElement({ amp: '' })), true);
});

test('isAmpDocument recognises the "⚡" attribute', () => {
  assert.equal(amp.isAmpDocument(makeElement({ '⚡': '' })), true);
});

test('isAmpDocument is false for an ordinary page', () => {
  assert.equal(amp.isAmpDocument(makeElement({ lang: 'en' })), false);
});

test('isAmpDocument is false for null/undefined', () => {
  assert.equal(amp.isAmpDocument(null), false);
  assert.equal(amp.isAmpDocument(undefined), false);
});

// ---- findCanonicalHref ------------------------------------------------

test('findCanonicalHref finds a canonical link among several', () => {
  const doc = makeDoc({ amp: '' }, [
    makeLink({ rel: 'stylesheet', href: '/style.css' }),
    makeLink({ rel: 'canonical', href: 'https://example.com/real-article' }),
  ]);
  assert.equal(amp.findCanonicalHref(doc), 'https://example.com/real-article');
});

test('findCanonicalHref returns null when there is no canonical link', () => {
  const doc = makeDoc({ amp: '' }, [makeLink({ rel: 'stylesheet', href: '/style.css' })]);
  assert.equal(amp.findCanonicalHref(doc), null);
});

// ---- resolveCanonical ---------------------------------------------------

test('resolveCanonical resolves a relative href against the page URL', () => {
  const result = amp.resolveCanonical('/real-article', 'https://amp.example.com/amp/page');
  assert.equal(result, 'https://amp.example.com/real-article');
});

test('resolveCanonical rejects a non-http(s) scheme', () => {
  assert.equal(amp.resolveCanonical('javascript:alert(1)', 'https://example.com/'), null);
});

test('resolveCanonical returns null for a garbage href', () => {
  assert.equal(amp.resolveCanonical(null, 'https://example.com/'), null);
});

// ---- shouldNotify: the whole decision -----------------------------------

test('shouldNotify reports the canonical URL for an AMP page pointing elsewhere', () => {
  const doc = makeDoc({ amp: '' }, [makeLink({ rel: 'canonical', href: 'https://example.com/real-article' })]);
  const result = amp.shouldNotify(doc, 'https://example-amp.cdn.ampproject.org/c/s/example.com/amp/real-article');
  assert.equal(result, 'https://example.com/real-article');
});

test('shouldNotify is null on a non-AMP page even with a canonical link', () => {
  const doc = makeDoc({}, [makeLink({ rel: 'canonical', href: 'https://example.com/real-article' })]);
  assert.equal(amp.shouldNotify(doc, 'https://example.com/real-article'), null);
});

test('shouldNotify is null when the canonical points at the current page itself', () => {
  const doc = makeDoc({ amp: '' }, [makeLink({ rel: 'canonical', href: 'https://example.com/page' })]);
  assert.equal(amp.shouldNotify(doc, 'https://example.com/page'), null);
});

test('shouldNotify is null when there is no canonical link at all', () => {
  const doc = makeDoc({ amp: '' }, []);
  assert.equal(amp.shouldNotify(doc, 'https://example.com/amp/page'), null);
});

// ---- buildMessage / postCanonical ----------------------------------------

test('buildMessage matches the documented shape', () => {
  assert.deepEqual(amp.buildMessage('https://example.com/real'), {
    name: 'shield.amp',
    body: { canonical: 'https://example.com/real' },
  });
});

test('postCanonical posts through window.chrome.webview.postMessage', () => {
  const calls = [];
  const fakeWindow = { chrome: { webview: { postMessage: (msg) => calls.push(msg) } } };
  const posted = amp.postCanonical(fakeWindow, 'https://example.com/real');
  assert.equal(posted, true);
  assert.deepEqual(calls, [{ name: 'shield.amp', body: { canonical: 'https://example.com/real' } }]);
});

test('postCanonical never throws when the bridge is missing', () => {
  assert.doesNotThrow(() => amp.postCanonical({}, 'https://example.com/real'));
  assert.equal(amp.postCanonical({}, 'https://example.com/real'), false);
});

test('postCanonical never throws when postMessage itself throws', () => {
  const fakeWindow = { chrome: { webview: { postMessage: () => { throw new Error('boom'); } } } };
  assert.doesNotThrow(() => amp.postCanonical(fakeWindow, 'https://example.com/real'));
});

// ---- init: never throws on a hostile/incomplete window --------------------

test('init never throws when handed nothing at all', () => {
  assert.doesNotThrow(() => amp.init(undefined));
});

test('init never throws when document has no documentElement yet', () => {
  const fakeWindow = { document: {}, location: { href: 'https://example.com/' } };
  assert.doesNotThrow(() => amp.init(fakeWindow));
});
