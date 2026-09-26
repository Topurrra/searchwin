---
tags: [searchwin, packs, framework]
updated: 2026-09-27
---

# Packs: Framework and Details

Back to [[README]] · Tools: [[Tools]] · FFmpeg: [[#FFmpeg pack specifications]]

**Packs** are optional downloadable components that ship Search with extended features: tools, media codecs, OCR models, voice recognition, semantic search. Each pack is independently verified, installed, updated and removed through **Settings › Packs**.

**Current implementation:** FFmpeg is the only downloadable runtime pack in the
manifest. The eight tool categories are bundled pages. Voice, OCR and other
runtime packs are future work; see [[Remaining Work]].

## How packs work

### Manifest (`packs.json`)

Embedded in the Kit at build time (`Search.Kit/Packs/packs.json`):

```json
{
  "packs": [
    {
      "id": "ffmpeg",
      "name": "FFmpeg",
      "version": "8.1.2-50-g1a748fe2cd",
      "size": 70835150,
      "sha256": "e9712ffbdb03ef71bbab660c75b835bfe698ef6fad0247c76d8d394a39a3db63",
      "url": "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-08-31-13-27/ffmpeg-n8.1.2-50-g1a748fe2cd-win64-lgpl-shared-8.1.zip",
      "licence": "LGPL-3.0-or-later",
      "licenceUrl": "https://github.com/FFmpeg/FFmpeg/blob/1a748fe2cd43e3ead22fafb1b5b7d77f153898a8/COPYING.LGPLv3",
      "build": "BtbN/FFmpeg-Builds, win64 LGPL shared build of 2026-08-31: no x264, x265 or other GPL parts",
      "buildPage": "https://github.com/BtbN/FFmpeg-Builds/releases/tag/autobuild-2026-08-31-13-27",
      "source": "https://github.com/FFmpeg/FFmpeg/commit/1a748fe2cd43e3ead22fafb1b5b7d77f153898a8",
      "enables": "Plays MKV, AVI, WMV and more in the player, with their subtitles, and runs Media Utility",
      "folder": "ffmpeg-n8.1.2-50-g1a748fe2cd-win64-lgpl-shared-8.1",
      "files": [
        "bin/ffmpeg.exe",
        "bin/ffprobe.exe",
        "bin/avcodec-62.dll",
        "bin/avdevice-62.dll",
        "bin/avfilter-11.dll",
        "bin/avformat-62.dll",
        "bin/avutil-60.dll",
        "bin/swresample-6.dll",
        "bin/swscale-9.dll",
        "LICENSE.txt"
      ]
    }
  ]
}
```

Each entry specifies:
- `version`: semantic version (not the URL; the manifest is authoritative).
- `size`: byte count (checked during download).
- `sha256`: verified during download, before unpacking.
- `url`: https only; direct link to release archive.
- `licenceUrl`, `buildPage`, `source`: transparency; visible in Settings › Packs.
- `files`: only these are extracted; anything else in the archive is skipped. Paths checked to stay inside the pack folder.

### Download and verify

**Search/Core/Packs.cs, PackStore.cs:**

1. `HttpClient` streams the download.
2. **While streaming:** hash computed incrementally.
3. **Before unpacking:** size and SHA-256 checked against manifest. If either fails, download refused, old version (if any) stays intact.
4. **Staging:** files extracted to `Packs/<id>/.unpack-<nonce>` (dot-prefixed, hidden).
5. **Atomic move:** staged folder renamed to `Packs/<id>/<version>`. If this fails (e.g., folder in use), old version stays and new one is rolled back.
6. **Old version:** stays until new one is in place; removed only after successful move.

### Update

Call `PackStore.InstallAsync` with the same id:
1. If version is newer: download new one, atomic move.
2. If same version: rename-and-restore repair (old copy comes back on move failure).
3. Versions are selected by Search's embedded manifest; there is no arbitrary
   version picker or general semantic-version downgrade check in PackStore.

### Remove

1. Folder renamed to `.removed-<id>-<nonce>` (dot-prefixed, quarantined).
2. Deletion is attempted after the rename. A failed rename preserves the install;
   successful rename with failed deletion returns CleanupPending and reports the
   leftover files while the pack is logically removed.
3. **Crash cleanup:** Sweep looks for `.removed-*` at start of next Install/Remove.

Root-level removal leftovers are now retried on later pack operations. Browser
Player jobs and pack mutations share a reservation; engine Media Utility jobs
still need broader coordination. See [[Log/2026-09-26-stabilization]].

### Engine integration

**Engine/src/core/packs.rs, lib.rs:**

```rust
pub fn bin_dir(pack_id: &str) -> Option<String> {
  // Reads Engine\packs.json (written by Search), returns the pack's bin dir
}
```

No new command; the engine reads the file on each lookup, so:
- Installed while engine running → immediately available.
- No race on pipe connect.
- Other runtime packs can use the same lookup pattern; PaddleOCR is the chosen optional OCR pack.

**Engine/src/commands/ffmpeg.rs resolve():**
```
check runnable explicit user override → pack → PATH
```

**Report:** `ffmpeg_status` tells tools whether ffmpeg is available and its source (`user`, `pack`, or `path`).

### UI: Settings › Packs

**Search/UI/Panels/SettingsPanel.cs, Search/Core/Packs.cs:**

- New rail item (Package icon) between Downloads and Privacy.
- Each pack row: name, version, "what it enables" (one line).
- **Installed:** version shown, Update or Remove button (if update available). Licence, Build and Source links are also visible before install; Licence uses the manifest's pinned URL. Progress changes the existing row text, with a Cancel button, rather than rebuilding Settings on every tick.
- **Not installed:** Install button. "Get <Pack>" (or disabled if already installing elsewhere).
- Note: "Packs download only when you click. We check them with SHA-256." No background update checks.

### Field integration

**Search/Core/Commands.cs:**
- `packs` → opens Settings › Packs.
- `install ffmpeg` → opens Settings › Packs and starts FFmpeg install.
- Plain `>ffmpeg` opens pack information without starting a download.

## FFmpeg pack specifications

**Used by:** Player (remux/transcode/subtitle extraction), Media Utility (extract audio, compress video).

### Build details

| | |
|---|---|
| **Source** | BtbN Autobuild ([github.com/BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds)) |
| **Build date** | 2026-08-31-13-27 (month-end autobuild, kept long-term) |
| **Tag** | `autobuild-2026-08-31-13-27` |
| **Exact commit** | `1a748fe2cd43e3ead22fafb1b5b7d77f153898a8` (GitHub verified) |
| **Configuration** | `--enable-version3` (LGPL 3.0-or-later; includes patented codecs) |
| **Platform** | win64 (64-bit Windows) |
| **Build type** | lgpl-shared (shared libs; no static linking required) |

### Contents

**Included:**
- `ffmpeg.exe` — codec and container operations.
- `ffplay.exe` — test player (not used by Search).
- `ffprobe.exe` — stream inspection.
- Helper libraries (ffmpeg.dll, swscale.dll, etc.).

**Excluded (to save space):**
- Static headers, docs, examples.
- x264/x265 (video encoders, would add ~50 MB; using h264_mf instead).

**Codecs included:**
- Video: H.264 (h264_mf, Windows hardware encoder), HEVC, VP8, VP9, AV1.
- Audio: AAC, MP3, Opus, FLAC.

**Player uses:**
- H.264 / HEVC / VP8 / VP9 / AV1 video: **copied** (no re-encode).
- AAC / MP3 / Opus / FLAC audio: **copied**.
- Other video: converted to H.264 via `h264_mf` (hardware-accelerated).
- Other audio: converted to AAC, downmixed to stereo if >2 channels.

### Licence

**LGPL 3.0-or-later** (multiple files, see LICENSE.txt in the pack).

**Obligations:**
1. Distribute the source or a written offer to provide it on request (Search is open source on GitHub).
2. Allow users to replace LGPL components (not applicable here; we don't redistribute libs separately).
3. Include the licence text (visible in Settings › Packs, link to online copy).

**Fulfilment:**
- `third_party/FFmpeg-NOTICE.md` cites the build, date, source commit, and licence.
- PROVENANCE.md updated with the exact source and commit.
- Settings › Packs shows "LGPL-3.0-or-later" and links to the build's own COPYING.LGPLv3.

### Size and performance

| Metric | Value |
|---|---|
| Download | 70.8 MB |
| Installed | 144 MB (after unpacking; docs and ffplay excluded) |
| Remux (1 min video, codec copy) | 0.7–0.8 seconds |
| Transcode (mpeg4 → h264_mf) | ~0.2 seconds per 20 seconds of video |

### Why this build?

- **BtbN's month-end autobuild is kept long-term** (~6 months). Daily builds are deleted after ~2 weeks, causing broken links.
- **LGPL (not GPL or AGPL):** allows redistribution in closed products.
- **Shared build (not static):** smaller download and install footprint.
- **No x264/x265:** saves space; h264_mf (hardware encoder) is built into Windows.
- **Exact commit pinned:** ensures reproducibility; the source is always available on GitHub.

### Updating FFmpeg

If a newer BtbN month-end build is available:

1. Find the release at [github.com/BtbN/FFmpeg-Builds/releases](https://github.com/BtbN/FFmpeg-Builds/releases).
2. Copy the checksums.sha256 file URL and the release's archive URL.
3. Verify the commit hash in the build filename or metadata.
4. Update `Search.Kit/Packs/packs.json`:
   - `version` to the new FFmpeg version (e.g., "8.2.0").
   - `size`, `sha256` from checksums.sha256.
   - `url` to the archive.
   - `source` with the new commit hash.
5. Update `third_party/FFmpeg-NOTICE.md` with the new details.
6. Run `dotnet test` + `pnpm build` + `cargo test` + live verification in a SEARCH_PROBE world.
7. Commit and push.

**Note:** Search's updates don't force a pack re-download; installed versions stay until the user removes them. New Search installs get the new manifest version.

## Future packs

**Planned (not yet wired):**
- **Vosk:** speech recognition model (phase 4).
- **PaddleOCR:** optional OCR runtime/model; Windows OCR is planned for Core.
- **pdfium:** PDF rendering for documents.
- **MiniLM:** semantic search embeddings.
- **Background Removal:** image processing model.

Each will follow the same manifest + verify + atomic-install pattern.

## Component locations

```
Search.Kit/Packs/
  packs.json                 # manifest (embedded at build time)
  Pack.cs, PackStore.cs      # download, verify, install, remove, repair
  PackTests.cs               # 15 test cases
Search/Core/
  Packs.cs                   # UI / Settings integration
  SettingsPanel.cs           # the Settings › Packs page
  Player.cs                  # uses ffmpeg from pack or override
Engine/src/core/
  packs.rs                   # reads Engine\packs.json, bin_dir()
  ffmpeg.rs                  # resolve() checks pack first
Tools/src/routes/
  play/                      # player page (uses ffmpeg when pack installed)
third_party/
  FFmpeg-NOTICE.md           # build details, licence, source
  PROVENANCE.md              # pack sources and commits
```

See [[Log/2026-09-27#Builder A]] for implementation details, tests, and measurements.
