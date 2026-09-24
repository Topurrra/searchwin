<p align="center">
  <img src="workspace.png" alt="KeepItLocal Workspace" width="120">
</p>

<h1 align="center">KeepItLocal Workspace</h1>

<p align="center">
  <b>A local-first, privacy-first productivity suite for Windows.</b><br>
  One command palette, focused local tools, three flagship pillars —
  search, clipboard, and voice — all running on your machine.<br>
  No account. No telemetry. No cloud dependency. Works fully offline.
</p>

<p align="center">
  <sub>Windows 10/11 · Tauri 2 + SvelteKit 5 + Rust · runs on 4–8 GB machines, no GPU required</sub>
</p>

---

## What it is

KeepItLocal Workspace is a keyboard-driven desktop app that folds the little
tools you'd otherwise keep in ten browser tabs and five installed utilities into
one place — reachable from a single palette, with everything computed on your own
machine.

It is built around one promise: **your files, clipboard, and voice never leave
your device.** The heavy work — full-text indexing, OCR, encryption, speech
recognition, embeddings — happens in a native Rust backend, not in the cloud and
not behind a login.

### The nine non-negotiables

1. **Local-first** — indexing, OCR, crypto, voice and embeddings all run on-device.
2. **Privacy-first** — no telemetry, no analytics SDKs, no account wall. Sensitive
   data is **DPAPI-encrypted at rest** (bound to your Windows account).
3. **Works fully offline** — no cloud dependency for any core feature.
4. **Resource-efficient** — designed for 4–8 GB / no-GPU machines: RAM-aware
   throttling, bounded reads, adaptive concurrency, Windows Job Objects with hard
   memory caps.
5. **Deterministic over ML** — a correct algorithm beats a model. **AI is optional,
   never required.**
6. **Beat the free version** — every tool has to clearly beat its free competitor
   for the local workflow, or it doesn't ship.
7. **One job per surface** — sharp separation; overlapping features get trimmed, not
   stacked.
8. **No admin / no UAC** — every feature works at normal user privilege.
9. **No phone-home, no in-app updater** — updates are a manual download.

> **On privacy and the network:** No telemetry, no phone-home. Core work stays on
> your device. Optional model downloads and user-configured services connect only
> when you explicitly start or configure them. The posture is "won't unless you
> say so," not "can't."

---

## The three flagship pillars

Everything else is a tool. These three are the reason the product exists.

### 🔍 Search — the flagship

A single index over your machine that most tools don't attempt:

- **Filenames** (Tantivy, BM25, kept watcher-fresh) **and file _contents_** —
  search inside documents, code, and text, not just names.
- **OCR-on-index** — text inside images and scanned PDFs becomes searchable.
- **App & command launch** — start apps, reach Control Panel, kill processes.
- **Natural-language queries** — "screenshots from yesterday" works; stopwords are
  stripped automatically.
- **Optional semantic search** (off by default) — an on-device MiniLM model
  (Apache-2.0, int8, ~22 MB) finds meaning-related content, not just keyword matches.
- A fused ranker blends keyword strength, filename/path match quality, frecency,
  recency, and (when on) semantic similarity.

### 📋 Clipboard History

More than a paste buffer:

- Searchable history of everything you copy, text and images.
- **Auto-categorised** and **sensitive-content tagged** — passwords, keys and cards
  are flagged and given a shorter retention window.
- **Excludes password managers by default** — copies from KeePass/Bitwarden aren't
  captured.
- Pin items, expand snippets, and paste back into the previous app.

### 🎙️ Voice to Text

Fully offline dictation and voice commands:

- **Vosk** speech recognition — no cloud, no upload, no account.
- Dictate into any app, or drive the app by voice in **command mode** (a
  deterministic grammar parser — no LLM, ever).
- Low-friction model install; degrades cleanly when a model isn't present.

---

## The tool catalog

Plus always-on surfaces: **Command Palette**, **Notes + Quick Notes**,
**Snippets** (a device-wide text expander), **My Commands**, **Time Tracker**, and
**Privacy Audit**.

| Pack | Tools |
|------|-------|
| **Utilities** | Hash Check · Encoders (Base64/hex/URL/…) · QR Code · Format Converter (JSON/YAML/TOML/XML) · Password Generator · Color Picker · Calculator |
| **Files** | File Manager (dual-pane) · Archive Utility (ZIP / 7Z / TAR) · Cleaner / Analyzer · Duplicate Finder · Bulk Rename · Folder Diff & Merge |
| **Images** | Image Studio (resize + crop + background removal + convert + compress) · Image → Text (OCR) · Document Scanner · Favicon Generator · Watermark |
| **Media** | Screen Recorder (primary screen or region → MP4) · Media Utility (video → MP3 or target-size compression) |
| **Documents** | Markdown Converter (→ HTML / PDF / Word / text) · CSV Toolkit (Excel ⇄ CSV, merge, clean, split, → JSON) |
| **Development** | Developer Tools (JWT · regex · SQL format · diff · ID/UUID · fake data · cron · secret scan) · SSH Key Manager · Encrypt / Decrypt · Image → Base64 |
| **Privacy** | File Shredder · Screenshot Redact |
| **Time & Focus** | Reminders · Focus Mode |

*Everything runs on-device. Feature packs are toggled locally; the Core pack (search,
clipboard, voice, notes) is always installed.*

---

## How it compares

KeepItLocal doesn't have a single competitor — it competes tool-by-tool with the
best free option for each job. Here's the honest picture against the tools people
actually reach for.

### vs. launcher / palette tools

| | **KeepItLocal** | Raycast | PowerToys Run | Alfred |
|---|---|---|---|---|
| Platform | **Windows** | macOS (Windows in beta) | Windows | macOS |
| Model | Local-first suite | Cloud-connected launcher | Launcher | Launcher + workflows |
| Account required | **No** | Yes (for sync/AI/store) | No | No |
| Searches file **contents** | **Yes** (Tantivy index) | No (names/metadata) | No | Limited (Spotlight) |
| OCR text in images/PDFs | **Yes** | No | No | No |
| Offline AI / semantic | **Yes**, on-device, opt-in | Cloud AI | No | No |
| Built-in tool suite | **~30 tools** | Extensions (mostly online) | A handful | Workflows |

Raycast is a superb macOS launcher, but it's a different philosophy: extension-
and cloud-driven, account-based, AI in the cloud. KeepItLocal is a self-contained
Windows suite whose search indexes file *contents* locally — the thing launchers
generally don't do — and keeps the AI on your machine and optional.

### vs. file search

| | **KeepItLocal** | Everything (voidtools) | Windows Search |
|---|---|---|---|
| Filename search | **Yes**, instant | **Yes**, instant (the gold standard) | Yes |
| **Content** search | **Yes** | No (names only) | Partial, slow |
| OCR / scanned PDFs | **Yes** | No | No |
| Semantic / meaning | **Yes** (opt-in) | No | No |
| App & command launch | **Yes** | No | Partial |

Everything is unbeatable at *filename* lookup and we don't pretend otherwise. Where
KeepItLocal wins is everything past the filename: contents, OCR'd text, meaning, and
launching — one index instead of four tools.

### vs. clipboard managers

| | **KeepItLocal** | Ditto | Windows Win+V |
|---|---|---|---|
| Text + image history | **Yes** | Yes | Yes |
| Search history | **Yes** | Yes | No |
| Auto-tag sensitive data | **Yes** | No | No |
| Exclude password managers | **Yes**, by default | Manual | No |
| Part of a wider suite | **Yes** | No | — |

### vs. the "grab a website" tools

The tools you currently open a browser and upload a private file to — CyberChef,
ezgif, cloudconvert, online CSV/JSON converters, favicon generators, hash checkers —
KeepItLocal does the common cases **locally, instantly, with no upload, no size cap,
no watermark, and no ads.** For a confidential document or a personal photo, that's
not a convenience — it's the difference between private and not.

---

## Under the hood

- **Shell:** Tauri 2 (native Windows webview, tiny footprint)
- **Frontend:** SvelteKit 5, runes-only, token-based theming
- **Backend:** Rust — all real work runs here, off the UI thread
- **Search:** Tantivy (BM25) for filenames + content; optional `fastembed` → ONNX
  Runtime with all-MiniLM-L6-v2 (int8) for semantic search
- **Voice:** Vosk (offline)
- **OCR:** Tesseract (English, Georgian, Russian language packs)
- **Encryption at rest:** Windows DPAPI (account-bound), over an embedded `redb` store
- **Resource safety:** RAM-aware indexing, Windows Job Objects with hard memory caps,
  adaptive watcher debouncing that scales with index size

---

## Status

KeepItLocal Workspace is in active development, hardening its shipped v1 tools
while licensing remains the final v1 item. Some tools that need more work are
hidden rather than shipped half-ready, so they can return when they clear the bar.

**[`KeepItLocal.md`](KeepItLocal.md) is the source of truth** — full architecture,
the complete decision registry, and the roadmap live there.

---

<p align="center"><sub>Built to run on your machine, and only your machine.</sub></p>
