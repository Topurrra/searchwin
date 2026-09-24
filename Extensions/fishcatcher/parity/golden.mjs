// Runs the ORIGINAL FishCatcher engine (../src/engine) over the parity corpus
// and writes what it says to Search.Kit.Tests/FishCatcher/golden.json. The C#
// port's parity test reads that file and must agree on every verdict.
//
//   node Extensions/fishcatcher/parity/golden.mjs
//
// The data is loaded the way background.js loadData() does it. Nothing here
// touches the network. feed-fixture.json (a feed signed with a throwaway test
// key) is written only when it is missing, because ECDSA signatures differ on
// every run; pass --resign to make a new one.
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { webcrypto } from 'node:crypto';
import { analyzeUrl, realSiteFor } from '../src/engine/analyzer.js';
import { Bloom } from '../src/engine/bloom.js';
import { mlPredict } from '../src/engine/ml.js';
import { applyBundle, bundlePayload, verifyBundle } from '../src/engine/remote.js';
import { aitmKindForText } from '../src/engine/aitm.js';
import { scamCryptoSeedText, scamTechSupportText } from '../src/engine/scampacks.js';
import { matchDeviceCodeScam } from '../src/engine/devicecode.js';
import { classifyLinks, inspectDownload } from '../src/engine/links.js';
import { URLS, FEED_BLOOM_DOMAINS, FEED_BLOCKLIST, FEED_URLS, PAGES, TEXTS, LINKS, DOWNLOADS } from './corpus.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const src = join(here, '..', 'src', 'data');
const out = join(here, '..', '..', '..', 'Search.Kit.Tests', 'FishCatcher');
const json = (name) => JSON.parse(readFileSync(join(src, name), 'utf8'));

function loadData() {
  const safe = json('safe-list.json'), brands = json('brands.json'), tlds = json('tlds.json');
  const keywords = json('keywords.json'), psl = json('psl.json'), block = json('blocklist.json');
  const allow = json('aitm-allow.json');
  return {
    safeList: new Set(safe.domains),
    safeBloom: Bloom.fromPayload(json('safe-bloom.json')),
    brands: brands.brands,
    tlds: tlds.tlds,
    keywords: keywords.keywords,
    psl: psl.suffixes,
    blockList: new Set(block.domains),
    aitmIdp: new Set(allow.idp),
    aitmMediation: new Set(allow.mediation),
    registryKey: json('registry-key.json'),
    ml: json('ml-weights.json'),
    trustList: new Set(),
    bloom: null
  };
}

// analyzeUrl as background.js scoreTab uses it, plus the extras the panel shows.
// A thrown error is recorded as such: the port must fail the same addresses.
function score(url, data, opts) {
  let r;
  try {
    r = analyzeUrl(url, data, opts);
  } catch (e) {
    return { error: String(e?.message ?? e) };
  }
  if (!r) return null;
  return {
    host: r.host,
    registrable: r.registrable,
    score: r.score,
    level: r.level,
    reasons: r.reasons.map((x) => ({ key: x.key, params: x.params.map(String), weight: x.weight })),
    realSite: realSiteFor(r.reasons, data.brands) ?? null,
    known: !!data.safeBloom.has(r.registrable),
    ml: mlPredict(data.ml, r.host)
  };
}

// background.js: a probe message becomes these analyzeUrl options, and a
// device-code match (flagDeviceCode) overrides the verdict unless the site is
// safe-listed, trusted or known-legitimate.
function scorePage(url, facts, data) {
  const opts = {
    hasPasswordForm: facts.aitm?.interactions?.includes('password') === true,
    youngDomainDays: facts.youngDomainDays ?? null,
    aitm: facts.aitm ?? null,
    scam: facts.scam ?? null,
    gsbThreat: facts.gsbThreat ?? null
  };
  const normal = score(url, data, opts);
  if (!facts.deviceCode) return normal;
  const base = score(url, data);
  if (!base || base.error) return base;
  if (data.safeList.has(base.registrable) || data.trustList.has(base.registrable) || data.safeBloom.has(base.registrable)) return normal;
  return {
    ...base,
    score: 100,
    level: 'critical',
    reasons: [...base.reasons, { key: 'reasonDeviceCode', params: [], weight: 100 }],
    realSite: null // flagDeviceCode builds its result without one
  };
}

function feedBundle() {
  const bloom = new Bloom(4096, 7, 3);
  for (const d of FEED_BLOOM_DOMAINS) bloom.add(d);
  return {
    version: 3,
    generated: '2026-09-25T06:00:00Z',
    // Awkward on purpose: the signed bytes are JSON.stringify's, so the port has
    // to reproduce its key order, number forms and string escapes exactly.
    sources: [{ name: 'synthetic "test" feed', url: 'https://example.invalid/list?a=1&b=<2>', notes: 'tab\there' + String.fromCharCode(0x2028) + 'línea 🐟 \x07' + String.fromCharCode(0xd800), 10: 1, 2: 'two', b: [1.5, 1e21, 1e-7, -0, 123456789012, 0.1, 5e-324, 1.7976931348623157e308, 100, 1e20, 123e-20], nested: { z: null, a: true, f: false } }],
    count: FEED_BLOOM_DOMAINS.length,
    bloom: { m: bloom.m, k: bloom.k, seed: bloom.seed, bits: bloom.toBase64() },
    blocklist: FEED_BLOCKLIST
  };
}

async function signedFixture(bundle) {
  const file = join(out, 'feed-fixture.json');
  if (existsSync(file) && !process.argv.includes('--resign')) return;
  const { subtle } = webcrypto;
  const pair = await subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, true, ['sign', 'verify']);
  const spki = Buffer.from(await subtle.exportKey('spki', pair.publicKey)).toString('base64');
  const sig = Buffer.from(await subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, pair.privateKey, new TextEncoder().encode(bundlePayload(bundle)))).toString('base64');
  const signed = { ...bundle, sig };
  if (!(await verifyBundle(signed, { spki }))) throw new Error('fixture does not verify');
  const fixture = {
    note: 'A feed signed with a throwaway test key (not the registry key). Synthetic data, not real threats.',
    testKey: { spki },
    payload: bundlePayload(bundle),
    bundle: signed
  };
  writeFileSync(file, JSON.stringify(fixture, null, 1) + '\n');
}

const data = loadData();
const bundle = feedBundle();
const fed = applyBundle(data, bundle);

const golden = {
  note: 'Generated by Extensions/fishcatcher/parity/golden.mjs from the original JS engine. Do not edit by hand.',
  urls: URLS.map((url) => ({ url, result: score(url, data) })),
  feed: {
    bundle,
    urls: FEED_URLS.map((url) => ({ url, result: score(url, fed) }))
  },
  pages: PAGES.map(({ url, facts }) => ({ url, facts, result: scorePage(url, facts, data) })),
  texts: TEXTS.map(({ text, title }) => ({
    text,
    title,
    kind: aitmKindForText(text),
    seed: scamCryptoSeedText(text),
    tech: scamTechSupportText(text),
    deviceCode: matchDeviceCodeScam(text, title)
  })),
  links: LINKS.map(({ links, deep }) => ({ links, deep, findings: classifyLinks(links, data, deep) })),
  downloads: DOWNLOADS.map(({ name, mime }) => ({ name, mime, result: inspectDownload(name, mime) }))
};

mkdirSync(out, { recursive: true });
writeFileSync(join(out, 'golden.json'), JSON.stringify(golden, null, 1) + '\n');
await signedFixture(bundle);

const scored = golden.urls.filter((u) => u.result && !u.result.error);
const levels = {};
for (const u of scored) levels[u.result.level] = (levels[u.result.level] ?? 0) + 1;
console.log(`${golden.urls.length} addresses (${scored.length} scored, ${golden.urls.length - scored.length} not web or refused), levels ${JSON.stringify(levels)}`);
console.log(`${golden.feed.urls.length} feed, ${golden.pages.length} pages, ${golden.texts.length} texts, ${golden.links.length} link sets, ${golden.downloads.length} downloads`);
