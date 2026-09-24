---
tags: [searchwin, roadmap]
updated: 2026-09-24
---

# Roadmap

Back to [[README]] · The full version is `searchwin\PLAN.md`.

## Now (after 1.0)

- [ ] User testing of the installed build. Fix what comes up.
- [ ] Passwords with real accounts: save, fill, the account list, CSV import (user to try)
- [ ] Import from the user's real browsers (user to try)
- [ ] Camera/mic permission bar (needs a camera)

## 1 · Installer prerequisites: done, with one variant held back

- **Done:** bundle the WebView2 bootstrapper; WinUI stays inside the app.
- **Held back:** rely on the shared Windows App Runtime and install it when
  it's missing. The app drops to about 15 MB, but a fresh PC downloads about
  135 MB. Do it when Windows ships Windows App Runtime 2.5+, or on the
  Microsoft Store.
  - `WindowsAppSDKSelfContained=false`
  - The installer detects `Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe` ≥ 2.5.1,
    otherwise downloads `aka.ms/windowsappsdk/2.5/latest/windowsappruntimeinstall-x64.exe`,
    checks its signature, and runs it with `--quiet`.

## 2 · The frame in C++: the road to a Mac-sized Search

**Stack:**
- C++20, MSVC and CMake
- Win32 window with a custom frame (`WM_NCCALCSIZE`, `WM_NCHITTEST`; this also
  gives Windows 11 snap layouts)
- DirectComposition layers and animations
- Direct2D and DirectWrite for drawing
- WebView2 through `ICoreWebView2CompositionController`, which gives back real
  `ZoomFactor`, `AcceleratorKeyPressed`, `IsVisible`, `MemoryUsageTargetLevel`
  and `RasterizationScale`

**Carries over unchanged:**
- every page script
- every JSON file format and the Credential Manager names
- the bench protocol and `bench.ps1`
- the NSIS script

| | C#/WinUI now | C++ (estimate) |
|---|---|---|
| Installer | 21 MB | ~2 MB |
| Installed | 73 MB | ~3–4 MB |
| First window | ~250 ms | ~80–120 ms |
| App process | ~100 MB | ~25–40 MB |
| Page engine | ~250 MB | ~250 MB |

**Phases**, each ending in a build used daily:
1. Window and pages
2. Strip and column
3. The field: our own text editor with IME. The riskiest phase; start it early on its own branch.
4. Panels
5. Everything else, then retire the C# build

**Risks:**
- Text input (IME, emoji panel, screen readers). Fallback: a borderless `EDIT`/RichEdit.
- Accessibility needs a UI Automation provider, planned into phase 2.
- High contrast, DPI and multiple monitors become ours to handle.

**Size:** about 20–25k lines, several months for one person.

## 3 · Smaller things

- [ ] **Code signing** (EV or Azure Trusted Signing). This removes the SmartScreen warning, and updates need it.
- [ ] **Updates**: a daily appcast check, as on the Mac, once builds are signed
- [ ] **Arm64 Native AOT**
- [ ] Extension features as WebView2 adds them: badges, context menus, commands, native messaging
