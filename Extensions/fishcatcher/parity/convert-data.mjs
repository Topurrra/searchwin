// Copies FishCatcher's data into the C# port (Search.Kit/FishCatcher/Data) and
// turns the two big base64-in-JSON tables into raw binary, so the browser
// reads them straight into memory instead of parsing and decoding ~330 KB of
// JSON text. Run it again whenever src/data changes:
//
//   node Extensions/fishcatcher/parity/convert-data.mjs
//
// safe-bloom.bin (little-endian):
//   "FCBF"  u32 version=1  u32 m  u32 k  u32 seed  u32 count  u8[ceil(m/8)] bits
// ml-weights.bin (little-endian):
//   "FCML"  u32 version (the model's, 2)  u32 buckets  u32 ngram count  u32[] ngrams
//   f64 scale  f64 bias  f64 threshold  u32 table length  i8[] table
// Doubles are written as the exact values JSON.parse gave the extension.
import { readFileSync, writeFileSync, copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const src = join(here, '..', 'src', 'data');
const dst = join(here, '..', '..', '..', 'Search.Kit', 'FishCatcher', 'Data');
mkdirSync(dst, { recursive: true });

// Small tables stay JSON (read once, lazily, off the UI thread).
for (const name of ['brands.json', 'tlds.json', 'keywords.json', 'psl.json', 'blocklist.json', 'safe-list.json', 'aitm-allow.json', 'registry-key.json']) {
  copyFileSync(join(src, name), join(dst, name));
}

function u32(n) {
  const b = Buffer.alloc(4);
  b.writeUInt32LE(n >>> 0);
  return b;
}
function f64(n) {
  const b = Buffer.alloc(8);
  b.writeDoubleLE(n);
  return b;
}

{
  const p = JSON.parse(readFileSync(join(src, 'safe-bloom.json'), 'utf8'));
  // Bloom.fromPayload: the array is ceil(m/8) bytes, filled from the payload as far as it goes.
  const bits = Buffer.alloc(Math.ceil(p.m / 8));
  Buffer.from(p.bits, 'base64').copy(bits, 0, 0, bits.length);
  writeFileSync(join(dst, 'safe-bloom.bin'), Buffer.concat([Buffer.from('FCBF'), u32(1), u32(p.m), u32(p.k), u32(p.seed ?? 1), u32(p.count ?? 0), bits]));
}

{
  const ml = JSON.parse(readFileSync(join(src, 'ml-weights.json'), 'utf8'));
  const ngrams = ml.ngrams ?? [3, 4, 5];
  const table = Buffer.from(ml.table, 'base64');
  writeFileSync(join(dst, 'ml-weights.bin'), Buffer.concat([
    Buffer.from('FCML'), u32(ml.version), u32(ml.buckets), u32(ngrams.length), ...ngrams.map(u32),
    f64(ml.scale), f64(ml.bias), f64(ml.threshold ?? 0.6), u32(table.length), table
  ]));
}

console.log('wrote', dst);
