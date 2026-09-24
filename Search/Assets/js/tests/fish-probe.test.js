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
  const { doc, win } = page({ text: 'Windows Defender security alert: call support at 1-800-555-0100', select: { 'div,section': [overlay] } });
  win.getComputedStyle = () => ({ position: 'fixed', display: 'block', visibility: 'visible' });
  assert.equal(probe.collectScam(doc, win).fullscreen, true);
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
  win.chrome = { webview: { postMessage: (m) => sent.push(m) } };
  probe.init(win);
  win.location.protocol = 'file:';
  probe.init(win);
  assert.equal(sent.length, 0);
});

test('init never throws on a missing or hostile window', () => {
  assert.doesNotThrow(() => probe.init(null));
  assert.doesNotThrow(() => probe.init({}));
  const hostile = { location: { protocol: 'https:', href: 'https://x/' } };
  Object.defineProperty(hostile, 'document', { get() { throw new Error('no'); } });
  assert.doesNotThrow(() => { try { probe.init(hostile); } catch (e) { throw e; } });
});
