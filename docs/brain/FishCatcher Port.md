---
tags: [searchwin, fishcatcher, protection]
updated: 2026-09-25
---

# FishCatcher, native (Search.Kit)

Back to [[README]] · Plan: [[Master Plan]] (phase 1) · Idea: [[Ideas/Built-in Tools#2.6 FishCatcher]]

FishCatcher's scoring engine now runs in C#, in `Search.Kit/FishCatcher/`
(namespace `SearchKit.FishCatcher`). It gives the same verdicts as the
extension's JS engine, checked by a parity test over 604 addresses.

## What's there

| File | Port of | Role |
|---|---|---|
| `FishCatcher.cs` | background.js (loading, trust, feed) | The static entry point: `Check`, `CheckAsync`, `Warm`, `ApplyFeed`, `SetTrusted` |
| `Analyzer.cs` | analyzer.js + `flagDeviceCode` | Address + page facts → `Verdict` |
| `Signals.cs` | signals.js | S1–S17, in the extension's order |
| `Psl.cs` / `Punycode.cs` / `Bloom.cs` / `MlModel.cs` | psl.js / punycode.js / bloom.js / ml.js | The building blocks |
| `Aitm.cs` / `FormAction.cs` / `ScamPacks.cs` / `DeviceCode.cs` | aitm.js / formaction.js / scampacks.js / devicecode.js | Page-fact scoring and the probe's text matchers |
| `Links.cs` | links.js | Link findings and download-type checks |
| `FeedBundle.cs` / `FeedClient.cs` / `JsJson.cs` | remote.js + `loadRemoteLists` | The signed daily feed |
| `WebAddress.cs` | (`new URL()`) | Reads a `Uri` the way WHATWG/Chromium would |
| `PageFacts.cs` | probe.js messages | The probe's facts (`aitm-scan`, `scam-scan`, device code…) |
| `Messages.cs` | _locales/en | Reasons as English sentences |

Data: `Search.Kit/FishCatcher/Data/`, embedded resources named `FishCatcher/<file>`.
`Extensions/fishcatcher/parity/convert-data.mjs` copies the JSON tables and
turns the two big base64 tables into raw binary (`safe-bloom.bin` 179 KB,
`ml-weights.bin` 64 KB). Run it again when the extension's data changes.

## How the browser uses it
- `FishCatcher.Warm()` after the first window (or nothing: the first check
  starts loading). Loading is on a worker thread: ~10 ms cold in the AOT build.
- `FishCatcher.Check(uri)` in `NavigationStarting`: null until loaded, for
  non-web addresses, and on any error (**fail open**). Otherwise a `Verdict`:
  `Level` (Low/Elevated/High/Critical), `Score` 0–100, `Reasons` (sentences),
  `Signals` (keys, params, weights), `RealSite`, `Warns` (High or Critical).
  **Warn, never block.**
- Page facts: the probe script posts JSON; `PageFacts.Parse(json)` and
  `Check(uri, facts)`. The shapes are the extension's own messages.
- Feed (opt-in, as in the extension): `new FeedClient(http, folder,
  FishCatcher.Data.RegistryKeySpki)`; `LoadSaved()` at start, `RefreshAsync()`
  daily; `FishCatcher.ApplyFeed(bundle)` on success. Any failure keeps the last good feed.
- The class `FishCatcher` sits in the namespace `SearchKit.FishCatcher`: code
  inside `namespace SearchKit…` should alias it (`using Fish = SearchKit.FishCatcher.FishCatcher;`).

## Numbers (2026-09-25)
- Parity: 604 addresses (594 scored), 18 with a feed, 29 page-fact cases, 37
  texts, 14 link sets, 14 downloads: all identical to the JS engine; the model's
  probability within 1e-9.
- A check: ~130 µs mean in a Debug test run, ~90 µs in Native AOT.
- Native AOT: builds with no warnings and runs (resources, generated regexes, ECDSA).

## Lessons
- **.NET `Uri` is not WHATWG.** It accepts `999.1.1.1` and `example.1` as names
  (WHATWG refuses anything ending in a number that isn't IPv4), writes
  `[::ffff:1.2.3.4]` where WHATWG writes `[::ffff:102:304]`, and refuses
  `paypal.com%2eevil.com` (WHATWG decodes it to `paypal.com.evil.com`).
  `WebAddress` fixes these up. Its IDN mapping (fullwidth, ß, Greek capitals) matched Node's.
- **JS regexes don't port as-is:** `\b` is ASCII-only in JS, `\s` includes
  U+FEFF, `.` also stops at U+2028/2029, and `toLowerCase` turns U+0130 into two
  chars. `JsText` spells these out.
- **Verifying a signature over `JSON.stringify` output** means writing JS's JSON:
  array-index keys first, `1e+21`, `-0` → `0`, lone surrogates as `\ud800`.
  `JsJson` does it, and the fixture tests exactly those cases.
- **Claude's Write tool turns `\uXXXX` escapes into the characters** (U+2028 then
  broke a C# char literal). Write special characters as C# `\x2028` or JS
  `String.fromCharCode` instead.
- The worktree-isolated agent's shell refuses heredocs followed by other
  commands and globbed `sed -i`; write scripts to the scratchpad and run them.

## Open
- Wire into the browser: `NavigationStarting` indicator, the probe script, the
  bar for High/Critical, Settings (feed opt-in, trust list, strict mode).
- The probe's text matchers exist in C#, but the probe itself is still JS in the extension.
- Georgian translations of the reasons (the extension has `ka`).
