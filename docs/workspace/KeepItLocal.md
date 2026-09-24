# KeepItLocal — Source of Truth

> **Status:** living master document. Consolidated 2026-07-01 from ~18 scattered,
> partly-stale docs (Design, Backend, Frontend, architect, v1Goals,
> beat-the-free-audit, KeepItLocalTechicalFull, ScreenRecorderPlan, README, and
> several `docs/superpowers` specs). Where those docs disagreed with the code,
> **the code won**; where a claim couldn't be verified, it's flagged.
>
> **This file is authoritative.** `CLAUDE.md` (agent instructions) and the
> per-runtime `src-tauri/*/README.md` setup guides are kept separately; every
> other root markdown doc was folded into this and deleted.

---

## 0. What KeepItLocal is

A **local-first, privacy-first Windows productivity toolkit** — a tray-resident,
palette-driven suite of ~50 focused tools (search, clipboard, voice, docs,
images, dev, privacy, files, time) that all run **on the user's machine**.
Formerly "SwissKit"; renamed **KeepItLocal** (`com.keepitlocal.app`,
keepitlocal.app).

Built to run well on **ordinary consumer hardware (4–8 GB RAM, no GPU)**.
Windows 10/11 x64.

---

## 1. Pillars & non-negotiables

1. **Local-first** — all real work (indexing, extraction, OCR, crypto, voice,
   embeddings) happens in the Rust backend on-device.
2. **Privacy-first** — no telemetry, no analytics SDKs, no account wall.
   Sensitive data is **DPAPI-encrypted at rest** (Windows account-bound).
3. **Works fully offline** — no cloud dependency for any core feature.
4. **Resource-efficient** — designed for 4–8 GB / no-GPU machines; RAM-aware
   throttling; bounded reads; adaptive concurrency.
5. **Deterministic over ML** — prefer a correct algorithm to a model; **AI is
   optional, never required**.
6. **Beat the free version** — every tool must clearly beat its free competitor
   for the local workflow, or it doesn't ship.
7. **One job per surface** — sharp functional separation; trim overlapping
   features rather than stacking them.
8. **No admin / no UAC** — every feature works at user privilege (WH_KEYBOARD_LL
   hooks, Task Scheduler in user context, etc.).
9. **No in-app updater / no phone-home** — updates are a manual download.

### 1.1 Privacy and optional connections — RESOLVED (2026-08-03)

The product does not promise zero network calls. Its current, user-facing rule is:
**"Core work stays on your device. No telemetry or phone-home. Optional downloads
and configured services connect only when you start them."**

- This covers the optional Settings-owned model downloads and future user-configured
  services without weakening the local-first promise.
- A future online licence activation must remain explicit, occasional, and preserve
  offline operation with its grace window (§6). Licensing is not implemented today.
- About, README, onboarding, and Privacy Guide wording now follow this rule. Tool-
  specific claims that a local operation performs no network I/O remain valid.

### 1.2 Localization

- **English (en) only.** Georgian (ka) UI localization existed and was
  **deleted** (2026-07 decision — not needed; don't re-add).
- i18n plumbing via `svelte-i18n` (`src/lib/i18n/`) remains. Tool-level language
  support is separate and unchanged: OCR ships eng+kat+rus; Vosk supports en +
  ka models.

---

## 2. Architecture

### 2.1 Stack
- **Tauri 2** shell · **SvelteKit 5 (runes only)** frontend · **Rust** backend.
- SPA mode — `adapter-static` with `fallback: "index.html"` + `ssr=false` (root
  `+layout.ts`). **Nothing is prerendered** (no `prerender` export exists
  anywhere); every route resolves client-side off the single `index.html`
  fallback, which is why `build/` contains no per-route directories.
- Multi-window: every window runs the same bundle and branches on its label.
  Windows: `splashscreen`, `main` (900×700), `command` palette (900×560, created
  lazily on first show), `welcome`, `quicknotes`, overlays. Single-instance lock.

### 2.2 Frontend
- **Tool registry: `src/lib/appScreens.ts`** (`toolScreens`) is the authoritative
  source — id, title, category, component loader, icon, pack, keywords. Register
  a tool there, then add its Svelte file. `src/lib/tools.ts` re-exports it.
- **Runes only:** `$state`/`$derived`/`$effect`/`$props`/`onclick`. Never
  `export let`, `on:click`, `$:`, or `<slot>`.
- **Stores** (40+ in `src/lib/stores/`) hold cross-component state and **survive
  route navigation**, so background ops continue while the user switches tools.
  (Nav previously unmounted tools via `{#key}` — state was lifted into stores to
  fix that.)
- **UI primitives** (`src/lib/ui/`: `ToolPage`, `ToolToolbar`, `ToolPanel`,
  `ResultList`, `ResultRow`, `SideSheet`, Empty/Loading/Error states) + shared
  **components** (`src/lib/components/`: `PrivacyBlur`, `ConfirmDialog`, etc.).
- **Design tokens** in `src/styles.css` (CSS variables). Never inline hex.
  Multiple themes (dark default, light, dracula, nord, midnight, sepia,
  tokyo-night). Lucide (outline) icons. Inter + JetBrains Mono bundled locally.

### 2.3 Backend
- `src-tauri/src/lib.rs` — entry point, command registration (`invoke_handler!`),
  tray icon, global hotkeys, setup. `src-tauri/src/commands/` — one `.rs` per
  feature family (register in `commands/mod.rs`). `src-tauri/src/core/` — shared
  (dpapi, resources). ~280+ Tauri commands.
- **IPC:** `invoke()` camelCase args → Rust snake_case (`serde(rename_all =
  "camelCase")`); return `Result<T, String>` (Err → JS exception). Streaming via
  per-operation named events (`event-${opId}`); unsubscribe in `onDestroy`.
- **Concurrency:** any blocking I/O runs off the UI thread; extractor children
  run in **Windows Job Objects** with hard memory caps; Tantivy writer threads
  capped. **Three cancellation patterns:** walk-loop flag (static set),
  subprocess `.kill()` registry, file-sentinel polling.
- **Windows hygiene:** every subprocess uses `CREATE_NO_WINDOW` (0x0800_0000) to
  suppress console flashes. `windows` crate behind feature flags.

### 2.4 Data at rest (redb + DPAPI)
- Main store: `%APPDATA%\com.keepitlocal.app\keepitlocal.redb`, values
  **DPAPI-wrapped**. Version-byte check enables zero-downtime plaintext→ciphertext
  migration.
- **Encrypted:** main DB (settings, my-commands, etc.), clipboard history,
  snippets, time-tracker blobs, OCR cache, sensitive allowlist.
- **Plaintext by design (disclosed):** the Tantivy **content index**, **notes**
  (`.ki` files), and the semantic **vector cache** — all derived from / are the
  user's own content, protected by disk encryption (recommend BitLocker for
  sensitive corpora). Encrypt-at-rest for the remaining caches is a pending item.

### 2.5 Bundled native runtimes & cargo features
`src-tauri/Cargo.toml` gates heavy native deps behind features (default =
`vosk, ocr-leptess, screenrec, semantic`):

| Feature | Pulls | Bundled runtime |
|---|---|---|
| `vosk` | libvosk + cpal + webrtc-vad | `libvosk.dll` (offline speech) |
| `ocr-leptess` | leptess | in-process libtesseract (else spawns `tesseract.exe`); `tessdata-runtime/` (eng/kat/rus) |
| `screenrec` | cpal + wasapi + rubato + WGC/D3D11/MF windows features | user-installed **ffmpeg** (PATH or explicit location) |
| `semantic` | **fastembed 4.9.1 → ort (ONNX Runtime)** | `embedding-runtime/` (all-MiniLM-L6-v2 int8, Apache-2.0) + `onnxruntime.dll` |
| (always) | pdfium-render, lopdf, calamine, etc. | `pdfium.dll`, `qpdf.exe` |

Resources are shipped via `tauri.conf.json → bundle.resources`; the DLL search
dir is registered at startup.

### 2.6 Binding UI laws (transparent/overlay windows)
- **No DWM acrylic/mica** on transparent windows (leaks square dark wedges into
  rounded corners) — use `box-shadow` + `color-mix` glow instead.
- **No panel-level entrance animation** (panel mounts visible; a reveal from
  opacity 0 = flicker).
- **No `filter:blur` animation**; slides use `translate3d` only.
- **Selected/active pattern:** `panel-2` background + accent rounded-pill left
  strip (inset via `::before`) + accent icon + regular-weight text. NOT an
  accent-tinted background.
- **Never delete old code on big refactors** — keep a `.old.svelte` backup until
  the user explicitly says to remove it.

---

## 3. Feature catalog — what we have

Current, from `appScreens.ts`. Grouped by category / tool-pack. Core is locked;
other packs are optional/installable.

### Core (always present)
Home · About · **Command Palette** (search + clipboard + voice + launcher) ·
Notes (`.ki` Markdown+YAML) · Privacy Audit · Settings · Profiles · Tool Packs ·
My Commands · Documentation · Privacy Guide · File Search Index manager.

### Search pillar (the flagship) — see §3.1
File Search (filename) · Content search ("search inside files") · **Semantic
search** · Live-grep fallback.

### Utils
Hash Check (MD5/SHA-1/SHA-256/BLAKE3) · Encoders (Base64/Hex/URL/HTML/Binary/
ROT13) · QR Code (PNG/SVG) · Format Converter (JSON/YAML/TOML/XML) · Password
Generator (EFF diceware) · Color Picker · Calculator (Soulver-class) · Clipboard
History · Snippets (auto-expand) · Voice-to-Text.

### Development
Dev Toolkit (JWT · regex · SQL format/lint · diff · Markdown · generators · cron
· secret scanner) · SSH Key Manager (Ed25519/RSA/ECDSA) · Encrypt/Decrypt
(AES-256-GCM + Argon2id + keyfile, streaming).

### Privacy
File Shredder (quick/DOD-3/DOD-7) · Screenshot Redact (blur/black/pixelate/white) ·
Windows Hardening (reversible telemetry/ad tweaks, no admin).

### Document
Word Converter (**docx → Markdown/plain text** — *not* PDF↔Word; that was
removed) · CSV Toolkit (XLSX⇄CSV, merge/clean/split/→JSON) · Remove docx Password
(edit-restriction).

### Image
Image Studio (resize/crop/background removal/convert/compress) · OCR — Image to Text (Tesseract) ·
CamScanner (auto-deskew document scan) · Image → Base64 · Favicon Generator ·
Watermark.

### File
File Search · File Manager (dual-pane, robocopy backup) · Archive Utility
(ZIP/7Z/TAR) · Cleaner/Analyzer · Duplicate Finder (byte-identical + perceptual
dHash) · Bulk Rename · Folder Diff & 3-Way Merge.

### Automation (image-only today — see §5 B; built but hidden from the UI)
Automation Recipes (`resize/convert/compress/strip_metadata` chains over picked
images) + saved recipes + activity log/jobs. Fully implemented (incl. a
parallel job queue) but `HIDDEN_PACK_IDS` in `appScreens.ts` hides the pack from
Sidebar/Tool Packs — deferred to v2 (2026-05-26 decision); no UI path to enable
it today.

### Time & Focus
Time Tracker (foreground-window sampling, DPAPI-encrypted) · Focus Mode
(non-destructive app nudging) · Reminders (Task Scheduler; fire when app closed).

### Media
Screen Recorder — see §3.2 · Media Utility (extract audio and target-size video
compression, including a positive custom MB target).

### 3.1 Search stack (depth)
- **Filename index** — Tantivy, BM25, kept watcher-fresh; independent of content
  indexing.
- **Content index** ("search inside files") — Tantivy over extracted text (PDF
  via pdfium+lopdf, OOXML/ODF via zip+quick-xml, XLSX via calamine); **OCR-on-
  Index** fallback (Tesseract, BLAKE3-deduped `ocr_cache.redb`) for scanned PDFs
  & images; out-of-process extractor pool with RAM-aware child capping.
- **Semantic search** (shipped this session) — see §5 #2.
- **Live-grep** — ripgrep-as-library fallback for un-indexed folders.
- **Ranking:** single `rank::fuse` scorer — normalized BM25 + lexical-quality +
  frecency + recency + **semantic** signals, plus structural bonuses
  (entry-type-filter, extension-priority). Lexical stays dominant.

### 3.2 Screen Recorder (shipped v1)
WGC + D3D11 capture (not gdigrab); Windows H.264 through `h264_mf`. ffmpeg is
**user-installed, never bundled — LOCKED 2026-07-16**: detect it on PATH or let
the user choose `ffmpeg.exe` once, then persist that local selection in the
encrypted store. The recorder preflights `h264_mf` and the setup card explains
missing or incompatible installs. Rationale is
*we distribute nothing* → zero licensing obligations, zero paperwork, smaller
installer. Note the reasoning, so this doesn't get re-litigated: shelling out to
a separate `ffmpeg.exe` is **not linking**, so bundling would never have forced
open-sourcing our code — it would only have added LGPL obligations for the
binary we shipped (license text + source offer). The real trap is that most
prebuilt Windows ffmpeg binaries (gyan.dev, BtbN) are **GPL builds** (x264/x265
included); an LGPL-only build is one you must configure and maintain yourself.
Distributing nothing sidesteps all of it. `tauri.conf.json` has no FFmpeg
resource entry. Region/full-screen; WASAPI
system-audio loopback + mic via `amix`; single master clock for A/V sync; clean
stop (EOF, never kill ffmpeg); fragmented MP4 for partial-file recovery; privacy
redaction (hide selected windows via live per-frame rect tracking + black-box
compositing, plus user-drawn static black boxes; `SetWindowDisplayAffinity`
hides the recorder's own UI chrome from capture, not other apps' windows);
pause/resume; GIF export. *Deferred:* single-window capture, webcam PiP,
multi-monitor, cursor overlay, panic hotkey, recordings library.

---

## 4. Current status

- **Shipped & verified:** all pillars; the tool catalog above; snippet
  auto-expand (WH_KEYBOARD_LL); screen recorder v1; OCR-on-Index; **semantic
  search** (code-complete; not yet packaged for install — see §5 #2); RAM-aware
  indexing; encrypt-at-rest for most redb stores.
- **Command Palette V2 is the one current palette:** V1 was deleted 2026-07-18,
  and both the global-hotkey popup and embedded overlay use the canonical
  `/command` route. Parity was verified before deletion — **37/37 `invoke`
  commands, 5/5 listeners, 9/9 `emitTo`, and a byte-identical `onKeydown`** —
  so nothing functional was lost. V2 adds:
  scrollbar/token polish, crimson accent (dark/midnight), fullscreen preview
  expand, match highlighting, no-preview icon fallback, large-PDF render guard,
  clipboard icon pre-resolution. Deliberate V2 trims (not regressions): rows are
  single-line (no path/size subtitle — that detail lives in the preview) and the
  footer shows 2 hints instead of 9.
- **Pending:**
  - **Licensing** (§6) — not yet implemented; app runs with no license check.
  - Encrypt-at-rest for remaining plaintext-by-design stores (decision, not bug).
  - `tessdata` on-demand language packs.
- **Known open items (from the 2026-06-04 beat-the-free audit):** Image Studio
  ravif/mozjpeg wiring — verified shipped (both wired into `save_image()`).
  Watermark Georgian glyphs — verified still broken (font list has no Georgian
  coverage; lowercase/Cyrillic already fixed). Secret Scanner folder scan —
  verified absent (textarea-paste-only; no folder/repo scan exists).
  PDF-compress lossless default — still unconfirmed; re-verify.

---

## 5. Roadmap — what we're going to add

> **Live status lives in [`roadmap.md`](roadmap.md)** (created 2026-07-23) — that file
> is the scannable `[DONE]`/`[In Progress]`/`[Not Started]` tracker. This section keeps
> the *rationale* for each item; when they disagree on status, `roadmap.md` is fresher.

### v1 finishing slate (LOCKED 2026-07-06, in execution order)

**v1 = harden and polish what exists.** New tools/categories go to v2.

1. ~~**Command Palette V2 — finish + UX/UI polish**~~ **DONE.** V2 polished and
   parity-verified, made the popup default 2026-07-16, and **V1 deleted
   2026-07-18** — both the popup and the embedded overlay now render the one
   palette from the canonical `/command` route (§4, §8). Fully closed.
2. **Do-anything bar (Lean)** — **DONE.** Target-first file and clipboard actions
   now hand selected items to the right local tool (see #3A below).
3. **Image Studio upgrade** — **DONE.** Single-image crop and local background
   removal ship with bundled Fast U2NETP; Full U²-Net is an optional Settings download.
4. **Notes made ideal** — **DONE.** In-app Notes and Quick Notes include
   quick-note minimize/maximize, folders, trash, rich blocks, attachments,
   split view, local revisions, graph view, and export.
5. **About privacy-claim reword** (§1.1) — **DONE.** **AI in Notes** remains v2
   (see #4 below).
6. ~~**Browser bookmarks + history search**~~ **DONE (2026-07-18).** Chrome/Edge/
   Brave/Vivaldi + Firefox local SQLite (copies DB **+ `-wal`** to temp, mandatory
   orphan-snapshot sweep); zero network; the only "connector" allowed in v1. Surfaced
   in the palette via the Web chip. `src-tauri/src/commands/browser_search.rs`
7. **Screen Recorder FFmpeg migration** — user-installed only (§3.2, complete).
8. **Media utility (v1 slice of the v2 Video Toolkit) — ADDED 2026-07-21, complete.**
   Two ops only: **extract audio (mp4→mp3)** and **compress**. Rides the recorder's
   shared FFmpeg resolver, so this is not an application-bundled binary,
   and now lives in the Media category alongside Screen Recorder. **Must land AFTER #7**
   so there is one user-installed resolver + one
   graceful-degradation path. The rest of the Video Toolkit (format-convert mov→mp4,
   trim, →GIF) stays v2. ⚠ **Effort is NOT symmetric:** mp4→mp3 is ~a day (one command);
   compress is the real work — see §8 entry "v1 media utility". **Compress mode DECIDED
   (2026-07-21): single-pass computed-bitrate for v1** (target-size presets or a
   positive custom MB target, ~90% margin, one encode pass); **two-pass ABR is
   written down as the v2 accuracy upgrade.**
9. **Archive Utility** — **DONE.** Create, inspect, and extract ZIP, 7Z,
   TAR.ZST, TAR.GZ, and TAR.XZ locally; ZIP and 7Z can use a password. Creation
   uses a killable worker so Cancel stops promptly and does not promote a partial archive.
10. **Licensing** — deliberately **very last**, after everything is finished.

**Explicitly pushed to v2:** automation engine + triggers, A/V transcription
index, cloud connectors (Drive/email/Slack/GitHub/…), ask-your-files RAG,
selection/clipboard AI, send-to-device (LAN), PKM/tasks, screen-region OCR,
the full video/audio toolkit beyond the shipped Media Utility (user-installed-ffmpeg-based), E2E sync,
**own subsequence fuzzy matcher** (below). **Skipped:** CLIP
image-content search (OCR-in-images deemed sufficient), Georgian UI localization
(deleted).

**v2 — own subsequence fuzzy matcher (deferred 2026-07-20, NOT a v1 item).**
Search today ranks well and is *not* a pain point; this is an additive capability,
not a fix. Today's `fuzzy_keyword_hits` is **edit-distance-1 typo tolerance**
(`rank.rs:117`), so `notpad`→`notepad` works but `ntpd`→`notepad` does not —
subsequence matching is the gap. Build our own: 64-bit char-bitmask prefilter,
then subsequence scoring with consecutive-run / word-boundary / camelCase / first-char
bonuses, normalized to 0..1 so it drops into the existing bounded-component contract.
Only three consumers: `rank.rs`, `browser_search.rs:422`, `search.rs:10386`.
**The work is recalibration, not integration** — lexical carries the heaviest weight
(3.0) and those weights were tuned 2026-05-17, so changing the fuzzy component
reorders every result. Ship behind a cargo feature, default-off, A/B, then flip
(§2.6 / VAD law). Frontend `commandRegistry.ts` matching is **out of scope** — it is
TS, and reaching Rust would mean an IPC round-trip per keystroke.
Source it from **fzf (MIT)** and Forrest Smith's public Sublime-fuzzy write-up (MIT
sample), or Smith–Waterman directly — see §8 for what must NOT be used.

Agreed build sequence for the "real workspace" push:

### #1 — OCR → search index ✅ (already built; skipped)

### #2 — Semantic file search ✅ SHIPPED (2026-07-01, this session)
Embedding-based retrieval that finds files by **meaning**, layered on the content
index. Off by default (`Semantic search (beta)` toggle; needs content indexing).
- **Model/runtime:** fastembed 4.9.1 + **all-MiniLM-L6-v2 int8 (Apache-2.0)**,
  bundled offline. **No local LLM bundling.** (bge-small was rejected — CC-BY-NC.)
- **Multi-chunk:** each doc is split into ~180-word overlapping passages (≤16/doc,
  evenly sampled across long files), embedded as a batch; a doc is scored by its
  **best-matching passage** (fixes long-doc dilution).
- **int8 quantization:** vectors stored as `i8[384]` in `vector_cache.redb` — **4×
  less RAM** (the deliberate scale lever; cosine is scale-invariant). **ANN/HNSW
  deferred** (approximate + more RAM — wrong for the hardware target; revisit only
  past ~500k chunks).
- **Query:** query embedded → brute-force cosine → merged with BM25 into one
  ranked pool; **deep pagination** (`semantic_top_k` scales with page depth).
  `rank::WEIGHTS.semantic = 0.8` (below `lexical 3.0`) so exact hits always win.
- **UI:** `✨ Semantic on` header pill (pass ran) · `✨ Meaning match` (pure-
  semantic hit) · `✨ NN%` (keyword hit that also matches by meaning).
- **⚠ Migration:** store table is `vector_cache_v2` — **rebuild the content index
  once** to repopulate after the single-vector build.
- **⚠ Not yet packaged:** `tauri.conf.json → bundle.resources` has no entry for
  `embedding-runtime/` or `onnxruntime.dll` (unlike vosk/pdfium/tessdata) — a
  packaged installer ships without the model today, so semantic search is
  code-complete but inert for real users until this is wired in.
- Files: `commands/embedding.rs`, `commands/vector_cache.rs`, `rank.rs`
  (semantic signal), `search.rs` (index + query wiring), toggle in
  `FileSearchIndex.svelte`/`fileSearch.ts`. Activation guide:
  `src-tauri/embedding-runtime/README.md`.

### #3 — The glue (two independent pieces)

**A. Do-anything bar** *(DONE; small, builds on what exists)*
Extend the V2 palette's existing **Ctrl+Space actions menu** (`actionsForSelected`
in `PaletteV2.svelte`) so a selected **file or clipboard item** gets **tool
verbs**, not just nav/copy. Interaction is **target-first** (already how V2
works). **Execution is hybrid:**
- *Instant set (headless, result in the bar):* `Hash → copy` (hash-check) ·
  `Text → QR` (qr-code) · `Copy as Base64` (encoders).
- *Open-with-target (Lean tier, type-gated):* Encrypt (`encrypt-decrypt`) ·
  Compress/Resize image (`image-studio`) · OCR → text (`ocr-image-to-text`) ·
  Redact image (`screenshot-redact`) · docx → Markdown (`word-converter`).
- *Clipboard verbs:* copied image → Redact/Compress · copied text → Save as Note
  (`notes`).
- **Only real new plumbing:** `openMainAtTool(toolId, { targetFile })` stages a
  one-shot target that the tool consumes on mount. Everything else is data in
  `actionsForSelected` or the existing clipboard transform menu.
- **Scope decisions locked:** Lean verb set; targets = files + clipboard (no
  verb-first NL typing in v1); built into V2.
- Broad-tier verbs (Shred, doc-password, CamScanner, Watermark, text→Snippet)
  remain deferred.

**B. Automation engine** *(DEFERRED TO V2 — decision 2026-07-06)*
Today's "Automation" is **image-only recipes** (`automation.rs` +
`AutomationRecipes` — resize/convert/compress/strip-metadata, manual run, saved
recipes + activity log). v2 generalizes it into **cross-tool chains + triggers**
(manual/hotkey, schedule via the existing cron/reminders, folder-watch), reusing
A's tool verbs as building blocks. Keep it **local + tight** (not a Zapier clone).

### #4 — AI (scoped down; provider design locked 2026-07-06)
- **BYOK multi-provider router, no bundling.** Supported providers (all
  OpenAI-compatible, so ONE client + per-provider profiles): **Mistral · Groq ·
  Cerebras · OpenRouter · Ollama local · Ollama Cloud** — leaning on their free
  tiers; the user pastes whichever keys they own. We build **our own router**
  that rotates between the configured providers **round-robin with
  rate-limit-aware failover** (a 429/erroring provider is skipped until healthy —
  what juggling free tiers actually requires). Privacy-max users configure only
  local Ollama — nothing leaves the machine.
- **v1 = AI for Notes ONLY** (summarize / rewrite / continue / title / tag) —
  and Notes itself must be ideal first (v1 slate item 4). **Chat panel dropped**
  (least differentiated).
- **Flagship direction (post-v1):** **"Ask your files"** — private RAG over the
  semantic vector store built in #2 (answer from *your* files, with citations;
  fully private on local Ollama). This is why AI belongs here at all.
- **Requires the §1.1 claim reword.** AI is off by default and hard-gated.

---

## 6. Licensing & pricing

- **⚠ DIRECTION CHANGED 2026-07-20 — online check now allowed.** The owner reversed
  the offline-only stance: *"lets add a normal licence, we will check it online — we
  are not stealing user data and selling it."* An online activation/validation path
  is now in scope. **Still deliberately LAST (v1 slate #8) and NOT yet designed** —
  recorded here so no future session re-proposes offline-only as settled.
  - ~~**100% offline, file-based** — Ed25519-signed license (pasteable base64url code
    **or** dropped `license.json`), embedded public key, async signature check.
    **Zero network calls for licensing, ever.**~~ **SUPERSEDED.** Keep the Ed25519
    signed-file mechanism as the **offline fallback** (fail-soft still requires the
    app to work with no network); the online check is layered on top, not a replacement.
  - **Two integration constraints when it is designed** (both real, neither blocking now):
    1. **Current wording is resolved.** About explains that core work is local and
       optional downloads or configured services connect only when the user starts them.
       Licence activation needs an equally explicit explanation before it contacts our
       server.
    2. **Offline operation remains mandatory.** Online activation must keep the
       signed-file fallback and grace window; it cannot turn routine use into a
       per-launch network check.
- **⚠ MODEL RE-OPENED 2026-07-23 — current owner leaning (NOT finalized):**
  **free tier + one-time payment for personal + annual per-seat for business.**
  Owner's words: *"payment model is not defined yet… free version and 1 time payment,
  and for businesses etc yearly payment per seat."* Two shifts from the line below:
  1. **A free tier is now first-class**, not just a fail-soft degrade target. That
     changes the architecture: licensing needs real **feature-gating** (what's free
     vs paid), not a binary licensed/unlicensed check.
  2. **Business flips one-time → annual/subscription (per seat).** Personal stays
     one-time/perpetual. So it's a **hybrid**: perpetual personal + recurring business.
  Also: owner now considers a **pure offline file-license unsatisfactory** ("offline
  licence is shit") — consistent with the 2026-07-20 online-check reversal above.
- **✅ LICENSING PATTERN — DECIDED & RATIFIED 2026-07-23 (owner confirmed):**
  **online *activation* (once, or occasional) + offline operation with a grace
  window. NOT online-only, NOT online-check-every-launch.** Rationale: pillar 3
  ("works fully offline") is the whole brand; an app that bricks on a plane or when
  our licence server has a bad day betrays it. This is the resolution of the
  "offline licence is shit" reaction — it kills the pure-file license's pain WITHOUT
  swinging to online-only. **GUARDRAIL (owner's explicit request — he runs 6 projects
  + a full-time job and may forget or contradict this): if a future session or the
  owner himself proposes online-only / per-launch checks / anything that stops the
  app working offline, STOP and cite this decision + pillar 3 before proceeding.**
- ~~**SKUs:** Personal (named + household) · Business (per-seat, one-time).
  Perpetual + **12-month update window** (v2 = paid upgrade, no subscription).~~
  *(superseded by the leaning above — the 12-month-window / v2-paid-upgrade idea
  still applies to the PERSONAL perpetual tier; business becomes annual.)*
- **14-day trial**, no card; **fail-soft** — trial/expiry **degrades to a
  free-forever core, never bricks** (clock-rollback handled). Company-fleet misuse
  = settlement posture; friends/family sharing blessed. Device-cap deferred
  (schema slot only). Never a kill-switch. Updates = manual download.
- **⚠ PRICING UNRESOLVED — needs a decision.** Docs disagree: Sales.md `$20–30`;
  session memory `Personal $29 / Business $39-seat`; architect.md (2026-06-04, most
  recent) `Personal $39 / Business $59/seat`. **Pick final numbers.**

---

## 7. Engineering conventions & verification gates

- **Frontend:** `npm run check` (svelte-check; target 0 errors). Runes-only;
  tokens-only styling; per-surface checklist (focus-visible ring, reduced-motion,
  empty/loading/error states).
- **Pure-logic modules** (calcEngine, cronEngine, regexExplain, voice
  commandRegistry/safetyGate, appScreens, notes/preview): Vitest.
- **Rust:** `cargo check --manifest-path src-tauri/Cargo.toml
  --no-default-features --features screenrec` for a fast offline type-check;
  add `semantic` to verify the embedding path (needs the ONNX runtime cached);
  `cargo test --lib` for unit tests. Full app build needs the native runtime env
  (libvosk/tesseract) set up.
- **Backend patterns:** `spawn_blocking` for I/O; `Result<T,String>`; versioned
  `local_db` keys (`feature_v1`) for safe migration; BLAKE3+salt before using any
  secret as a DB key; never trust frontend validation (the Tauri boundary is the
  trust boundary).
- **Commit workflow:** never auto-commit — implement, verify, report; the user
  commits. Never expose/rename the obfuscated `halcyon` easter egg.

**Key ADRs (from architect.md, still valid):** one app-wide AES-256-GCM key
wrapped by DPAPI; Ed25519 for licensing; qpdf over pure-Rust PDF ops;
WH_KEYBOARD_LL for system-wide hooks; shared `kil-core` crate family for
multi-product reuse.

---

## 8. Decision registry — do NOT re-propose these

> **What this is.** Every deliberate *not-doing* in the product: deferred, rejected,
> removed, or tried-and-abandoned — with the real reason and where it is recorded.
> Compiled 2026-07-16 by a 10-agent survey that read the source directly (206 features
> mapped; 355 decisions, 350 of them stated explicitly in code/docs).
>
> **Why it exists.** Proposals kept getting made for things already decided against —
> whisper.cpp, the MFT fast-path, un-hiding Automation, wikilink rename-safety. Each
> cost real time to re-litigate. Worse, a *wrong* line in the old `Greatness.md`
> competitive scoreboard ("mft_walker.rs … proven for Duplicate Finder") was itself
> generating the bad MFT proposal — which is exactly why that file was **deleted
> 2026-07-23** (a drifting companion doc is a liability; recoverable from git if ever
> needed). A decision that only lives in someone's memory gets re-proposed the moment
> that memory is gone.
>
> **How to use it.** Grep here BEFORE proposing work. An entry is not a permanent veto —
> it is the price of admission: if you want to reopen one, address the stated reason.
> "It would be nice if" does not clear that bar. Entries marked *inferred* were concluded
> by a reader, not stated by the source — treat those as weaker.
>
> ### ⛔ READ THIS BEFORE ARGUING "THAT BREAKS LOCAL-FIRST"
>
> **"KeepItLocal" does NOT mean the app may never touch the network.** It means we do
> not take the user's data off their machine. Owner, verbatim and repeatedly:
> *"keepitlocal means you keep your files local, it does not mean you cannot access web
> from your device, it means i am not stealing data and selling it."*
>
> The "zero network calls" absolute was **RETIRED** on 2026-07-01 (§1.1). The posture is
> **"won't unless you say so"**, not "cannot". A feature that reaches the network at the
> user's explicit request — a browser extension over native messaging, an online licence
> activation, a user-configured AI endpoint, a cloud connector to the user's OWN account —
> **does not violate anything.** Telemetry, phone-home, and silently shipping user data
> anywhere are what violate it.
>
> **This objection has now been wrongly raised three separate times** (cloud connectors,
> licensing, browser tab switcher) and cost real time each round. If you are about to
> reject a feature because it "conflicts with local-first / zero-network", you are almost
> certainly making this mistake — **stop and re-read this paragraph.** Judge the feature on
> whether it moves user data off-device without consent. Nothing else.
>
> Related: a browser **extension** talking over native messaging is local IPC, not network
> — and KeepItLocal **Guard already ships exactly that architecture**, so it is proven
> in-family, not a new compromise.
>
> The code is the authority. Where this registry and the code disagree, the code wins —
> and fix the entry.

### Headline removals & deferrals

*(Carried over from the pre-2026-07-16 §8. Note what this list lacked: it said
whisper.cpp was removed but never said WHY — so it was re-proposed anyway. The
rationale is the load-bearing half of a decision; a bare "we removed X" does not
survive contact with someone who has a good reason to add X.)*

- **Removed tools:** PDF pack (merge/split/extract/redact — PDF unlock moved to a
  Privacy product); **PDF↔Word conversion** (both directions, deleted); Archive
  category; whisper.cpp & PaddleOCR (Vosk/Tesseract kept — see the Voice section
  below for the actual whisper reasons); some Word tools.
- **Deferred tech:** ANN/HNSW index (int8 quantization chosen instead); verb-first
  NL typing in the palette (target-first only for v1); generic AI chat panel;
  multi-monitor/webcam-PiP screen recording; encrypt-at-rest for the remaining
  plaintext-by-design stores.
- **Palette V1 was DELETED 2026-07-18 — there is now exactly one palette.**
  `PaletteV2.svelte` is it, mounted by the canonical `/command` route (a thin
  wrapper) for the global-hotkey popup, and directly as the embedded overlay in
  the main window. Removed with it: the temporary `/command-v2` route, the
  **Ctrl+Shift+D** swap hook, and the `keepitlocal.palette.showV2` localStorage
  flag — the swap hook's own comment always said it would go "once a design is
  chosen". Both surfaces now render the same component, so they can no longer
  disagree. V1 stays recoverable from git at **`f521f72`** if ever needed; it
  was kept as the rollback path from 2026-07-16 until V2 had real mileage.
  Parity had been verified before the swap: 37/37 `invoke` commands, 5/5 event
  listeners, 9/9 `emitTo` sites, byte-identical `onKeydown`.

### Core shell — windows, hotkeys, tray, registry, packs, Settings

- **The three standalone overlay windows (search, clipboard, voice) are RETIRED. The command palette is the single surface. Do not re-propose 'bring back the search overlay' or 'add a dedicated clipboard overlay'.** — Cleanup Waves 1.0–1.3 deleted the three legacy overlay routes (~5622 lines) after closing the palette's functionality gaps: voice gained a 'dictate' sub-mode running the full dictation pipeline; clipboard gained snippet /trigger expansion, Alt+P/D/L per-row shortcuts, and inline action buttons. Every old summon path… `git commit 87306ef (2026-05-28) 'Cleanup Waves 1.0–1.3: retire legacy overlays + palette parity polish'…`
- **Ctrl+Alt+S (search overlay hotkey) is never registered — but its entire scaffolding is KEPT on purpose. Do not 'clean up the dead overlay hotkey code'.** — The palette (Ctrl+Alt+K) is the single search surface, so the separate chord was dropped. The code was explicitly kept 'so the startup call site stays unchanged and the overlay code remains available if we ever re-enable it' / 'remains in the backend (unused) in case the overlay is ever re-enabled'. `src-tauri/src/lib.rs:2574-2580 (no-op with that exact comment); src/lib/stores/settings.ts:497-501…`
- **The 'doubleSpace' overlay trigger mode was REMOVED and is force-coerced away on load. Do not re-propose double-tap-to-summon.** — It needs a low-level keyboard hook (WH_KEYBOARD_LL) to detect double-tap without globally intercepting every spacebar press — 'non-trivial complexity for marginal benefit'. Legacy installs with mode:'doubleSpace' are silently coerced to 'shortcut' so the daemon doesn't hang trying to register the bare Space key. `src-tauri/src/lib.rs:1015-1031 (normalize_overlay_hotkey_config), :1033-1038 (resolve_hotkey_shortcut)`
- **The clipboard and voice hotkeys deliberately have NO double-tap mode, unlike the search overlay's original design.** — Clipboard: 'too destructive a thing to invoke on accidental double-spaces in prose'. Voice: 'triggering it accidentally would steal the user's mic and feels worse than missing a press'. `src-tauri/src/lib.rs:416-419; src-tauri/src/lib.rs:539-542`
- **Overlay windows are created LAZILY, NOT pre-created at startup — with the command palette as the single deliberate exception. Do not propose 'pre-warm the overlays for speed'; it was tried and reverted.** — Pre-create was briefly done to mask a cold-init deadlock, but costs ~100 MB resident RAM for a renderer most users don't need at boot. The lighter fix (defer non-critical prefetches past mount) keeps idle RAM at ~134 MB. The palette is the accepted exception: the user explicitly wants 1-press summon, so ~80-100 MB for… `src-tauri/src/lib.rs:3122-3153 (full history + the EXCEPTION paragraph), :353-358`
- **The Automation pack is hidden because Automation is moved to v2. The pack definition, the tool screen, the Rust commands, and the whole tools/Automation/* directory all STAY. Do not delete them; do not un-hide them.** — Per the 2026-05-26 Finalizing.md audit (Phase 11), Automation is moved to v2. Hiding (not deleting) keeps re-enabling to a one-line change. WelcomeSetup adds a second reason: 'listing it as if shipped is misleading'. `src/lib/appScreens.ts:162-173 (HIDDEN_PACK_IDS + rationale); src/lib/Sidebar.svelte:76-80 (commented-out nav…`
- **The nine light Development tools were folded into one 'Developer Tools' hub (dev-toolkit). Do not re-propose splitting them back out into separate sidebar entries.** — Dated 2026-06-05. The Development pack now holds only dev-toolkit + the two heavy managers (SSH Key Manager, Encrypt/Decrypt) — deliberately kept separate because they manage on-disk key material and files — so the pack renders as one ungrouped pill row. categoryToolGroups is consequently an empty object. `src/lib/appScreens.ts:215-231 (rationale + `categoryToolGroups = {}`), :756-760 (dev-toolkit docs.tip states…`
- **The three separate Word→PDF / Word→Markdown / Word→Text tools were merged into one 'Word Converter'. Word→PDF is gone entirely.** — The three entries 'cluttered the sidebar'. For PDF, users are pointed at Word's own built-in 'Save as PDF'. `src/lib/appScreens.ts:698-716`
- **The Search Guide page was RETIRED — its content moved into the search overlay as an inline `?` cheatsheet button.** — So users get the help without leaving the overlay; the Documentation page covers task-oriented examples for everything else. `src/lib/appScreens.ts:412-415`
- **Quick-note windows are DESTROYED on close, deliberately excluded from the close-to-hide and hide-on-blur handlers that every other window uses.** — RAM-conscious by design — WebView2 memory is reclaimed the moment the note is closed. The route is a bare <textarea> (no editor framework) for the same reason. `src-tauri/src/lib.rs:368-372, :1390-1394`
- **welcome_finished uses destroy() not close(). Do not 'clean this up' to close().** — Wave 7.9 used close(), which fires CloseRequested → the user-close handler → app.exit(0) → main vanished right after becoming visible (a user-reported bug). Wave 7.9.1's AtomicBool flag fix FAILED because close() queues CloseRequested for the next event-loop tick while welcome_finished returns immediately, so Drop… `src-tauri/src/lib.rs:705-726 (full three-wave history)`
- **The welcome flow lives in its OWN maximized Tauri window, not in the main window. Do not move it back into main.** — The old in-main approach showed a brief 900×700 dark frame because Windows runs a visible maximize animation when transitioning a hidden non-maximized window to maximized at show time — the JS-side maximize-before-set_ready fix in Wave 7.8.4 did not help. Born maximized+hidden solves it. Also conditionally created, so… `src-tauri/src/lib.rs:305-313, :3171-3190; src/routes/+page.svelte:13-18`
- **Window-show Tauri commands are `#[tauri::command(async)]`. Do not make them sync.** — WebviewWindowBuilder::build() needs the main event loop to create the OS window. If the show command is sync, the Tauri main loop is busy dispatching the IPC reply for that very command while the body waits on the main loop → deadlock, app freezes. The global-shortcut path is unaffected (own thread), which is why… `src-tauri/src/lib.rs:766-785, :800-826`
- **The command palette window does NOT reset size/center on every show.** — Palette Appearance Wave I (2026-05-27). Forcing geometry on show overrode the user's saved Width/Position from commandAppearance.ts — the frontend $effect applied the user's value, then the next show reset it to 640×560 centered. Accepted edge case: a user-dragged window stays put until they pick a Position (the… `src-tauri/src/lib.rs:1343-1352`
- **When desktop blur is ON, the palette forces SQUARE OS corners (DWMWCP_DONOTROUND). The earlier round-both approach was tried and REVERSED.** — Windows acrylic on a rounded window leaks dark square wedges into the rounded corners on most builds, unfixable from CSS. Policy reversed: no rounded corners = no wedges to leak into. The CSS side drops its border-radius to match; blur OFF restores the OS default and the full 16px radius. The blur toggle is hidden on… `src-tauri/src/lib.rs:828-896 (esp. :861-871), :898-921`
- **Reminder scheduling uses a Rust 15s heartbeat thread, not a JS setInterval.** — Chromium throttles JS setInterval to ~once a minute while the main window is hidden in the tray, which made due reminders fire late. `src-tauri/src/lib.rs:1203-1213`
- **`--reminder` relaunches never show splash or main, and a duplicate reminder instance exits SILENTLY (no 'already running' dialog).** — The already-running instance fires the reminder on its own heartbeat, so a MessageBox at every reminder time would be intolerable. The relaunch stays in the tray and only fires the toast. `src-tauri/src/lib.rs:629-634, :2611-2617, :3043-3051, :3241-3244`
- **UI ships English-only, but the `Locale` type deliberately keeps 'ka'. Do not delete the 'ka' locale id.** — The voice/dictation engine is locale-aware (Vosk Georgian model → voiceModelLocale() → Georgian command grammar) and Georgian OCR is still supported. Only the UI translation layer + language picker were removed; LOCALE_OPTIONS therefore lists English alone because it drives the UI, not voice. `src/lib/stores/settings.ts:26-37`
- **Global hotkey registration is owned by the MAIN window only.** — +layout.svelte boots the settings store in EVERY window (main, palette, each quick-note). Without the gate, every window re-registers the same chords and the 2nd+ registration collides ('HotKey already registered'), getting worse the more sticky notes are open. Non-Tauri dev (no window label) defaults to true so local… `src/lib/stores/settings.ts:475-488 (ownsGlobalHotkeys), :492`
- **Tool pack changes require an explicit save + app restart; they are not applied live.** — Staged in pendingEnabledPackIds; savePendingToolPacks sets restartRequired and toasts 'Restart KeepItLocal to apply changes.' Settings' own copy says 'optional packs apply after restarting KeepItLocal.' `src/lib/stores/toolPacks.ts:104-138; src/lib/tools/TopBar/Settings.svelte:1255-1258`
- **clipboardImagesEnabled default was FLIPPED false→true on 2026-05-26 with a one-time migration sentinel. Do not flip it back to opt-in.** — Per user verdict: 'a clipboard history that silently drops half the things you copy is the surprising behavior, not the cautious one'. Bounded by a short image retention default (2 days). `_clipboardImagesDefaultV2Applied` makes the migration run exactly once, so users who turn it off AFTER the migration keep that… `src/lib/stores/settings.ts:222-237, :336-343, :399-405`
- **webSearchEnabled defaults OFF; privacyAuditSchedule defaults 'off'; snippetAutoExpandEnabled defaults OFF; pushToTalkEnabled defaults OFF; voiceVadEnabled defaults OFF.** — Each is an explicit opt-in with a stated reason: web search because 'KeepItLocal is local-first'; the audit because it's normally user-initiated and this is 'the single owner-approved relaxation, gated behind explicit opt-in'; auto-expand because 'it installs a keystroke watcher'; PTT because 'it registers a global… `src/lib/stores/settings.ts:192-197, :205-207, :259-263, :285-299, :328-353`
- **~~Two command palettes coexist intentionally~~ SUPERSEDED 2026-07-18: V1 was DELETED; there is exactly one palette.** — V1 was kept as the rollback path only until V2 had real mileage. It is now gone, along with the /command-v2 route, the Ctrl+Shift+D swap hook and the showV2 localStorage flag. PaletteV2.svelte is mounted by the canonical /command route AND as the embedded overlay, so the two surfaces can no longer disagree. Recoverable from git at f521f72. `src/routes/command/+page.svelte (thin wrapper); src/routes/+page.svelte (embedded overlay); lib.rs get_or_create_command_window`
- **The palette's V1/V2 toggle is persisted in localStorage rather than being ephemeral state.** — 'V2 is being finished/tested (it kept resetting to V1 on every restart).' `src/routes/+page.svelte:320-327`
- **Continuous always-on voice command mode was DROPPED for v1.** — The per-utterance command sub-mode covers the use case. `git commit 87306ef body: 'Continuous always-on command mode dropped for v1 (per-utterance command sub-mode…`
- **Home/About split: Home is the landing screen, About is a secondary detail page. About was deliberately demoted from the landing role.** — 'Home is what users see when KeepItLocal opens; About is the "what is this product" detail page.' `src/lib/appScreens.ts:234-246, :247-254; src/routes/+page.svelte:55-57`
- **Clipboard History, Snippets, Voice to Text, My Commands, Time Tracker, and Privacy Audit are page screens (always installed, no pack toggle), NOT pack tools.** Screen Recorder and Media Utility are default-enabled Media-pack tools, reached through the Media category workspace rather than a permanent sidebar page. Users can turn the Media pack off in Tool Packs. `src/lib/appScreens.ts`
- **The nav-state-loss fix lifted tool state into stores; the {#key}-based unmount is gone. Do not reintroduce per-nav unmounting.** — Nav previously unmounted tools via {#key}; state was lifted into stores so background ops continue while the user switches tools. The router now caches component modules (MAX_CACHED_SCREENS bumped 10→100) and prefetches the entire installed-screen set on idle so navigation is always a cache hit. `KeepItLocal.md §2.2; src/routes/+page.svelte:63-71`
- **WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS disables background networking and component update; --enable-low-end-device-mode was tried and left commented out.** — Consistent with the zero-network / no-phone-home law. The low-end-device-mode flag is present but disabled — no rationale recorded for why it was backed out. *(inferred — not stated in the source; verify before relying on it)* `src-tauri/src/lib.rs:2666-2670`

### Command palettes (V1 / V2 / CommandWorkspace)

- **~~Keep V1 intact as the rollback path — do NOT delete it~~ SUPERSEDED 2026-07-18: the founder explicitly authorised deletion and V1 is deleted.** — The instruction held while V2 was unproven; V2 has since been the sole default in real daily use. Git (f521f72) is the rollback path now, which is exactly the condition the never-delete rule requires. `deleted: src/routes/command/+page.svelte (V1, 13949 lines) and src/routes/command-v2/` 
- **Two palettes coexist intentionally; the main-window embedded overlay is knowingly left on V1 while the hotkey gives V2** — Explicitly recorded as a known, accepted temporary disagreement pending unification. `KeepItLocal.md:442-448 ('The main window's embedded overlay still defaults to V1 and is swapped by…`
- **The three standalone overlay windows (/overlay, /clipboard-overlay, /voice-overlay) are retired; every legacy summon path redirects into the palette** — The palette is the single surface — those overlay windows are never created, costing zero RAM. Routes are deleted from src/routes/. `src-tauri/src/lib.rs:1331-1335, :1243-1248, :1261-1272, :1515; `ls src/routes/` shows no…`
- **The palette window IS pre-created at startup — a deliberate exception to the project's lazy-create rule** — The user explicitly wants a 1-press summon (no wasted-first-press dance), which requires a warm window. ~80-100 MB resident is the accepted stated tradeoff for the new front door. Pre-creating the OLD search overlay was tried and reverted (~100 MB for a renderer most users don't need at boot; the lighter fix —… `src-tauri/src/lib.rs:3128-3153`
- **No size/center reset when showing the palette window** — Forcing inner_size + center on every show overrode the user's saved Width/Position from commandAppearance.ts — the frontend $effect applied the user's value, the next show reset it to 640x560 centered. An intentional pick beats an implicit reset. `src-tauri/src/lib.rs:1343-1352`
- **Window is resizable(true) despite decorations(false)** — Programmatic setSize only works when the window is API-resizable; decorations(false) already removes the OS drag handles, so the user still cannot drag-resize. Required for the Width preset (Compact/Standard/Wide). `src-tauri/src/lib.rs:1310-1316 (Palette Appearance Wave F, 2026-05-27)`
- **Bare cursor-boundary ←/→ navigation REJECTED as 'too fragile'; replaced by explicit Ctrl+Alt+←/→** — An explicit modifier combo works regardless of cursor position and never fights text editing. This is why no bare-arrow folder navigation exists. `src/routes/command/PaletteV2.svelte:4135-4143`
- **Footer cut from ~9 persistent hint chips to 2; the Ctrl+Alt+I panel is the explicit trade-back** — 9 chips read as a permanent cheat sheet crowding the footer. One discoverable place listing every binding is calmer than a permanent one. `src/routes/command/PaletteV2.svelte:5115-5120; commit f521f72 ('It is also the trade made for the 9->2 footer…`
- **Ctrl+/ (syntax) kept SEPARATE from Ctrl+Alt+I (bindings) on purpose** — One job per surface: Ctrl+/ is what you can TYPE, Ctrl+Alt+I is what you can PRESS. Merging them was considered and rejected. `src/routes/command/PaletteV2.svelte:5112-5113, :7116 ('deliberately a different surface from bindings')…`
- **V2 rows are single-line (path/size moved into the preview) and the preview header + path were removed — design trims, NOT regressions** — Stated as deliberate design in the commit that made V2 default. `commit f521f72 ('Deliberate V2 trims (design, not regressions)'); src/routes/command/PaletteV2.svelte:9191`
- **Dev-telemetry scope-hint banner ('Showing only X matching Y · Esc to broaden') hidden in V2** — Scope reads from the active category tag, not a full banner. Note the DOM is still rendered and hidden with CSS, not removed. `src/routes/command/PaletteV2.svelte:10267-10272 (.cmd-scope-hint{display:none}) vs the still-live markup at…`
- **The explicit 'Load more' button was removed in favour of Raycast-style infinite scroll** — Scroll within 240px of the body bottom auto-fires loadMoreFileResults; a quiet sentinel spinner replaces the button. `src/routes/command/PaletteV2.svelte:8491-8496 (Wave I, 2026-05-27)`
- **'Remember last position' window option REMOVED; position is Top/Center/Bottom only. Legacy store fields kept on disk but ignored** — With the tauri://moved listener gone there is no user-dragged position to remember, so rememberedX/Y aren't read anymore. `src/routes/command/PaletteV2.svelte:5320-5326 (Wave I, 2026-05-27)`
- **No DWM acrylic/mica on the palette window; transparency + clip-path instead** — DWM acrylic paints the square window bounds and leaks dark wedges into the rounded corners. A transparent background ALONE is not enough — WebView2 still paints a square backdrop, so `clip-path: inset(0 round 16px)` forces the whole viewport into the rounded shape. Applied via JS gated on isCommandWindow, NOT a… `src-tauri/src/lib.rs:1220-1223; src/routes/command/PaletteV2.svelte:4374-4400 (approx, in onMount)`
- **When desktop blur is ON, the radius + clip MUST drop to 0** — The acrylic window is square (DWMWCP_DONOTROUND) but a still-clipped document gives the 'rounded panel inside square OS window' mismatch the user reported. `src/routes/command/PaletteV2.svelte:4389-4395 (Wave G, 2026-05-27)`
- **Web-search/openUrl results are gated behind the webSearchEnabled opt-in — but explicitly-typed bangs deliberately BYPASS that gate** — An explicitly-typed bang is a user-initiated action, so built-in bangs (seeded as My Commands) match and open a browser even when the backend Web-search opt-in is OFF. Bangs therefore work either way; the backend one is suppressed when a My Command bang matched so the user never sees it twice. `src/lib/stores/myCommands.ts:196-200; src/routes/command/PaletteV2.svelte:2787-2791, :2958-2964`
- **Image clipboard entries have NO dedup by design ('every image copy is its own entry') — this was NOT overturned when fixing the screenshot double-save** — Snipping Tool fires several WM_CLIPBOARDUPDATE per copy. Rather than overturn the deliberate no-dedup rule, a BLAKE3 burst guard drops byte-identical repeats inside 2s only; a later re-copy still gets its own entry. `commit f521f72; src-tauri/src/commands/clipboard_history.rs`
- **In voice DICTATE mode, partial transcripts are deliberately NOT mirrored into the search query** — The destination is the previous app, not the search box. Partials render for feedback only; the final transcript handler does the paste. `src/routes/command/PaletteV2.svelte:4663-4666`
- **Summon does NOT blanket-wipe state; only a TARGETED clipboard/voice hotkey wipes** — The window is created once and reused, so Svelte state survives hides. Blanket-wiping on every summon was 'the entire bug behind I searched, closed, and reopened to a blank palette'. General summon preserves query/mode/voiceSubMode/searchMode/selectedIndex; a targeted hotkey carries the intent 'open this surface… `src/routes/command/PaletteV2.svelte:4427-4450 (Raycast-style restore, Wave 1.4, 2026-05-26)`
- **Preview pane force-closes whenever the user leaves clipboard mode** — Default/voice don't have an inspectable 'current item' worth a 320px pane; snapping it shut stops re-entering default from reopening a previous clipboard visit's preview. `src/routes/command/PaletteV2.svelte:389-395`
- **My Commands are re-read from the backend on EVERY summon rather than via a storage listener** — The palette window is created once and reused, and the `storage` event does not cross WebView2 windows — so edits made in the main window's Settings would never arrive. `src/routes/command/PaletteV2.svelte:4446-4450`
- **Verb-first natural-language typing in the palette DEFERRED; target-first only for v1** — Scope decision locked for the do-anything bar: targets = files + clipboard, Lean verb set. `KeepItLocal.md:357-361; also listed under §8 'Deferred tech'`
- **Automation engine (cross-tool chains + triggers, reusing palette tool verbs) DEFERRED TO V2 — decision 2026-07-06** — Today's Automation is image-only recipes. Generalizing it is a v2 item; keep it local + tight, not a Zapier clone. (The Automation pack is also in HIDDEN_PACK_IDS.) `KeepItLocal.md:365-370; src/lib/appScreens.ts:173 (HIDDEN_PACK_IDS = {'automation'})`
- **DENSITY_OPTIONS rescaled 28/32/36 → 36/42/48 rather than wiring the control back on the old scale** — V2's polish pass hardcoded 42px rows, silently dead-ending the density control. Restoring the old scale would have shrunk every row, so cozy was redefined as 42 to keep V2 looking identical. Accepted side effect: V1's rows also grow to 42px at cozy — the table is shared. `src/lib/stores/commandAppearance.ts:32-40; commit f521f72`
- **Width preset 'Compact' (460px) was dropped from WIDTH_OPTIONS** — The search input got cramped at that width with the chip row + accent icons. Three sizes remain. `src/lib/stores/commandAppearance.ts:42-45 (Wave I, 2026-05-27)`
- **Overlay show/hide Tauri commands are `#[tauri::command(async)]` on purpose** — A JS-invoked show that needs to build/show a window must not block the main event loop while waiting on the IPC reply — the cold-init IPC↔main-loop deadlock. Applied even to commands only summoned by hotkey today, to protect any future button. `src-tauri/src/lib.rs:800-821`
- **Frontend quick-action evaluators run BEFORE the Rust backend** — Zero IPC cost (~0.5ms/call), and color conversion / date math / percentage / bitwise ops are awkward in the Rust expression evaluator but natural in JS. Frontend matches win because their parsers are more specific and the formatting is nicer. `src/lib/stores/quickActionEvaluators.ts:1-24`
- **Ctrl+Space on a clipboard TEXT row opens the wand transform popover, not the generic actions panel** — The generic panel only carries Paste/Pin/Label/Copy/Open URL; the wand is the matching 'actions for this item' surface for clipboard text (Format JSON / base64 / case / sort / Send to note). Falls through to the actions panel for images, snippets, and non-clipboard modes. `src/routes/command/PaletteV2.svelte:3973-3990 (Cleanup Wave 1.2, 2026-05-28)`
- **Ctrl+Shift+D is temporary dev-compare scaffolding, to be removed once a design is chosen (loser component + hook deleted)** — Stated in the hook's own comment. Persisted to localStorage only because it kept resetting to V1 on every restart while V2 was being finished. `src/routes/+page.svelte:319-328`
- **The Sidebar's Command button and Local-Only badge were removed at user request** — The sidebar is navigation only. This is why the embedded overlay has just one production entry point (the Home CTA). `src/lib/Sidebar.svelte:164-165`
- **Live grep runs literal, case-insensitive — not regex** — Constants are hardcoded, not user-configurable. *(inferred — not stated in the source; verify before relying on it)* `src/routes/command/PaletteV2.svelte:469-470 (LIVE_GREP_LITERAL = true, LIVE_GREP_CASE_SENSITIVE = false)`

### Search — indexes, ranking, NL filters, semantic

- **MFT/USN fast-path removed from the filename and content index; the deterministic `ignore` walker is the only indexing engine** — "The deterministic walker is fast enough (~20s for 500K files on consumer SSDs / mixed HDDs) that the MFT path's marginal speed win didn't justify its admin requirement, Win32 unsafe surface, or Mac-port portability cost." Removed wholesale in Wave 6. `src-tauri/src/commands/search.rs:960-966; mft_walker.rs:22-29 ("NOT used by the filename or content index per…`
- **MFT fast-path also DISABLED for DuplicateFinder (its only remaining consumer)** — Returned ~24 entries instead of the full tree; suspected raw-volume read alignment (kernel stricter than AlignedVolumeReader covers) + ntfs-crate tree traversal failing on a live volume vs a disk image. The path-concat double-slash bug was fixed but other failures remain. Root cause NOT known. Code kept in tree only… `src-tauri/src/commands/mft_walker.rs:4-14; files.rs:1129-1146 (`let mft_used = false;`)`
- **`mft_fast_index` and `allow_root_drive_watcher` config fields deleted; Windows system trees hard-blocked unconditionally instead of behind an opt-in checkbox** — With the MFT engine gone the flags had no meaning. System paths (C:\Windows, Program Files, Program Files (x86), ProgramData) are now always blocked from BOTH walker and watcher — "no opt-in flag needed". Old saved configs deserialize-ignore the dead fields. `src-tauri/src/commands/search.rs:258-263; :1554-1560 (is_system_protected_path replacing…`
- **ANN / HNSW vector index deferred; int8 quantization chosen as the scale lever instead** — "approximate + more RAM is the wrong trade here" for the 4-8GB/no-GPU target. int8 gives 4x less RAM while keeping search EXACT (100k chunks: ~154MB → ~38MB). Exact brute-force is fine to several hundred k chunks. Explicit revisit trigger: past ~500k chunks. `top_k`'s signature is deliberately stable so it can be… `src-tauri/src/commands/vector_cache.rs:34-38; embedding-runtime/README.md:92-94; commit 342124a message…`
- **bge-small rejected as the embedding model; all-MiniLM-L6-v2 chosen** — bge-small is CC-BY-NC and "cannot ship in a paid product". MiniLM is Apache-2.0, symmetric (no query:/passage: prefixes needed), 384-d. `src-tauri/embedding-runtime/README.md:16-20; commit 342124a message; embedding.rs:3-7`
- **No local LLM is bundled for search** — Stated flatly alongside the model choice — embeddings only, no generative model. `commit 342124a message ("No local LLM is bundled."); KeepItLocal.md §5 #2`
- **fastembed pinned to default-features=false + explicit ort-download-binaries; hf-hub/reqwest/native-tls deliberately excluded** — fastembed's default `hf-hub-native-tls` pulls hf-hub → reqwest + native-tls. This keeps the core binary free of a silent HTTP client; optional model downloads are separately user-started. Verified after fix: hf-hub gone from Cargo.lock, `cargo tree -i reqwest` finds nothing on… `src-tauri/Cargo.toml:149-161; commit 342124a message`
- **embedding-runtime/ + onnxruntime.dll NOT added to tauri.conf.json bundle.resources — packaging deferred as a separate decision** — "Known gap (documented, not fixed here) ... Works in a dev tree today; packaging is a separate decision (vendor the DLL + ort-load-dynamic vs the current build-time fetch)." This is a deliberate deferral, not an oversight. `commit 342124a message; src-tauri/tauri.conf.json:62-98 (no embedding-runtime entry); KeepItLocal.md §5 #2 "⚠…`
- **Parser-level fuzzy matching (`set_field_fuzzy`) removed from file search; replaced by a dedicated last-resort FuzzyTermQuery pass** — It "expanded *every* term in *every* query into an edit-distance-1 Levenshtein automaton. For short prefixes like 'fi', 'fo', or 'as', this expanded into thousands of terms each becoming a sub-query, causing 70ms–11s query times." Typo tolerance is preserved by Pass 3, which only fires when exact matching returns zero. `src-tauri/src/commands/search.rs:2332-2339`
- **The `content` field is deliberately excluded from the file-search query parser** — "Including `content` made file search scale with indexed *full text*: on a 67k-file content index a query like 'Asura' took ~865 ms instead of the tens of ms it costs against names alone." In-file text is the Content Search tab's job. `src-tauri/src/commands/search.rs:2319-2323`
- **Explicit search-reader reload removed from the hot query path (ReloadPolicy::OnCommitWithDelay does it in the background)** — "was blocking the search thread for 2-4 seconds after each watcher commit while it verified segments. Cost of removal: newly-indexed files take ~5s to appear in search instead of being instantly visible — acceptable trade for never blocking queries." `reload_search_reader_if_due` is kept as #[allow(dead_code)] for… `src-tauri/src/commands/search.rs:2311-2315; :1494-1509`
- **Filename-index merge runs BEFORE the expensive content-index fallbacks (Pass 2 AllQuery, Pass 3 fuzzy), not after** — "the filename index usually already holds what file search is after, so folding it in here means Pass 2 (an up-to-8k-doc AllQuery scan) and the Pass 3 fuzzy walk fire only when nothing matched anywhere — not fruitlessly on every query whose answer is a non-document file (the content index holds readable documents… `src-tauri/src/commands/search.rs:2607-2615`
- **Post-filter scan ceiling lowered from 250k to 30k docs (8k for the aggressive AllQuery pass)** — "the scan costs ~40 us/doc: a broad query (e.g. 'a folder' — the type filter passes few docs, so the scan never reaches target_count and would otherwise run to the ceiling) took ~11 s at the old 250k ceiling. 30k bounds the worst case to ~1 s". `src-tauri/src/commands/search.rs:2445-2460`
- **`from` and `in` are deliberately NOT date-filter triggers** — "they read as the release year in the *name* ('movies from 2025' == 'movies 2025'), not a date. Date filtering by year is explicit: `during`/`year`, or `created`/`modified` + a year." There is a unit test asserting `movies 2025` yields no date filter (search.rs:12060). `src-tauri/src/commands/search.rs:9531-9534; test at :12060-12061`
- **"movies"/"films" excludes web/streaming container formats (webm, ogv, flv); "videos" is the broad bucket** — "Deliberately EXCLUDES web/streaming formats (webm, ogv, flv) that movies aren't distributed in — use 'videos' for the everything bucket." Paired with EXT_PRIORITY_BONUS so a real .mp4 movie outranks a stray .ts (usually a TypeScript file). `src-tauri/src/commands/search.rs:11093-11128; rank.rs:85-102 and test at rank.rs:393-408`
- **`follow_symlinks` removed from index options (serde field kept for backward compat only)** — Wave 7.7 (2026-05-28) removal; the rationale is cross-referenced to the FileSearchStatus.include_hidden comment. `src-tauri/src/commands/search.rs:239-242`
- **HEIC/HEIF excluded from OCR-on-index** — "needs libheif-rs and that crate brings a native C library dependency the bundler would have to ship; revisit when there's user demand." `src-tauri/src/commands/text_extract.rs:256-258`
- **DOC / XLS / PPT (pre-2007 binary Office formats) not supported for content extraction** — Stated as a flat scope boundary in the module doc. `src-tauri/src/commands/text_extract.rs:13-14`
- **XLSX content extraction uses zip + quick-xml streaming over the shared-string table, NOT calamine — and per-cell numbers + inline strings are not indexed** — "Streaming that file with quick-xml keeps extraction cheap and bounded, instead of calamine loading the whole workbook (every sheet, every cell) into memory. Per-cell numbers and the uncommon inline string are not indexed — the shared strings carry a spreadsheet's text." (calamine IS a dependency, but only for the… `src-tauri/src/commands/text_extract.rs:1363-1369; calamine confined to spreadsheet.rs per grep`
- **Semantic vectors stored PLAINTEXT (not DPAPI-encrypted), unlike the OCR cache** — "a lossy transform of the content index, which is itself plaintext on disk by design (see the encrypt-at-rest disclosure); an int8 MiniLM vector is strictly less recoverable than the indexed text beside it." Upgrade path named: wrap quantize/pack in DPAPI if the content index ever goes encrypted-at-rest. `src-tauri/src/commands/vector_cache.rs:21-25; embedding-runtime/README.md:101-103`
- **Quantization scale factor deliberately NOT stored** — "cosine is invariant to positive scaling of each argument, so cosine(q, i8) ≈ cosine(q, f32)" — storing a scale would cost bytes for no benefit. `src-tauri/src/commands/vector_cache.rs:66-69; :12-19`
- **Semantic deep-pagination capped at 1000 candidates; semantic-only hits augment the current page only** — ponytail-marked ceilings: "capped at 1000; a page past that many pure-semantic hits stops deepening — raise the cap or stream if a corpus ever needs it" and "these augment the current page only; they aren't deep-paginated." `src-tauri/src/commands/search.rs:3199-3201; :3331`
- **chunk_text uses whitespace word-splitting, not real tokenization** — ponytail: "close enough for chunk sizing, and it never over-runs the model (fastembed truncates internally). Upgrade to token-accurate windows if recall on CJK / no-space scripts matters." `src-tauri/src/commands/embedding.rs:50-53`
- **One global model mutex serializes all embed calls across indexer threads and queries; per-thread sessions deferred** — fastembed's session isn't Sync. "per-thread sessions if throughput ever matters." Model load failure is cached (try once) so queries don't thrash. `src-tauri/src/commands/embedding.rs:97-102; embedding-runtime/README.md:98-100`
- **On a corrupt-stored-config fallback, OCR and semantic are forced OFF (not restored from status)** — "if we can't trust the saved config, we shouldn't accidentally auto-enable OCR scanning. The user reconfigures from the UI on next open." Same reasoning applied to the semantic beta. `src-tauri/src/commands/search.rs:3799-3815; only caller is the unwrap_or_default branch at :1983`
- **Periodic commits deliberately not used inside the parallel indexing section** — "Tantivy's `commit` needs `&mut self` (can't be called through Arc). The writer's 96 MB internal budget auto-flushes segments as it fills, so memory stays bounded. A single final commit happens after all workers finish." `src-tauri/src/commands/search.rs:6280-6285`
- **Walk uses try_send into the extractor pool's bounded channel, never blocking send** — "so a walk thread never parks inside a full channel: when the pipeline is the bottleneck the walk must still be able to notice a cancellation request rather than stalling on the send for the whole build." `src-tauri/src/commands/search.rs:6592-6598`
- **Multiple indexer drain threads rather than one** — "a single drainer was a hard throughput bottleneck" — per-document work (sensitive scan + add_document tokenisation) must run in parallel. Count = extractor_child_count.clamp(1,8). `src-tauri/src/commands/search.rs:6357-6362`
- **Multi-chunk embedding (≤16 windows/doc, evenly subsampled) chosen over one averaged doc vector** — "a long document's specific matching passage is not diluted into one averaged doc vector"; long docs stay represented end-to-end while embedding cost stays bounded. Forced a store-table rename to `vector_cache_v2` — indexes built by the earlier single-vector build must be rebuilt once. `src-tauri/src/commands/embedding.rs:19-22, :42-48; vector_cache.rs:50; embedding-runtime/README.md:105-107`
- **Semantic weight fixed at 0.8, below bm25 (1.0) and far below lexical (3.0)** — "so an exact text match always outranks a merely conceptual one — semantic only surfaces meaning-related content the keyword signals miss, and orders it *below* the literal hits". Guarded by unit tests asserting WEIGHTS.lexical > WEIGHTS.semantic and WEIGHTS.bm25 >= WEIGHTS.semantic. `src-tauri/src/commands/rank.rs:55-60, :70-73, :361-391`
- **Filename and content indexes kept as two fully independent indexes with separate lifecycles** — "so filename search is never blocked by content indexing" — separate dirs in the same state directory, separate workers, separate IPC files, separate schema versions. Post-step-7d "the two indexers no longer touch each other". `src-tauri/src/commands/search.rs:33-38; :58-62; :2209-2213; :8005-8009`
- **`filename_roots` split from `roots`** — "so filename search can be broad/whole-disk while content search stays folder-scoped. Migrated from `roots` on first load when empty." `src-tauri/src/commands/search.rs:233-237`
- **Content indexing defaults OFF on Lite (low-RAM) profiles at first run despite a backend default of true** — "Default true; the frontend defaults it OFF on first run on Lite (low-RAM) profiles where a content index can't converge." (Task 4.3, 2026-06-18.) `src-tauri/src/commands/search.rs:288-294, :305-307`
- **OCR image gating happens in the walker, not in the extractor child** — "so opt-out images never even cost an IPC round-trip". `src-tauri/src/commands/search.rs:8919-8925; text_extract.rs:260-262`
- **Per-format text tiers rather than one global cap; raising the user cap past a tier has no effect** — "we use min(tier_limit, user_max_bytes) so users can throttle everything down to save disk space, but raising it past the tier's natural limit has no effect (the tier caps win)." `src-tauri/src/commands/text_extract.rs:24-31; search.rs:67-71`
- **PDF input ceiling (64MB) is load-bearing and stays tight while the other heavy formats got a 4GB sanity backstop** — "lopdf::Document::load builds the whole object graph up front with no streaming-load API, so its ceiling stays tight and load-bearing." The streaming extractors' memory no longer scales with file size, so their ceiling is just a backstop. `src-tauri/src/commands/text_extract.rs:74-100`
- **OCR cache kept in its own redb file, separate from keepitlocal.redb** — "so the encrypted-config DB and the high-traffic cache don't share a single writer lock." Same sidecar pattern reused for vector_cache.redb. `src-tauri/src/commands/ocr_cache.rs:13-16; vector_cache.rs:21-22`
- **All OCR-cache and vector-store errors swallow to empty/miss; never fail-stop** — "a cache miss / write failure must never fail-stop the rebuild. The worst case is one rebuild's worth of duplicate OCR work." / "a vector-store failure never breaks search, it just means no semantic candidates that query." `src-tauri/src/commands/ocr_cache.rs:24-26; vector_cache.rs:30-32`
- **Empty cached content forces a fresh extract on rebuild rather than being reused** — "happens for old indexes built before content was STORED, plus genuine 'no extractable text' cases. Forcing a fresh extract handles both: it's idempotent for empty files, and self-heals old indexes on first rebuild." `src-tauri/src/commands/search.rs:5780-5786`
- **Rebuild-skip pre-pass (content_index_unchanged) accepts the same (mtime,size) staleness risk the reuse cache already takes** — "this relies on the exact same (mtime, size) equality the reuse path already trusts, so it introduces no staleness risk beyond the existing incremental rebuild — a file edited in place that preserves both mtime and size is already treated as 'unchanged' today." Read errors return false so a rebuild is never skipped on… `src-tauri/src/commands/search.rs:6005-6021`
- **Corrupt indexes are quarantined, not deleted** — "Quarantined copy of a corrupt content index, preserved for diagnostics." Applies to both indexes (index-corrupt-* / filename-corrupt-*). `src-tauri/src/commands/search.rs:31-32, :45-46, :3422-3434`
- **Live-grep defaults to literal (not regex) from the palette despite the backend defaulting to regex** — "Most palette users want literal — the frontend defaults to literal=true." `src-tauri/src/commands/live_grep.rs:87-91`
- **Live-grep runs against the user's configured indexed roots (multi-folder), not a single picked folder** — Wave 3.3.1 (2026-05-27) made multi-folder the default mode: "the palette's auto-fallback runs against the user's configured indexed roots, which is typically Documents + Downloads + Desktop or a custom list. Empty list → zero-result summary, not an error". `src-tauri/src/commands/live_grep.rs:70-76; PaletteV2.svelte:2530-2551`
- **Extractor pool respawn budget capped at 32** — Bounds the blast radius of repeated OS kills under memory pressure; exhaustion is treated as an abnormal pool death that must not be reported as success (`pool_died` flag exists specifically so the code doesn't "fall through to success=true with a misleading partial count"). `src-tauri/src/commands/search.rs:4198; :6395-6400`
- **Filename index gets a safety-net full rebuild every 6 hours** — "Filename indexing is a cheap metadata-only walk, so a safety-net rebuild a few times a day costs little; live updates (task 9) will later keep it current between these full rebuilds." `src-tauri/src/commands/search.rs:90-94`
- **Dependencies compiled at opt-level 3 even in `tauri dev`** — "Without this, search/image/render math runs orders of magnitude slower in dev than release." Directly affects any dev-time search benchmarking. `src-tauri/Cargo.toml:16-20`
- **Watcher debounce/batch scale with index size rather than being fixed** — Three named strategies keyed on indexed_files: responsive (<250k), large-index batched (≥250k), conservative batched (≥1M) — trading freshness for CPU on huge indexes. *(inferred — not stated in the source; verify before relying on it)* `src-tauri/src/commands/search.rs:85-86, :1518-1538`
- **`nucleo` / `nucleo-matcher` REJECTED on licence (2026-07-20)** — MPL-2.0 file-level copyleft. Usable when linked unmodified, but KeepItLocal is a paid proprietary product ($29/$39) and bge was already rejected for CC-BY-NC on the same grounds (§5 #2); this deserved the same deliberate call, and the answer was no. Also only half-applicable: it is a Rust crate, so it could never serve the frontend `commandRegistry.ts` matching path. **Do not re-propose as "just a small dependency."**
- **Zed's `fuzzy` crate REJECTED — GPL-3.0-or-later, and must NOT be read-then-reimplemented (2026-07-20)** — Verified directly at `F:\Projects\zed\crates\fuzzy`: `license = "GPL-3.0-or-later"` in Cargo.toml plus a crate-local LICENSE-GPL. Strong copyleft — linking it would force KeepItLocal itself under GPL-3.0. Critically, **"just take the idea from their code" is also off the table**: reading GPL source and writing a close reimplementation is derivative-work territory, and this ships in a product we sell. The algorithm is public and permissively licensed elsewhere — source it from **fzf (MIT)**, Forrest Smith's public Sublime-fuzzy write-up (MIT sample code), or Smith–Waterman (published 1981). Ideas are not copyrightable; someone else's expression of them is.
- **Raycast is NOT the search-quality benchmark — our search already beats it (owner, 2026-07-20)** — An earlier proposal cited Raycast/Alfred as the bar for subsequence matching. The owner has used Raycast and rates its search **worse** than ours, which was rebuilt to beat it ~2026-05. Search today "works very good" and is explicitly **not** a pain point. Treat the v2 fuzzy matcher (§5) as an *additive capability*, never as fixing something broken — and do not cite Raycast as an upgrade target. *(owner judgement, stated directly; no benchmark in repo)*

### Clipboard History + Snippets

- **Sensitive clipboard content is TAGGED, never skipped — it gets a 5-minute retention window instead of being dropped** — "if you copied an API key you almost certainly want to paste it" — but "credentials don't linger for days". Applies regardless of the user's retention setting; pinned entries are exempt because the user explicitly chose to keep them. `src-tauri/src/commands/clipboard_history.rs:17-24, :103-108, :878-881, tests :3537-3546`
- **Password managers are excluded from capture BY DEFAULT (11-app list), rather than relying on the OS marker alone** — "these apps put the password on the clipboard for ~20-30 seconds then clear it. Capturing into a permanent history defeats the security model they explicitly opt into." Users who want it can remove the entry. `src-tauri/src/commands/clipboard_history.rs:162-184`
- **Image capture default flipped OFF → ON on 2026-05-26, with a one-time migration sentinel that never overrides a later user choice** — "changed 2026-05-26 per user verdict — previously off, 'opt-in', which silently dropped half of what users copy". `images_default_v2_applied` guarantees a user who turns it off AFTER the migration keeps that off forever. `src-tauri/src/commands/clipboard_history.rs:336-353, :927-933, :1050-1063`
- **NO dedup across image entries — every image copy is its own entry. Reaffirmed rather than overturned on 2026-07-16** — Original: "Computing a perceptual hash to dedup *similar* screenshots isn't worth the complexity for v1." When Snipping Tool duplicates surfaced, the fix was explicitly framed as "Rather than overturn that, a BLAKE3 burst guard drops byte-identical repeats inside 2s". The ponytail note names the upgrade path (a hash… `src-tauri/src/commands/clipboard_history.rs:1518-1522, :404-422; commit f521f72 body`
- **Oversized animated images (GIF / animated WebP) are stored untouched rather than downscaled** — "re-encoding them would silently flatten the animation, so an oversized animation is kept as-is and the cumulative disk-budget enforcer bounds the cache instead." `src-tauri/src/commands/clipboard_history.rs:1459-1481`
- **Pinned entries are immune to ALL eviction — count cap, time retention, sensitive override, AND the 256 MB disk budget** — "we honor the explicit pin over the implicit budget" — the function leaves the pinned set alone even if it alone blows the budget. `src-tauri/src/commands/clipboard_history.rs:1636-1638, :843-845, :1613-1614`
- **One `run_clipboard_action` dispatch command instead of one Tauri command per transform** — "Keeps the Tauri capability surface tiny (one allow-rule covers every action, not a dozen)"; "adding a new action a 5-line change… friction is what kills feature shipping"; "text-in, text-out: no path validation, no filesystem, no network. Hardening surface = zero." `src-tauri/src/commands/clipboard_actions.rs:8-14`
- **A quick-action's result is copied to the clipboard; the history entry is NOT replaced** — "we deliberately don't replace the history entry — that's destructive and not what they expect from 'transform this'" / "the user might want both versions in their history". `src/lib/components/ClipboardActionMenu.svelte:11-14, :190-193`
- **Title-case is a crude first-letter-uppercase, not real linguistic title-casing** — "Good enough for the 'I want this in title case' everyday case; users who need real linguistic title-casing should use a dedicated tool." `src-tauri/src/commands/clipboard_actions.rs:108-110`
- **{{cursor}} renders as a literal `\|` rather than positioning the caret — deferred as "a Pro+ feature for a later release"** — "Full caret positioning needs a global hook + post-paste keystroke injection". NOTE: the auto-expand hook LATER delivered exactly that (split_cursor + VK_LEFT), so this decision now only stands for the palette/paste path. `src-tauri/src/commands/snippets.rs:325-329 vs src-tauri/src/commands/snippet_expand.rs:105-122, :322-330`
- **Single-character date/time format tokens (M, D, H, m, s) are deliberately NOT supported** — "they would match inside arbitrary text — e.g. the `s` in literal 'is' would get expanded to '0' inside the format string 'Today is YYYY'. Users who want unpadded values can hand-edit the leading zero off after expansion." `src-tauri/src/commands/snippets.rs:358-363`
- **Unknown {{variables}} pass through verbatim instead of being stripped** — "Unknown variables are left as-is so users see what they typo'd instead of silent removal." Covered by a test. `src-tauri/src/commands/snippets.rs:257-259, :293-298; test :502-507`
- **A colliding snippet trigger is an ERROR, not an overwrite** — "UX is 'edit the existing one' not 'lose the old body'." `src-tauri/src/commands/snippets.rs:124-126, :141-147`
- **Snippet auto-expand is opt-in, default OFF, and keystrokes are matched in memory only** — "Opt-in, default off. Keystrokes are matched in memory only — never logged, persisted, or transmitted." Surfaced verbatim to the user in the UI copy. `src-tauri/src/commands/snippet_expand.rs:1-8, :125-128; src/lib/stores/settings.ts:332…`
- **Auto-expand v1 deliberately does NOT resolve {{clipboard}} and does NOT bump use_count** — "v1: {{clipboard}} is not resolved in auto-expand (None) — it still works via the overlay. use_count is not bumped here (no AppHandle on this thread): a minor sort-order gap." (The first half of that claim is false — see docContradictions.) `src-tauri/src/commands/snippet_expand.rs:313-315`
- **Auto-expand terminators are limited to space / tab / enter for v1; every other non-alphanumeric key resets the word buffer** — "v1 terminators: space/tab/enter. Other non-alnum keys (caret keys, modifiers, punctuation) reset the current word." Rationale for reset: "the buffer no longer reflects what is before the caret." `src-tauri/src/commands/snippet_expand.rs:269-295, :95-102`
- **split_cursor honors only the FIRST literal `\|`; escaping a real pipe is deferred** — "to keep it simple we treat the first `\|` as the marker (documented limitation: a literal pipe in a template is rare and can be escaped in a later rev)." `src-tauri/src/commands/snippet_expand.rs:105-110`
- **The standalone clipboard-overlay WINDOW is retired (Cleanup Wave 1, 2026-05-28) — everything lands in the unified command palette's Clipboard tab** — "every old summon path now lands HERE on the right tab, so the palette is the single surface and those overlay windows are never created (zero RAM)." hide_* was kept "as a defensive no-op in case some legacy code path still has a label-targeted window reference." `src-tauri/src/lib.rs:1263-1272, :1332-1336`
- **Palette V1 (`/command`) is kept fully intact as the rollback path even though V2 is now the hotkey default** — "V1 is untouched at /command as the rollback path — revert the one WebviewUrl line in lib.rs to go back." Parity was set-diffed (37/37 invokes, 5/5 listeners, 9/9 emitTo) before the swap. KeepItLocal.md §8: "Do not gut either." `src-tauri/src/lib.rs:1302-1305; src/routes/command-v2/+page.svelte:1-8; KeepItLocal.md §8; commit f521f72 body`
- **pasteSnippet order is expand → guard-empty → hide → paste. Do not hide first.** — Documented regression history: "an earlier version hid the palette BEFORE expanding the template; if the expansion threw, the error toast was invisible… It also pasted an empty string into the previously focused app when the snippet template was empty — silent regression of '/sig doesn't paste'." `src/routes/command/PaletteV2.svelte:6407-6419`
- **Ctrl+Space in clipboard mode opens the row's wand transform popover, NOT the generic action panel** — "The generic action panel doesn't carry those transforms, only Paste/Pin/Label/Copy/Open URL." Falls through to the action panel for images, snippet rows, and non-clipboard modes. `src/routes/command/PaletteV2.svelte:3977-3990`
- **The action popover is portaled to document.body — two earlier positioning fixes were insufficient and are recorded** — Wave 1.1 (2026-05-28): absolute positioning was clipped by `.ch-rows-scroll`. Wave 1.2: position:fixed alone still failed because `.cmd-panel`'s backdrop-filter creates a containing block AND overflow:hidden clips it — "The popover existed in the DOM but rendered invisible." `src/lib/components/ClipboardActionMenu.svelte:52-69`
- **CF_UNICODETEXT is hardcoded as raw 13 rather than importing the windows-rs constant** — "the path of `CF_UNICODETEXT` keeps shifting between windows-rs versions; the value is stable since Windows 95." `src-tauri/src/commands/clipboard_history.rs:44-49`
- **Non-Windows clipboard listening is an intentional no-op stub, not a bug** — "Non-Windows clipboard listening is a separate platform integration (Cocoa NSPasteboard polling on macOS, X11/Wayland on Linux). Phase 1 is Windows-first; macOS/Linux ports come later." / "The non-Windows listener is an intentional no-op stub — nothing to supervise until the macOS/Linux ports land." `src-tauri/src/commands/clipboard_history.rs:2313-2318, :2368-2373`
- **Mutex poisoning is recovered from (into_inner) rather than panicking** — "none of those fields become internally inconsistent if a previous lock-holder panicked… Using a panicking `.unwrap()` here would mean one rare panic permanently breaks every clipboard command for the rest of the session." `src-tauri/src/commands/clipboard_history.rs:452-464, :436-440`
- **A corrupt history file is quarantined, never overwritten** — "the bytes are preserved for inspection / manual recovery, the daemon does not crash, and clipboard history simply starts empty." The notice is parked in a static rather than emitted "because load_from_disk runs at startup when no window is open to receive it". `src-tauri/src/commands/clipboard_history.rs:1073-1103, :397-402`
- **An ambiguous elevation verdict on the paste target is treated as BLOCKED** — "The handle opened but the token would not — the classic signature of a non-elevated process looking at an elevated one. Treat as blocked: a needless 'press Ctrl+V' toast is far cheaper than a keystroke that silently disappears." Root cause: "SendInput gives no error… it *reports success* and the keystroke vanishes". `src-tauri/src/commands/clipboard_history.rs:2617-2621, :2646-2653`
- **Snippet variables were moved OUT of the plaintext localStorage settings blob into DPAPI-encrypted secure_kv, with a destructive one-time migration** — "since values can be personal" — the migration deletes the key from the legacy blob "so they no longer sit in plaintext". Same pattern as the myCommands store. `src/lib/stores/snippets.ts:40-44, :73-96`
- **{{name}} and user variables resolve CLIENT-side for the palette but SERVER-side (expand_template_full) for auto-expand** — "{{name}} resolves client-side: the user's name lives in settings, not the backend" / "the auto-expand hook has no frontend, so it resolves them here from data the frontend pushed". Hence sync_snippet_expand_data pushing name+vars+exclusions into the Rust watcher. `src/lib/stores/snippets.ts:186-190, :240-255; src-tauri/src/commands/snippets.rs:407-412`
- **Text dedup promotes an existing entry to the front against the WHOLE ring (Phase 1 only checked the front)** — "this catches the common 'I copied X, copied Y, copied X again' pattern too." The defensive `if let Some` over `.expect()` is also deliberate: "a future refactor that decouples the position from the remove can never crash the daemon". `src-tauri/src/commands/clipboard_history.rs:1384-1411`
- **The clipboard-overlay hotkey config is intentionally simpler than the palette's — no double-space mode** — "Intentionally simpler than `OverlayHotkeyConfig` — no double-space mode (clipboard is too [frequent])". `src-tauri/src/lib.rs:416-418`
- **Only HIGH+MEDIUM sensitive tiers tag a clipboard entry — LOW (email / phone / IPv4) never does** — clipboard_history calls `sensitive_scan::scan_text` which is `scan_text_detailed(text, false)`; LOW is "off by default everywhere. Only surfaces when a tool / setting explicitly opts in." Consequence: copying an email address does NOT trigger the 5-min window. `src-tauri/src/commands/clipboard_history.rs:1233-1236; src-tauri/src/commands/sensitive_scan.rs:29-32…`
- **CF_DIB is always published alongside the registered MIME format on paste-back; a registered-format failure is non-fatal, a CF_DIB failure is fatal** — "it's the lowest-common-denominator format and must always succeed for the paste to work in at-minimum apps like Paint" / registered-format failure "is non-fatal: the CF_DIB write already succeeded, so paste still works — animation just doesn't survive." `src-tauri/src/commands/clipboard_history.rs:3251-3285`
- **type_out_text's success path never touches the clipboard — dictation must not clobber a copy or land in history** — "the success path never touches the clipboard, so dictating does not overwrite whatever the user had copied (and dictation never lands in clipboard history)". The failure rescue is history-suppressed for the same reason: "the user picked type-out specifically to keep dictation OUT of clipboard history". `src-tauri/src/commands/clipboard_history.rs:2006-2011, :2023-2029`

### Voice — Vosk, dictation, command mode, UI automation

- **whisper.cpp removed; Vosk is the only engine. Do not re-propose switching to whisper.** — "whisper.cpp was built here, tested, and deliberately removed — in practice it was *not* faster than Vosk, its start/stop lifecycle was unreliable (sometimes wouldn't start, sometimes wouldn't stop), and its RAM use broke the binding 4-8GB/no-GPU hardware target. The gap is real; the obvious fix is already spent. Any… `Greatness.md:75; Greatness.md:313; KeepItLocal.md:437; verified absent: zero `whisper` hits in…`
- **Windows WinRT `SpeechRecognition` engine removed — voice runs ONLY on Vosk.** — "it is Windows-only, a closed OS component KeepItLocal cannot audit or prove is offline, and could not serve the grammar-constrained command-mode ambition. Voice now runs only on Vosk, an engine KeepItLocal ships and fully controls." This is the privacy-first law applied to a dependency, plus a capability requirement… `src-tauri/src/commands/voice.rs:12-16; src-tauri/Cargo.toml:32-33 ("the Windows WinRT speech engine was…`
- **Speech models are NOT bundled in the installer — downloaded on demand instead.** — "Even the small models add tens of MB; the large ones are well over a gigabyte. Bundling them would bloat the installer. Users often want to try multiple sizes / languages. A built-in downloader sidesteps the 'find the model, copy URL, pick directory' dance the user complained about." `src-tauri/src/commands/voice.rs:54-64`
- **Model downloads use Windows' in-box `curl.exe`, NOT a Rust HTTP client and NOT PowerShell `Invoke-WebRequest`.** — Two separate reasons, both recorded. (a) No reqwest: "so we don't pull `reqwest` (and TLS, and the related deps) into the build just for occasional model fetches," keeping the installer lean. (b) Not IWR: "On Windows PowerShell 5.1 (the version that… `src-tauri/src/commands/voice.rs:160-192`
- **`voice_download_model` is `#[tauri::command(async)]` on a SYNC fn — deliberately, not by accident.** — "a plain `#[tauri::command]` on a *synchronous* fn runs on Tauri's MAIN thread, so the blocking `curl.output()` below would freeze the whole UI for the entire download (no scrolling, no clicks — exactly the 'app goes very slow while downloading' bug). The `(async)` form tells Tauri to run this sync fn on a dedicated… `src-tauri/src/commands/voice.rs:184-192`
- **The #23 webrtc-vad pre-filter ships OPT-IN, OFF by default.** — "a voice-activity detector can misclassify quiet speech, and silently dropping a user's words is far worse than the CPU the gate would save." This is the recorded instance of the binding rule that an optimization in front of a working feature ships default-off until verified on real hardware. `src-tauri/src/commands/voice.rs:1115-1121 (`VAD_ENABLED` init false); src/lib/stores/settings.ts:352…`
- **The VAD uses `VadMode::Quality` (the LEAST aggressive mode) and fails OPEN.** — "`Quality` is the LEAST-aggressive webrtc-vad mode — it errs toward classifying borderline audio as speech. For a gate whose failure mode is 'dropped the user's words', erring toward feeding Vosk is the right bias." And: "Fails OPEN — a too-short / malformed frame counts as speech, so the gate can never swallow a real… `src-tauri/src/commands/voice.rs:1174-1177, :1186-1196`
- **webrtc-vad chosen specifically BECAUSE it is DSP, not ML.** — "webrtc-vad is a small deterministic DSP voice-activity detector (the WebRTC project's VAD) -- NOT an ML model, in keeping with the deterministic-over-ML rule." `src-tauri/src/commands/voice.rs:1131-1139`
- **Voice commands are matched by a deterministic parser — no LLM, ever.** — "Every recognized utterance is matched by the deterministic `executeVoiceCommand` parser — no LLM". Reinforced in the context resolver: "Pure and deterministic — no AI, no network." `src/lib/stores/commandMode.ts:10-12; src/lib/stores/voiceContext.ts:18`
- **The entire command pipeline (normalizer, registry, gate, executor, coordinators) lives in TypeScript, NOT Rust — despite the original plan tagging #10 as "(Backend)".** — "the whole command/dictation pipeline this feeds — commandRegistry.ts (matcher), dictationCommands.ts (formatter), commandMode/pushToTalk/voiceSession (coordinators) — is already TypeScript. The normalizer's OUTPUT is consumed by those TS modules; a Rust normalizer would just have to hand the text straight back across… `src/lib/stores/transcriptNormalizer.ts:29-37; src/lib/stores/commandRegistry.ts:29-31…`
- **Keyboard/mouse voice commands are an explicit CURATED list, not an open "press <anything>" slot.** — "The Vosk command grammar is a flat phrase list, so the set is an explicit curated list (named keys, F-keys, common shortcuts, mouse actions) rather than an open 'press <anything>' slot." This is an engine constraint, not a design preference — Vosk grammars cannot express slots. `src/lib/stores/commandRegistry.ts:456-460`
- **UI elements are picked by NUMBER, not by an open "click <label>" command.** — "the Vosk command grammar is fixed for a session and cannot hold arbitrary per-app button labels. The overlay starts its OWN recognizer with a grammar built from the enumerated labels, so label-speak works there; the numbers are the universal fallback for unlabeled / duplicate / icon-only controls." Same root… `src-tauri/src/commands/voice_ui.rs:13-18; src/routes/ui-elements/+page.svelte:15-20`
- **The web-search command (`capturesQuery`) is deliberately EXCLUDED from the Vosk command grammar.** — "The web-search command's query slot can't be grammar-constrained — command mode reaches search via 'open search'." `src/lib/stores/commandRegistry.ts:718-721`
- **Confirm/cancel words are ALWAYS in the grammar, not added when a command is parked.** — "Always present: the grammar is fixed for the session, so it can't be extended once a command is parked." `src/lib/stores/commandRegistry.ts:730-736; src/lib/stores/voiceSafetyGate.ts:88-94`
- **A fuzzy match of any command riskier than `safe` is REFUSED outright — never confirmed, never guessed.** — "A fuzzy match of anything riskier than `safe` is refused outright: KeepItLocal never acts destructively on a guess." And on ambiguous tool ties: "A genuine tie between two tools is not safe to guess from — ask the user to name the one they meant." `src/lib/stores/voiceSafetyGate.ts:16-19, :210-216; src/lib/stores/commandRegistry.ts:842-846`
- **`literal` (keyboard/mouse) commands are EXEMPT from the repeat cooldown.** — "deliberate rapid repeats ('scroll down' several times) are normal use and must not be mistaken for a duplicate." `src/lib/stores/voiceSafetyGate.ts:201-206`
- **Command mode never persists across launches and stops on tray-hide.** — "It never persists across launches — every app start begins with command mode OFF — and it stops automatically when the main window is hidden to the tray (KeepItLocal's standing 'no recording in the background' guarantee)." This is a privacy law, not a convenience choice — do not propose "remember command mode state". `src/lib/stores/commandMode.ts:15-21, :374-385; src/lib/stores/voiceSession.ts:390-411…`
- **Push-to-talk does COMMANDS ONLY — never dictation.** — "PTT does COMMANDS, not dictation... Dictation lives on the other surfaces (the voice overlay, the Voice tool) — never here." One job per surface, applied deliberately. `src/lib/stores/pushToTalk.ts:11-20`
- **The 400 ms PTT grace delay and the 400 ms post-stop flush delay are both load-bearing — not arbitrary sleeps.** — Grace: "the user often releases the hold key a fraction before the last word's sound has fully finished. Cutting the audio the instant the key comes up would clip the tail." Flush: "`voice_stop_continuous` only SIGNALS the backend's continuous worker to stop — it returns immediately. The worker then finishes its… `src/lib/stores/pushToTalk.ts:57-83`
- **The 250 ms settle delay before opening a cpal stream after preemption is deliberate.** — "cpal supports multiple streams in principle, but back-to-back open/close on the same device occasionally races on Windows WASAPI." `src-tauri/src/commands/voice.rs:1055-1063, :943-950`
- **The idle model-release backstop is 90 s — deliberately LONGER than the frontend's 60 s timer.** — "Slightly longer than the frontend's 60 s timer so this acts as a backstop for the paths that timer does not cover, rather than racing it." And the whole backstop exists because "resource release must not *depend* on a frontend timer firing or a cleanup call being reached." `src-tauri/src/commands/voice.rs:567-583`
- **`EmptyWorkingSet` is called on model release, accepting a brief page re-fault.** — "without that, the committed-but-unused pages linger and Task Manager keeps showing the old high number even though the memory is logically free... That brief re-fault is an acceptable trade for the user actually seeing RAM drop after they stop voice — and the timing is right (they just finished a task)." `src-tauri/src/commands/voice.rs:509-556`
- **A crude linear-interpolation resampler instead of a real DSP crate.** — "Crude but fine for speech bandlimited well below the Nyquist frequency of typical capture rates (44.1/48k). Avoids pulling in a heavier DSP crate just for one feature." `src-tauri/src/commands/voice.rs:1866-1868, :1582-1585`
- **Per-word confidence deferred; `confidence` is hardcoded "high" on any success.** — "Vosk doesn't expose a single confidence bucket per result by default — its per-word confidences live behind `set_words(true)`. For v1 we report 'high' on success; future iteration can wire per-word confidences." The `VoiceRecognitionResult.confidence` field's four documented buckets are therefore aspirational — only… `src-tauri/src/commands/voice.rs:1824-1832 vs the doc at :905-908`
- **Command-mode priority is the FLOOR of the mic ladder (10) — everything preempts it.** — "Push-to-talk is the momentary, explicit gesture and tops the ladder; command mode is the ambient background and yields to every focused surface, resuming when the mic next falls idle." An unknown surface deliberately lands mid-tier (20) as the conservative default. `src-tauri/src/commands/voice.rs:707-713, :737-751`
- **The tray "listening" indicator is set from the mic arbiter ONLY — deliberately removed from the individual capture paths.** — "Light the tray 'listening' indicator here, at the one arbitration chokepoint, so EVERY capture path (single-shot, continuous, command mode, the overlays) shows it, and a preemption hand-off stays correct." Explicitly noted at the old call site: "The tray 'listening' indicator is driven by the mic arbiter… `src-tauri/src/commands/voice.rs:829-835, :961-963, :854-862`
- **`voice_scripts.rs` is deliberately NOT gated to Windows, unlike the rest of the voice backend.** — "Cross-platform: this is plain file IO + a `notify` watcher, so it is NOT gated to Windows like the rest of the voice backend. Voice only works on Windows today, but a command file resolving / reading on any OS is harmless and keeps the command surface uniform." `src-tauri/src/commands/voice_scripts.rs:12-15`
- **The command-file watcher watches the PARENT DIRECTORY, not the file.** — "notify cannot watch a file that does not exist yet -- watch the parent directory and filter events down to our filename." `src-tauri/src/commands/voice_scripts.rs:144-146`
- **User-authored commands are registered LAST so a built-in wins any phrase collision.** — "User-authored commands (#21) come last: a built-in literal command therefore wins a phrase collision with a user one." And within user commands, app-scoped are sorted first: "in the matcher's literal step the first phrase match wins, so a scoped command must out-rank a same-phrase global one when its app is focused." `src/lib/stores/commandRegistry.ts:653-655; src/lib/stores/userVoiceCommands.ts:251-254`
- **Dynamic app/tool commands and the web-search command are deliberately EXCLUDED from the built-in phrase-override editor.** — "Excludes the dynamic app/tool commands (those track live names) and the query-only web-search command (it has no fixed phrase)." `src/lib/stores/commandRegistry.ts:669-671`
- **The command grammar/matcher locale follows the loaded VOSK MODEL, not the UI locale setting.** — "The grammar fed to Vosk and the phrases the matcher compares against MUST match the model: a Georgian model recognizes only Georgian, so feeding it English phrases would recognize nothing." Derived from the model FOLDER NAME (`vosk-model-small-ka-0.22` → `ka`), defaulting to `en`. `src/lib/stores/commandRegistry.ts:742-763`
- **Georgian (`ka`) is deliberately KEPT in the codebase for voice even though Georgian UI localization was deleted.** — "The UI ships in **English only**. The `Locale` type still carries `'ka'` because the VOICE/dictation engine is locale-aware (Vosk Georgian model → `voiceModelLocale()` → Georgian command grammar) and Georgian OCR is still supported — only the UI *translation* layer + language picker were removed." Do NOT propose… `src/lib/stores/settings.ts:26-31; src-tauri/src/commands/voice.rs:133-156; only `src/lib/i18n/locales/en/`…`
- **Georgian normalizer alias tables are intentionally EMPTY scaffolds, not forgotten.** — "The Georgian tables are intentionally scaffolds: a Georgian alias must correct TO a real Georgian command or dictation phrase, and those vocabularies do not exist yet... The Georgian tables are populated alongside those tasks." Similarly the ka key/action vocabulary mirrors en: "the key/action vocabulary is not yet… `src/lib/stores/transcriptNormalizer.ts:39-46, :108-114; src/lib/stores/commandRegistry.ts:543-545, :26-28`
- **The normalizer's alias DATA is admitted to be a starter set — the machinery, not the tables, is the deliverable.** — "Alias DATA is a curated starter set — its real value comes from tuning against observed Vosk output over time. The machinery is the deliverable; the tables grow." The EN command table is 4 entries. Don't file "the normalizer barely does anything" as a bug; it's a known, deliberate state awaiting real-usage data. `src/lib/stores/transcriptNormalizer.ts:39-41, :83-88`
- **`voiceContext` deliberately ships with the SURFACE axis only; foreground process / focused-input kind / selection state were deferred.** — "#12 ships the resolver focused on the KeepItLocal SURFACE — the immediately-real context axis. `resolveVoiceContext` is the designated extension point." Note the deferred foreground-process axis was later solved differently — per-app scoping went through `voice_get_foreground_app` in the matcher… `src/lib/stores/voiceContext.ts:10-17, :30-34`
- **Every command in the registry is `context: 'global'` — scoped contexts are built but unused by design so far.** — "Every command today is 'global'; scoped commands populate this as their per-surface features land." `src/lib/stores/commandRegistry.ts:191-196`
- **The standalone `/voice-overlay`, `/overlay` and `/clipboard-overlay` routes were retired; `show_voice_overlay_window` is now a redirect to the palette's Voice tab, and `hide_voice_overlay_window` is a deliberate defensive no-op.** — "Cleanup Wave 1 retired the standalone voice overlay too — `show_*` redirects to the command palette's Voice tab. `hide_*` remains as a defensive no-op in case some path still holds the label." Do not re-create a standalone voice overlay. (This is also the root cause of the command-mode reachability break — see… `src-tauri/src/lib.rs:1505-1526; src/lib/stores/voiceActionExecutor.ts:144-157; commit 87306ef (2026-05-28)…`
- **Two command palettes coexist intentionally; V2 is the default as of 2026-07-16 and V1 is the rollback path.** — "V1 remains fully intact at `/command` — **revert that one `WebviewUrl` line to roll back**... Do not gut either." And in-code: "V1 keeps its own route at /command as the rollback path — do not delete it." This matters for Voice because BOTH palettes carry a voice mode, so voice code appears duplicated on purpose. `KeepItLocal.md:441-449; src/routes/command-v2/+page.svelte:1-8; src-tauri/src/lib.rs:1305`
- **`voiceSession`'s single-flag loop design (`state.active` only) replaced an older two-flag design — do not "clean it up" back.** — "The older two-flag design (`stopRequested` AND `state.active`) caused the 'press OFF twice and then can't turn back ON' bug: the loop's finally block clobbered state with INITIAL right after a re-arm had already set it active, leaving the user stuck." The empty-looking `finally` block is load-bearing: "We DO NOT… `src/lib/stores/voiceSession.ts:140-156, :286-293`
- **`voice-to-text` (with `file-search`, `clipboard-history`, `snippets`) is a CORE surface, not a tool — the router redirects the legacy id to the merged Command page.** — "the merged Search / Clipboard / Voice page (tabs). The legacy ids (file-search, clipboard-history, voice-to-text, snippets) still exist as screens but the router redirects them HERE onto the right tab." `acceptsSelected: true` on voice-to-text is also deliberate: "Without this flag the parent's router instantiates… `src/lib/appScreens.ts:255-267, :339-350; src/routes/command/PaletteV2.svelte:1189-1198`
- **The `.partial` file is deliberately KEPT on download failure.** — "#25 -- KEEP the partial file on failure so the next download attempt resumes it with `-C -` rather than restarting." Paired with: a `.zip` on disk is always a whole file, because the partial is only renamed after a clean exit. `src-tauri/src/commands/voice.rs:216-224, :296-305, :321-325`
- **`FindAllBuildCache` with a 5-property cache request instead of per-element property reads.** — "Cache the five properties we read so FindAllBuildCache fetches them in ONE cross-process call -- reading them per-element would otherwise be one COM round-trip each (hundreds on a busy window)." A perf decision on the 4-8 GB target; don't refactor to naive per-element reads. `src-tauri/src/commands/voice_ui.rs:120-141`
- **UI-element enumeration is capped at 50 elements (scanning at most 4000 nodes).** — "Keeps the on-screen badge grid readable and the overlay's spoken-number grammar small (1..=50)." And: "a pathological web page can expose thousands of nodes; we never need more than enough to fill MAX_ELEMENTS." `src-tauri/src/commands/voice_ui.rs:40-47`
- **COM is never `CoUninitialize`d on the blocking-pool thread — deliberate.** — "We never CoUninitialize -- a blocking-pool thread staying MTA-initialized is harmless and the pool reuses threads. RPC_E_CHANGED_MODE (the thread is already an STA) is fine: UIA works from either apartment." `src-tauri/src/commands/voice_ui.rs:94-100`
- **Voice input synthesis does NOT do a focus dance before SendInput — unlike the clipboard paste path.** — "Voice commands come from command mode / push-to-talk, where the user's target app is already foreground -- no focus dance is needed (unlike the clipboard overlay's paste path, where KeepItLocal's own overlay is foreground and must hand focus back first)." NOTE: this premise is weakened by the palette's command… `src-tauri/src/commands/voice_input.rs:10-14`
- **The mouse nudge is 40 px and coarse ON PURPOSE.** — "Coarse by design — precise pointing is the #18b mouse grid." `src/lib/stores/commandRegistry.ts:462-464`
- **Vosk model paths are validated against `forbid_system_path` even in the non-vosk build.** — "Even when Vosk isn't compiled in, run path validation so misuse surfaces consistently across both build modes." And the rationale for validating a read-only path at all: "we don't want users accidentally pointing at random system folders." `src-tauri/src/commands/voice.rs:1482-1486, :1509-1512`
- **The `voice-preempted` event exists so a preempted surface's UI can't go stale — a blind backend stop was rejected.** — "Stop the preempted session, then tell its surface so its UI state does not go stale behind a blind backend stop." `src-tauri/src/commands/voice.rs:805-818; consumed at src/lib/stores/voiceSession.ts:130-137 and…`
- **Voice-triggered app launches pass `confirmed: false` to `execute_system_command` deliberately.** — "Voice-triggered launches are never destructive commands (voice only triggers app launchers, not shutdown/restart). `confirmed: false` signals this; the backend gate accepts it for non-destructive ids and rejects destructive ones." Do not "fix" this to `true`. `src/lib/stores/voiceActionExecutor.ts:201-205; src/lib/stores/voiceSession.ts:51-55`
- **`open-url` records a frecency point on the ATTEMPT, not on success.** — "matching the old launcher, where an app command always counts as a launch regardless of whether the page actually opened." `src/lib/stores/voiceActionExecutor.ts:176-180`
- **The Vosk model cache is deliberately never evicted MID-session on model swap.** — "If a user swaps between models the prior model stays in memory; that's an acceptable tradeoff for the latency win, and worst-case memory is bounded by the number of distinct models the user has pointed at (typically 1)." (But see docContradictions — this comment's wording is now stale.) `src-tauri/src/commands/voice.rs:665-669`
- **The phonetic alphabet is the Talon set, chosen for acoustics not aesthetics.** — "The Talon set: short, acoustically distinct, common English words a Vosk model recognizes reliably." Don't propose NATO alphabet or "nicer" words. `src/lib/stores/commandRegistry.ts:507-518`
- **Do not market Voice-to-Text on transcription quality; position it on offline + privacy + command-mode.** — "**stop implying Voice-to-Text competes on transcription quality** — position it on the offline+privacy+command-mode axis instead." And: "This is the one place where the honest answer may be 'don't compete on this axis' rather than 'build the fix.'" `Greatness.md:83; Greatness.md:313`
- **Live-adjust was removed from the Screen Recorder and 'bounded-stop = kill-to-unblock' is its pattern — mentioned here only because the recorder shares `cpal` with voice via an INDEPENDENT feature gate.** — "screenrec — Screen Recorder. Pulls `cpal` (microphone capture, INDEPENDENT of the `vosk` feature so the recorder builds without libvosk)". Do not propose merging the two audio-capture stacks or making screenrec depend on the vosk feature. `src-tauri/Cargo.toml:84-91`

### Notes + Quick Notes

- **No app-level encryption for note bodies — notes stay plaintext `.ki` on disk** — The point of an open `.ki` format in the user's own Documents folder is portability and longevity ('yours forever, openable in any editor'), which app-level encryption would defeat. Unlike the redb stores (clipboard/snippets/OCR cache — all DPAPI-encrypted), notes rely on OS disk encryption (BitLocker). Optional… `src-tauri/src/commands/notes.rs:9-16 (dated 'Encrypt-at-rest audit, 2026-06-21'); echoed…`
- **Notes' own search is NOT routed through the Tantivy content index, even though notes ARE indexed into it** — The Tantivy path silently no-ops until the content index has been built, which would make the notes app's own search quietly depend on an unrelated subsystem's state. A direct scan over a few hundred plaintext files has no such coupling and no cold-start. This is deliberate duplication, not a missed reuse opportunity. `src-tauri/src/commands/notes.rs:305-309`
- **WikiLink is a NODE, not the already-installed Link mark with `href="ki-note://X"`** — Reusing the Link mark would reuse an existing dependency, but tiptap-markdown would serialize it back as `[X](ki-note://X)` — not `[[X]]` — silently breaking both Obsidian portability and the project's own 'plaintext .ki, yours forever, openable in any editor' promise. The whole point of the format is that another… `src/lib/tools/Notes/tiptap/WikiLink.ts:1-13`
- **Wikilink resolved-ness is never stored as a node attribute** — Whether `[[X]]` points at a real note is a function of the notes list, which changes as notes are created/renamed/deleted elsewhere. Baking it into an attr would guarantee stale styling. Computed live via the `resolve` callback instead. `src/lib/tools/Notes/tiptap/WikiLink.ts:24-26`
- **Unresolved wikilinks render muted + dashed, NOT red/error — and clicking one does nothing** — A link you haven't written yet is a normal state in note-taking, not an error. The 'create it from here' affordance is explicitly deferred to a later slice; the no-op is intentional. This is also why link rot is not yet a live problem: unresolved links render visibly dashed and self-announce via a tooltip ('— no note… `src/lib/tools/Notes/NotesEditor.svelte:519-521; src/lib/tools/Notes/Notes.svelte:257-259; WikiLink.ts:98-101`
- **Wikilink chips are deliberately NOT styled like an `<a>`** — An external link leaves the app, an internal one doesn't, and they shouldn't look alike. Bracket glyphs are drawn in ::before/::after so the underlying Markdown stays visible as syntax without living in the document text. `src/lib/tools/Notes/NotesEditor.svelte:496-499`
- **The markdown-it wikilink rule MUST be `ruler.before('link')`, never `ruler.push()`** — Verified against the real markdown-it 14.1.1 the app ships: exactly two inputs change result — `[[Title]](https://x.com)` and `[[Title]][ref]` — where the built-in link rule claims the `[Title]` inside our brackets and the wikilink is destroyed. Also recorded as explicitly NOT a hazard (so nobody adds defensive code)… `src/lib/tools/Notes/tiptap/WikiLink.ts:137-157; regression test WikiLink.test.ts:86-113`
- **tiptap-markdown's `transformPastedText` is disabled; paste is driven manually** — ProseMirror only honours transformPastedText when the clipboard is plain-text-ONLY. The moment a source also offers an HTML flavour (most chat apps, rendered Markdown views, browsers) it's skipped and raw `# heading` lands as literal text. Manual paste renders text that looks like Markdown and bails to the default for… `src/lib/tools/Notes/NotesEditor.svelte:84-94, 186, 249-259`
- **Templates live in a `.templates` DOTFOLDER, not a `templates/` folder** — Every walker (collect_notes, collect_folders, the folder-name guard) already skips `.`-prefixed dirs, so templates stay out of the note list, folder chips, and search with ZERO changes to any walker. A plain `templates/` would show up as a normal folder full of normal notes, and reserving that name would break anyone… `src-tauri/src/commands/notes.rs:33-38`
- **Template variables reuse the snippet system's `expand_template_full` rather than a second variable syntax** — It's a pure string function with no coupling to snippet storage or the keyboard hook, and users already know `{{date}}` from snippets. Unknown `{{...}}` are left verbatim, so a template can still contain literal braces. `src-tauri/src/commands/notes.rs:356-364`
- **Templates have no schema and no templating language — a template IS just a note** — Stated flatly as the design. The folder is created on demand, so an empty list simply means the user has none. `src-tauri/src/commands/notes.rs:329-332`
- **The template split-button only renders once a .ki exists in .templates/** — Templates are opt-in and most users have none, so: 'no dead affordance in the default install'. A real decision with a real reason — but it has an unintended consequence (see genuineGaps). `src/lib/tools/Notes/Notes.svelte:419-422`
- **Commas are a commit key in the tag input, not a typeable character** — The frontmatter tag list is split on ',' in THREE separate places (notes.ts:188, notes.rs parse_tags:244, preview.ts:167) with a naive split that doesn't honour quoting, so a tag containing a comma would silently tear in two on reload. Making ',' commit the chip means one can never be created. A test exists purely to… `src/lib/tools/Notes/NoteTagInput.svelte:8-12, 71-81; src/lib/stores/notes.test.ts:38-51`
- **Tag and pin edits persist immediately; title/body stay debounced** — Adding/removing a tag is a discrete committed action like pinning, not a keystroke stream. Clobbering other frontmatter is impossible because flushSave re-serializes the whole meta object, so title/created/pinned come along untouched. `src/lib/stores/notes.ts:315-332`
- **The quicknote window is DESTROYED on close, never hidden — and the old single-shared-window helper was deleted** — Destroying frees its WebView2 memory the moment it closes (a 4-8GB-RAM-target concern); it is intentionally NOT in the close-to-hide or hide-on-blur handlers. `get_or_create_quick_note_window` is gone so multiple sticky notes can be open at once, Windows Sticky Notes style. The consequence is made explicit: to PARK a… `src-tauri/src/lib.rs:1395-1409; src/routes/quicknote/+page.svelte:7-11`
- **Quicknote emptiness is reported to Rust on transitions only, not per keystroke; and the registry is eager-seeded at window build** — Emptiness lives only inside each note's webview, so it must be pushed; per-keystroke would be a wasted IPC per character. Eager-seeding at build (before the webview's onMount can round-trip its state back) shrinks the rapid-double-press window where a second hotkey press would spawn a duplicate blank. `src-tauri/src/lib.rs:531-537, 1418-1425; src/routes/quicknote/+page.svelte:46-54`
- **Both the emerald and crimson quicknote themes stay; the 2026-07 rebrand did NOT replace emerald** — 'The sticky note's colour is the user's choice, not a brand surface.' The comment also pre-empts the next change: add another by cloning the token block and adding its selector to the shared rules — don't fork the layout. `src/routes/quicknote/+page.svelte:461-465`
- **Quicknote swatch colours are hardcoded hex, knowingly breaking the 'design tokens only, never inline hex' law** — The swatches preview each theme's accent LITERALLY. A token would resolve to the ACTIVE theme's accent and every swatch would look identical. A reasoned exception, not a violation to 'fix'. `src/routes/quicknote/+page.svelte:381-389`
- **`.qn-win-btn` is included in the white-on-accent theme rules** — Called out as deliberate: without it, minimize/maximize sit near-invisible in dark grey on the coloured header bar. `src/routes/quicknote/+page.svelte:498-500`
- **The wikilink autocomplete popup is imperative DOM, not a mounted Svelte component** — Suggestion hands us imperative lifecycle hooks (onStart/onUpdate/onExit) that don't map onto Svelte 5's declarative mount, and the list is 8 rows of text. A component would mean manual mount/unmount plumbing for no gain. `src/lib/tools/Notes/tiptap/WikiLinkSuggestion.ts:5-8`
- **The wikilink popup CSS is unscoped/global inside NotesEditor rather than in styles.css** — Suggestion appends the popup to document.body (it positions against the caret in viewport coords, so it can't live inside the editor's overflow). Kept beside the code that builds it because NotesEditor is its only owner. `src/lib/tools/Notes/NotesEditor.svelte:587-592`
- **No DOMPurify for the note preview sanitizer** — The markdown source is constrained (only marked + hljs output), marked escapes unsafe HTML by default, and the hand-rolled allowlist walk is defense-in-depth at <200 lines — much smaller than a 40KB dep for one read-only surface. `src/lib/notes/preview.ts:40-44`
- **Note saves are revision-checked through one exclusive file handle** — The frontend sends the BLAKE3 revision it opened; a mismatch leaves the external version untouched and asks the user to Reload or Keep mine. A missing note can only be recreated by an explicit Keep mine retry, using `create_new` so a file recreated in the gap is never overwritten. The content still writes in place rather than by temp-rename because of Windows replacement caveats. `src-tauri/src/commands/notes.rs`
- **Normal notes are created at root then moved into the active folder** — If that folder disappears mid-click, the root note remains instead of losing the creation. Automatic captures use the optional destination directly, so they land atomically in the visible Inbox. `src/lib/stores/notes.ts:405-455; src-tauri/src/commands/notes.rs:710-727`
- **Today is one deterministic portable note per local day** — The Today action opens `Daily/YYYY-MM-DD.ki` and creates it only once, even if invoked twice. A user-owned `.templates/Daily Note.ki` supplies the body when present; otherwise the daily note starts clean and blank. `src/lib/stores/notes.ts; src-tauri/src/commands/notes.rs`
- **The note title input is decoupled from the store via a local buffer** — Binding the input straight to `$activeNote.meta.title` would rewrite input.value on every keystroke (setActiveTitle → store → binding feedback) and fight the caret on fast typing / rapid Delete+retype. The buffer is seeded ONLY when a DIFFERENT note becomes active (path change), never on our own per-keystroke edits. `src/lib/tools/Notes/Notes.svelte:71-86`
- **TipTap is driven as a vanilla Editor via onMount/onDestroy, not the official Svelte wrapper** — The official wrapper targets Svelte 4; direct driving is the supported Svelte-5-runes pattern. TipTap is bundled only into the lazy-loaded Notes chunk so it never touches cold-start cost. `src/lib/tools/Notes/NotesEditor.svelte:6-14`
- **The notes PDF export goes through a DOCX intermediate rather than rendering PDF directly** — The DOCX intermediate is what lets it match Word's kerning, line-breaking, and list-marker alignment that direct printpdf rendering couldn't match. Side benefit: Heading1-6 paragraphs are recognized by Word's Navigation Pane, auto-TOC, and screen readers. `src-tauri/src/commands/notes_pdf.rs:1-31; src/lib/tools/Notes/Notes.svelte:151-157`
- **`class` on `<span>` is allowed through the note HTML sanitizer** — hljs emits `<span class="hljs-keyword">` for syntax tokens. `class` cannot execute code, and it's confined to whatever hljs outputs. Dated Wave K (2026-05-28). `src/lib/notes/preview.ts:250-255`
- **Link `openOnClick:false`; external links open in the system browser on Ctrl/Cmd+click only, never navigating the webview** — A plain click must place the cursor — this is an editor. Wikilinks invert this (plain click navigates) and are handled FIRST and separately so the two can never race for the same click. `src/lib/tools/Notes/NotesEditor.svelte:159-162, 189-213`
- **An unreadable note is skipped, not fatal, during body search** — One bad file must not break search across the whole folder. `src-tauri/src/commands/notes.rs:424-425`
- **Body search returns paths plus localized excerpts** — Title/preview/tag matching stays instant and client-side, while a direct async scan provides the one disk-backed signal and the excerpt that explains a deep body match. `src-tauri/src/commands/notes.rs:351-376, 464-521`
- **`stripOuterMarkdownFence` is deliberately narrow — only a whole-body ```markdown/```md fence is unwrapped** — A bare ``` fence or an inline fence is left untouched, so a legitimate code block is never destroyed. `src/lib/stores/notesMarkdown.ts:22-31`
- **`looksLikeEscapedHtml` requires an escaped CLOSING tag** — Distinguishes real corrupted markup from Markdown that merely contains a stray `&lt;query&gt;` placeholder — a closing tag is the signature that never appears in placeholder text. `src/lib/stores/notesMarkdown.ts:11-19`
- **notesMarkdown.ts has no DOM/Tauri/Svelte imports** — They are string→string functions kept pure specifically so they can be unit-tested directly. This is why the DOM-dependent `decodeHtmlEntitiesOnce` lives in notes.ts instead and is re-exported. `src/lib/stores/notesMarkdown.ts:1-9; re-export at src/lib/stores/notes.ts:17-19`
- **AI in Notes is deferred behind two gates and scoped to Notes only; the generic AI chat panel is DROPPED** — v1 AI = Notes ONLY (summarize/rewrite/continue/title/tag), and 'Notes itself must be ideal first' (v1 slate item 4). Requires the §1.1 About privacy-claim reword first. Chat panel dropped as least differentiated. BYOK router, off by default, hard-gated. `KeepItLocal.md:379-382, §5 items 4-5 (lines 295-297); 'generic AI chat panel' also under §8 deferred tech`

### Development + Privacy tools

- **The Secret Scanner's ~150 lines of frontend JS regex rules were DELETED and replaced by the Rust engine. Do not re-propose JS-side detection or 'a quick client-side prescan'.** — The JS rules duplicated and were inferior to sensitive_scan.rs — quoted example: the AWS-secret-key rule 'matched any 40-char base64 string, an enormous false-positive surface'. The backend rejects placeholders (YOUR_API_KEY_HERE), env refs ($DB_PASS, process.env.X), and low-entropy Bearer tokens, and gives three real… `src/lib/tools/Development/devkit/SecretScanPanel.svelte:1-23 (Phase 6.5-4, 2026-05-27 rewrite header)`
- **SQL analysis by frontend heuristic was replaced by the sqlparser-backed Rust `analyze_sql`.** — Comment states analysis 'now goes through the sqlparser-backed analyze_sql command' and labels the result 'real SQL lint ... Honest' (SB-4) — i.e. the prior heuristic lint was not honest about what it could detect. `src/lib/tools/Development/devkit/SqlPanel.svelte:62-68`
- **SSH Key Manager and Encrypt/Decrypt deliberately stay OUTSIDE the Dev Toolkit hub. Do not propose folding them in for consistency.** — They are 'the two heavy, stateful managers'; the shipped docs tip reinforces it: 'SSH Key Manager and Encrypt / Decrypt stay separate tools — they manage on-disk key material and files.' `src/lib/tools/Development/DevToolkit.svelte:11 + src/lib/appScreens.ts:759 (docs.tip)`
- **The nine Development tools were folded from standalone sidebar entries into one hub (2026-06-05).** — Recorded as a deliberate consolidation into 'one calm surface for the light developer utilities', navigated by a sectioned rail using the app's canonical selected-item pattern. Verified complete: no orphaned standalone panels remain in src/lib/tools/Development/. `src/lib/tools/Development/DevToolkit.svelte:2-17`
- **LOW tier (email/phone/IP) is OFF by default everywhere, and specifically off on the index path. Do not propose 'also flag PII in search'.** — 'the index path doesn't want every document that mentions an email tagged kind:sensitive'. LOW only surfaces when a tool explicitly opts in — only the Secret Scanner's Privacy + Release presets do. `src-tauri/src/commands/sensitive_scan.rs:653-660 + :30-31; src/lib/tools/Development/devkit/SecretScanPanel.sv…`
- **The sensitive engine stores only finding KINDS at index time, never the matched contents. Do not propose caching matched values for faster review.** — 'Users can find sensitive files without anyone (including the app) seeing what the secrets are.' Storing the secret would defeat the product premise. `src-tauri/src/commands/sensitive_scan.rs:9-13; src-tauri/src/commands/search.rs:5814-5816`
- **The allowlist stores salted BLAKE3 hashes, never raw text. Per-DEVICE salt was chosen over a per-user pepper. Salt is NOT rotated on restore.** — Storing `password123` to remember 'the user said this isn't sensitive' 'would defeat the entire point of a privacy-first product'. Per-device is sufficient because an attacker reading the redb file already has account-level access and could read a pepper too; the salt's only job is defeating off-device rainbow tables… `src-tauri/src/commands/sensitive_allowlist.rs:10-31, :248-250`
- **Allowlist normalization is trim() ONLY — case and interior whitespace are deliberately significant.** — 'case + spacing matter for tokens (abc and ABC are different secrets)'. Locked in by regression tests that assert hash('abc') != hash('ABC') and hash('a b c') != hash('abc'), with a comment warning that changing the recipe silently invalidates every existing dismissal. `src-tauri/src/commands/sensitive_allowlist.rs:20-21, :282-317`
- **Per-tier regex engine split is intentional: HIGH on regex::RegexSet, MEDIUM on fancy-regex, LOW on plain regex. Do not propose unifying on one engine.** — RegexSet's one-pass parallelism 'is the whole point' for HIGH. MEDIUM needs negative lookahead to reject placeholders/env-refs before producing a match, which the `regex` crate cannot do. LOW needs neither. Credit card (Luhn) and SSN (never-assigned ranges) stay Rust validators because they are 'neither expressible as… `src-tauri/src/commands/sensitive_scan.rs:33-50`
- **Privacy Audit is a first-class always-available surface, NOT a Privacy-pack tool. Do not propose moving it into the pack for tidiness.** — 'the feature that most defines KeepItLocal shouldn't be hidden behind an optional, default-off pack'. `src/lib/appScreens.ts:352-362`
- **The Privacy Audit is user-initiated only. The scheduler is the SINGLE, owner-approved relaxation and ships off by default.** — 'a privacy auditor that silently watched you without that consent would betray its own premise'. Scheduler is explicitly described as 'the single, owner-approved relaxation of that rule'. `src-tauri/src/commands/privacy_audit.rs:6-14; src/lib/stores/privacyAuditScheduler.ts:4-10`
- **The scheduler is frontend-driven; there is deliberately NO OS-level scheduled task for the audit.** — 'The app is tray-resident, so a frontend-driven scheduler in the long-lived main window is sufficient; there is no OS-level scheduled task.' Note the Reminders feature DOES use Task Scheduler — so this is a considered difference, not an oversight. `src/lib/stores/privacyAuditScheduler.ts:32-35`
- **A scheduled audit NEVER fires at the moment you enable it, and a launch-due run is deferred past the startup window.** — Enabling 'daily' today must not scan today. And 'app launch is I/O-heavy, so a burst of disk/registry/process work then would read as a stutter' — the scans are already off the main thread, but the perception cost is the point. `src/lib/stores/privacyAuditScheduler.ts:22-30`
- **The Privacy Audit never CHANGES a permission — remediation is a deep-link into Windows Settings.** — 'We READ it only — we never change a permission; the "fix" is a deep-link into the Windows Settings page so the user stays in control.' `src-tauri/src/commands/privacy_audit.rs:18-22`
- **`open_privacy_setting` takes a catalog target, not an arbitrary URI — and the generic `open_external_url` deliberately REFUSES ms-settings:.** — The frontend 'can only ever request the known privacy pages, never an arbitrary URI'. The refusal in the generic opener is called out as deliberate. `src-tauri/src/commands/privacy_audit.rs:153-154`
- **Privacy Hardening will NEVER elevate. Admin-needed tweaks are a separate guide tier the app refuses to run.** — 'NO admin, ever (the app installs per-user and never elevates)'. Machine-wide tweaks 'genuinely need admin ... We never run these; we show a step-by-step guide + a copyable elevated command so the user does it themselves' — 'with eyes open'. `src-tauri/src/commands/windows_hardening.rs:4-10, :48-49, :226-228`
- **The hardening apply-tier scope is deliberately limited to cosmetic/telemetry/ads user prefs — no boot, no security, no service-disable.** — So that 'the worst case of any toggle is a convenience feature turning off, fully restorable via Revert'. `src-tauri/src/commands/windows_hardening.rs:15-18`
- **The hardening catalog is CLEAN-ROOM — deliberately NOT lifted from privacy.sexy's AGPL data.** — Licensing contamination avoidance: 're-authored from well-known Windows registry conventions, NOT lifted from privacy.sexy's AGPL data'. This constrains any future 'just import a bigger tweak list' proposal. `src-tauri/src/commands/windows_hardening.rs:69-71`
- **Registry WRITES live in windows_hardening.rs shelling out to reg.exe; `winreg` stays read-only by convention repo-wide.** — Keeping writes out of winreg avoids 'tainting the audit's read-only guarantee'. CREATE_NO_WINDOW avoids console flash, matching quick_actions::toggle_explorer_advanced. `src-tauri/src/commands/windows_hardening.rs:20-23`
- **Revert restores the exact snapshotted prior value, never a guessed default; and `applied` is read from the LIVE registry, not from the snapshot.** — 'prior: None means "the value was absent" → revert deletes it rather than guessing a default'. Live read means 'settings the user already changed in Windows show as on' — the snapshot is consulted only for HOW to revert. `src-tauri/src/commands/windows_hardening.rs:270-299`
- **Blur tier is about STRUCTURAL CONFIDENCE, not secrecy — public crypto wallet addresses are deliberately Tier::High.** — HIGH is defined as 'structurally unambiguous → default to BLUR ON'. The wallet-address rows are explicitly annotated 'these are the *public* wallet addresses' with 'Deterministic prefix/length → near-zero false positives' — so their HIGH tier is a confidence statement, knowingly made. `src-tauri/src/commands/sensitive_scan.rs:22-25, :167-178`
- **Findings spans deliberately cover only the secret, not the surrounding assignment.** — 'for a password = "hunter2" match we report [start..end] of hunter2, not of the whole assignment. That keeps the blur tight (only the secret hides, the keyword stays readable so the user knows what they're looking at).' `src-tauri/src/commands/sensitive_scan.rs:73-77`
- **File preview returns EXCERPTS only, not whole files — a deliberate revision of the original design.** — Header literally says 'revised to return EXCERPTS only': 'a 5000-line config with 3 secret lines would force the user to scroll for the actual hits'. Windows carry ±2 lines of context and adjacent windows are merged. Spans are REWRITTEN relative to the excerpt so PrivacyBlur works unchanged. `src-tauri/src/commands/sensitive_scan.rs:694-729`
- **PrivacyBlur explicitly does NOT defend against a local attacker with full screen access.** — Documented threat model: 'Local attacker with full screen access — NOT defended (they can just open the source). Not the goal.' Scope is shoulder-surfing + accidental screen sharing. `src/lib/components/PrivacyBlur.svelte:19-28`
- **Copying from PrivacyBlur never reveals visually — copy is decoupled from reveal.** — 'The unblurred text is always available via the optional Copy affordance — copying never reveals visually.' Blurred spans are user-select:none 'so a stray drag-select doesn't leak the underlying glyphs into the OS clipboard'. `src/lib/components/PrivacyBlur.svelte:6-14`
- **Cron→Task Scheduler mapping fails LOUDLY and specifically rather than approximating. Several cases are deliberately deferred as 'fast-follow'.** — Each rejection names the real reason and refuses to silently drift: seconds precision (Windows has a one-minute minimum); DOM+DOW together ('cron fires when either matches, which Windows can't express as one schedule without double-firing'); '*/n' that doesn't divide evenly into an hour/day ('can't map to a repeating… `src/lib/stores/cronEngine.ts:351-396 (esp. :357, :363, :364-365, :375, :383, :394)`
- **SSH private key material is never returned to the frontend.** — 'Private key *material* is never returned to the frontend — only metadata, fingerprints, and the public key.' Also: pure-Rust ssh-key crate chosen so there is 'no OpenSSL / native deps'. `src-tauri/src/commands/ssh_keys.rs:1-8`
- **Encrypt/Decrypt stores KDF params in the file header specifically so defaults can change later without orphaning old files.** — 'The header stores the KDF params so files stay decryptable if defaults change later.' This is the pre-authorised upgrade path — a KDF-hardening change does not need a migration. `src-tauri/src/commands/crypto_tool.rs:9-11`
- **Free-space wipe deliberately offers a NON-cryptographic RNG ('fast') as a first-class option.** — Non-crypto pseudo-random is 'indistinguishable for forensic purposes' while being much faster; crypto random is framed as 'Only needed against state-level forensics. Overkill for most users.' Fits the resource-efficiency law — this is a considered choice, not laziness. `src-tauri/src/commands/shredder.rs:458-489; src/lib/tools/Privacy/FileShredder.svelte:35-40`
- **The Shredder ships an honest 'this may not work' warning for SSDs rather than overclaiming.** — 'On SSDs, wear-leveling means actual erasure isn't guaranteed — for SSDs, the right answer is full-disk encryption from day one.' Free-space wipe adds: 'Don't run on SSDs (wear).' The tool tells users when NOT to use it. `src/lib/tools/Privacy/FileShredder.svelte:130-135, :418-419`
- **Privacy Audit `Severity` has no Low/Ok variants in Rust even though the frontend styles them.** — 'The frontend also styles low/ok; no scan emits those yet, so they're omitted here — add them back when a scan needs that granularity.' Explicit YAGNI, with the re-add condition named. `src-tauri/src/commands/privacy_audit.rs:46-48`
- **Privacy Audit finding ids are name-based, not PID-based.** — 'a PID-based id would make an acknowledgement never re-match' — acknowledgements must survive process restarts. `src-tauri/src/commands/privacy_audit.rs:1059`
- **Acknowledged findings were MOVED from localStorage to redb.** — The localStorage list 'didn't survive relaunch'. Do not re-propose localStorage for this. `src-tauri/src/commands/privacy_audit.rs:28-30`
- **Many narrow/benign Windows permissions are deliberately NOT surfaced by the audit.** — 'the many narrow/benign permissions we deliberately don't surface' — to 'keep the signal high for a general audience'. A 'show everything' proposal is a regression here. `src-tauri/src/commands/privacy_audit.rs:926-927, :219, :346`
- **audit_unencrypted_pii reuses index data and deliberately runs NO new content scan.** — 'Reuses existing index data — NO new content scan — and is a purely LOCAL read'. The 500-file cap and the index_unavailable honesty flag are part of the same posture: prompt to build the index rather than 'report a misleading "all clear"'. `src-tauri/src/commands/search.rs:10442-10456; src-tauri/src/commands/privacy_audit.rs:409-424`
- **Stale sensitive findings are re-verified against disk, and unopenable files conservatively KEEP their finding.** — Deleted-since-index files are excluded; a file whose secret was removed is re-scanned (first 64KiB) and dropped. But 'can't open → conservative: keep the finding' — a false positive is preferred over a missed secret. `src-tauri/src/commands/search.rs:10530-10542`
- **Crypto/hash/diff long operations use a shared cancel-registry pattern rather than per-tool bespoke cancellation.** — 'Pattern mirrors hash.rs + pdf.rs — a shared HashSet of cancelled operation IDs'. Cancel is checked per 256KiB chunk and half-written output is cleaned up on both the encrypt and decrypt paths. `src-tauri/src/commands/crypto_tool.rs:29-40`
- **Shredder validates paths BEFORE touching the filesystem, rejecting traversal and OS-critical locations.** — 'shredding is destructive, so reject traversal and OS-critical locations up front'. The canonical path returned by validate_user_path is what is then operated on — not the user string. `src-tauri/src/commands/shredder.rs:79-82`
- **devkit panel state is lifted to module stores because of the {#key} unmount — the same pattern as stores/duplicateFinder.ts.** — 'A user who typed a regex / SQL / diff / markdown and walked away would lose it.' Only fields that must survive nav are lifted; 'ephemeral UI flags that are cheap to recompute (busy spinners, focus refs) stay component-local'. Do not propose lifting everything. `src/lib/stores/devkitPanels.ts:1-21`
- **Regex and Diff panels cap input size and degrade explicitly rather than freezing.** — Regex sets a `truncated` flag and shows an inline notice; Diff sets `tooLarge`, skips auto-diff and requires an explicit Compare press. These are the documented freeze-guards. *(inferred — not stated in the source; verify before relying on it)* `src/lib/stores/devkitPanels.ts:42-44, :76-79`

### Document + Image

- **DOCX→PDF conversion removed; Word Converter does exactly ONE thing (docx → clean GFM Markdown / plain text)** — "Option D (2026-05-29): this tool does ONE thing excellently ... and the legacy DOCX→PDF path has been removed (everyone has Save-as-PDF in Word/LibreOffice)." The tool's own docs entry repeats the guidance to users. `src-tauri/src/commands/word_pdf.rs:3-5; src/lib/appScreens.ts:714 ("For PDF, use Word's built-in 'Save as…`
- **The whole PDF pack (merge/split/extract/redact/protect/compress/render) removed and migrated to the sibling product KeepItLocal Privacy; qpdf deleted with it** — Commit 520dac1 'Remove PDF category (migrated to KeepItLocal Privacy)' deleted pdf.rs (4557 lines), qpdf.rs (320 lines) and src-tauri/qpdf-runtime/. PDF unlock is now a Privacy-product feature; metadata.rs was edited in the same commit to point users there. `git 520dac1 --stat; src-tauri/src/commands/metadata.rs:77-80; src/lib/appScreens.ts:744 ("PDF unlocking now…`
- **Standalone Image Compressor / Converter / Resizer retired and merged into one Image Studio — but ONLY the frontend was deleted** — "Image Studio fully replaces all three (verified working). Delete the now-unreferenced components + stores." The commit removed 6 frontend files (1591 deletions) and touched zero Rust — which is exactly why convert_images/compress_images are still registered and dead today. `git 5c6e855 (message + --stat: only ImgCompressor/ImgConverter/ImgResizer.svelte +…`
- **SUPERSEDED 2026-08-03: Image Studio crop and background removal ship.** The locked v1 item is complete: crop is a single-image workflow, Fast U2NETP is bundled, and Full U²-Net is an optional Settings download. `src/lib/tools/Image/ImageStudio.svelte; src-tauri/src/commands/image_tools.rs`
- **v2 "Video Toolkit" = a merged FFmpeg utility surface, NOT a timeline editor (office-hours 2026-07-21).** The editor framing is rejected permanently: it cannot beat DaVinci/CapCut, timeline preview fights the 4–8GB resource target, and a studio of many jobs violates one-job-per-surface. v1 now ships the small Media Utility: extract audio and single-pass target-size compression through the shared user-installed resolver. v2 may add convert, trim, and GIF under one compact surface; two-pass ABR is the accuracy upgrade. Do not add a video editor, Whisper provisioning, PiP, or multi-monitor scope.
- **v1 media utility = extract-audio (mp4→mp3) + compress, pulled forward from the v2 Video Toolkit (owner, 2026-07-21).** Slate #8. The two ops are NOT equal in cost: **mp4→mp3 is ~a day** (`ffmpeg -i in.mp4 -vn -c:a libmp3lame -q:a 2 out.mp3`, needs the `libmp3lame` encoder present in the user's build → capability-probe + degrade). **Compress cost hinges on an undecided fork:** (a) *"just smaller"* = single-pass CRF, one quality knob, ~a day, does NOT guarantee a file lands under a hard cap; (b) *"under N MB"* two-pass ABR, ~a week. **DECIDED 2026-07-21: v1 ships a middle path — single-pass computed-bitrate.** Compute `video_bitrate = (target_bytes·8·0.90 − audio_bits) / duration`, one encode pass at that ABR with a ~90% safety margin; target-size presets (Discord 10/25/50MB) plus a positive custom MB target added 2026-08-03. Usually lands under the cap; if the computed bitrate falls below a floor (clip too long/busy for the target) → offer downscale or an honest "can't fit under N MB", never a silent overshoot. **Two-pass ABR (reliably exact, doubles encode time) is written down as the v2 accuracy upgrade** — swap-in behind the same UX. Also note: "mp4→mp3" is *audio extraction*, not format conversion — mov→mp4 (the iPhone case) is a different op and stays v2. Do NOT quietly expand this into a general converter.
- **Automation pack hidden from all user surfaces and deferred to v2 — code fully preserved on purpose** — "Per the 2026-05-26 Finalizing.md audit (Phase 11), the Automation surface is **moved to v2**. We *hide* it ... the pack definition, the tool screen registration, the Rust commands, and the entire tools/Automation/* directory all stay. To re-surface in v2: remove 'automation' from this set." `src/lib/appScreens.ts:162-173 (HIDDEN_PACK_IDS + rationale comment); KeepItLocal.md:198-203`
- **The `image` crate's WebP encoder rejected in favour of libwebp; plain deflate PNG rejected in favour of oxipng** — "The `image` crate's WebP path is LOSSLESS-only (it silently ignored the quality slider and often produced files BIGGER than the source). `webp` wraps libwebp for proper quality-controlled lossy WebP. `oxipng` is a pure-Rust lossless PNG optimizer (real byte savings vs image's plain deflate). Batch + these encoders is… `src-tauri/Cargo.toml:258-266 (Elite Wave / SB-2, 2026-05-29); src-tauri/src/commands/image_tools.rs:275-307…`
- **The baseline `image` JPEG encoder rejected in favour of mozjpeg trellis quantization** — "mozjpeg — libjpeg-turbo + mozjpeg trellis quantization ... (visibly smaller files than the baseline `image` encoder at the same quality)" — restated in-line as "which is the whole point of a compress tool." `src-tauri/Cargo.toml:254-257; src-tauri/src/commands/image_tools.rs:259-261`
- **AVIF is ENCODE-ONLY on purpose — the image crate's avif decode feature is deliberately not enabled, and .avif is excluded from the input filter to match** — "AVIF (ravif/rav1e) — handled before the `image`-crate format lookup, since ext_to_format() doesn't map 'avif' and the `image` crate's AVIF feature isn't enabled." The frontend's supportedExts list correctly omits avif, so the asymmetry is consistent, not a bug. `src-tauri/src/commands/image_tools.rs:233-251; src-tauri/Cargo.toml:174 (features…`
- **Tesseract shelled out as a subprocess rather than linking libtesseract (original decision); later softened by an opt-in in-process path** — "We invoke the system `tesseract` binary ... no heavy build-time C dep, no bundled engine to maintain, and the user controls the engine version + which language packs (incl. Georgian `kat`) are installed. Fully on-device / air-gapped — Tesseract never touches the network." Wave 8.4 later added the feature-gated… `src-tauri/src/commands/ocr.rs:1-7 (original); ocr.rs:298-306 + Cargo.toml:228-233 (Wave 8.4 leptess…`
- **leptess in-process OCR deliberately does NOT wire preprocessing or PSM — callers needing either fall back to subprocess** — "Only available when the `ocr-leptess` feature is on AND `preprocess=false` AND `psm` is None (the leptess path doesn't yet wire the preprocessing or PSM knobs — adding them is straightforward but not done yet, so callers that need either fall back to subprocess for now)." `src-tauri/src/commands/ocr.rs:358-376`
- **leptess mid-call cancellation deliberately not wired; the known upgrade path is recorded** — "for in-process leptess we currently can't kill mid-call, so the timeout watchdog from 8.2 doesn't apply here — but leptess calls are fast enough that a runaway is unlikely in practice. If it ever matters, leptess exposes `set_cancel_func` which we can wire up." `src-tauri/src/commands/ocr.rs:364-370`
- **OCR watchdog thread leak knowingly accepted for v1** — "Watchdog leak note: the watchdog thread keeps sleeping even after OCR completes naturally, then calls `cancel_ocr_operation` on an already-removed op_id which is a harmless no-op. Marginal — at most ~timeout_secs lingering threads at any moment; each holds zero shared state. Could be tightened with a condvar/channel… `src-tauri/src/commands/text_extract.rs:790-795`
- **OCR cancel = kill-the-subprocess, not a cooperative loop flag** — "OCR's cancel shape is different from the walk-loop pattern used by hash / crypto / live-grep: Tesseract is a single subprocess; the natural cancel is 'kill it.'" Cancellation is also recorded in a set so a kill racing the wait is distinguishable from a genuine non-zero exit. `src-tauri/src/commands/ocr.rs:26-58 (Wave 2.5b, 2026-05-27); git 1f96e06`
- **docx 'open password' (AES/CFB encryption) deliberately NOT supported — only edit-restriction removal** — "Open password ... the file becomes a Compound File Binary container with AES-encrypted streams. Removing this requires decrypting with the password — significantly more complex. **Not supported by this command.** Users with that kind of protection need to open the file in Word first, then save without password." The… `src-tauri/src/commands/doc_unlock.rs:11-21; src/lib/appScreens.ts:744`
- **PDF metadata stripping deliberately excluded from metadata.rs and pushed to the sibling Privacy product** — The PDF arm returns an explicit user-facing message rather than attempting the work: "PDF metadata stripping isn't available here — use the Privacy Sanitizer for PDFs." Added in the same commit that removed the PDF category. `src-tauri/src/commands/metadata.rs:77-80; git 520dac1 (metadata.rs \\| 2 +-)`
- **dHash chosen over pHash and aHash; robustness to large rotation knowingly traded away** — "Algorithm chosen: **dHash (gradient)**. Faster than pHash (no DCT), more robust than aHash (mean) ... Less robust to large rotations (a 90° turn breaks it — that's the trade-off vs the more expensive pHash + four-rotation comparison; revisit if users report a real-world miss)." Explicit revisit trigger recorded. `src-tauri/src/commands/perceptual_hash.rs:16-22 (Quality Pass Wave 1 / DF-1, 2026-05-29); Cargo.toml:192-199`
- **Word Converter's Markdown fidelity gaps are documented, accepted edge cases — not bugs** — Under an explicit "Deferred / documented edge cases:" heading: nested tables flatten to a single-line cell joined with ' / ' because "GFM cannot express a table inside a cell" and "no content is lost"; multi-paragraph cells join with <br>; image-extraction failure emits ![image](unavailable) and carries on rather than… `src-tauri/src/commands/word_pdf.rs:27-36`
- **CamScanner deliberately owns no OCR — it reuses the existing ocr commands (one job per surface)** — "The optional OCR/text step reuses the EXISTING `ocr` commands (`ocr_image`, `ocr_available`, `ocr_languages`, `cancel_ocr_operation`) — this module does NOT touch OCR." `src-tauri/src/commands/camscan.rs:20-22`
- **CamScanner corner detection deliberately never hard-fails on a decodable image** — "If nothing convincing is found it returns the image bounds inset ~2 % with `detected = false`, so the frontend ALWAYS receives 4 usable corners — this command never fails hard on a decodable image." `src-tauri/src/commands/camscan.rs:10-13`
- **Watermark font is read from Windows at runtime, deliberately not bundled; the old 5x7 ASCII bitmap was replaced** — "ab_glyph — TrueType/OpenType rasterizer for the text watermark (real glyphs: lowercase, accents, Cyrillic — replaces the old 5x7 ASCII-uppercase bitmap). Pure Rust, no native build; the font itself is a Windows system font read at runtime." Prefers system fonts for "broad Latin/Cyrillic/Greek coverage ... always… `src-tauri/Cargo.toml:182-185; src-tauri/src/commands/image_extra_tools.rs:888-891`
- **Watermark renders a transparent 1px no-op rather than erroring when no font loads** — "No readable system font — emit a transparent 1px image; the caller still composites (a no-op overlay) rather than the whole watermark erroring." `src-tauri/src/commands/image_extra_tools.rs:832-836`
- **imageproc pulled with default features OFF — text/fft extras and their ab_glyph/rustdct deps deliberately dropped** — "We disable its default features (which would force `image/default`, plus the `text`/`fft` extras and their ab_glyph/rustdct deps) and keep only `rayon`, which reuses the rayon crate we already depend on. No native build. ~600 KB binary impact." Binary-size discipline consistent with the 4-8GB hardware target. `src-tauri/Cargo.toml:175-181`
- **`zip` kept after the Archive-category removal; only its archive-only features were dropped** — "`zip` stays after the 2026-05-29 Archive-category removal — it's used by the Office-file modules (doc_metadata, doc_unlock, metadata, privacy_sanitizer, text_extract for DOCX/PPTX/ODT/ODS/ODP). The `aes-crypto` and `zstd` features were archive-only and went out with archive.rs." `src-tauri/Cargo.toml:205-209; git f24e44a ('Drop Archive category + sweep dead code')`
- **ScreenshotRedact's strip_metadata flag is a deliberate no-op** — "let _ = input.strip_metadata; // We re-encoded from raw pixels — metadata is gone by definition". The flag is accepted for API shape and intentionally ignored. `src-tauri/src/commands/redact.rs:97; src/lib/tools/Privacy/ScreenshotRedact.svelte:330`
- **Image Studio's default suffix is deliberately non-empty to prevent silent overwrite** — "Non-empty default so a keep-format + keep-size pass never silently overwrites the original." A real data-loss guard on the one code path (mode:'none' + format:'keep') where input and output paths would otherwise collide. `src/lib/stores/imageStudio.ts:28-29`
- **oxipng output is guarded to never grow the file, and oxipng failure never fails the file** — "Guard: never let optimization make the file larger" and "oxipng failed (corrupt input, etc.) — fall back to the un-optimized but valid PNG rather than failing the file." `src-tauri/src/commands/image_tools.rs:296-306`
- **Word→Markdown and Word→Text were merged into one screen to de-clutter the sidebar** — Registry comment describing the merge of the "Word→Markdown / Word→Text entries that previously cluttered the sidebar" into one screen with a target-format selector that dispatches to the underlying conversion store. Matches the 'one job per surface' law. `src/lib/appScreens.ts:699-703`
- **Pre-2007 binary Office formats (DOC/XLS/PPT) deliberately unsupported in content extraction** — "DOC / XLS / PPT (pre-2007 binary formats) are not supported." Stated flatly as a scope boundary in the module doc. `src-tauri/src/commands/text_extract.rs:13-15`
- **PDFium chosen as primary PDF text extractor over lopdf, verified on a real Georgian corpus; lopdf kept as fallback** — "Primary PDF text extractor for content indexing: lazy-loading (low memory on huge PDFs) + accurate Unicode where lopdf returns nothing (verified on a real Georgian-book corpus — see text_extract.rs). lopdf stays as the automatic fallback." `src-tauri/Cargo.toml:221-227; src-tauri/src/commands/text_extract.rs:824-827`
- **CLIP image-content search skipped — OCR-in-images deemed sufficient** — Listed under 'Skipped' in the roadmap: "CLIP image-content search (OCR-in-images deemed sufficient)". Do not re-propose visual/semantic image search. `KeepItLocal.md:306-308`
- **Screen-region OCR pushed to v2** — Listed under 'Explicitly pushed to v2' alongside automation, A/V transcription and connectors. `KeepItLocal.md:303-306`
- **tessdata on-demand language packs deliberately deferred — eng/kat/rus are bundled instead** — Listed under §4 'Pending'. The three traineddata files are physically bundled (~23MB) and shipped via bundle.resources, so the shipped posture is 'three languages, zero install' rather than a downloader. `KeepItLocal.md:269; src-tauri/tessdata-runtime/{eng,kat,rus}.traineddata; src-tauri/tauri.conf.json:68-70`
- **ravif's build-machine nasm requirement knowingly accepted** — "rav1e's x86 assembly needs `nasm` at build time (a build-machine requirement; SIMD-accelerated encode)." Contrast with mozjpeg, where the note records the softer posture: "SIMD auto-disables without nasm." `src-tauri/Cargo.toml:251-257`

### File management + Automation / Time / Focus

- **MFT/USN fast-path REMOVED wholesale from the filename + content search index (Wave 6). The walker is the only path; the engine selector UI, the `mftFastIndex` / `allowRootDriveWatcher` / `elevated` config fields, and the i18n keys were all…** — Quoted from the code: the deterministic walker 'is fast enough (~20s for 500K files on consumer SSDs / mixed HDDs) that the MFT path's marginal speed win didn't justify its admin requirement, Win32 unsafe surface, or Mac-port portability cost.' THE ADMIN REQUIREMENT IS THE KEY POINT — it collides head-on with the… `src-tauri/src/commands/search.rs:960-966; corroborated by search.rs:258-259, :555-556, :1793…`
- **MFT fast-path DISABLED in Duplicate Finder pending debugging; the module is deliberately KEPT IN TREE rather than deleted.** — 'Initial Windows test showed this returning ~24 entries instead of the full tree — likely a combination of raw-volume read alignment quirks (the kernel's requirements are stricter than AlignedVolumeReader covers) and the ntfs-crate's tree-traversal hitting edge cases on a live NTFS volume rather than a disk image… `src-tauri/src/commands/mft_walker.rs:1-14; src-tauri/src/commands/files.rs:1129-1146`
- **Pomodoro Timer DELETED as a standalone tool and folded into Focus Mode (commit b832590 'focus mode elite', 2026-05-30). `src/lib/stores/pomodoro.ts` (245 lines) and `src/lib/tools/Focus/Pomodoro.svelte` (351 lines) were both removed; the…** — The commit removes 740 lines and adds 519, replacing a separate Pomodoro surface with a richer Focus Mode (+190 lines to FocusMode.svelte, +243 to focusMode.ts). Consistent with the recorded 'one job per surface / trim overlaps rather than stack them' preference. The break overlay is the leftover of this removal, NOT… `git show b832590 --stat (deletes src/lib/stores/pomodoro.ts, src/lib/tools/Focus/Pomodoro.svelte); git show…`
- **Hosts-file editing REJECTED for Focus Mode site blocking. Hard network blocking (firewall/DNS) deferred.** — Quoted from the spec added in the same commit: 'Hosts-file editing blocks sites system-wide but requires admin → violates the "no admin rights" promise. Decision: do not touch the hosts file. Use no-admin "soft" blocking instead — app/process minimize-or-close + browser-tab nudging via active-window title..… `git show b832590 -- .claude/specs/2026-05-27-greatness-roadmap.md ('Focus Mode blocking — no-admin…`
- **Automation pack HIDDEN from all user-facing surfaces and moved to v2 (2026-05-26 Finalizing.md audit, Phase 11). The pack definition, tool registration, Rust commands, and the entire tools/Automation/ directory all deliberately STAY.** — Recorded reason in code is only 'the Automation surface is moved to v2'. The task brief supplies the real reason (the founder dislikes the current design and rejects the planned 'cross-tool chains reusing tool verbs' successor as not ambitious enough) — that motivation is NOT recorded anywhere in the repo. The code… `src/lib/appScreens.ts:162-173 (HIDDEN_PACK_IDS + rationale comment); consumers at Sidebar.svelte:135…`
- **Automation pack independently OMITTED from the Welcome/onboarding pack picker (Wave 7.5, 2026-05-28) — a second, hardcoded exclusion that does not use HIDDEN_PACK_IDS.** — Quoted: 'Automation pack is hidden — it's a v2 feature, listing it as if shipped is misleading.' `src/lib/WelcomeSetup.svelte:289-293, WELCOME_PACKS array at :294`
- **dHash (gradient) chosen for perceptual similarity over pHash and aHash. Rotation robustness knowingly sacrificed.** — 'Faster than pHash (no DCT), more robust than aHash (mean), and gives a clean 64-bit value out. Robust to scaling, brightness shifts, format changes, and JPEG re-encodes. Less robust to large rotations (a 90° turn breaks it — that's the trade-off vs the more expensive pHash + four-rotation comparison; revisit if users… `src-tauri/src/commands/perceptual_hash.rs:16-21`
- **O(N²) hash-comparison accepted for similar-image grouping; BK-tree / VP-tree deferred until ~50K images.** — 'is O(N²) in the hash-comparison step, which is fine up to ~50 K [images]... BK-tree or VP-tree' — a shortcut with a named ceiling and a named upgrade path. `src-tauri/src/commands/perceptual_hash.rs:159-161`
- **Duplicate Finder deliberately does NOT honor .gitignore / .ignore / git exclude / parent ignores.** — 'users legitimately want to dedup tracked + ignored files alike (think node_modules duplicates across projects). The walker is purely an enumerate; filtering happens via the user's extension_filter / min_size knobs.' Five builder flags are explicitly set false. `src-tauri/src/commands/files.rs:1229-1238`
- **Cleaner's hard entry caps (MAX_ANALYZER_WALK_ENTRIES / MAX_OLD_FILE_SCAN_ENTRIES) REMOVED in favor of streaming + adaptive throttling.** — 'Memory is bounded by streaming (bounded top-N heap for old files; fixed per-dir / per-type accumulators for the maps), responsiveness by RAM-sized walk concurrency (core::resources) + CPU throttle + user Cancel.' Caps were the wrong tool — bounding memory structurally beats truncating results. `src-tauri/src/commands/cleaner.rs:34-39`
- **Cleaner deletion scope restricted to regenerable cache/temp targets, behind a literal "CONFIRM" typed gate. It never deletes personal files, applications, or startup entries.** — Only entries independently flagged `safe_to_clean` are cleanable ('Each entry is independently and safely regeneratable', cleaner.rs:561), and clean_system_cache hard-errors unless payload.confirm == "CONFIRM". `src-tauri/src/commands/cleaner.rs:1515-1516, :561, :89, :1375`
- **Cleaner's RAM probe moved from a PowerShell spawn to the in-process `core::resources` sysinfo probe; CREATE_NO_WINDOW added for the remaining `reg query`.** — 'Without this, opening the Cleaner page or pressing Refresh briefly pops a black console window — both bad UX and at odds with the "no surprising console output" expectation.' `src-tauri/src/commands/cleaner.rs:19-25`
- **Reminder heartbeat driven from a Rust thread (`reminders-tick`, every 15s) instead of a JS setInterval. The 60s setInterval survives only as a non-Tauri dev fallback.** — 'Chromium throttles JS setInterval to ~once a minute while the main window is hidden in the tray, which made due reminders fire late. This (non-throttled) thread emits a reminders-tick... so its due-check runs on time regardless of window state — putting reminders on the same footing as the other Rust-side background… `src-tauri/src/lib.rs:1203-1211; src/lib/stores/reminders.ts:139-157`
- **Reminders driven through PowerShell `*-ScheduledTask` cmdlets rather than `schtasks.exe`.** — 'PowerShell takes a real [datetime] trigger (locale robust — no MM/DD/YYYY vs DD/MM/YYYY guessing) and converts the epoch from the frontend with [datetimeoffset]::FromUnixTimeMilliseconds. Tasks run in the current user's context (no admin, no stored password).' `src-tauri/src/commands/reminders.rs:11-15`
- **Cron scheduling deliberately does NOT implement a general cron engine in Rust; only cleanly-representable trigger shapes are supported, and there is zero background process of our own.** — 'Windows Task Scheduler is not cron, so only the cleanly-representable shapes reach here.' The frontend translator (`planSchedule`) is the gatekeeper; Rust only maps 5 native trigger kinds. Tasks live in a separate `\KeepItLocal\Cron\` folder 'so the two never collide on reconcile' with reminders. `src-tauri/src/commands/cron_tasks.rs:1-10, :20-21, :31-40`
- **Focus break overlay window is DESTROYED on dismiss, not hidden.** — 'so the full-screen break overlay's WebView2 renderer (~100-150 MB) is freed the moment the break ends. Breaks are infrequent and not latency-sensitive, so a cold re-create on the next break is a fine trade for the reclaimed RAM' — mirrors the quick-note window pattern. Directly serves the 4-8GB RAM target. `src-tauri/src/lib.rs:1713-1722, :1669-1671`
- **Focus glow overlay never calls set_focus and is click-through.** — 'Deliberately NOT set_focus — never steal focus from the user's app.' The glow is a peripheral nudge; pointer events pass through to the app underneath via set_ignore_cursor_events(true). `src-tauri/src/lib.rs:1765-1771, :1726-1733`
- **Focus Mode enforcement is non-destructive by rule: graceful WM_CLOSE, never a hard kill; blocked websites are nudge-only; no admin.** — 'Everything is NON-DESTRUCTIVE — close sends a graceful WM_CLOSE, never a hard kill. No admin rights required (all user-level Win32 reads).' Site nudging is title-based and limited to a BROWSERS set. There is always a manual Stop and an auto-end deadline — 'never a way to get "stuck" in focus mode.' `src/lib/stores/focusMode.ts:1-20`
- **Time Tracker reads local dates from the OS (`GetLocalTime`); the `time` crate's local-offset path is deliberately avoided.** — 'so we never touch the time crate's multithread-unsound local-offset path; the frontend (which knows the user's locale) passes the explicit list of YYYY-MM-DD dates to aggregate.' A soundness constraint, not a style choice. `src-tauri/src/commands/time_tracker.rs:16-18`
- **Time Tracker registered as a first-class page screen, deliberately NOT a default-off pack tool.** — 'a first-class, always-visible workspace surface (like Notes / Privacy Audit), NOT a default-off pack tool: it's a flagship paid feature and a background capture runs regardless, so it gets its own sidebar entry.' `src/lib/appScreens.ts:278-288`
- **Notes/Search-style engine selector for MFT removed from the File Search Index UI, along with its i18n keys.** — 'MFT engine selector removed 2026-05-28 — walker is the only path now. Engine i18n keys deleted: title/desc/walker/walkerHint/mft/mftHint.' Recorded as a tombstone key inside the locale file itself so it isn't re-added. `src/lib/i18n/locales/en/pages.json:508; src/lib/tools/TopBar/FileSearchIndex.svelte:830`
- **The MFT duplicate-scan path was designed to be single-volume only; multi-volume scans intentionally return None and fall back to the walker.** — 'v1 supports a single-volume scan (most users' libraries live under one drive letter); multi-volume scans return None and use the walker.' Also returns None on non-NTFS, OneDrive-virtualized files, network shares, and permission denial — 'opt-in by attempt', never propagating errors. `src-tauri/src/commands/mft_walker.rs:186-206, :24-29`

### Media / Screen Recorder + cross-cutting infrastructure

- **ffmpeg is USER-INSTALLED, never bundled** — 'We distribute nothing → zero licensing obligations, zero paperwork, smaller installer.' The doc explicitly pre-empts re-litigation: shelling out to a separate ffmpeg.exe is NOT linking, so bundling would never have forced open-sourcing our code — it would only have added LGPL obligations for the binary we shipped… `KeepItLocal.md:227-236 ('LOCKED 2026-07-16'); corroborated by src-tauri/tauri.conf.json:62-90 having zero…`
- **H.264 via h264_mf (Media Foundation), never libx264/libx265; AAC via FFmpeg's native encoder** — the app requires `h264_mf` at recorder setup and invokes the built-in `aac` encoder. Because KeepItLocal does not distribute FFmpeg, it does not impose a license build gate on the user's installation. `commands/ffmpeg.rs`; `encode.rs` (hard-coded `h264_mf`); `audio.rs` (native `aac`).
- **SetWindowDisplayAffinity is NOT used to hide other apps' windows — it CANNOT be** — Per the Win32 docs the API requires the window to belong to our OWN process, so it can't hide another app's window. The only cross-process way is to read each window's live rect (GetWindowRect is read-only and cross-process safe) and composite black per-frame — which also makes the box FOLLOW the window as it moves… `mod.rs:100-107 and mod.rs:559-566; contrasted with screenrec_cmds.rs:302, :358, :416 (.content_protected on…`
- **content_protected gated to Windows 10 build ≥ 19041 (2004); left UNPROTECTED on older builds** — WDA_EXCLUDEFROMCAPTURE only exists on 19041+. On OLDER builds the affinity silently falls back to WDA_MONITOR, which turns the window SOLID BLACK in the capture — worse than just recording it. So on old builds we deliberately accept 'visible in the recording' over 'a black box in the recording'. Gated on the REAL… `screenrec_cmds.rs:19-46`
- **Stop = close stdin (EOF), never kill — EXCEPT as a bounded escape hatch after 2s** — Killing truncates the file. But the pump's one blocking call is write_all of a multi-MB frame into ffmpeg's small pipe; if the encoder wedges that write never returns and the pump never observes stop. The ONLY thing that can unblock a stuck write is closing ffmpeg's read end — i.e. killing ffmpeg. So on timeout we do… `encode.rs:14-16 (the rule), :311-351 (the documented exception + watchdog)`
- **Fragmented MP4 (frag_keyframe+empty_moov+default_base_moof) everywhere** — A killed/crashed recording stays playable — no missing moov atom. Applied to the video pass, the audio temps, and the mux output. `encode.rs:16-17, :196-198, :486-491; audio.rs:126-127`
- **The pump breaks BEFORE any end-of-stream catch-up — trailing frames are dropped on purpose** — Flushing a large backlog at stop (e.g. after a heavy live resize slowed the pipeline) would block ffmpeg finalize for a long time — leaving the capture frame up and the save arriving 'late'. 'A couple of dropped trailing frames is fine.' `encode.rs:595-601`
- **resolve_crop clamps POSITION, not SIZE, at the screen edge** — A live MOVE (incl. dragging toward an edge) must keep its width/height so captured frames keep matching the encoder's fixed output and stay on the fast straight-copy path. If we shrank the size at the edge instead, a mere move would silently become a resize → per-frame CPU letterbox → encoder backlog → the Stop write… `mod.rs:596-608`
- **Mic pre-gain kept at unity (MIC_GAIN = 1.0); loudness handled at mux via dynaudnorm** — ffmpeg's adaptive dynaudnorm lifts a quiet voice far better than a fixed pre-gain and never clips. The constant is deliberately LEFT IN PLACE as a knob for future per-device tuning — do not delete it as dead code. `audio.rs:34-37`
- **No hand-rolled DSP or A/V-sync math in Rust — ffmpeg does resampling and mixing** — Each source encodes to its own temp AAC; the mux pass does `-c:v copy` + amix and lets ffmpeg handle resampling. Explicitly stated as the design. `audio.rs:6-8`
- **wasapi crate for system audio instead of cpal** — cpal cannot do loopback on Windows. Mic stays on cpal (cross-platform, simple). This is why `screenrec` pulls BOTH cpal and wasapi. `audio.rs:10-13; Cargo.toml screenrec feature comment`
- **cpal is pulled by `screenrec` INDEPENDENTLY of the `vosk` feature** — So the recorder builds without libvosk — `--no-default-features --features screenrec` is a supported fast offline type-check path. `src-tauri/Cargo.toml screenrec feature comment; KeepItLocal.md:415`
- **Three-phase audio start (all captures → all encoders → hand stdins back-to-back)** — Starting sources sequentially offset them by ~100ms (an ffmpeg spawn + device init each), 'which you'd hear as an echo/delay between the system audio and the mic'. The slow ffmpeg spawns are deliberately moved BEFORE any stream starts. `audio.rs:151-154, :169-171, :189-190`
- **Never CoUninitialize; one dedicated MTA thread per capture source** — Stated as a repo rule proven by the 'Stage 0 harness'. RPC_E_CHANGED_MODE is tolerated. `mod.rs:278-281; audio.rs:14-16, :76-81`
- **An audio glitch NEVER fails the recording — it degrades to silent video** — A missing device degrades that one source to 'off' rather than failing the take; if the mux fails the video temp is promoted to the final path so nothing is lost. Failures are eprintln-only. `encode.rs:127-143, :375-405; audio.rs:14-16, :159, :165`
- **kilmedia:// replaces Tauri's built-in asset: protocol for previews** — asset: serves files synchronously inside the protocol closure — i.e. on the WebView/UI thread — and for a range-less request reads the WHOLE file into memory (tauri 2.11 protocol/asset.rs ~L217). Previewing a multi-GB movie froze every window until that read finished. `src-tauri/src/media_protocol.rs:4-15`
- **Images and PDFs are served WHOLE on a range-less GET; only video/audio are chunked** — A capped chunk yields a truncated file that the viewer reports as 'cannot be opened' (notably large scanned PDFs). <img> and the PDF viewer expect the complete resource + a 200 on initial load. Safe because the read is off-thread. `media_protocol.rs:155-165, :183-197`
- **NTFS MFT/USN fast-path REMOVED WHOLESALE — Wave 6 (2026-05-28)** — The deterministic walker is fast enough (~20s for 500K files on consumer SSDs / mixed HDDs) that the MFT path's marginal speed win didn't justify its ADMIN REQUIREMENT (violates the no-UAC law), its Win32 unsafe surface, or its Mac-port portability cost. Old code preserved at search.rs.backup-pre-mft-removal. The… `src-tauri/src/commands/search.rs:960-966`
- **DPAPI over master password / TPM / keyfile** — No password for users to manage (the OS holds the key material), no external dependencies (no key files, no TPM, no network), and per-user scope matches the stated threat model: 'anyone with physical access to the user's logged-in account can already see this data; we only need to protect against another user account… `src-tauri/src/core/dpapi.rs:1-18`
- **Content index, notes (.ki), and the semantic vector cache stay PLAINTEXT by design (disclosed)** — They are derived from / are the user's own content; protected by disk encryption (BitLocker recommended for sensitive corpora). Encrypt-at-rest for the remaining caches is a named pending item — not an oversight. `KeepItLocal.md:120-125; also listed in §8 deferred tech at KeepItLocal.md:441`
- **secure_kv shares the ONE main redb rather than getting its own DB file** — Values land in the same DPAPI-wrapped redb as snippets + preferences, so they get identical at-rest protection 'without each store needing its own DB file'. Keys are namespaced `securekv:` so they can't collide. `src-tauri/src/commands/secure_kv.rs:1-8, :20-22`
- **redb handles are opened ONCE and cached; the mutex never spans a transaction** — The old code re-opened the DB on EVERY call behind one global mutex AND ran a begin_write+commit (an fsync) even for reads just to ensure the table existed. Under a busy disk / AV scan that fsync stalled for hundreds of ms while the global lock was held, parking every window's pending IPC together — 'the intermittent… `src-tauri/src/commands/local_db.rs:17-34`
- **core::resources ported VERBATIM from kil-privacy-suite; its unused API is retained on purpose** — 'so the two products share one resource-awareness philosophy'. The full API (batch preflight, disk checks, recommend_workers) is retained for parity and future consumers even though the Cleaner/Analyzer is today's only caller. `#![allow(dead_code)]` is deliberate and documented — do not prune it. `src-tauri/src/core/resources.rs:16-32`
- **Resource probes fail OPEN, never closed** — A failed RAM probe returns u64::MAX so callers fall back to the CPU cap (today's behavior), and an unresolved disk reports ok=true — 'never warn on a measurement we don't have'. recommend_workers only ever REDUCES under pressure (a safe direction). `src-tauri/src/core/resources.rs:10-14, :53-54`
- **Extractor pool tiers key on AVAILABLE RAM, not total; optimizations ship conservative** — 'A 4 GB machine with 3 GB in use reads as Lite. Optimizations ship default-off on real hardware (Golden Rule #2), so the caps are conservative.' `src-tauri/src/commands/search.rs:943-946`
- **The screen-recording hotkey action lives in the FRONTEND, not the backend** — The backend deliberately just emits `screenrec:hotkey-toggle` to all webviews 'so it reuses the recorder's full start/stop flow — including saving to the remembered folder and stopping from the content-protected toolbar even while the app is minimized'. The frontend owns the save-folder/dialog choice + multi-window… `src-tauri/src/lib.rs:504-507, :1155-1171`
- **Recorder helper-window commands are registered UNCONDITIONALLY, outside the screenrec feature gate** — They are pure Tauri + windows-crate (no capture/encode), so selecting a region or showing the bar is harmless in a build without the feature; only screenrec_start itself is gated. This keeps lib.rs registration unconditional while the heavy impl stays feature-gated. `src-tauri/src/commands/screenrec_cmds.rs:1-6, :262-269`
- **Standalone search overlay (Ctrl+Alt+S) retired** — The command palette (Ctrl+Alt+K) is the single search surface; Ctrl+Alt+S is no longer registered, so there is no fallthrough action for any other shortcut. `src-tauri/src/lib.rs:1173-1176`
- **Deferred recorder features: single-window capture, webcam PiP, multi-monitor, cursor overlay, panic hotkey, recordings library** — Listed as *Deferred* in the shipped-v1 section; multi-monitor and webcam-PiP are additionally in §8's do-not-re-propose list. `KeepItLocal.md:246-247 and KeepItLocal.md:439-440`
- **GIF export parameters are fixed (12 fps, 640px) rather than user-configurable** — Chosen 'for a shareable file' — the stated goal is that the result is shareable, not gigantic. export_gif() takes fps/max_width params and clamps them (5-30, 120-1920), but control.rs passes literals. *(inferred — not stated in the source; verify before relying on it)* `control.rs:135-141; encode.rs:649-656`
