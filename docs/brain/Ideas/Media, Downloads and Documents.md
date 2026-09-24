---
tags: [searchwin, ideas, media, installer, licensing]
updated: 2026-09-25
status: decided (2026-09-25)
---

# Media, downloads and documents, and what the installer carries

> **Decided 2026-09-25 (user):**
> - **No home-made converters.** The user spent a month on a pdfium-based
>   converter in Workspace, and its quality wasn't close to iLovePDF's.
>   Word ⇄ PDF uses **Microsoft Word, silently, when installed**, else LibreOffice.
>   Neither installed: Settings offers LibreOffice as a download.
> - **yt-dlp removed entirely.** Not now; Store rejection isn't worth it.
> - **FFmpeg:** LGPL build without x264/x265; Search stays MIT.
> - **NASM** is only for the build machine; users never need it (see §1).
>   Downloads from Settings are for packs (FFmpeg, voice models, OCR, LibreOffice…), the way Workspace does AI models.

Back to [[README]] · Related: [[Master Plan]], [[Build and Release]]

## 1 · Build machine vs installer: two different lists

**NASM is a build tool, not something users need.** It only lets `rav1e`
compile its assembly (faster AVIF encoding). Users get the faster code inside
`kil-engine.exe` without ever having NASM. So it belongs on the build machine
and in CI, not in the installer. `build.ps1` now switches it on by itself
when `nasm` is found (the `fast-avif` feature).

### Build machine (a bootstrap script / CI)
| Tool | Why | Licence |
|---|---|---|
| .NET 9 SDK | the browser | MIT |
| VS 2022 Build Tools (C++) | Native AOT link, C deps of the engine | Microsoft (build only) |
| Rust (stable, MSVC) | the engine | MIT/Apache |
| Node 22+ and pnpm | the tool pages | MIT |
| **NASM** | fast AVIF encoding in the engine | BSD-2 |
| NSIS 3 | the installer | zlib |
| Later: Vosk SDK, vcpkg + Tesseract | building the voice/OCR packs | Apache-2.0 |

### What a user's PC needs
- **WebView2**: the installer already brings Microsoft's bootstrapper.
- **The Visual C++ runtime: found missing and fixed (2026-09-25).**
  `kil-engine.exe` needed `VCRUNTIME140.dll`, which a clean Windows install
  doesn't always have, so the engine would silently not start there. It now
  links the C runtime statically (`Engine/.cargo/config.toml`, `+crt-static`),
  and nothing extra needs installing. `Search.exe` (Native AOT) only needs the
  Universal CRT, which Windows 10+ ships.

### Two installers
- **Search-Setup** (core, about 30–35 MB): browser, engine, tool pages. Packs download when you turn them on.
- **Search-Setup-Full** (offline): the same, with every pack inside, for PCs without internet or people who want it all at once.

| Pack | About | Licence | Notes |
|---|---|---|---|
| Voice | libvosk ~20 MB + English model ~40 MB | Apache-2.0 | more languages downloadable |
| OCR+ | Tesseract ~30 MB + tessdata ~23 MB | Apache-2.0 | Windows' own OCR stays in core |
| Semantic search | ONNX Runtime + MiniLM ~26 MB | MIT / Apache-2.0 | |
| Image AI | u2netp ~4.5 MB | Apache-2.0 (confirm) | background removal |
| PDF engine | pdfium ~7 MB | BSD-3 / Apache-2.0 | text extraction, rendering |
| **FFmpeg** | ~30 MB (LGPL build) or ~80–130 MB (full GPL build) | LGPL-2.1+ / GPL | see §2 |
| **yt-dlp** | ~18 MB | Unlicense (public domain) | see §3 |
| LibreOffice | ~350 MB | MPL-2.0 | too big to carry, so found on the PC or downloaded from The Document Foundation; see §4 |

(Sizes are approximate.)

---

## 2 · FFmpeg and playing video and music in the browser

### Licensing: Search stays MIT
- FFmpeg is **LGPL-2.1+**, and **GPL** only when built with GPL parts (x264, x265…).
- Search runs it as a **separate program** (`ffmpeg.exe`, over the command
  line). That's "mere aggregation": it doesn't make Search GPL or LGPL, and
  **Search doesn't need to change its licence.**
- What we must do is honour FFmpeg's own licence for the binary we ship:
  - ship its licence text;
  - say which build it is;
  - point to its exact source.
- Workspace's old rule, "never bundle FFmpeg", came from not wanting even
  those duties. With Search open source, carrying them is easy.
- **Codec patents** are a separate question from copyright. Shipping H.264/HEVC
  *encoders* (x264/x265) has patent implications in some countries, while
  decoding is widely done. The safe default is an **LGPL build without x264/x265**:
  it plays everything, and it encodes with AV1/VP9/Opus, plus H.264 through
  Windows' own Media Foundation encoder.

### What you'd get
- **Open any video or song in a tab:** from file search, drag-and-drop, "Open
  with Search", or `search://play?path=…`. It's a player page with
  playlist/folder playback, and it keeps playing in a background tab. Media
  keys and Windows' media overlay work through Chromium's MediaSession.
- **What plays directly** (WebView2 is Chromium): MP4 (H.264/AAC), WebM
  (VP8/VP9/AV1, Opus), MP3, AAC/M4A, FLAC, WAV, OGG. HEVC plays where the PC has
  the hardware and Windows' HEVC extension.
- **Everything else** (MKV with AC3/DTS, AVI, WMV, MOV/ProRes, FLV…): the
  engine runs FFmpeg to **remux on the fly** into fragmented MP4 (stream copy,
  near-instant, no quality loss). It re-encodes only the parts the browser
  can't decode (typically AC3/DTS audio to AAC/Opus), and transcodes video only when unavoidable.
- **Subtitles:** SRT/ASS/embedded tracks, converted to WebVTT by FFmpeg.
- **Needs first:** `files.search` has to answer **HTTP Range** requests
  (206), or seeking in a long video won't work. It's small, but it isn't there yet.
- Workspace's media tools (extract audio, compress video) become tool pages that use the same FFmpeg.

---

## 3 · yt-dlp: downloading videos

### Licensing: fine
yt-dlp is **public domain** (Unlicense).

### The real constraints
1. **Microsoft Store:** the Store has rejected and removed apps that
   download from YouTube, and its policies don't allow facilitating downloads
   that break a service's terms. **Expect a Store build that includes this to
   be rejected.**
2. **YouTube's terms** forbid downloading except through YouTube's own
   features. Many sites yt-dlp supports have no such rule.
3. **It breaks often:** sites change, and yt-dlp ships fixes every few days, so the
   pack must update itself. Its releases publish checksums and signatures,
   which we verify before anything replaces the exe.

### Recommended shape
- **An optional "Video downloads" pack**, off by default, in the
  **direct-download edition** (GitHub/installer). The **Store edition leaves it out.**
- **UX:**
  - on a page with a video, *Download video…* in the page menu, or `>download`;
  - choose video (best / 1080p / 720p) or audio only (M4A/MP3), and a folder;
  - progress shows in the Downloads panel.
- **FFmpeg** merges the best video and audio. That's another reason FFmpeg is a pack of its own.
- **Signed-in videos:** only when you ask, the browser exports that one site's
  cookies from its WebView2 profile into a temporary file for yt-dlp, and
  deletes it straight after.

---

## 4 · Word ⇄ PDF

### MuPDF would change Search's licence
**MuPDF is AGPL-3.0** (or a paid licence from Artifex). Bundling it means
**Search becomes AGPL**, which is stronger than GPL: anyone who ships a
modified Search, including as a network service, must publish their changes.
That's a real option for an open-source project, but it's a decision to make
deliberately for the whole product, not a detail. (PyMuPDF and pdf2docx sit on
MuPDF too.)

### An MIT path that's actually good
| Direction | Best available on this PC | Fallback, always there |
|---|---|---|
| **Word → PDF** | **Microsoft Word installed:** Word's own `ExportAsFixedFormat` (perfect fidelity, zero size). **Or LibreOffice installed/downloaded:** `soffice --headless --convert-to pdf` (very good, MPL-2.0) | The engine's existing DOCX → PDF renderer (Workspace's `docxide-pdf` path; fine for ordinary documents) |
| **PDF → Word** | **Microsoft Word installed:** Word opens PDFs itself (PDF Reflow), then saves as DOCX; good fidelity | **Our own converter** in the engine: pdfium (BSD) reads text runs, positions, fonts and images, and `docx-rs` writes paragraphs, headings, lists and simple tables. Good for text documents; complex layouts are hard for everyone |
| Page → PDF | WebView2 `PrintToPdfAsync` (built in) | |

**Recommendation:** stay MIT. Use Word or LibreOffice when present, and our
own pdfium-based converter otherwise. Revisit MuPDF only if we choose AGPL on purpose.

---

## 5 · Where these land in the phases
- **Phase 2:** local file search opens media and PDFs in tabs, which needs Range support in `files.search`.
- **Phase 3 (tools as pages, packs):**
  - the packs framework (download, verify, update);
  - the FFmpeg pack plus the player page;
  - Word ⇄ PDF;
  - the media tools.
- **Phase 3b:** the yt-dlp pack (direct-download edition only).
- **Phase 7:** two editions (Store without yt-dlp; direct with it); `Search-Setup-Full`.

## Decisions for the user
- [ ] yt-dlp: in the direct-download edition only (recommended), everywhere (risking the Store), or not at all?
- [ ] PDF ⇄ Word: stay MIT with Word/LibreOffice/pdfium (recommended), or relicense Search as AGPL to use MuPDF?
- [ ] FFmpeg build: LGPL without x264/x265 (recommended), or a full GPL build?
- [ ] Install NASM on this PC for release builds? (It isn't on PATH here and `winget` isn't available, so it's a download from nasm.us.)
