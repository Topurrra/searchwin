# Task 3 report — media input and player layout

## Root causes and changes

- FFprobe and FFmpeg inferred the input demuxer from file contents. With the installed 8.1.2 pack, a synthetic `ffconcat version 1.0` file named `disguised.mkv` followed a relative `segment.webm` in the same folder: the old probe returned `format_name: concat` and VP9 from the other file. A protocol whitelist of `file,pipe` alone permits that local segment. `MediaInput` now supplies the supported format from the extension plus `-protocol_whitelist file,pipe` before every input. `Player.Prepare` validates the format before cache or pack paths, and `Make` uses the shared policy for probe, remux/transcode, and sidecar subtitles. Subtitle demuxers are selected from `.srt`, `.ass`, `.ssa`, `.vtt`.
- The old player used `min-height: 100vh`, leaving the flex column free to grow around portrait video. Root's old AOT reproduction at 800×600 measured a 540 px content viewport, 1398.04 px video, and controls at y=1398.04–1450.84. The player now has a definite viewport height and hidden overflow; its stage and video can shrink, while controls cannot. The playlist scrolls inside that height.
- Direct files keep their source during the asynchronous `check`. No streaming path was added.

## Regression evidence

- New `RemuxTests.A_media_input_uses_its_expected_demuxer_before_ffmpeg_opens_it` failed all 5 rows on the old plan, then passed after the shared policy was used.
- Installed FFprobe: old command read the synthetic disguised playlist (`format_name: concat`); bounded `-f matroska` command reported `Invalid data found when processing input`; bounded probe still read the valid portrait WebM (`format_name: matroska,webm`, VP9). Installed FFmpeg converted a synthetic SRT to WebVTT with the sidecar flags.
- Focused Remux tests: 37 passed. Player page: 5 passed, including direct WebM source present while `check` remains pending.
- Full `Search.Kit.Tests`: 799 passed. Full Tools Vitest: 232 passed across 32 files. `Search/Search.csproj` build: succeeded with 0 warnings, 0 errors. `git diff --check`: clean.
- Svelte check has one existing error in `Tools/vite.config.js:8`: parameter `file` implicitly has type `any`; no player diagnostics.

## Remaining live verification

Root owns final Native AOT browser check with the portrait fixture, including short-window controls and fullscreen. JS DOM tests do not establish the rendered layout.
