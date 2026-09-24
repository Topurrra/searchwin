---
tags: [searchwin, fishcatcher, protection]
updated: 2026-09-25
---

# FishCatcher, native (Search.Kit)

Back to [[README]] · Plan: [[Master Plan]] (phase 1) · Idea: [[Ideas/Built-in Tools#2.6 FishCatcher]]

FishCatcher's scoring engine now runs in C#, in `Search.Kit/FishCatcher/`
(namespace `SearchKit.FishCatcher`). It gives the same verdicts as the
extension's JS engine, checked by a parity test over 604 addresses, and the
browser warns with it before a scam site loads (see *In the browser* below).

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

## The engine's API
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

## In the browser (`Search/Core/Fish.cs`)

Settings › Privacy › **Warn about scam and phishing sites** (on by default,
`fishcatcher` in settings.json) and **Daily list of reported scam sites** (off
by default, `fishcatcher.feed`: it's a download, so it's yours to ask for).

- **Three places an address is checked**, all with the same verdict (~0.1 ms):
  1. `Tab.Forewarn`, before Search hands the engine an address (typed,
     bookmarks, links from other apps, bench, a tab waking). A warned address
     never reaches the engine: **no DNS lookup, no connection** (checked with
     the DNS cache and a local server).
  2. `NavigationStarting`, for navigations that start inside the engine
     (links, redirects, `location.href`). It cancels, and puts the warning up.
  3. `WebResourceRequested` on document requests (the page and its frames,
     not subresources): the request `NavigationStarting` just stopped is
     answered with an empty 204, so it never goes out. Needed because
     `NavigationStarting` is **not** a gate (see Lessons).
- **High/Critical** → `TroubleKind.Scam`, Search's own page (Stage): the host,
  what such pages are after, the reasons as sentences, and **Go back** (filled),
  **Go to paypal.com** (when the engine names the real site) and **Continue
  anyway** (quiet; holds for that host until Search quits).
- **Elevated** → a quiet line at the bottom (Bars): the strongest reason and
  "Double-check the address", with OK (quiet for that host until quit).
- **The probe** (`Search/Assets/js/fish-probe.js`, embedded in the exe): the
  extension's `collectAitm` + `collectScam` and its text matchers, posting
  `fish.facts` over the bridge at DOMContentLoaded, a moment after load, and on
  the first focus of a password or code field; only when something is found
  and only when it changed. The browser checks the page again with those facts
  and acts only on a stronger level than the address alone gave (the page is
  then covered: `Loaded`).
- **Fail open**: before the tables load (they load 0.5 s after the first
  window), a navigation goes ahead and is checked late (`Late`); errors give no
  verdict. The first check runs on a worker so the UI thread never pays for JIT.
- **Feed**: `FishFeed`, 15 s after start and then hourly (the client only goes
  out once a day), on a lowest-priority thread, into `Store.Folder\FishCatcher`.
  Verified end to end in a test world: `Updated, 392611 sites`.
- **Bench**: `"fish"` on every tab description, `fish ID back|continue|real|ok`,
  and `SEARCH_HOST_RULES` / `SEARCH_NET_LOG` for test runs (see [[Testing]]).

### Measured (2026-09-25)
- Address check in the AOT build: 0.004–0.12 ms (google.com 0.006, a Critical
  lookalike 0.06–0.12). Debug: ~0.03–0.5 ms warm.
- Navigation cost of the document-request gate: request start 2.72 ms vs
  2.55 ms with warnings off (12 local navigations each), so ~0.2 ms.
- Probe pages (local server): a PayPal-branded password form on an unrelated
  host → Critical 100 (AiTM mismatch + form posting elsewhere), a fake
  "computer infected, call 1-888" page → High 70, a recovery-phrase request →
  Critical 75. Each covered by the warning.

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
- **WebView2's `NavigationStarting` is not a gate.** A local server got the
  full `GET /path` 17 ms after `location.href=…` while the handler was still
  running (it slept 5 s to prove it). The netlog shows a preconnect and the
  main-frame `URL_REQUEST` starting in parallel with the event. Cancelling
  still stops the page, but only `WebResourceRequested` (document context) can
  stop the request. `Navigate()` looks the name up before the event too, so
  check before calling it.

## Open
- **In-page links still get a speculative connection.** For navigations that
  start inside the engine, Chromium looks the name up and opens a TCP (and TLS)
  connection to the site *before* `NavigationStarting` is raised; the request
  itself (path, cookies) is stopped by the document gate, but the host sees a
  connection. `--disable-features=SpeculativePreconnect` / `LoadingPredictor`
  and a few Edge names didn't stop it (the feature isn't named anything we
  found). Addresses Search opens itself have no such gap.
- A redirect to a warned site: `NavigationStarting` cancels it, but whether
  the redirected request passes through `WebResourceRequested` is untested.
- An iframe from a warned host is never warned about or blocked (the
  extension didn't look at frames either).
- Not ported to the browser: a trust list and strict mode in Settings, the
  link scan and download checks (`CheckLinks`, `CheckDownload` exist),
  RDAP/Safe Browsing (opt-in in the extension), Family mode.
- Georgian translations of the reasons (the extension has `ka`).
