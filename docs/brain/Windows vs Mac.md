---
tags: [searchwin, parity]
updated: 2026-09-24
---

# Windows vs Mac

Back to [[README]]

The goal is **the same app**: same UI, same behaviour, same speed as far as
Windows allows. Keys use `Ctrl` where the Mac uses `⌘`.

## Different on purpose

| Area | Mac | Windows | Why |
|---|---|---|---|
| Menu bar | macOS menu bar (File, View, History…) | **Hamburger menu** (`AppMenu.cs`) at the end of the strip / first in the column foot | Windows has no global menu bar |
| Window buttons | Traffic lights, top-left | The app's own min/max/close at the far end of the strip or the column corner; they fold away with the column | Where Windows users look for them |
| Tab width | 186 | `TabWidth = 160` | 186 looked too long at Windows' 125% scaling |
| Sleeping tabs | Torn down, rebuilt | `TrySuspendAsync`: they come back exactly as they were, nothing re-fetched | WebView2 can suspend |
| Floating video | Custom window with ±15 s buttons | Chromium's own PiP window | What WebView2 offers |
| Passwords from other browsers | Read directly | Via each browser's **CSV export** | Chrome/Edge use app-bound encryption on Windows. Bookmarks, history and icons are still read directly |
| Password storage | Keychain | Credential Manager (`Search:…`) | The platform's equivalent |
| Spaces keys | `⌃1`–`⌃9` | `Alt+1`–`Alt+9`; a sideways swipe over the column changes space once it's long enough (no slide under the fingers) | |
| Shortcuts inside pages | Local event monitor | Global `WH_KEYBOARD_LL` hook while the window is in front | WebView2 keeps keys in its own process |
| Spelling | Autocorrect | "Check spelling as you type" (`Prefs.Spelling`) | Windows' spell checker; no autocorrect |
| Updates | Daily signed appcast | None yet | No Windows update server or signing yet |
| Extra Windows keys | — | `F5`, `F6`/`Alt+D`, `F3`, `F11`, `F12`, mouse back/forward, `Ctrl+H`, `Ctrl+J` | Windows habits |

## Not there yet

Mostly extension features WebView2 doesn't expose to apps:

- badges and changing icons on extension buttons
- an extension's own right-click menu items
- extensions without a popup reacting to their button
- keyboard commands other than opening the popup
- native messaging
- the automatic update check
- Arm64 Native AOT (Arm64 falls back to ReadyToRun)
- Windows 11 snap layouts on the maximize button. The WinUI build can't do
  this; the C++ plan can. See [[Roadmap]].
