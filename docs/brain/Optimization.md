---
tags: [searchwin, optimization, todo]
updated: 2026-09-27
---

# Optimization: what we have now

Back to [[README]] · Related: [[Fixes]], [[Master Plan]]

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
