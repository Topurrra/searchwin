'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs');
const probe = require('../fish-probe.js');

// ---- parity with the extension's own matchers -----------------------------
//
// golden.json holds what FishCatcher's original engine answered for every
// text in its parity corpus (Extensions/fishcatcher/parity/golden.mjs). The
// probe's copies of the matchers must answer the same.

const golden = JSON.parse(fs.readFileSync(
  path.join(__dirname, '..', '..', '..', '..', 'Search.Kit.Tests', 'FishCatcher', 'golden.json'), 'utf8'));

test('the text matchers agree with the extension on every corpus text', () => {
  assert.ok(golden.texts.length >= 30);
  for (const t of golden.texts) {
    assert.equal(probe.aitmKindForText(t.text), t.kind, `kind: ${t.text}`);
    assert.equal(probe.scamCryptoSeedText(t.text), t.seed, `seed: ${t.text}`);
    assert.equal(probe.scamTechSupportText(t.text), t.tech, `tech: ${t.text}`);
    assert.equal(probe.matchDeviceCodeScam(t.text, t.title), t.deviceCode, `device code: ${t.text}`);
  }
});

// ---- a tiny stand-in for a page ------------------------------------------

function el(tag, attrs, extra) {
  const a = attrs || {};
  return Object.assign({
    tagName: tag.toUpperCase(),
    offsetParent: {},
    disabled: false,
    type: a.type,
    value: a.value,
    textContent: a.text || '',
    placeholder: a.placeholder,
    name: a.name,
    id: a.id,
    src: a.src,
    href: a.href,
    content: a.content,
    getAttribute: (n) => (Object.prototype.hasOwnProperty.call(a, n) ? a[n] : null),
    querySelector: () => null,
  }, extra || {});
}

// Selectors are matched by the first word the probe uses for each query, so
// a fixture says which elements each question finds.
function page({ url = 'https://evil.example/login', title = '', text = '', select = {} } = {}) {
  const doc = {
    title,
    body: { innerText: text },
    fullscreenElement: null,
    querySelectorAll(selector) {
      for (const key of Object.keys(select)) if (selector.startsWith(key)) return select[key];
      return [];
    },
    querySelector(selector) {
      return this.querySelectorAll(selector)[0] || null;
    },
  };
  const u = new URL(url);
  const win = {
    location: { href: u.href, hostname: u.hostname, protocol: u.protocol },
    innerWidth: 1000,
    innerHeight: 800,
    performance: { getEntriesByType: () => [] },
    getComputedStyle: () => ({ position: 'static', display: 'block', visibility: 'visible' }),
  };
  return { doc, win };
}

// ---- collectAitm ------------------------------------------------------------

test('an ordinary page says nothing', () => {
  const { doc, win } = page({ text: 'Welcome to the recipe blog. Password tips inside.' });
  assert.equal(probe.collectAitm(doc, win), null);
  assert.equal(probe.collectScam(doc, win), null);
  assert.equal(probe.collect(doc, win), null);
});

test('a visible password field is a password interaction, with who the page claims to be', () => {
  const password = el('input', { type: 'password' });
  const logo = el('img', { alt: 'PayPal' });
  const form = el('form', { action: 'https://collector.example/steal' }, {
    querySelector: () => password,
  });
  const { doc, win } = page({
    title: 'PayPal: Log in',
    select: {
      'input[type="password"]': [password],
      'header img[alt]': [logo],
      'form[action]': [form],
      'meta[property="og:site_name"]': [el('meta', { content: 'PayPal' })],
      'script[src]': [el('script', { src: 'https://cdn.evil.example/a.js' })],
    },
  });
  const aitm = probe.collectAitm(doc, win);
  assert.deepEqual(aitm.interactions, ['password']);
  assert.equal(aitm.identityHints.title, 'PayPal: Log in');
  assert.equal(aitm.identityHints.ogSiteName, 'PayPal');
  assert.deepEqual(aitm.identityHints.logoAlts, ['PayPal']);
  assert.deepEqual(aitm.formActions, ['collector.example']);
  assert.equal(aitm.resourceHosts['cdn.evil.example'], 1);
  assert.equal(aitm.resourceHosts['collector.example'], 1);
});

test('a hidden password field does not count', () => {
  const password = el('input', { type: 'password' }, { offsetParent: null });
  const { doc, win } = page({ select: { 'input[type="password"]': [password] } });
  assert.equal(probe.collectAitm(doc, win), null);
});

test('a one-time-code field is otp', () => {
  const code = el('input', { autocomplete: 'one-time-code' });
  const { doc, win } = page({ select: { 'input[autocomplete="one-time-code"]': [code] } });
  assert.deepEqual(probe.collectAitm(doc, win).interactions, ['otp']);
});

test('a form posting to its own host is not a foreign form action', () => {
  const password = el('input', { type: 'password' });
  const form = el('form', { action: '/session' }, { querySelector: () => password });
  const { doc, win } = page({ select: { 'input[type="password"]': [password], 'form[action]': [form] } });
  assert.deepEqual(probe.collectAitm(doc, win).formActions, []);
});

test('resource hosts stop at forty', () => {
  const scripts = [];
  for (let i = 0; i < 60; i++) scripts.push(el('script', { src: `https://h${i}.example/x.js` }));
  const { doc, win } = page({ select: { 'input[type="password"]': [el('input', { type: 'password' })], 'script[src]': scripts } });
  assert.equal(Object.keys(probe.collectAitm(doc, win).resourceHosts).length, 40);
});

// ---- collectScam --------------------------------------------------------------

test('a seed-phrase request is a crypto scam fact', () => {
  const { doc, win } = page({ text: 'To restore your wallet, enter your secret recovery phrase below.' });
  const scam = probe.collectScam(doc, win);
  assert.equal(scam.cryptoSeed, true);
  assert.equal(scam.techScare, false);
});

test('a wallet saying it never asks for the phrase is not', () => {
  const { doc, win } = page({ text: 'We will never ask you to enter your seed phrase.' });
  assert.equal(probe.collectScam(doc, win), null);
});

test('a field labelled for the recovery phrase is a seed input', () => {
  const field = el('textarea', { placeholder: 'Recovery phrase' });
  field.placeholder = 'Recovery phrase';
  const { doc, win } = page({ select: { 'input:not([type=hidden])': [field] } });
  assert.equal(probe.collectScam(doc, win).seedInput, true);
});

test('a fake virus alert with a toll-free number is a tech-support scare with a phone', () => {
  const { doc, win } = page({ text: 'Your computer is infected! Call Microsoft support now at 1-888-555-0199. Do not restart.' });
  const scam = probe.collectScam(doc, win);
  assert.equal(scam.techScare, true);
  assert.equal(scam.phone, true);
});

test('a full-window overlay counts as full screen', () => {
  const overlay = el('div', {}, { getBoundingClientRect: () => ({ width: 1000, height: 800 }) });
  const { doc, win } = page({ text: 'Windows Defender security alert: call support at 1-800-555-0100' });
  doc.elementsFromPoint = (x, y) => {
    assert.deepEqual([x, y], [500, 400]);
    return [overlay, el('body'), el('html')];
  };
  win.getComputedStyle = (e) => ({ position: e === overlay ? 'fixed' : 'static', display: 'block', visibility: 'visible' });
  assert.equal(probe.collectScam(doc, win).fullscreen, true);
});

test('a box that is hidden, or smaller than the window, is not full screen', () => {
  const small = el('div', {}, { getBoundingClientRect: () => ({ width: 400, height: 300 }) });
  const hidden = el('div', {}, { getBoundingClientRect: () => ({ width: 1000, height: 800 }) });
  const { doc, win } = page({ text: 'Windows Defender security alert: call support at 1-800-555-0100' });
  doc.elementsFromPoint = () => [small, hidden];
  win.getComputedStyle = (e) => ({ position: 'fixed', display: 'block', visibility: e === hidden ? 'hidden' : 'visible' });
  assert.equal(probe.collectScam(doc, win).fullscreen, false);
});

test('the overlay look styles only what is under the middle of the window, not every box', () => {
  // A long page: thousands of boxes, each one styled and measured was
  // tens of milliseconds of layout on the page's own thread.
  const boxes = [];
  for (let i = 0; i < 5000; i++) boxes.push(el('div', {}, { getBoundingClientRect: () => ({ width: 10, height: 10 }) }));
  const overlay = el('div', {}, { getBoundingClientRect: () => ({ width: 1000, height: 800 }) });
  boxes.push(overlay);
  const { doc, win } = page({ text: 'Security alert! Your computer is locked. Call support now.', select: { 'div,section': boxes } });
  doc.elementsFromPoint = () => [overlay, boxes[0]];
  let styled = 0;
  win.getComputedStyle = (e) => { styled++; return { position: e === overlay ? 'fixed' : 'static', display: 'block', visibility: 'visible' }; };
  assert.equal(probe.collectScam(doc, win).fullscreen, true);
  assert.ok(styled <= 2, `${styled} boxes styled`);
});

// ---- how often the page's text is read -----------------------------------------

// innerText lays the whole page out; count each read.
function counting(fixture) {
  let reads = 0;
  const text = fixture.doc.body.innerText;
  Object.defineProperty(fixture.doc.body, 'innerText', { get() { reads++; return text; } });
  return () => reads;
}

test('one look reads the page text once, for both the sign-in and the scam checks', () => {
  const password = el('input', { type: 'password' });
  const fixture = page({ text: 'Enter your seed phrase to unlock your wallet', select: { 'input[type="password"]': [password] } });
  const reads = counting(fixture);
  const facts = probe.collect(fixture.doc, fixture.win);
  assert.deepEqual(facts.aitm.interactions, ['password']);
  assert.equal(facts.scam.cryptoSeed, true);
  assert.equal(reads(), 1);
});

test('the first look, as the page is parsed, leaves the text alone; the settled one reads it when idle', () => {
  const sent = [];
  const listeners = {};
  const timers = [];
  const idle = [];
  const password = el('input', { type: 'password' });
  const fixture = page({ text: 'Enter your seed phrase to unlock your wallet', select: { 'input[type="password"]': [password] } });
  const { doc, win } = fixture;
  const reads = counting(fixture);
  doc.readyState = 'loading';
  doc.addEventListener = (type, fn) => { listeners['doc:' + type] = fn; };
  win.document = doc;
  win.addEventListener = (type, fn) => { listeners['win:' + type] = fn; };
  win.setTimeout = (fn, ms) => timers.push({ fn, ms });
  win.requestIdleCallback = (fn, opts) => idle.push({ fn, opts });
  win.chrome = { webview: { postMessage: (m) => sent.push(m) } };
  probe.init(win);

  listeners['doc:DOMContentLoaded']();
  assert.equal(reads(), 0);
  // The password field is structural: said at once.
  assert.equal(sent.length, 1);
  assert.equal(sent[0].body.scam, undefined);

  listeners['win:load']();
  const settle = timers.find((t) => t.ms === 1200);
  assert.ok(settle);
  settle.fn();
  assert.equal(reads(), 0);
  assert.equal(idle.length, 1);
  idle[0].fn();
  assert.equal(reads(), 1);
  assert.equal(sent.length, 2);
  assert.equal(sent[1].body.scam.cryptoSeed, true);

  // The stuck-load fallback finds the text look already done.
  timers.find((t) => t.ms === 5000).fn();
  timers.filter((t) => t.ms === 0).forEach((t) => t.fn());
  assert.equal(idle.length, 1);
});

test('a page that never finishes loading still has its text read', () => {
  const sent = [];
  const listeners = {};
  const timers = [];
  const fixture = page({ text: 'Your computer is infected! Call Microsoft support now at 1-888-555-0199.' });
  const { doc, win } = fixture;
  doc.readyState = 'loading';
  doc.addEventListener = (type, fn) => { listeners['doc:' + type] = fn; };
  win.document = doc;
  win.addEventListener = () => {};
  win.setTimeout = (fn, ms) => timers.push({ fn, ms });
  win.chrome = { webview: { postMessage: (m) => sent.push(m) } };
  probe.init(win);
  listeners['doc:DOMContentLoaded']();
  assert.equal(sent.length, 0);
  timers.find((t) => t.ms === 5000).fn();
  timers.find((t) => t.ms === 0).fn();
  assert.equal(sent.length, 1);
  assert.equal(sent[0].body.scam.techScare, true);
});

// ---- collect, post, init -------------------------------------------------------

test('collect carries the page address so late facts can be told apart', () => {
  const { doc, win } = page({ url: 'https://wallet-restore.example/', text: 'Please paste your seed phrase to continue' });
  const facts = probe.collect(doc, win);
  assert.equal(facts.href, 'https://wallet-restore.example/');
  assert.equal(facts.scam.cryptoSeed, true);
  assert.equal(facts.aitm, undefined);
});

test('post sends { name, body } over the bridge and never throws', () => {
  const sent = [];
  assert.equal(probe.post({ chrome: { webview: { postMessage: (m) => sent.push(m) } } }, { href: 'x' }), true);
  assert.deepEqual(sent, [{ name: 'fish.facts', body: { href: 'x' } }]);
  assert.equal(probe.post({}, {}), false);
  assert.equal(probe.post({ chrome: { webview: { postMessage: () => { throw new Error('gone'); } } } }, {}), false);
});

test('init posts once for a scam page, and not again for the same facts', () => {
  const sent = [];
  const listeners = {};
  const { doc, win } = page({ text: 'Enter your seed phrase to unlock your wallet' });
  doc.readyState = 'complete';
  doc.addEventListener = (type, fn) => { listeners['doc:' + type] = fn; };
  win.document = doc;
  win.addEventListener = (type, fn) => { listeners['win:' + type] = fn; };
  win.setTimeout = (fn) => fn();
  win.chrome = { webview: { postMessage: (m) => sent.push(m) } };
  probe.init(win);
  assert.equal(sent.length, 1);
  // The focus look finds the same facts, so says nothing more.
  listeners['doc:focusin']({ target: { tagName: 'INPUT', type: 'password', getAttribute: () => null } });
  assert.equal(sent.length, 1);
});

test('init stays quiet on an ordinary page and on a non-web page', () => {
  const sent = [];
  const { doc, win } = page({ text: 'Hello' });
  doc.readyState = 'complete';
  doc.addEventListener = () => {};
  win.document = doc;
  win.addEventListener = () => {};
  win.setTimeout = (fn) => fn();
  win.chrome = { webview: { postMessage: (m) => sent.push(m) } };
  probe.init(win);
  win.location.protocol = 'file:';
  probe.init(win);
  assert.equal(sent.length, 0);
});

test('a page that overwrites chrome.webview.postMessage after init still reaches the sender captured at start', () => {
  // fish-probe.js runs before any page script (document-created, main
  // world). A hostile page running after it can still overwrite
  // window.chrome.webview.postMessage — it's a plain writable property — to
  // silence the probe. init() must capture the bridge's postMessage once, up
  // front, and later posts must use that captured reference rather than
  // looking the property up again at post time.
  const original = [];
  const hijacked = [];
  const listeners = {};
  const password = el('input', { type: 'password' });
  const fixture = page({ text: 'Enter your seed phrase to unlock your wallet', select: { 'input[type="password"]': [password] } });
  const { doc, win } = fixture;
  doc.readyState = 'loading';
  doc.addEventListener = (type, fn) => { listeners['doc:' + type] = fn; };
  win.document = doc;
  win.addEventListener = () => {};
  win.setTimeout = () => {}; // discard the stuck-load fallback timer
  win.chrome = { webview: { postMessage: (m) => original.push(m) } };

  probe.init(win);
  // A page script running after ours mutes the bridge.
  win.chrome.webview.postMessage = (m) => hijacked.push(m);

  listeners['doc:DOMContentLoaded']();
  assert.equal(hijacked.length, 0);
  assert.equal(original.length, 1);
  assert.equal(original[0].body.aitm.interactions[0], 'password');
});

test('captureSender keeps working after chrome.webview.postMessage is reassigned', () => {
  const sent = [];
  const win = { chrome: { webview: { postMessage: (m) => sent.push(m) } } };
  const sender = probe.captureSender(win);
  win.chrome.webview.postMessage = () => { throw new Error('should never run'); };
  sender({ name: 'x' });
  assert.deepEqual(sent, [{ name: 'x' }]);
});

test('captureSender keeps working after the page replaces Function.prototype.call', () => {
  const sent = [];
  const win = { chrome: { webview: { postMessage(m) { sent.push([this === win.chrome.webview, m]); } } } };
  const sender = probe.captureSender(win);
  const call = Function.prototype.call;
  Function.prototype.call = function () { throw new Error('muted'); };
  try {
    sender({ name: 'x' });
  } finally {
    Function.prototype.call = call;
  }
  assert.deepEqual(sent, [[true, { name: 'x' }]]);
});

test('captureSender returns null when there is no bridge, and never throws', () => {
  assert.equal(probe.captureSender(null), null);
  assert.equal(probe.captureSender({}), null);
  assert.equal(probe.captureSender({ chrome: {} }), null);
});

test('init never throws on a missing or hostile window', () => {
  assert.doesNotThrow(() => probe.init(null));
  assert.doesNotThrow(() => probe.init({}));
  const hostile = { location: { protocol: 'https:', href: 'https://x/' } };
  Object.defineProperty(hostile, 'document', { get() { throw new Error('no'); } });
  assert.doesNotThrow(() => { try { probe.init(hostile); } catch (e) { throw e; } });
});
