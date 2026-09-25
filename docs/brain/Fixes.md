---
tags: [searchwin, fixes, todo]
updated: 2026-09-27
---

# Fixes: phase 3 follow-ups (paused 2026-09-27)

Back to [[README]] · Found by the phase 3 review and check ([[Log/2026-09-27]]).
Paused on the usage limit before the fix round started. `main` is at `c7001c7`,
5 commits ahead of GitHub (not pushed).

## Ready to use
- Worktrees `C:\Users\user\Projects\searchwin-wt\safety` (branch `p3-safety`)
  and `...\packs2` (branch `p3-packs2`), both at `c7001c7`, with
  `Engine\target\debug` copied in from main (warm engine build: 36 s instead
  of 3–5 min). Delete with `git worktree remove` if no longer needed.
- Speed-ups for the next workflow: seed each worktree's engine cache
  (robocopy `Engine\target\debug`, skip `incremental`); Sonnet at medium
  effort for routine fixes, Opus for design, security and review; only the
  final check publishes the AOT build.

## Blockers (high)
1. **Screen Recorder is back in the tools list.** `Tools/src/lib/appScreens.ts:834`
   sets `hidden` from `offInSearch` and overwrites A's `hidden: true` (line 415).
   Fix: add `'screen-recorder'` to `Tools/src/lib/offInSearch.ts`, delete the
   dead `hidden: true`, and assert in `Tools/src/routes/index.test.ts` that
   `#/tool/screen-recorder` isn't listed (fails today).
2. **Opening Reminders deletes scheduled tasks.** `Engine/src/commands/reminders.rs:123`
   `reconcile_reminder_tasks` unregisters everything under `\KeepItLocal\*`,
   which includes `\KeepItLocal\Cron\` (Dev Toolkit's Cron panel) and real
   Workspace reminders, from any world. Fix: match the exact folder
   (`$_.TaskPath -eq ...`), move Search's tasks to a per-world folder
   (`\Search\<world>\Reminders\`), a pure-fn test that a Cron task isn't
   selected. **Until fixed, don't open Reminders.**

## Medium
3. Cleaner/Analyzer "Open location" (`CleanerAnalyzer.svelte:685`) and
   Duplicate Preview (`DuplicatePreview.svelte:137`) call
   `open_search_result_path`, which runs .exe/.msi/.bat and mounts .iso. Use
   reveal and `host:open.file`; retire `open_search_result_path` from tool pages.
4. Hidden tools' engine commands stay reachable from any tool page
   (`apply_hardening_tweak`, `revert_*`, `reconcile_reminder_tasks`,
   screenrec). Refuse them in `Search.Kit/Web/ToolCalls.cs` with a Kit test.
5. `FOF_WANTNUKEWARNING` (`Engine/src/commands/file_manager.rs:830`): Windows'
   "delete permanently?" prompt comes from the headless engine with no owner
   window and can open behind Search. Pass Search's HWND (`SetOwnerWindow`)
   or pre-check recyclability and let the page ask.
6. FFmpeg: add `-protocol_whitelist file,pipe` before every `-i`
   (`Search.Kit/Media/Remux.cs:53`, `Search/Core/Player.cs:117,141`); force
   the sidecar subtitle demuxer from its extension. RemuxTests row.
7. **OCR:** OcrTool and CamScanner only know Tesseract. The plan says Windows
   OCR in core: add a host call using `Windows.Media.Ocr`, Tesseract later as
   a pack (`Engine/src/core/packs.rs` `bin_dir()` is ready).

## Low
8. `Engine/src/commands/ffmpeg.rs:96`: the user's own ffmpeg.exe should win
   over the pack.
9. `Search.Kit/Packs/PackStore.cs:96`: sweep root-level `.removed-*`; call
   `Player.Forget` before an update swap; test the repair path.
10. `Search/Core/Packs.cs:59`: during a download Settings rebuilds 4×/s
    (focus and scroll lost). Update only the pack line.
11. `Search/Core/Browser.Field.cs:226`: `OpenLocal` should use
    `FileKinds.Opening` (MKV from File Manager → player when the pack is in).
12. `SettingsPanel.cs:686`: show the licence link before install; fix
    FFmpeg-NOTICE's tag naming.
13. `Commands.cs:59`: `>ffmpeg` alone starts a 71 MB download; keep only
    "install ffmpeg".
14. Dead code: `helperPages` 'file-search-index', `TopBar/FileSearchIndex.svelte`,
    `TopBar/Settings.svelte.bak`, stores used only by them.
15. `Installer/Search.nsi:197`: uninstall should remove
    `%LOCALAPPDATA%\Search\Packs` and `Cache\Play`.

## Also open
- **Live AOT check never ran** (the verify agent skipped it): FFmpeg install
  from Settings › Packs, MKV + AC3 + SRT in the player with seeking, Media
  Utility, every tool opening without errors, bench-field 21/21, first
  window and keystroke timings.
- Media Utility: trim/convert not built; compress-to-size overshoots at low
  targets. Player converts the whole file before playing (no MSE streaming).
- Screen Recorder needs a `screenrec` engine build and native overlays.
- Drag-and-drop from Explorer not tested with a real drag.
- Notes' phase 3 status says ✅ done: correct it to "done, with fixes open"
  after this list is finished.

## Then
Fix round → merge → live AOT check → review → notes → push → installer.
