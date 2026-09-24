---
tags: [searchwin, index]
updated: 2026-09-24
---

# SearchWin — second brain

**Search for Windows**: the Windows port of *Search*, Office Commun's small,
fast, quiet macOS browser. Rewritten in **C# + WinUI 3 + WebView2** to look and
behave like the Mac app, with the same UI, keys and data.

- Code: `C:\Users\user\Projects\Search\searchwin` (branch `main`, GitHub: https://github.com/Topurrra/searchwin)
- Mac original: `C:\Users\user\Projects\Search`. It's the roadmap: **read it, never change it.**
- Installer: `searchwin\build\Search-Setup-1.0.0-x64.exe`

## Start here

| Note | What's in it |
|---|---|
| **[[Master Plan]]** | **Where Search is going: browser + Workspace engine + tools as pages + agent, in phases** |
| **[[Cloud Agent Brief]]** | **Tasks T0–T8 and rules for a cloud Claude agent (Linux, CI for Windows)** |
| [[Overview]] | What the app is, where it stands, the numbers |
| [[Architecture]] | How the code is put together, file by file |
| [[Build and Release]] | Building (debug, Native AOT), publishing, the installer |
| [[Testing]] | Test worlds, the bench, how to verify on screen, the checklist |
| [[Windows vs Mac]] | Where the port differs from the Mac app on purpose, and what's missing |
| [[Decisions]] | Why things are the way they are (ADR log) |
| [[Lessons Learned]] | Every gotcha we hit: symptom, cause, fix |
| [[FishCatcher Port]] | Scam and phishing warnings: the C# engine (parity with the extension) and how the browser uses it |
| [[Roadmap]] | What comes next: installer variant, C++ frame, signing, updates |
| [[Ideas/Agentic Browser]] | Proposal: Brave-grade shields plus a private, visible AI agent |
| [[Ideas/Built-in Tools]] | Brainstorm: converters, bangs, voice, clipboard, file search, FishCatcher, file manager, free AI — checked against the user's own projects |
| [[Ideas/KeepItLocal Workspace]] | How the user's Workspace app (search, clipboard, voice, tools) feeds Search: port, wrap, or leave |
| [[Ideas/Media, Downloads and Documents]] | FFmpeg player, yt-dlp downloads, Word ⇄ PDF, what the installer carries, licences |
| [[Conventions]] | Rules for working on it: git identity, code style, don't-touch list |
| [[Changelog]] | What landed, commit by commit |
| [[Log/2026-09-24]] | Session journal |

## Rules of thumb

1. **Test the release (Native AOT) build**, not only the debug build. Several
   bugs only show up there. See [[Lessons Learned#Build and Native AOT]].
2. **Test in a test world** (`SEARCH_PROBE=<name>`), never the real profile. See [[Testing]].
3. **The Mac app is the spec.** When unsure how something should behave, read
   the Swift file the C# file mirrors.
4. **Keep this vault current.** After each work session, add to the
   [[Changelog]], [[Lessons Learned]] and a `Log/` entry.
