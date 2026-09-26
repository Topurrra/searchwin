---
tags: [searchwin, optimization, todo]
updated: 2026-09-27
---

# Optimization: what we have now

Back to [[README]] · Related: [[Fixes]], [[Master Plan]]

## Stabilization status — 2026-09-27

Merged fixes and measurements: [[Log/2026-09-26-stabilization]]. Player controls
now fit the viewport, including portrait video and minimum-width checks. Pack
progress updates the existing Settings row. Direct media already plays while
the codec check runs; remuxed/transcoded media still waits for complete output.
The YouTube shield already handles both `/player` and `/next`; the live ad cause
is not established. The original hypotheses below are historical, not confirmed
diagnoses. The current outstanding checklist is [[Remaining Work]].

**Rule:** this note covers what exists today (up to phase 3). Each future
feature or phase gets its own optimization note (for example
`Optimization - Voice.md`), linked from here and from [[README]].

## From the user (2026-09-27)
1. **Opening a movie is slow; Workspace's HTML player starts at once.**
   - Search's player (`Tools/src/routes/play`, `Search/Core/Player.cs`,
     `Search.Kit/Media/Remux.cs`) runs ffprobe first and, for formats
     WebView2 can't play, remuxes or transcodes the **whole file** into a
     `+faststart` MP4 in `Cache\Play` before playing, behind a loading screen.
   - Workspace's player just points a `<video>` at the file.
   - To do:
     - Formats WebView2 plays (MP4/H.264, WebM, MP3, M4A, FLAC, WAV, OGG)
       must start at once. Check that no probe or loading screen runs for
       them, and that `files.search` answers the first Range request quickly.
     - For the rest, play while converting: fragmented MP4 through Media
       Source Extensions, or FFmpeg piping fMP4 that is served as it's
       written. Seeking into parts not yet written restarts FFmpeg with `-ss`.
     - Cache the probe result per file (path + size + mtime).
     - Measure: time to first frame, direct and remuxed, for a 2-hour MKV.
   - **Controls hidden outside full screen:** when a movie isn't in full
     screen, the player's buttons and controls (play/pause, seek bar, volume,
     next/previous, subtitles) aren't visible. The video should fit the tab
     with the controls always reachable, as in Workspace's player; check small
     windows and tall videos too.
2. **Settings is weaker than Workspace's settings.**
   - Moving the mouse over the Settings sidebar gives an odd highlight
     animation. The field's result list hover looks good: use the same hover
     behaviour and timing for the sidebar (`Search/UI/Panels/SettingsPanel.cs`
     rail, compare `Search/UI/Omnibox.cs` rows).
   - Review Workspace's Settings screen for layout, grouping, search within
     settings and descriptions, and bring over what's better.
   - During a pack download Settings rebuilds 4×/s (see [[Fixes]] #10).

3. **YouTube ads get through (user screenshot, 2026-09-27).** A video
   started with a pre-roll ad pair: "Ad 2 of 2", an advertiser card
   (pc.stateofsurvival.game, "Play now") and the yellow ad progress bar.
   Phase 1 checked that the player data had no ads, so YouTube has found
   another way, or the shield misses a path. Suspects:
   - `Search/Assets/js/youtube-shield.js` only cleans some responses:
     `ytInitialPlayerResponse` and the `/youtubei/v1/player` fetch/XHR. Check
     `/youtubei/v1/next`, `get_watch`, the service-worker path, and in-page
     navigation (yt-navigate) after the first video.
   - Server-side ad insertion (ads stitched into the stream, SABR). Then
     cleaning the JSON isn't enough: skip ad segments by time (the
     `adSlots`/`adPlacements` offsets), or speed through and mute them as
     uBlock Origin's scriptlets do.
   - The script injected too late on some loads (`AddScriptToExecuteOnDocumentCreatedAsync`
     timing in new tabs, or a tab restored from the session).
   - What to do: reproduce in a test world with the same video, log which
     response carried the ads, add a node test with that response's shape
     (must fail today), and compare with uBlock Origin's current YouTube
     filters and scriptlets. Treat as a protection regression, not polish
     (listed in [[Fixes]] too).

## Known from the phase 2–3 reviews
- **Engine walk and watcher:** hidden and excluded folders skipped at the
  walker (done); watchers don't read `.gitignore`.
- **Filename search:** the prefix pass reads a small page and scores flat;
  on very large indexes check `note` → notebook-style results stay found.
- **Content search:** the half-of-typed-words rule reads each candidate;
  measure on a 100k-file index.
- **Engine start:** about 12–20 MB when clipboard history starts the engine
  3 s after the first window; check idle memory stays under the 60 MB
  budget with the index loaded.
- **Tool pages:** each tool tab is a WebView2 page. Check memory per tool
  tab, and whether idle tool tabs should be suspended like web tabs.
- **Clipboard popup:** filtering is off the UI thread with a 4 KB prefix;
  thumbnails cached by id.
- **Installer:** core target ≤ 45 MB. The FFmpeg pack is 71 MB download /
  144 MB unpacked; ffplay, docs and headers are already left out.

## Budgets to keep
First window ≤ 300 ms · keystroke in the field < 5 ms · engine idle < 60 MB
· movie first frame: direct < 300 ms, remuxed < 1 s (target).
