---
tags: [searchwin, tools, inventory]
updated: 2026-09-27
---

# Tools: Inventory and Status

Back to [[README]] · Phase: [[Master Plan#Phases]] · Packs: [[Packs]]

The current catalog has **30 available tool entries across eight categories**.
Tool pages are bundled, at `https://tools.search/index.html#/tool/<id>`.
FFmpeg is the only current downloadable runtime pack. The older 32-entry
catalog included unsupported Recorder and Reminders routes; both are now
withheld. See [[Log/2026-09-26-stabilization]] for the current verification.

## Packs

### Utilities (7 tools) — always available, client-side

| Tool | Status | What it does |
|---|---|---|
| **Hash Check** | ✅ | MD5, SHA1, SHA256 of text or file |
| **Encoders** | ✅ | Base64, hex, URL, Unicode encode/decode |
| **QR Code** | ✅ | QR code generator (text → PNG data URI) |
| **Format Converter** | ✅ | JSON ↔ YAML / TOML / XML |
| **Password Generator** | ✅ | Random passwords with entropy display |
| **Color Picker** | ✅ | Hex / HSL / RGB conversions |
| **Util Calculator** | ✅ | Basic math expressions |

### Files (6 tools) — file manager, archive, cleaner, deduper, renamer, diff

| Tool | Status | What it does |
|---|---|---|
| **File Manager** | ✅ | Browse, open (in tab or app), show in Explorer, copy/move/rename/delete |
| **Archive Utility** | ✅ | Create/extract ZIP, 7Z (uses engine commands) |
| **Bulk Rename** | ✅ | Find/replace on filenames, undo, preview |
| **Duplicate Finder** | ✅ | Scan for identical files, move or delete, undo |
| **Cleaner / Analyzer** | ✅ | Read-only disk analysis; destructive clean behind confirm |
| **Folder Diff & Merge** | ✅ | Compare two folders, copy missing files, mirror (with confirm) |

### Privacy (3 tools) — audit, shred, redact

| Tool | Status | What it does |
|---|---|---|
| **Privacy Audit** | ✅ | Read-only scan: mic/camera access, startup programs, hosts file, scheduled tasks |
| **File Shredder** | ✅ | Secure file/folder delete (after SHRED confirm); progress tracked |
| **Screenshot Redact** | ✅ | Draw region, export without original |

### Images (5 tools) — studio, OCR, watermark, favicon

| Tool | Status | What it does |
|---|---|---|
| **Image Studio** | ✅ | Resize, rotate, crop, convert format, metadata strip |
| **OCR** (Image to Text) | ⚠️ | Extract text from images; needs Tesseract (not wired to pack yet, shell-out only) |
| **Document Scanner** | ⚠️ | Scan image as document with perspective fix; also needs Tesseract |
| **Image Watermark** | ✅ | Add text/image overlay to photos |
| **Favicon Generator** | ✅ | Generate favicon from image (PNG, ICO, Apple touch) |

### Documents (2 available tools) — CSV and Markdown; hub panels are not separate tools

| Tool | Status | What it does |
|---|---|---|
| **CSV Toolkit** | ✅ | Hub: Convert / Merge / Clean / Split / JSON converters (5 panels) |
| **Markdown Converter** | ✅ | Render Markdown, export to HTML / PDF / DOCX (uses engine's renderer, NOT Word) |
| **Word Converter** | ❌ | Word ⇄ PDF (deferred to phase 4, D33) — hidden from index |
| **Remove Password** | ❌ | PDF/Office password removal — hidden from index (deferred) |

### Development (4 tools) — includes the eight-panel Developer Tools hub

| Tool | Status | What it does |
|---|---|---|
| **Dev Toolkit** | ✅ | Hub: 8 panels (JWT / Regex / SQL / Diff / ID Generator / Fake Data / Cron / Secret Scanner) |
| **Encrypt / Decrypt** | ✅ | AES-256-GCM text encryption (+ file input) |
| **SSH Key Manager** | ✅ | List, generate, import keys from `~/.ssh` (read-only in UI) |
| **Image to Base64** | ✅ | Convert image to data URI |

### Media (1 available tool) — Media Utility; player uses a separate file route

| Tool | Status | What it does |
|---|---|---|
| **Media Utility** | ✅ | Extract audio to MP3, compress video to size (gates on FFmpeg pack) |
| **Screen Recorder** | ❌ | Excluded from catalog and direct tool route; engine bridge operations refused until native capture controls work |
| (Player) | ✅ | Play MKV / AVI / WMV / FLV / 3GP / WMA with remux, subtitles, range serving (pack-gated; wired via file action route) |

### Focus (2 available tools) — time tracker and focus mode

| Tool | Status | What it does |
|---|---|---|
| **Reminders** | ❌ | Withheld. Scheduler namespaces are isolated, but native activation and notification delivery still need implementation |
| **Time Tracker** | ✅ | Track time on current window, export report |
| **Focus Mode** | ✅ | Block distracting sites, notifications, apps (read-only demo) |

## Deferred and Cut

| id | Why | Phase |
|---|---|---|
| `word-converter` | Word ⇄ PDF (with Word silent or LibreOffice) → phase 4 | D33, phase 4 |
| `snippets` | Text expansion with global hotkeys → phase 4 | Phase 4 |
| `voice-to-text` | Vosk model pack + voice input → phase 4 | Phase 4 |
| `automation-recipes` | Agent recipes, not a tool — no link yet | Phase 6 |
| `notes` | Waits for Search's side-panel Notes | Side panel (phase 5?) |
| `windows-hardening` | Loses to free tools (ShutUp10++, O&O); too narrow scope | Deferred |
| `doc-password` | PDF/Office password removal; doc processing priority unclear | Deferred |
| `screen-recorder` | Needs native capture controls and the engine feature | Phase 4+; hidden and guarded |
| `reminders` | Needs Search-owned activation, notification and persistence | Hidden and guarded |
| `file-search` | Duplicates field's native search (Ctrl+L, same commands); moved to field-settings | Cut (field does it better) |

## How to hide a tool

1. Edit `Tools/src/lib/offInSearch.ts`, add a line:
   ```
   '<id>': '<user-facing reason>',
   ```
2. Run `pnpm build` (rebuilds catalog.json).
3. Test: `pnpm vitest run` in Tools (index.test.ts asserts it's gone), then `dotnet test` and live in a SEARCH_PROBE world.

## How to add a tool

1. Create `Tools/src/lib/tools/<Pack>/<Name>.svelte`.
2. Add to `Tools/src/lib/searchTools.ts`, `toolsByPack[<packId>]`.
3. Run `pnpm build` (catalog.json auto-updated).
4. If the tool needs an engine command, register it in `Engine/src/lib.rs` `generate_handler!`.
5. Test in a SEARCH_PROBE world: field completion, tools index, tool page.

## Known issues in shipped tools

| Tool | Issue | Workaround |
|---|---|---|
| OcrTool, CamScanner | Shell-out Tesseract only; no Windows-OCR fallback, no pack wiring | Not usable without `tesseract.exe` on PATH |
| Reminders, Screen Recorder | Native lifecycle is incomplete | Routes remain unavailable |
| Player | Whole-file conversion delays incompatible media | Incremental playback remains planned |
| File Manager | Permanent-delete confirmation lacks a Search owner window | Pending fix |
| (Full list) | See [[Remaining Work]] and [[Log/2026-09-26-stabilization]] | — |

## Component locations

```
Tools/
  src/lib/
    searchTools.ts           # catalog (toolsByPack, toolsByPack, openTool)
    offInSearch.ts           # visibility gate (add lines here to hide)
    toolIcon.ts              # per-tool favicon (Lucide SVG)
    tools/
      <Pack>/
        <Name>.svelte        # each tool page
  src/routes/
    tool/[id]/+page.svelte   # tool frame (title, favicon, error boundary)
    +page.svelte             # tools index (search://tools)
  dist/
    catalog.json             # built by pnpm build (33 tools base, minus cut ids)
    catalog.js               # build script (reads searchTools, writes catalog.json)
```

See [[Log/2026-09-27#Builder B]] for restyling and token details.
