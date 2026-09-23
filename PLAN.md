# Plan — Search for Windows

Where the Windows port goes after 1.0, and why in that order. Numbers are from
this machine (Windows 11, 1920×1080 at 125%), release build, one Wikipedia
page open, each run in a fresh profile.

## Where it stands

| | Search for Windows 1.0 | How |
|---|---|---|
| Installer | 21 MB | NSIS, per-user, no admin prompt |
| Installed | 73 MB | 15 MB native `Search.exe` + 58 MB WinUI runtime |
| First window | ~240–300 ms | Native AOT, engine started before the window draws |
| App process | ~100 MB private, ~123 MB working set | almost all of it WinUI itself |
| Page engine | ~250 MB with one page | WebView2 (Chromium): browser, GPU, network, storage, renderer |

The Mac app is 3 MB because macOS carries WebKit and SwiftUI. Windows carries
the engine (WebView2) — so that half is already at parity — but not the UI
framework. Everything below is about the other half.

## 1. The installer's prerequisites — done, and one variant kept in reserve

**Done in 1.0.** The installer carries Microsoft's WebView2 Evergreen
bootstrapper (1.8 MB, signed by Microsoft, redistributable) and runs it only on
a PC without WebView2 — which in practice means an out-of-date Windows 10. WinUI
travels inside the app.

**In reserve: the shared Windows App Runtime.** The app could instead rely on
Microsoft's Windows App Runtime being installed, and the installer install it
when it isn't. That makes the app itself ~15 MB — but Microsoft's runtime
redistributable for 2.5 is **114.6 MB**, so on any PC that doesn't have it yet
(almost all of them today) the download goes from 21 MB to ~135 MB. Not worth
it until the runtime is commonly present. Revisit when either is true:

- Windows ships `Microsoft.WindowsAppRuntime.2` (2.5 or later) in the box, or
  Windows Update / the Store installs it widely. (Windows 11 already carries a
  2.2 build for its own apps as `Microsoft.WindowsAppRuntime.CBS.2`, which
  third-party apps can't bind to — but it shows where this is heading.)
- Search is published in the Microsoft Store, where the Store installs the
  framework package itself and the download is the app alone.

What it takes, when the time comes:
- `Search.csproj`: `WindowsAppSDKSelfContained=false`; keep the bootstrapper's
  auto-initialiser (framework-dependent, unpackaged).
- Installer: detect the framework with
  `PackageManager.FindPackagesForUser("", "Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe")`
  at version ≥ 2.5.1 (a tiny helper exe, or PowerShell's `Get-AppxPackage`);
  if missing, download
  `https://aka.ms/windowsappsdk/2.5/latest/windowsappruntimeinstall-x64.exe`
  (Microsoft's link, checked for Microsoft's signature before running) and run
  it with `--quiet`. Offer an offline installer with it inside for machines
  without internet.

## 2. The frame in C++ — the road to a Mac-sized Search

The only way to a Windows Search the size of the Mac one is to stop carrying
a UI framework at all: draw the frame with what Windows itself has, and keep
WebView2 for pages.

### What it would be

- **C++20**, one native exe, no runtime beside it. Built with MSVC and CMake.
- **Win32** for the window: one top-level window with a custom frame
  (`WM_NCCALCSIZE`, `WM_NCHITTEST` returning `HTCAPTION`/`HTMAXBUTTON` — which
  also gives Windows 11's snap layouts on the maximise button, something the
  WinUI version can't).
- **DirectComposition** for the layers — the strip, the column, the field, the
  panels — and their animations. Springs run on the compositor
  (`IDCompositionAnimation`), the same way the Mac's are Core Animation's.
- **Direct2D + DirectWrite** to draw: text, rounded rectangles, the ring, the
  logomark (the same path data). Segoe UI Variable and Segoe Fluent Icons, as
  now.
- **WebView2 through `ICoreWebView2CompositionController`**, its visual placed
  in our DirectComposition tree — the page and the frame composited together,
  overlays over the page without airspace problems. This also gives back the
  controller the WinUI control hides: real `ZoomFactor` (instead of CSS zoom),
  `AcceleratorKeyPressed` (instead of a keyboard hook), `IsVisible` and
  `MemoryUsageTargetLevel` per tab, `RasterizationScale`.
- Text fields (the address field, find, panel fields): a small single-line
  editor of our own on DirectWrite with IME support through the Text Services
  Framework — the one genuinely hard piece (see risks).

### What carries over unchanged

Most of what makes Search *Search* is already engine-side or plain data, and
moves as it is:

- Every page script: forms and passwords, the curtain and its picker, reader
  mode, the store relay, scroll reporting, picture-in-picture — the JavaScript
  strings, byte for byte.
- Every file format: `session*.json`, `history.json`, `bookmarks.json`,
  `hidden.json`, `downloads.json`, `spaces.json`, `settings.json`, the icons
  folder, the WebView2 profile folder, the Credential Manager names — so a C++
  build opens the same profile as the C# one, and switching is invisible.
- The bench protocol and `bench.ps1`, unchanged: the C++ build is tested with
  the same scripts.
- The installer: same NSIS script, a smaller payload.

### Expected

| | C# / WinUI (now) | C++ / Win32 + DirectComposition |
|---|---|---|
| Installer | 21 MB | ~2 MB |
| Installed | 73 MB | ~3–4 MB |
| First window | ~250 ms | ~80–120 ms |
| App process | ~100 MB private | ~25–40 MB private |
| Page engine | ~250 MB | ~250 MB (same WebView2) |

(Estimates for the C++ column, from comparable Win32 + WebView2 apps; to be
measured, not promised.)

### Phases

Each phase ends in a build that is used daily, with the bench driving it.

1. **Window and pages** — the custom frame, one WebView2 per tab through the
   composition controller, tab switching, session restore, keyboard shortcuts
   through `AcceleratorKeyPressed`, the data files read as they are.
2. **The strip and the column** — tabs, pins, drag to reorder, the sliding
   wash, fold and peek, drag regions; all on DirectComposition.
3. **The field** — the text editor (with IME), suggestions, completion,
   Ctrl+K. The riskiest phase; started early in a branch of its own.
4. **Panels** — Settings, History, Downloads, Bookmarks, Passwords, Hidden,
   Welcome, as a small immediate-mode layout on Direct2D.
5. **Everything else** — spaces, extensions UI, the bench server, the
   installer switch-over. Then the C# build retires.

Rough size of the work: the frame and panels are ~9,000 lines of C# today;
in C++ with no UI framework expect 20,000–25,000 lines, most of it layout,
text and input. Several months for one person; the phases above are ordered so
that the first two already make a usable browser.

### Risks

- **Text input.** A text field that behaves like Windows' own — IME for
  Chinese, Japanese and Korean, the emoji panel, touch keyboard, screen
  readers — is real work. Fallback: host a borderless Win32 `EDIT` or a
  RichEdit control for the few fields there are, styled to match.
- **Accessibility.** WinUI gives screen readers the frame for free; a
  hand-drawn frame needs its own UI Automation provider. Required, not
  optional — planned into phase 2, not after.
- **High contrast, scaling, multiple monitors** — all handled by WinUI now,
  all ours to handle then (per-monitor DPI v2 is straightforward; high
  contrast needs its own palette).

## 3. Smaller things, whichever way the frame goes

- **Code signing.** An unsigned installer meets SmartScreen's "Windows
  protected your PC". An EV or Azure Trusted Signing certificate removes that;
  it is also what an update check needs, to verify what it downloads — the
  Mac's updater checks Office Commun's signature the same way.
- **Updates.** A daily check against an appcast, as on the Mac, once there is
  a signed build to point it at.
- **Arm64** Native AOT builds (today Arm64 falls back to ReadyToRun).
- **What WebView2 doesn't yet expose to extensions** — badges, context-menu
  items, command dispatch, native messaging — followed as the WebView2 SDK
  adds them.
