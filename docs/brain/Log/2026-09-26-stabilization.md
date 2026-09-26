---
tags: [searchwin, stabilization, verification]
updated: 2026-09-27
---

# Stabilization pass — 2026-09-26 to 2026-09-27

Branch: `codex/stabilization`, based on `904708d`. Runtime and release changes
through `50cdd8c`; later documentation records this session. This is the first
bounded fixes/optimization pass, not completion of the whole roadmap.

## Implemented

- Unsupported Screen Recorder and Reminders are absent from catalog and direct
  routes, with informative reasons. Their engine bridge operations and hidden
  hardening commands are refused. The current catalog has 30 tool entries.
- Cleaner/Analyzer reveals locations instead of shell-opening them. Duplicate
  Preview uses the browser's file route; that route now honors the FFmpeg pack.
- Scheduler operations use exact, separate Search namespaces derived from the
  engine data directory. Invalid reconciliation IDs fail closed. Legacy
  KeepItLocal tasks are untouched. Reminder creation is unavailable until native
  activation and notification delivery exist.
- Player inputs use explicit supported demuxers and local protocol restrictions,
  including probing and subtitles. A disguised concat playlist is rejected.
- Portrait video shrinks inside the viewport so controls stay reachable. Direct
  playback still starts before the background codec check finishes.
- Settings updates pack progress text in place. Licence/source links are visible
  before installation. Bare `>ffmpeg` opens pack information; `>install ffmpeg`
  remains the deliberate download command.
- Explicit FFmpeg selection wins over pack/PATH discovery. Pack repair and
  interrupted-removal cleanup are covered. Player job admission and pack changes
  share a gate. Removal distinguishes blocked rename from deferred cleanup after
  logical removal; incomplete deletion is reported honestly.
- CI requires engine/tools checks, frozen dependency installation, tests and
  builds. Release preflight validates SDK/toolchains and safe local output before
  cleanup. Packaging reads the current Cargo executable artifact and validates
  x64 PE format rather than copying a possibly stale default-path binary.
- pnpm 9.15.9 is pinned; workspace/lock configuration now installs frozen without
  changing dependency versions. The field smoke script leaves clipboard access
  behind explicit `-IncludeClipboard` opt-in.

## Verification

| Check | Result |
|---|---|
| Search.Kit | 806 passed |
| Rust, no default features, normal Windows context | 390 passed, 4 ignored |
| Tools Vitest | 232 passed |
| Browser JavaScript | 125 passed |
| Tools type check | 0 errors, 0 warnings |
| Complete `build.ps1` pipeline with pinned pnpm | Passed; Native AOT app, current engine, 30-tool catalog |
| Packaged AOT field smoke | 17 checked, 2 clipboard checks skipped, 0 failed |
| Independent reviews | Each work package reviewed; final whole-branch review and scoped fix review passed |

Runnable folder: `build/Search` (about 105 MB uncompressed). `Search.exe` is
20,989,440 bytes; `kil-engine.exe` is 22,431,744 bytes. This session did not
produce a new NSIS installer.

AOT UI checks used fresh SEARCH_PROBE worlds with clipboard recording disabled:
- Old portrait layout: controls began at y=1398 in a 540px content viewport.
  Updated layout: controls occupied y=487.2..540.
- Synthetic MKV with AC-3 audio remuxed to playable MP4; SRT captions rendered;
  seeking reached 1s; fullscreen controls stayed at the viewport bottom.
- At the supported 512px window minimum, playlist and captions were present and
  every player button remained hit-testable. The compact row can extend beneath
  the playlist; that existing visual polish issue does not hide controls.
- Bare `>ffmpeg` selected the information action, opened Settings and created no
  pack download. The Install, Licence, Build and Source controls were visible.
- The built Screen Recorder direct route displayed its unavailability reason.

The full engine suite needs the normal Windows DPAPI context: six tests fail in
the restricted sandbox with 0x80070002, and pass in the same normal context used
for the baseline. Existing optional-feature/dead-code warnings remain.

Automatic approval review rejected temporarily restoring the old pack-first
FFmpeg resolver solely to demonstrate the regression test's pre-fix failure,
citing the risk of invoking the wrong executable. That action was not retried.
The corrected resolver test passes; its pre-fix failure remains unverified.

## Remaining work

- Reproduce the live YouTube ad case; a reliable URL/sign-in context was requested.
- Incremental playback while remuxing/transcoding (the player still waits for a
  complete converted file), probe/range latency and large-movie measurements.
- Give the native permanent-delete confirmation a Search owner window.
- Coordinate engine Media Utility jobs with pack mutations; the current gate
  covers browser Player jobs. Leftover files may need deferred cleanup.
- Live download focus/scroll check, real Task Scheduler registration, Explorer
  drag/drop, larger index/resource benchmarks and broader Settings hover/search
  polish remain unverified or unfinished. No real scheduled tasks were used.
- Restore Reminders/Recorder only with working native lifecycle; finish Windows
  OCR before optional PaddleOCR, then proceed to later feature phases.

The original [[Fixes]] and [[Optimization]] notes contain the broader backlog.
