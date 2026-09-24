---
tags: [searchwin, ideas, tools, keepitlocal]
updated: 2026-09-24
status: proposal
---

# KeepItLocal Workspace → Search

Back to [[README]] · Related: [[Ideas/Built-in Tools]], [[Ideas/Agentic Browser]]

> Brainstorm only. It covers the user's private repo
> [Topurrra/KeepItLocal-Workspace](https://github.com/Topurrra/KeepItLocal-Workspace),
> analysed by 7 agents (5 area readers, 1 synthesis, 1 critical review), with
> the review's corrections folded in. **This note updates
> [[Ideas/Built-in Tools]]:** where they disagree, this one wins.

## What Workspace is

A local-first productivity suite for Windows: Tauri 2 + SvelteKit 5 + Rust
(one crate, 280+ Tauri commands in ~80 files). It has three pillars plus
~40 tools, all on-device, with no account and no telemetry. It follows **the same
rules as Search:** local-first, offline, no admin, no phone-home,
"deterministic over ML".

| Part | What it does | Maturity | Where |
|---|---|---|---|
| **Search** | Two Tantivy indexes (file names + contents), a `notify` watcher, OCR-on-index, optional MiniLM semantic search, a fused BM25 + frecency + recency ranker | High; the largest and best-tested part. Queries take single-digit to tens of ms | `search.rs` (~12,400 lines), `rank.rs` |
| **Clipboard history** | `WM_CLIPBOARDUPDATE` listener with a self-healing supervisor; excludes password managers; tags sensitive items with shorter retention; text DPAPI-encrypted | High | `clipboard_history.rs` (3,655) |
| **Sensitive detector** | Tiered secret/PII scanner with entropy gating and Luhn/SSN validators | High, 63 tests | `sensitive_scan.rs` (1,766) |
| **Voice** | Offline **Vosk**, a mic-arbitration ladder (push-to-talk 40 > overlays 30 > ambient), a deterministic command grammar, UI Automation control | Medium-high, no automated tests | `voice*.rs`, `commandRegistry.ts`, `voiceSafetyGate.ts` |
| **Command registry** | Declarative id / phrases / risk tier / context / confirm / action; drives the voice grammar *and* the text matcher | Good shape, but bangs live in a **second, hand-synced list** (TS + Rust) | `commandRegistry.ts`, `myCommands.ts`, `quick_actions.rs` |
| **Disclosure Firewall** | Fail-closed mask/generalize pipeline before AI, with a typestate `Disclosed` guarantee | Good code, **no production users yet** | `commands/firewall/*` (~740 lines, no Tauri coupling) |
| **Text extraction + OCR** | DOCX/PPTX/XLSX/RTF/PDF parsers, Tesseract, pdfium | High, the cleanest API boundary | `text_extract.rs`, `ocr.rs` |
| **Other-browser search** | Copy, query, sweep over Chromium/Firefox profiles; 90 s TTL; nothing kept | High | `browser_search.rs` |
| **File manager** | Dual pane, `IFileOperation`, deletes go to the Recycle Bin | Solid | `file_manager.rs` |
| **Converters** | Base64/hex/hash/QR, JSON↔YAML↔TOML↔XML, regex, SQL format | Good; `format.rs` has known bugs and no tests | `encoders.rs`, `format.rs`, `hash.rs`, `qr.rs` |
| **Screenshot redact, image studio, archive, DOCX↔MD↔PDF, doc metadata strip** | Mature tools | High | `redact.rs`, `image_tools.rs`, `archive.rs`, `word_pdf.rs`, `doc_metadata.rs` |
| `mft_walker.rs` | Raw `$MFT` reader | **Dead and buggy**, disabled. The MFT fast-path idea is closed | |

**It's very likely the voice app you meant.** Its voice (grammar, safety gate,
mic arbitration) goes well beyond KeepItLocal Memory's Vosk code.

**No outside interface exists today:** no CLI, pipe, HTTP or deep link, and
no `extern "C"` functions. Anything Search uses has to be ported or wrapped.

## How to integrate: the decision

| Option | Verdict |
|---|---|
| **A · Companion app over IPC** (Search talks to a running Workspace) | Not for v1. There's no protocol in either app, and Search would depend on a second app running |
| **B · A shared headless Rust engine (sidecar)** | Later, maybe. Every command is Tauri-typed, so carving Tantivy/OCR out is a multi-week refactor. Worth it only if Search needs Tantivy-grade content + OCR search |
| **C · Rust DLLs via P/Invoke** | **Yes, for a few large, tuned, pure modules.** Each needs a hand-written C ABI wrapper (none exist) |
| **D · Port to C#** | **Yes, for everything small and pure.** Best fit for Search: one codebase, no native binaries |
| **E · Host Workspace's Svelte UI in Search** | No. It's coupled to Tauri, and `PaletteV2.svelte` alone is ~16,000 lines |

**Recommendation:** a hybrid of D and C. Copy *designs* freely, wrap the few
modules that are expensive to re-derive, and keep Search one small C# app.
P/Invoke works in both of Search's builds (Native AOT when MSVC is present,
and the ReadyToRun fallback).

## The tools, revised

| Tool | Now comes from | Path | Effort |
|---|---|---|---|
| **Converters** | `encoders`, `hash`, `qr`, `format` | Port to C# (the .NET base library covers most of it). **Fix `format.rs`'s known bugs** during the port. Adds JSON↔YAML↔TOML↔XML | S |
| **Bangs / custom commands** | The shape of `commandRegistry.ts` | New C#: **one list**, not Workspace's two hand-synced ones | M |
| **Voice** | The Vosk pipeline, the command grammar, `voiceSafetyGate` (risk × confidence, spoken confirm with expiry), the mic ladder | Vosk's official .NET binding in a **lazily started sidecar** (models are 40 MB to 1.8 GB). Copy the safety gate and the ladder. Push-to-talk from any app needs `RegisterHotKey` | M |
| **Clipboard** | `clipboard_history.rs` + `sensitive_scan.rs` | Port the listener to C# on Search's HWND. **The detector:** a C# port (.NET `Regex` supports the lookarounds `fancy_regex` was needed for) or a DLL wrap. **Encrypt images too** (Workspace keeps them in plaintext) | M |
| **File search** | `search.rs`/`rank.rs`, `text_extract.rs`, `live_grep.rs` | v1 is still **SQLite FTS5 + the frecency formula** in C#. Wrap `text_extract` (DOCX/PPTX/XLSX/PDF text) and `live_grep` as DLLs for format coverage. Tantivy only via option B, later | M → L |
| **FishCatcher** | not in Workspace | Unchanged, see [[Ideas/Built-in Tools#2.6 FishCatcher]] | S → M |
| **File manager** | `file_manager.rs` | Port the design to C# (`IFileOperation`, progress and cancel, Recycle Bin by default). Read-only v1 | M → L |
| **Free AI / pre-send redaction** | `commands/firewall/*` | **Port it to C#** so the `Disclosed` guarantee lives in Search's types. Search becomes its first real user, so it needs its own tests | M |

### Which source wins over the earlier picks
| Need | Earlier pick | Now | Why |
|---|---|---|---|
| Secret/PII detector | KeepItLocal Guard `guard-core` | **Workspace `sensitive_scan.rs`** | More tuned and more tested (63 tests, entropy gating, validators). Guard's *policy presets* and hash-chained audit log still apply |
| Clipboard listener | Guard `clipboard.rs` | **Workspace `clipboard_history.rs`** | A supervisor thread and more mature password-manager exclusion. Guard's smaller file is a simpler starting point if the port gets heavy |
| Voice | KeepItLocal Memory `voice.rs` | **Workspace voice stack** | Grammar, safety gate and mic arbitration; Memory only has STT/TTS |
| Pre-AI redaction | ExcelAddIn `security.rs` (design) | **Workspace Disclosure Firewall** | A complete pipeline (it came from KeepItLocal Memory in the first place) |
| Memory/semantic index | KeepItLocal Memory | Unchanged: FTS5 first, `kip-memory-core` or Workspace's MiniLM later | |

## One command registry: settled

Build it **first**, in C#, before bangs, voice or the agent:
- use Workspace's declarative shape (id, phrases, **risk tier**, context, confirm, action);
- make it one static dictionary (AOT-friendly), with user-defined entries stored as `JsonNode`;
- have one list feed the field, the voice matcher, the agent's tools and the bench/MCP;
- use `voiceSafetyGate` as the model for read / act / always-asks.

**The lesson from Workspace:** its voice commands, bangs and tools are three
lists joined only in the UI. Don't repeat that.

## Browser extras Workspace unlocks

1. **Save page as PDF.** No port needed: WebView2's `PrintToPdfAsync` does it.
   **As Markdown:** Turndown (see [[Ideas/Agentic Browser#Page → Markdown: what the model reads]]).
2. **OCR a page image** (right-click an `<img>`). Windows' own OCR
   (`Windows.Media.Ocr`) comes first, because it's free and needs no Tesseract;
   Tesseract only if its quality falls short.
3. **Redact a screenshot** of a page before sharing (the design of `redact.rs`,
   ~176 lines, which really destroys pixels).
4. **Strip metadata** from downloaded DOCX/PDF before sharing (`doc_metadata.rs`).
5. **Compress or convert an image** on download or upload (encoders in a sidecar, never in the browser process).
6. **QR:** make one for the page or a selection, or decode one shown on a page.
7. **Other browsers' bookmarks and history in the field**, with the
   `browser_search.rs` copy-query-sweep pattern (nothing kept). Also decide how
   Search protects *its own* history from the same trick.
8. **Converters on a selection:** `hash <selection>`, `json2yaml <selection>`.

## Keep out of the browser

- Tantivy + OCR + semantic search in-process: tantivy, pdfium 7 MB, Tesseract
  ~30 MB + 23 MB tessdata, ONNX + model.
- Vosk loaded all the time. Copy Workspace's *policy* (opt-in download,
  released when idle), not the weight.
- Notes (`.ki`), SSH keys, the CSV toolkit, CamScanner, duplicate finder: no browsing use.
- The Svelte UI, the dead MFT walker, and `halcyon.rs`'s XOR "encryption".
- Assuming a Rust DLL is free: every export needs a written wrapper.

## Licensing

Search is **MIT**. Apache-2.0 and BSD code or binaries are fine *with
notices*; GPL and AGPL are not.

- **Workspace ships Apache-2.0/BSD binaries with no LICENSE/NOTICE files.**
  That means `libvosk.dll`, the Tesseract DLLs, `pdfium.dll` (a BSD-3 and
  Apache-2.0 mix, exact terms unverified) and `u2netp.onnx` (provenance
  unrecorded). That's a gap in Workspace itself, and it has to be fixed before
  either app (re)ships them.
- **Adopt Workspace's own precedents** (from `KeepItLocal.md`):
  - Zed's `fuzzy` crate was rejected for being GPL-3.0;
  - privacy.sexy's AGPL catalog was not copied, but re-authored clean-room;
  - FFmpeg is **never bundled**; shelling out to a user-supplied `ffmpeg.exe` avoids the LGPL obligations.
- **Provenance:** `core/resources.rs` was ported verbatim from
  **kil-privacy-suite**, and the firewall came from KeepItLocal Memory. Confirm
  ownership and licence for every sibling project.
- These checks were spot checks, not a full `cargo license` / npm audit.
  Audit anything we copy.

## Suggested order

1. **The command registry** (Workspace's shape, one list).
2. **C# ports:** converters (with the bugs fixed), frecency, DPAPI-at-rest, the clipboard listener.
3. **FishCatcher** (it closes the SmartScreen gap; see [[Ideas/Built-in Tools]]).
4. **Detector:** port or wrap `sensitive_scan`, used by the clipboard guard and pre-send redaction.
5. **Voice:** a Vosk sidecar with the safety gate.
6. **File search v1** (FTS5), then `text_extract`/`live_grep` DLLs.
7. **File manager**, read-only v1.
8. **The Disclosure Firewall port**, together with BYOK AI and the agent.
9. **Deferred:** the shared Rust engine (B), companion IPC (A), Svelte hosting (E).

## Open questions

- [ ] Does Workspace stay a separate app, with Search maybe offering "Open in Workspace" when it's installed, or should the two converge over time?
- [ ] The detector: a C# port (one codebase) or a Rust DLL (exact tuned behaviour)?
- [ ] OCR: Windows' built-in OCR first, or is Tesseract quality required?
- [ ] Encrypt clipboard images in Search (recommended), and fix the same gap in Workspace?
- [x] kil-privacy-suite is the user's own; it's now **KeepItLocal Redact**.
- [x] Workspace goes inside the browser: see [[Master Plan]].
- [ ] Add LICENSE/NOTICE files for Workspace's bundled binaries (a fix in Workspace itself).
