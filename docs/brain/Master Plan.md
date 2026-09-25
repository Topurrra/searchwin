---
tags: [searchwin, plan]
updated: 2026-09-25
status: agreed
---

# Master Plan: Search with everything in it

Back to [[README]] · Built from [[Ideas/Agentic Browser]], [[Ideas/Built-in Tools]], [[Ideas/KeepItLocal Workspace]]

> **The one-line version:** Search becomes the home for everything. It's a
> fast private browser whose one field reaches the web, your tabs, files,
> apps, clipboard, commands and AI. KeepItLocal Workspace's engine runs
> beside it, and Workspace's tools open as pages in tabs.
>
> **Positioning (user, 2026-09-24):** *a browser first, then a Raycast-like
> launcher, but better.* The browser experience comes before anything else.

## Decided (2026-09-24)
- **Tray/background mode: off by default.** It's a browser first.
- **No standalone Workspace alongside Search.** Search is the product.
  **Workspace itself is kept and never deleted:** it's the user's first
  project. Search works from a *copy* of its engine and tools; the original repo is never touched.
- **SmartScreen, the Windows one:** the blue "Windows protected your PC"
  dialog on an unsigned installer is solved by shipping through the
  **Microsoft Store** once the product is finished (see phase 7).
- **SmartScreen, the Edge one:** the WebView2 feature that checks each site
  you visit with Microsoft is a different thing. It stays **off**, and
  **FishCatcher replaces it**, fully on the device. Settings › Privacy could
  later offer it as an opt-in.
- **OCR:** Windows' built-in OCR in Core; Tesseract as an optional pack.
- **Order:** protection first (phase 1), then the universal field.
- **KeepItLocal Redact is not integrated, in any form.** It's the user's
  upcoming **paid** product (on-device redaction of files, audio and video).
  See [[#Redaction boundary]].

## Should Workspace go inside the browser? Yes, done like this

**Why yes:**
- A browser is the app people keep open all day, and it already has a field, tabs and a web renderer.
- Workspace has the engine (search, clipboard, voice, OCR, crypto, media) and 68 tool screens.
- Both run on WebView2 and share the same privacy rules, so together they're one product with one command surface.

**How, without losing Search's speed:**
1. **Don't rewrite the engine in C#** (~2.9 MB of tuned Rust). **Copy it into
   `searchwin/Engine`**, remove Tauri, and run it as a **headless helper
   process** (`kil-engine.exe`) that Search starts *after* the first window,
   or only when a feature is used.
2. **Tools are pages.** Workspace's Svelte tool screens run inside Search tabs
   (`https://tools.search/…`). A small shim replaces the Tauri API they use
   (`invoke`, events, dialogs, fs, opener, path), so they need little rewriting.
   They're restyled with Search's tokens.
3. **Search's own chrome stays native WinUI:** the field, tabs, panels,
   overlays. Workspace's palette (`PaletteV2.svelte`, 16k lines) is **not**
   carried over; **the field replaces it.**
4. **One command registry** in C# feeds the field, voice, the agent and
   scripts/MCP. Engine commands and tool pages register into it.

## What it looks like (the user's view)

### The field does everything
Type in the one field (`Ctrl+L`) or the palette (`Ctrl+K`):

| You type | You get |
|---|---|
| `github.com` / `rust ownership` | Go there / search (as today) |
| `invoice 2024` | **Files** by name and contents (OCR too) and **apps**, alongside tabs, history and bookmarks. Enter opens the file **in a tab** (PDF, image, video, text, Markdown) or in its own app |
| `128 usd to gel`, `sha256 hello`, `#e07a5f to hsl`, `json2yaml` | An **instant answer** inline; Enter copies it |
| `!yt cats`, `!gh searchwin`, `!jira ABC-1` | **Bangs**, built in and your own |
| `>` + a command | **Actions**: `>close others`, `>new space Work`, `>shred`, `>screenshot redact`, `>save page as pdf`, `>copy as markdown` |
| `clip: invoice` or `Ctrl+Shift+V` | **Clipboard history** (search, pin, paste as plain text) |
| `? what's the catch here` | **Ask the page / tabs** with your own model ([[Ideas/Agentic Browser]]) |
| `tools` or `>image studio` | Opens a **tool page** in a tab |

### Surfaces
- **Tabs and spaces:** as today, plus an **Agent space** with a coloured ring.
- **Tool pages** (`tools.search/…`), grouped by pack:
  - Utilities
  - Files: the dual-pane file manager, archive, bulk rename, duplicates, cleaner
  - Images: studio, OCR, redact, watermark
  - Media: screen recorder, media utility
  - Documents: Markdown ⇄ DOCX/PDF, CSV
  - Development: JWT, regex, SQL, diff, SSH, encrypt
  - Privacy: shredder, screenshot region redact, privacy audit (file/audio/video redaction stays with KeepItLocal Redact)
  - Focus: reminders, time tracker, focus mode

  They're bookmarkable, pinnable and sit in spaces like any page.
- **Side panel** (one at a time): clipboard, notes/quick notes, the agent's step log, downloads.
- **Overlays:** the voice listening pill, region select (capture and redact), notification toasts.
- **Tray / background mode** (optional, **off by default**, decided): clipboard history,
  the snippet expander, global hotkeys and voice dictation into *other* apps
  keep working with the browser window closed. Only the engine stays
  running, under a hard memory cap. Opening Search from the tray is instant.
- **The hamburger menu:** gains *Tools*, *Clipboard*, *Voice*, *Packs*.

### Settings › Packs
Workspace's feature packs, turned on or off locally.
- **Core** (always on): the field, shields, FishCatcher, converters, bangs,
  clipboard, file search (names + text).
- **Optional**, downloaded when enabled: Voice (Vosk model), OCR
  (Tesseract/tessdata), Semantic search (MiniLM), Images (background-removal
  model), Documents (pdfium), Media (uses *your* ffmpeg, never bundled), AI.

## Architecture

```
Search.exe (C#, WinUI 3, Native AOT) ─ the shell; first window ~250 ms, nothing else loads before it
 ├─ UI: field · tabs/spaces · panels · overlays · tray
 ├─ Command registry (one list, permission tiers: read / act / always-asks)
 ├─ Shields 2.0 (lists, YouTube scriptlets, cookies, params) · FishCatcher (native port)
 ├─ Agent: eyes (Reader + AX tree + Turndown) · hands (bench verbs) · leash · BYOK models
 ├─ Tools host: WebView2 tabs at https://tools.search/ ← Svelte tool pages + Tauri-API shim
 │        shim: invoke/event/dialog/fs/opener/path → postMessage → C# → engine
 └─ Engine client (named pipe, JSON-RPC + event stream), started lazily
                │
kil-engine.exe (Rust, headless, copied from Workspace, Tauri removed)
 ├─ Index: Tantivy names + contents, watcher, OCR-on-index, MiniLM (pack), frecency ranker
 │    └─ ALSO the agent's memory: history/page text go in the same index (one index)
 ├─ Clipboard listener + sensitive detector + DPAPI store (images encrypted too)
 ├─ Snippets expander · global hotkeys · voice (Vosk, grammar, safety gate)
 ├─ Disclosure Firewall (pre-AI redaction) · text extraction · crypto · archive
 ├─ Image / document / media tools (heavy ones in worker subprocesses, as Workspace does)
 └─ Job Object memory cap · RAM-aware throttling (from Workspace)
WebView2 processes: pages, tool pages
MCP / bench: same pipe protocol, same registry, same leash
```

**Why two processes:** Search's UI starts in ~250 ms without Tantivy, Vosk or
OCR loaded. A crash in a heavy tool doesn't take the browser down. And the
engine can stay resident alone (tray mode) at a fraction of the UI's memory.

**It still works with the C++ frame plan** ([[Roadmap]]): the engine and the
tool pages don't depend on the shell, so a future C++ shell reuses both as they are.

## Repo layout (everything copied in; source repos untouched)

```
searchwin/
  Search/                 C# shell (today's app)
  Engine/                 Rust workspace — copied from KeepItLocal-Workspace/src-tauri
    kil-engine/           bin: pipe server, dispatcher replacing #[tauri::command]
    crates/…              index, clipboard, voice, firewall, tools… (split as we go)
  Tools/                  Svelte app — copied from Workspace src/lib/tools (+ ui, stores, i18n)
    shim/                 @tauri-apps/api stand-ins (Vite alias)
  Extensions/fishcatcher/ copy of FishCatcher (stopgap until the native port)
  Shields/                filter-list build + scriptlets
  Installer/  build.ps1   builds shell + engine + tools; packs as separate zips
  third_party/            PROVENANCE.md (source repo + commit per copy), NOTICE for Vosk/Tesseract/pdfium
```

Sources to copy from:
- KeepItLocal-Workspace (engine + tools)
- FishCatcher
- Verifier (TOTP, QR, migration and CSV import)
- Guard (policy presets, audit chain)

**Never** copy from KeepItLocal Redact. One piece of Workspace,
`core/resources.rs` (RAM-aware throttling), came from Redact's predecessor
(kil-privacy-suite). Search's engine gets **its own** small throttling module
instead, so nothing from Redact ships in Search.

## Redaction boundary
Search keeps only the small redaction that belongs to browsing:
- **pre-AI text masking** (the Disclosure Firewall, which came from KeepItLocal Memory);
- **secret warnings in the clipboard/paste guard**;
- **blurring a region of a page screenshot** (Workspace's own `redact.rs`).

**File, document, audio and video redaction is KeepItLocal Redact's job.**
Search doesn't build it. Later, when Redact is out, Search could offer "Open
in KeepItLocal Redact" when it's installed: a hand-off, not an integration.

## Engine protocol (one for everything)
- Named pipe `search-engine[-world]`, `CurrentUserOnly`: JSON-RPC requests + a server event stream.
- Methods keep Workspace's command names (`search.query`, `clipboard.list`,
  `ocr.image`…), so the tool pages' `invoke("…")` calls map one to one.
- Every method declares a **tier**. The registry enforces the leash: an
  always-asks call from a script, the agent or MCP needs an on-screen confirm.
- Test worlds carry over (`SEARCH_PROBE` → the engine's data dir and pipe name).

## Budgets (the bench measures them in CI)

| | Budget |
|---|---|
| First window | ≤ 300 ms. Nothing from the engine before it |
| Keystroke in the field | < 5 ms for local sources; file results stream in (< 50 ms) |
| Engine idle (tray mode) | Target < 60 MB, capped by a Job Object |
| Installer (Core) | Target ≤ 45 MB (shell 21 + engine ~15–20 + tools ~5). Packs are separate downloads |
| Page load with shields | Faster than with shields off |

## Phases

| # | Phase | Ships | Rough size |
|---|---|---|---|
| **0** ✅ | **Foundations** (2026-09-25: engine headless, engine client, tool pages, command registry; packs framework moved to phase 3) | Repo layout; copy the engine and strip Tauri (dispatcher + pipe); tools host + shim; command registry; packs framework; provenance/NOTICE | 3–5 wks |
| **1** ✅ | **Protection** (2026-09-25: FishCatcher native + Shields 2.0, reviewed twice, fixes verified) | FishCatcher native (probe budget, message caps, fact sanitising); Shields 2.0 (wildcard O(n) matcher, whole-word token index, PSL registrable domains, signed feed with rollback, cookie/AMP/params/redirects, ~200 KB cosmetics); lists + feed on by default | 3–4 wks |
| **2** ✅ | **The universal field** (2026-09-26: built, reviewed three times, all high and medium findings fixed and verified; small items in [[Log/2026-09-26]]) | Converters, bangs/custom commands, files + apps from the engine index, clipboard history + secret guard, open files in tabs, search://play media route | 3–4 wks |
| **3** | **Tools as pages** | Pack by pack: Utilities → Files (file manager) → Privacy → Images → Documents → Dev → Media → Focus, each restyled and tested | 4–8 wks |
| **4** | **Voice + background** | Vosk pack, push-to-talk everywhere, safety gate, tray mode, snippets, global hotkeys | 2–3 wks |
| **5** | **Ask** | BYOK models (local + keys), Disclosure Firewall on every send, page → Markdown, ask page / tabs / video, receipts | 3–4 wks |
| **6** | **Act + remember** | Agent space, leash, step log; memory in the shared index; watchers; recipes; MCP server/client | 5–8 wks |
| **7** | **Ship-ready: Microsoft Store** | MSIX package (Store signing removes the SmartScreen warning, and Store updates replace our own updater); Windows App Runtime as a Store framework dependency (the smaller-download variant from [[Roadmap]] §1); signed pack downloads; Arm64; accessibility pass; red-team suite | 2–3 wks |

Each phase ships something usable. Phases 1 and 2 alone already make Search
better than most browsers.

## What happens to Workspace (the standalone app)
**Decided:** Search is the product, and there's no second frontend. The
Workspace repo stays as it is (kept, not deleted, not modified). Search's
`Engine/` and `Tools/` are copies that evolve independently from phase 0 on.

**Store notes for phase 7:**
- A packaged (MSIX) app runs the engine as a full-trust helper.
- Global hotkeys, the clipboard listener and a tray icon all work in full trust. Test them early.
- The per-user NSIS installer can stay as a direct-download option.

## Risks
- **The phase 0 refactor** (Tauri types in every command) is the biggest single
  job. Do it mechanically, one module at a time, behind the dispatcher.
- **Scope.** 68 tool screens: ship packs one at a time, and cut any tool that
  doesn't "beat the free version" (Workspace's own rule 6).
- **Two UI technologies** (WinUI chrome + web tool pages). Shared design tokens
  and one icon set keep them looking like one app.
- **Resident mode** has to stay honest: off by default, a visible tray icon, a hard memory cap.
- Third-party runtimes (Vosk, Tesseract, pdfium, models) need NOTICE files. FFmpeg is never bundled.

## Decisions
- [x] Tray/background mode: **off by default**.
- [x] Workspace: **no standalone frontend**; the repo is kept untouched.
- [x] OCR: **Windows OCR in Core, Tesseract as a pack**.
- [x] Site checks: **Edge SmartScreen off, FishCatcher instead**; installer signing through the **Microsoft Store**.
- [x] Phase order: **protection first**.
- [x] KeepItLocal Redact: **not integrated**; a possible hand-off later.
