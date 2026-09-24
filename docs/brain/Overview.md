---
tags: [searchwin, overview]
updated: 2026-09-24
---

# Overview

Back to [[README]]

## What Search is

A row of tabs, across the top or down the left, and the page. No toolbar, no
start page, no account. One field: type an address and go there, or type words
to search. Addresses are completed from your own history, and nothing you type
leaves the machine until you press Enter.

Features (all ported from the Mac):

- **Tabs**: pin, rename, duplicate, reopen closed, `Ctrl+K` switcher, session
  restore (restored tabs cost nothing until clicked), sleep after 30 min
- **Layout**: tab strip or sidebar (`Ctrl+Shift+S`), fold the sidebar (`Ctrl+S`)
- **Page tools**: reading mode, hide elements for good (the "curtain"), ad
  blocker before the page loads, floating video (PiP), find, zoom per site
- **Data**: passwords (Windows Credential Manager), bookmarks, history,
  downloads, import from other browsers
- **Chrome extensions** through WebView2's extension engine
- **Spaces**: separate sets of tabs with their own profiles; `Alt+1`–`Alt+9`
- **Looks**: light, dark or system; tabs show letters or site icons
- **App menu** (hamburger): holds what the Mac keeps in its menu bar
- **Bench**: a named pipe a script can drive the app over (see [[Testing]])

## Stack

| Layer | Choice |
|---|---|
| Language/runtime | C# 13, .NET 9, **Native AOT** (ReadyToRun fallback) |
| UI | WinUI 3 (Windows App SDK 2.x *component* packages, self-contained, unpackaged), built in C# code, not XAML |
| Pages | WebView2 (Evergreen runtime, SDK 1.0.4191.47) |
| Installer | NSIS 3, per-user, bundles the WebView2 bootstrapper |
| Min OS | Windows 10 1809+ (x64; Arm64 builds with ReadyToRun) |

## Where it stands (1.0, 2026-09-24)

Measured on this machine (Windows 11, 1920×1080 at 125%), release build, one
Wikipedia page open:

| | Value |
|---|---|
| Installer | 21 MB |
| Installed | 73–75 MB (15 MB native `Search.exe` + ~58 MB WinUI runtime) |
| First window | ~240–300 ms |
| App process | ~100 MB private, ~123 MB working set (mostly WinUI) |
| Page engine | ~250 MB with one page (WebView2 processes) |
| Code | ~17,700 lines of C# |

The Mac app is ~3 MB because macOS ships WebKit and SwiftUI. Windows ships the
engine (WebView2) but not the UI framework. Getting close to the Mac's size
means the C++ frame described in [[Roadmap]].

## Status

- Everything the Mac app does is built. It has been verified on screen in the
  debug build, and the menus and dialogs in the native build. See [[Testing#What has been verified]].
- Not tested yet: camera/mic permission prompt (no camera here), password
  save/fill with real accounts, and importing real browser data (left to the user).
- Known gaps are in [[Windows vs Mac#Not there yet]].
