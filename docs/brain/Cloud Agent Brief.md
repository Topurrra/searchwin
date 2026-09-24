---
tags: [searchwin, handoff, cloud-agent]
updated: 2026-09-24
---

# Cloud Agent Brief

Back to [[README]] · The plan this executes: [[Master Plan]]

> For a Claude Code agent working on `github.com/Topurrra/searchwin` in the
> cloud. Read this, then [[Master Plan]], [[Architecture]], [[Conventions]] and
> [[Lessons Learned]]. In the repo these notes are in `docs/brain/`.

## What you're building

**Search for Windows:**
- a small, fast, private browser (C# 13 / .NET 9, WinUI 3, WebView2, Native AOT);
- it's becoming *"a browser first, then a Raycast-like launcher, but better"*;
- the copied KeepItLocal Workspace engine (`Engine/`, Rust) runs beside it as a headless helper;
- Workspace's tool screens (`Tools/`, Svelte) open as pages in tabs;
- one field reaches the web, tabs, files, apps, clipboard, commands and AI;
- everything is on-device, with no telemetry and no server.

## Your environment: read this first

You run on **Linux**. You **cannot** build or run the WinUI app (`Search/`),
the installer or Native AOT, and you can't see the app. What you *can* do:

| Check | How |
|---|---|
| Portable C# logic | `dotnet build` / `dotnet test` on `Search.Kit` + `Search.Kit.Tests` (net9.0, **no WinUI/WebView2 references**) |
| Tools (Svelte) | `pnpm install && pnpm build && pnpm test` in `Tools/` |
| Engine (Rust) | `cargo check --no-default-features` (heavy native features are gated). For the Windows target: `rustup target add x86_64-pc-windows-gnu` + `apt install mingw-w64`, then `cargo check --target x86_64-pc-windows-gnu --no-default-features` |
| JS assets (FishCatcher engine, page scripts) | Node tests |
| **Windows build of everything** | **GitHub Actions CI** (`.github/workflows/ci.yml`, `windows-latest`). Push your branch and read the checks. The CI result is the proof that the Windows build works |

## Hard rules
1. **Work in `searchwin` only.** Never modify any other repository.
2. **KeepItLocal Redact is off-limits.** Don't copy, port or imitate it.
   `Engine/src/core/resources.rs` was deliberately left out (it came from
   Redact's predecessor). **Write a new, original** throttling module (task T7).
3. **Don't touch `Search/`** (the WinUI app) except the one `ProjectReference`
   in T1. You can't test UI changes. The user and the local Claude do the UI integration.
4. **Native AOT safe:**
   - no reflection;
   - source-generated `System.Text.Json` for fixed types, `JsonNode` for open-ended user data;
   - `IsAotCompatible=true` on `Search.Kit`, with its analyzer warnings as errors;
   - prefer the .NET base library over NuGet packages, and check any package's AOT status before adding it.
5. **Speed budgets** ([[Master Plan#Budgets (the bench measures them in CI)]]):
   - field logic < 5 ms per keystroke;
   - nothing loads before the first window;
   - large data loads lazily;
   - add a benchmark test for every hot path.
6. **Privacy:** no network calls, except explicit, user-started downloads such as
   filter lists or feeds, and those go only to the documented publisher URLs.
7. **Git:**
   - one branch per task, `cloud/<task-id>-<slug>`, and a PR to `main`; **never push to `main`**;
   - commit messages **as short as possible**, one line;
   - **never** a `Co-Authored-By` line or any AI attribution.
8. **Third-party code:** Search is MIT. Don't paste GPL code (for example,
   uBlock Origin's scriptlets are GPL-3.0: write your own). Record anything
   copied in `third_party/PROVENANCE.md`.
9. **Keep the brain updated.** In each PR, update:
   - `docs/brain/Log/<date>.md` (what you did);
   - `docs/brain/Lessons Learned.md` (anything that cost more than 10 minutes);
   - `docs/brain/Decisions.md` (any real choice, as the next Dxx).

   The local Claude syncs these back into the Obsidian vault.

## Tasks, in order

Portable, Linux-testable work comes first; the riskier Engine and Tools work comes last.
Every task ends with: CI green, tests added, the brain updated, and the PR open.

### T0 · CI green
`.github/workflows/ci.yml` exists, but it's untested. Make every job pass,
or mark the Engine/Tools jobs `continue-on-error` until T7/T8 land, with a
comment. Add caching (NuGet, pnpm, cargo).

### T1 · `Search.Kit` + tests
- `Search.Kit/` (class library, net9.0, `IsAotCompatible`, `Nullable`, no UI references)
- `Search.Kit.Tests/` (xUnit)
- Add `<ProjectReference>` from `Search/Search.csproj`. CI must still build the app.
- Add both to CI.

### T2 · Command registry (the heart of it)
Read `Tools/src/**/commandRegistry.ts`, `voiceSafetyGate.ts`, `myCommands.ts`
and `Engine/src/commands/quick_actions.rs` to understand the shape, then
build it **once**, in C#, in `Search.Kit`:
- **Command:** `id`, title, aliases/bangs, spoken phrases, parameters, **tier**
  (`Read` / `Act` / `AlwaysAsks`), context (page, any, files…), handler.
- **Field parser:** `!bang query`, `>command args`, `?question`,
  converter-looking input, otherwise address/search. Allow an escape for a
  literal leading `!` or `>`.
- **User-defined bangs and commands** are stored as `JsonNode` (the file lives
  in the app's data folder; take the folder path as a parameter).
- Built-in bangs: a reasonable DuckDuckGo-style starter set, with user overrides.
- **One** list. Workspace's mistake was three lists joined only in the UI.
- Tests: parsing, precedence, tiers, and < 5 ms per parse.

### T3 · FishCatcher native engine (phase 1: protection)
Port `Extensions/fishcatcher/src/engine/*` to C# in `Search.Kit` (`FishCatcher/`):
- The 17 URL signals, PSL, brand/homoglyph checks, the ML scorer and the Bloom filters.
- Data files become **embedded resources** loaded lazily (off the UI thread,
  on the first check). **Bloom filters as raw binary**, not base64 JSON.
- The daily feed client: download, **verify the ECDSA signature** (find the
  key and format in the extension source), fail open. Keep "warn, never block".
- **Parity test:** run the original JS engine in Node over a corpus of
  URLs (phishing and legitimate, as fixtures) and assert the C# scores and
  verdicts match (golden file).
- Budget: < 2 ms per URL check once loaded.
- Keep `probe.js` (the DOM probe) as a JS asset; document how the app will inject it.

### T4 · Shields core (phase 1)
In `Search.Kit/Shields/`:
- **Tracking-parameter stripping** (`utm_*`, `fbclid`, `gclid`, `mc_eid`, … as a data list) for navigations and "Copy Address".
- **Debounce:** unwrap known redirectors (`l.facebook.com`, `t.co`, Google `/url?q=`…).
- **AMP → canonical** detection script (JS asset).
- **Filter-list compiler:** parse EasyList/EasyPrivacy syntax into:
  - (a) domain-anchored network rules in a compact binary set, with a fast
    matcher (benchmark it, since it's a candidate for `WebResourceRequested`);
  - (b) generic and per-site cosmetic CSS.

  Lists are downloaded at runtime (the user starts it), never bundled.
- **YouTube:** write **our own** small scriptlets (`json-prune`-style removal
  of `adPlacements`/`playerAds`/`adSlots` from `ytInitialPlayerResponse` and
  fetch/XHR player responses, plus `set-constant`). Test them against
  recorded JSON fixtures. Don't copy uBO's GPL code.

### T5 · Converters (phase 2)
In `Search.Kit/Convert/`: units, currency (offline math plus a rates-provider
interface; no fetching here), base64/hex/URL, hashes, colours, epoch, JWT
decode, JSON↔YAML↔TOML↔XML, and QR encode.
- Read `Engine/src/commands/{encoders,format,hash,qr}.rs` for behaviour, and
  **fix `format.rs`'s known bugs** (self-closing tags, XML root arrays).
- AOT-safe libraries only, or write it yourself.
- Registered as commands in T2's registry.
- < 5 ms per conversion.

### T6 · Page → Markdown asset
A JS bundle (Turndown + the GFM plugin, both MIT) with:
- a **visible-only** filter: drop `display:none`, `visibility:hidden`,
  zero-size and off-screen elements;
- content selection the same way `Search/Core/Reader.cs` picks the article;
- absolute URLs.

Tests with jsdom/happy-dom fixtures. Output to `Search/Assets/js/page-markdown.js`.
This is the one allowed addition under `Search/`: a new asset file, not code.

### T7 · Engine goes headless (phase 0, the big one)
Turn `Engine/` into `kil-engine`, a headless binary with no Tauri:
- Remove the `tauri*` dependencies. Replace `#[tauri::command]` with a
  **dispatcher** (method name → handler), and `AppHandle`/`emit` with an
  event-sink trait.
- **JSON-RPC 2.0 over a Windows named pipe** `search-engine` (plus `-<world>`
  in test worlds; the data dir goes the same way), restricted to the current user
  (security descriptor), with a server → client event stream.
  Document it in `docs/brain/Engine Protocol.md`.
- **Keep Workspace's command names** as the method names, so the tool pages' `invoke()` maps one to one.
- A new, **original** `core/throttle.rs` in place of the missing
  `resources.rs` (RAM-aware worker counts, Job Object memory cap). Don't
  reconstruct the old file; design it from what its callers need.
- Heavy features stay behind cargo features, off by default: vosk,
  ocr-leptess, semantic, screenrec, image-background-removal.
- Do it module by module. Search, clipboard and sensitive_scan first; each module is a commit.
- CI's Windows job must `cargo build` the default (light) configuration.

### T8 · Tools as pages (phase 0)
In `Tools/`:
- **Vite aliases** that replace `@tauri-apps/api/{core,event,window,webviewWindow,path,dpi,app}`
  and `@tauri-apps/plugin-{dialog,fs,opener,notification,autostart}` with a
  **shim** over `window.chrome.webview.postMessage`. The shim uses
  request ids, promises, and event subscriptions matching T7's protocol.
- **A mock host** for `pnpm dev` in a normal browser, with fake engine responses, so the pages can be developed on Linux.
- **Remove what the field replaces:** the palette (`PaletteV2.svelte`),
  window-only routes (splash, overlays, capture bar) and tray/autostart.
- **Design tokens** mapped to Search's palette. The colours are in `Search/UI/Design.cs`.
- A static build to `Tools/dist`.
- Tests for the shim.

## Out of scope for the cloud
- The WinUI app code
- The installer and MSIX
- Running or screenshotting the app
- Voice/OCR model packaging
- Anything to do with KeepItLocal Redact
- Changes to other repos
- Merging PRs

## When you stop
Leave a summary comment on each PR: what's done, what's verified (which
tests, and CI), and what's **not** verified (anything that needs Windows or a
human). The user and the local Claude continue from there.
