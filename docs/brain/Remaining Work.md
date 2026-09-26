---
tags: [searchwin, roadmap, todo]
updated: 2026-09-27
---

# Remaining work after stabilization

Back to [[README]] · Direction: [[Master Plan]] · Evidence: [[Log/2026-09-26-stabilization]]

The first stabilization pass is merged into `main`. The full implementation
history and validation are in the stabilization log. This is the current
checklist; older logs describe the state at the time they were written.

## First: existing features and optimizations

- [ ] Reproduce and fix the live YouTube ad regression. The shield already
  handles both player and next responses; determine the actual failing path.
- [ ] Start incompatible movies while remuxing/transcoding, rather than after
  the whole file finishes. Cover seeking, cancellation, subtitles and cache limits.
- [ ] Measure direct playback and Range response latency on short and two-hour
  files. Direct playback already starts before its background codec check.
- [ ] Give Windows' permanent-delete confirmation a Search owner window.
- [ ] Coordinate engine Media Utility jobs with pack updates/removal. The new
  reservation covers browser Player jobs; engine jobs can still hold files open.
- [ ] Check focus and scroll during a real pack download. Progress now updates
  the existing Settings row; sidebar hover, grouping and settings search remain.
- [ ] Benchmark 100k-file search recall/latency, watcher exclusions, loaded-index
  idle memory and tool-tab suspension without interrupting active jobs.
- [ ] Finish real Explorer drag/drop, controlled-account password/import,
  camera/microphone and clean-machine checks.
- [ ] Review remaining lower-priority cleanup in [[Fixes]], including uninstall
  pack/play-cache cleanup and unused legacy UI code.

## Then: complete the promised integrations

- [ ] Windows OCR in Core for OCR and Document Scanner; optional PaddleOCR later.
- [ ] Finish Media Utility trim/convert and compression-to-size behavior.
- [ ] Restore Reminders only after Search owns scheduled activation, notification
  delivery and data persistence. Namespaces are now isolated; creation is disabled.
- [ ] Restore Screen Recorder after engine support and native capture controls.

## Later feature phases

1. **Phase 4:** opt-in background/tray lifecycle, hotkeys, snippets, Vosk voice,
   and Word ⇄ PDF through Word or LibreOffice.
2. **Phase 5:** page-to-Markdown, local/BYOK models, checked context sends and
   receipts, Ask Page, selected-tab comparison and video transcripts.
3. **Phase 6:** visible agent space, permissions and stop/takeover, opt-in memory,
   watchers, recipes and MCP. T3 Code requires a separate integration plan.
4. **Phase 7:** Store/MSIX distribution, complete Arm64 support, accessibility,
   migration/recovery and clean-machine release checks.

The available catalog now has **30 entries across eight categories**. Tool pages
are bundled; **FFmpeg is the only current downloadable runtime pack**. Additional
runtime packs are future work. Keep the C++ shell rewrite separate from these fixes.

The original detailed review plan remains in the code repository at
`docs/superpowers/plans/2026-09-26-remaining-roadmap.md`; its baseline numbers
predate the stabilization pass.
