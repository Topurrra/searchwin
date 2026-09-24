---
tags: [searchwin, decisions, adr]
updated: 2026-09-24
---

# Decisions

Back to [[README]]

Each entry covers the decision, why it was made, and when to revisit it.
Newest at the bottom.

### D1 · C# + WinUI 3 + WebView2
- **Decision:** port the Swift/WebKit/SwiftUI app to C#, WinUI 3 and WebView2.
- **Why:** the user asked for it. It's also the closest Windows equivalent of
  "only what the OS carries": WebView2 is Chromium that Windows already ships
  and updates.
- **Revisit:** see D9.

### D2 · Mirror the Mac app file for file
- **Why:** the Mac app is the spec. Reading `Browser.swift` next to
  `Browser.cs` is the fastest way to answer "how should this behave?"

### D3 · UI in C# code, not XAML
- **Why:** it keeps the code shaped like the SwiftUI views it mirrors, and
  there's no XAML-compiler reflection for AOT to trip on. `App.xaml` is just the
  application object.

### D4 · Unpackaged and self-contained
- **Why:** one folder, double-click, nothing to install first. MSIX would add
  signing and Store-style friction for a 1.0.

### D5 · Windows App SDK *component* packages, not the metapackage
- **Why:** the metapackage pulls in ONNX Runtime, DirectML and the AI stack,
  about 100 MB nobody asked for.

### D6 · Native AOT for release, with a ReadyToRun fallback
- **Why:** the user wanted Mac-like size, speed and memory. AOT gives a 15 MB
  exe with no .NET runtime (73 MB installed instead of about 215 MB), a faster
  first window and less memory.
- **Cost:** needs the MSVC linker. CsWinRT needs `partial` classes and `As<T>()`.
  JSON must be source-generated. See [[Lessons Learned#Build and Native AOT]].

### D7 · Installer bundles the WebView2 bootstrapper, not the WinUI runtime
- **Context:** the user suggested the installer install WinUI and WebView2 if
  they're missing.
- **Decision:** WebView2: yes, the 1.8 MB Microsoft-signed bootstrapper runs
  only when it's missing. WinUI: no, it stays inside the app.
- **Why:** Microsoft's Windows App Runtime redistributable is 114.6 MB, so a
  fresh PC would download about 135 MB instead of 21 MB.
- **Revisit:** when Windows ships Windows App Runtime 2.5+ in the box, or if
  Search goes to the Microsoft Store. The steps are in `PLAN.md` §1.

### D8 · Per-user NSIS installer
- **Why:** no admin prompt, and it installs to `%LOCALAPPDATA%\Programs`. NSIS
  is small and scriptable.

### D9 · C++ Win32 + DirectComposition frame is the future plan, not now
- **Why:** it's the only route to a Mac-sized app (about 3–4 MB installed, 25–40 MB
  RAM), but it's 20–25k lines and months of work. Written up in [[Roadmap]].

### D10 · Global keyboard hook for shortcuts
- **Why:** the WinUI WebView2 control doesn't expose `AcceleratorKeyPressed`,
  and a focused page's keys never reach our windows. A `WH_KEYBOARD_LL` hook
  that's active only while our window is in front acts like the Mac's local
  event monitor. The C++ plan replaces it with `AcceleratorKeyPressed`.

### D11 · Bench over a named pipe
- **Why:** the Mac uses a Unix socket. On Windows, `AF_UNIX` connect failed
  under AppData, so we use a named pipe with `CurrentUserOnly`, one per test world.

### D12 · Custom window chrome
- **Decision:** `OverlappedPresenter.Create()` + `SetBorderAndTitleBar(true,false)`,
  the app's own `WindowButtons`, and `InputNonClientPointerSource` caption and
  passthrough regions.
- **Why:** the strip *is* the title bar, like the Mac's.

### D13 · Hamburger menu for the Mac's menu bar
- **Why:** Windows has no global menu bar, so the Mac's menu-bar commands
  (settings, passwords, themes…) had nowhere to live. `AppMenu` is rebuilt
  every time it opens, so it's never stale.

### D14 · WinUI styles added in code, in `OnLaunched`
- **Why:** declared in XAML they weren't found in the AOT build, and adding
  them in the App constructor crashed with 0xC000027B. The real fix was also
  shipping `Search.pri` (see D15).

### D15 · Ship `Search.pri` + `*.xbf` with every publish
- **Why:** without the resource index, WinUI has no themes in release builds.
  See [[Lessons Learned#The missing resource index]].

### D16 · Search absorbs KeepItLocal Workspace (2026-09-24)
- **Decision:** Workspace's Rust engine is copied into `searchwin/Engine`
  (Tauri removed) and runs as a lazy headless helper process. Its tool
  screens run as pages in tabs through a Tauri-API shim. The field replaces
  Workspace's palette.
- **Why:** positioning is *a browser first, then a Raycast-like launcher but
  better*. Rewriting ~2.9 MB of Rust in C# makes no sense, and the helper
  process keeps the first window at ~250 ms. See [[Master Plan]].
- **Workspace itself:** kept untouched. There's no standalone frontend next to Search.

### D17 · Tray mode off by default (2026-09-24)
- **Why:** it's a browser first. The resident engine is opt-in.

### D18 · KeepItLocal Redact stays out (2026-09-24)
- **Decision:** nothing from Redact is integrated. That includes Workspace's
  `core/resources.rs`, which came from Redact's predecessor; the engine gets
  its own throttling module.
- **Why:** Redact is the user's upcoming paid product (on-device
  file/audio/video redaction). Search keeps only browsing-sized redaction:
  pre-AI masking, paste guard, screenshot region blur.

### D19 · Site safety: FishCatcher instead of Edge SmartScreen; the Store for signing (2026-09-24)
- **Why:** Edge SmartScreen sends every site to Microsoft. FishCatcher works
  on the device. The installer's "Windows protected your PC" warning goes
  away through Microsoft Store signing (phase 7).
