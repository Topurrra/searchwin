# KeepItLocal Workspace — Roadmap

**The live status tracker: what's shipped, what's left, and what's planned for v2.**

> **This file tracks _roadmap status_. It is not the source of truth for _decisions_.**
> - For architecture, pillars, and the full feature catalog → **`KeepItLocal.md`** (§0–§4).
> - For everything deliberately **rejected / deferred / removed** (and _why_) → **`KeepItLocal.md` §8**, the decision registry. Grep it before proposing anything here; a "gap" may already be a closed decision.
> - Where this file and the code disagree, the code wins — and fix this file.

**Status legend:** `[DONE]` shipped & verified · `[In Progress]` actively being built · `[Not Started]` planned, no code yet · `[Verify]` code-complete but needs a real-app run to confirm.

Last updated: 2026-08-03.

---

## 1. What we have — shipped

### Flagship pillars (always-on)
- `[DONE]` **Command Palette** — the do-anything overlay; V2 is the one palette (V1 deleted 2026-07-18), Raycast-descended UI, match highlighting, scope chips.
- `[DONE]` **Search** — Tantivy filename + **content** index, OCR-on-index, app/command launch, natural-language queries, fused ranker.
- `[DONE]` **Semantic search (beta, off by default)** — all-MiniLM-L6-v2 **int8** (Apache-2.0), int8 vector cache, brute-force cosine. (int8 model bug fixed 2026-07-23 — was shipping fp32.)
- `[DONE]` **Clipboard History** — text + images, searchable, sensitive-tagged, excludes password managers by default.
- `[DONE]` **Voice to Text** — offline Vosk dictation + deterministic command mode.

### Core surfaces
- `[DONE]` Notes + Quick Notes (trash, folders, md/HTML/PDF export, quick-note min/max)
- `[DONE]` Snippets (device-wide text expander, WH_KEYBOARD_LL)
- `[DONE]` My Commands · Time Tracker · Privacy Audit · Settings · Profiles · Tool Packs

### Tool catalog
- `[DONE]` **Utilities (7):** Hash Check · Encoders · QR Code · Format Converter · Password Generator · Color Picker · Calculator
- `[DONE]` **Files (7):** File Search · File Manager (now leads the pack) · Archive Utility · Cleaner/Analyzer · Duplicate Finder · Bulk Rename · Folder Diff & Merge
- `[DONE]` **Images (5):** Image Studio (resize+crop+background removal+convert+compress) · OCR · Document Scanner · Favicon Generator · Watermark
- `[DONE]` **Media (2):** Screen Recorder · Media Utility
- `[DONE]` **Documents (2 visible):** Markdown Converter (→HTML/PDF/Word/text) · CSV Toolkit
- `[DONE]` **Development (4):** Developer Tools (JWT/regex/SQL/diff/IDs/fake-data/cron/secret-scan) · SSH Key Manager · Encrypt/Decrypt · Image→Base64
- `[DONE]` **Privacy (2 visible):** File Shredder · Screenshot Redact
- `[DONE]` **Time & Focus (2):** Reminders · Focus Mode
- `[DONE]` Browser bookmarks + history search (Chrome/Edge/Brave/Vivaldi + Firefox; palette Web chip)

### Recent implementation milestones
- `[DONE]` Semantic **int8 model swap** + loader test (was fp32, 86→22 MB; fixed the wrong-file mis-ship at the README source).
- `[DONE]` **Per-tool hide mechanism** (`hidden?` flag) filtered across all 6 surfaces (sidebar, category workspace, tool packs, palette, voice grammar, profile picker).
- `[DONE]` **Hidden for v1:** Privacy Hardening (loses to ShutUp10), Word Converter (superseded by Markdown Converter), Remove Password.
- `[DONE]` **Markdown Converter** — absorbed the devkit Markdown panel + added HTML/PDF/Word/text file export (reuses notes export backend; added `notes_export_docx`).
- `[DONE]` **"See what's running"** — Tinycast-style running dot on app rows in the palette (`launch_targets_running`, resolves `.lnk`→exe). `[Verify]` needs a real-app run (dev preview can't see live processes).
- `[DONE]` **Palette-only App mode fix** — a 2s splash fallback was un-hiding the main window every launch. `[Verify]` launch-path change, needs a real build.
- `[DONE]` Hash Check + CSV→JSON cancel bugs (snake_case invoke keys → camelCase).
- `[DONE]` **Disclosure Firewall port** — `firewall/{mod,policy,mask}.rs`, 24 tests. ⚠ **Staged/inert: zero consumers** until an AI transport exists (see §3). Do not delete as "dead code."

---

## 2. v1 finishing slate — what's left

**v1 = harden and polish what exists.** New tools/categories go to v2. (LOCKED 2026-07-06; order below.)

1. `[DONE]` ~~Command Palette V2 polish~~
2. `[DONE]` **Do-anything bar (Lean)** — target-first file and clipboard actions in the V2 palette: copy SHA-256, encrypt, image handoff, OCR, redaction, DOCX→Markdown, clipboard QR/Base64/Notes. Verb-first NL remains deferred to v2.
3. `[DONE]` **Image Studio upgrade** — single-image crop and local background removal ship with bundled Fast U2NETP; Full U²-Net is an optional, user-started Settings download.
4. `[DONE]` Notes made ideal (trash/folders/export/quick-note min-max shipped).
5. `[DONE]` **About privacy-claim reword** (§1.1) — trust copy now says core work stays local, with optional downloads and configured services only when the user starts them. ⚠ **AI is NOT in v1** (owner, 2026-07-23) — it remains a v2 item.
6. `[DONE]` ~~Browser bookmarks + history search~~
7. `[DONE]` **Screen Recorder FFmpeg migration** — user-installed only: PATH detection or one explicit local selection persisted in encrypted local storage. Recorder preflights `h264_mf`; setup cards explain missing or incompatible installs. `[Verify]` choose a non-PATH install, restart, then record a short clip.
   - **Capptivo reference (2026-08-02):** borrow only the compact capture-bar hierarchy, typed IPC/capability scoping, hardware-free test seam, atomic artifact manifests, and cancellable progress pattern when applicable. Do **not** port its GPL FFmpeg sidecars, Whisper provisioning, video editor/timeline, PiP/multi-monitor scope, or assets.
8. `[DONE]` **Media Utility** (v1 slice of the v2 Video Toolkit) — **extract audio (video→MP3) + target-size compression** only. Compression uses one pass and a computed bitrate for presets or any positive custom MB target, reserves audio budget, and refuses an unwatchable target. It has live progress, cancellation, and runtime state that survives tool navigation. Rides #7's shared resolver. `[Verify]` extract one MP3 and compress one short MP4; confirm outputs and expected sizes.
9. `[DONE]` **Per-app hotkeys** — Settings → Shortcuts dynamic app list toggles focus/hide (or launches the target). Fixed-shortcut collisions are rejected instead of registering a misleading binding. *(Added 2026-07-23 from the Tinycast analysis; running-state — its sibling — already shipped, see §1.)*
10. `[DONE]` **Archive Utility** — create, inspect, and safely extract ZIP, 7Z, TAR.ZST, TAR.GZ, and TAR.XZ archives locally. ZIP and 7Z support passwords; long operations show progress and cancel cleanly without promoting a partial archive.
11. `[Not Started]` **Licensing** — deliberately **last**, after everything else. Pattern LOCKED: **online activation + offline operation + grace window, never online-only** (§6). Payment SKUs (free + one-time personal + annual per-seat business) still not finalized.


### Deferred regressions to repair
- `[DONE]` **Document export fidelity:** Notes rich blocks now export as portable Markdown, readable HTML, and valid PDF output; resized images and soft line breaks are retained. Verified with focused Rust export regression tests.
---

## 3. v2 plans — remaining work

Everything below is post-v1. Items marked `[DONE]` were pulled forward; the rest are not started.

### AI (moved out of v1 entirely, 2026-07-23)
- `[Not Started]` **AI in Notes** (summarize / rewrite / continue / title / tag) — the first AI surface, once the model story is settled.
- `[Not Started]` **Model story** — local small model (download-on-demand, no egress) as default + optional BYOK power path. BYOK is gated behind the **Disclosure Firewall** (already ported, §1) so redaction happens before any egress. Local-only ⇒ firewall is dormant; it matters when a cloud/BYOK path exists.
- `[Not Started]` **"Ask your files" (private RAG)** — the flagship AI direction; RAG over the semantic vector store, answers from your files with citations, fully local on the small model.

### Video / media
- `[Not Started]` **Video Toolkit (full)** — merged ffmpeg utility surface (NOT an editor): convert (mov→mp4) · compress · trim (fast/precise) · extract-audio · →GIF. One drop-target + one dispatcher (mirrors `clipboard_actions`). Two-pass ABR compress as the accuracy upgrade over v1's single-pass. Design doc: `~/.gstack/projects/topuria-KeepItLocal/`.
- `[Not Started]` **A/V transcription index** — searchable transcripts (user-installed ffmpeg).

### Search / launcher
- `[Not Started]` **Open browser tabs in the palette** *(added 2026-07-29, from the vicinae comparison)* — search the tabs you currently have open, alongside bookmarks/history. **Cheapest item in v2: it is the shipped `browser_search.rs` feature pointed at a different file in the same profile folder**, reusing its three invariants (copy to temp, read-only, never write to a browser profile).
  - **Open tabs are ON DISK — no extension, no debug port, no network.** Verified 2026-07-29 on the owner's machine: Firefox `sessionstore-backups/recovery.jsonlz4` was **2.4 MB and rewritten minutes earlier** (Firefox flushes it ~every 15s, so it is near-live). It is mozlz4-wrapped JSON — windows → tabs → title + URL; `lz4_flex` is the only new dep.
  - **Ship Firefox first.** Chromium/Edge/Brave keep `Sessions/Session_*` + `Tabs_*` in **SNSS**, a pickled binary command log — parseable but materially harder. Firefox alone delivers most of the value.
  - **v1-of-the-feature = find, not switch.** Listing/searching is easy; *activating* a specific tab is the genuinely hard half (browsers expose no external "switch to tab"). So Enter opens the URL — which covers the real need, "I know I had that page open somewhere". True tab activation is a later upgrade behind an optional **native-messaging extension** (local IPC, and KeepItLocal **Guard already ships that architecture**).
  - ⛔ **This is NOT a local-first violation and must not be rejected as one** — it reads local files, exactly like bookmarks/history. See the §8 heading "READ THIS BEFORE ARGUING 'THAT BREAKS LOCAL-FIRST'"; this feature was already wrongly rejected once on that basis.
  - *Could be pulled into v1* if wanted — it is small and extends an already-shipped feature. Parked in v2 only because the v1 slate has already grown twice this session. **Owner's call.**
- `[Not Started]` **Own subsequence fuzzy matcher** — closes the `ntpd`→`notepad` gap (today is edit-distance-1 only). Bitmask prefilter + subsequence scoring, normalized to the bounded-component contract. Ship behind a cargo feature, default-off, A/B, then flip (VAD law). Source from fzf (MIT) / Sublime-fuzzy write-up — see §8 for what must NOT be used.
- `[DONE]` **Window switcher** *(vicinae)* — the palette Windows scope lists open windows and focuses the chosen one. It shares the same window machinery as per-app hotkeys.
- `[DONE]` **Emoji picker** *(vicinae)* — the palette includes a self-contained offline emoji dataset and copies the selected emoji.

### Connectors & sync (the local-first-compatible kind)
- `[Not Started]` Cloud connectors (Drive / email / Slack / GitHub / …) — read access to your own accounts; not telemetry.
- `[Not Started]` Send-to-device (LAN) · E2E sync.

### Automation & productivity
- `[Not Started]` **Automation engine + triggers** — a proper redesign (the current pack is hidden; the owner dislikes the reuse-tools approach). Cross-tool chains + triggers.
- `[DONE]` **Time Tracker redesign** — an interactive daily rhythm, activity flow, app/category summaries, and a clearer local tracking workflow.
- `[Not Started]` PKM / tasks · screen-region OCR · selection/clipboard AI.

---

## 4. Deliberately NOT doing (see `KeepItLocal.md` §8 for full reasons)

Quick pointers so they don't get re-proposed here:
- **MFT fast-path** — removed; needs admin (violates the no-UAC law).
- **whisper.cpp** — built, tested, deleted (slower than Vosk, RAM-heavy, unreliable start/stop).
- **A video _editor_ ("Video Studio")** — rejected; can't beat DaVinci/CapCut, fights the resource target. The Video Toolkit is utilities, not a timeline.
- **`nucleo` (MPL-2.0) / Zed's `fuzzy` (GPL-3.0)** — licence-rejected for the fuzzy matcher; clean-room from fzf/Sublime instead.
- **Georgian UI localization** — deleted. **CLIP image-content search** — skipped (OCR sufficient).
- **Online-only / per-launch licensing** — forbidden; breaks "works offline" (§6, ratified 2026-07-23).
