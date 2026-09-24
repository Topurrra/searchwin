---
tags: [searchwin, architecture]
updated: 2026-09-24
---

# Architecture

Back to [[README]] · See also [[Decisions]], [[Windows vs Mac]]

## The shape

```
Program.Main (App.xaml.cs)      single instance via AppInstance; hands links to the running one
 └─ App.OnLaunched              UI.Queue, XamlControlsResources, light/dark, Web.Environment (warm), MainWindow
     └─ MainWindow              custom frame, KeyHook, window buttons, caption regions
         ├─ TabBar / SideBar    the tabs (strip or column), doors (bookmarks, extensions, menu)
         ├─ Omnibox             the one field + suggestions + Ctrl+K
         ├─ Stage               the page (WebView2 of the active Tab), wake snapshot, error text
         ├─ Bars                bottom bars: toasts, permission asks, password offers
         └─ Panels\*            Settings, History, Downloads, Bookmarks, Passwords, Hidden, Welcome…
Browser (Core)                  the model: tabs, active tab, spaces, prefs; everything draws from it
 └─ Tab                         one page; its WebView2 is created lazily on first need
Web                             one shared CoreWebView2Environment; per-space profiles; InPrivate for private tabs
Store                           JSON files in %LOCALAPPDATA%\Search (or a test world)
```

**Mirrors the Mac app file for file**, so the two can be read side by side
(`Browser.cs` ↔ `Browser.swift`, and so on). When behaviour is in doubt, the
Swift file is the spec.

**UI is written in C#, not XAML.** `App.xaml` is only the application object.
Every control is built in code.

## Core (`Search\Core`)

| File | Role |
|---|---|
| `Browser.cs` | The model: tabs, active tab, field state, panels open. Properties notify by name (`Model.cs`), and the UI redraws from them |
| `Browser.Features.cs` | Partial-method hooks that the feature files plug into |
| `Browser.Page.cs` | Find (WebView2 Find API, match counter), `chrome-extension:` pages, page-level commands |
| `Browser.Passwords.cs` | Offers to save captured sign-ins, fills |
| `Browser.Spaces.cs` | Spaces: create, switch, rename, icon, order, delete |
| `Browser.Float.cs` | Floating video: Chromium PiP through CDP `Runtime.evaluate` with `userGesture` |
| `Browser.Sleep.cs` | Tabs asleep after 30 min (`sleep.after` overrides), `TrySuspendAsync` |
| `Browser.Zoom.cs` | Zoom per site, 0.4×–3× |
| `Tab.cs` | One tab; WebView2 built on demand; navigation, new windows, permissions |
| `Web.cs` | The shared environment; `--disable-features=msSmartScreenProtection,msEdgeShoppingUI,msWebOOUI,msPdfOOUI,msHubApps`; extensions on |
| `Store.cs` | Where files live; test worlds (`SEARCH_PROBE`); `Shape<T>()`; bad files quarantined only on `JsonException` |
| `Json.cs` | Source-generated `System.Text.Json` context for every file (required for AOT) |
| `Prefs.cs` | Preferences (`settings.json`): look, sidebar, glyph, spelling, sleep, spaces… |
| `Session.cs` | `session.json`: open tabs, names you gave them |
| `History.cs` / `Bookmarks.cs` / `Loot.cs` | `history.json` / `bookmarks.json` / `downloads.json` |
| `Space.cs` | `spaces.json`, icons, profile names |
| `Address.cs` | What the field's text means: address, search, refused scheme |
| `Shield.cs` | The ad blocker: `WebResourceRequested` filters per domain |
| `Curtain.cs` | Hide elements for good (`hidden.json`), with the picker script |
| `Reader.cs` | Reading mode (leaving it reloads the page) |
| `Forms.cs` | Page script that sees sign-in forms, and the account list |
| `Vault.cs` | Passwords in Windows Credential Manager as `Search:…` (a test world uses its own prefix) |
| `Import.cs` | Import from Chrome/Edge/Brave/Firefox profiles, plus password CSVs |
| `Extensions.cs` / `Crx.cs` / `StoreRelay.cs` | Chrome extensions: install from the Web Store or a folder, unpack `.crx`, take over the store's "Add to Chrome" |
| `PageScripts.cs` | Scripts injected on document creation (spelling on/off…) |
| `Favicons.cs` | Site icons, shared by host |
| `DefaultBrowser.cs` | Whether https opens here (`UserChoiceLatest` / `UserChoice`) |
| `Links.cs` | Launch args, redirected activations, `Trouble(e)` error sink |
| `Bench.cs` | The script-driving server over a named pipe (see [[Testing#The bench]]) |
| `Log.cs` | `debug.log`, only in a test run |
| `Updater.cs` | Version only; there is no Windows update server yet |

## UI (`Search\UI`)

| File | Role |
|---|---|
| `MainWindow.cs` | The window: custom presenter, caption and passthrough regions, bring to front, light/dark |
| `Chrome.cs` | The app's own min/max/close buttons (`WindowButtons`) |
| `TabBar.cs` / `SideBar.cs` / `TabFace.cs` | Strip, column, and what is inside a tab. `TabWidth = 160` |
| `Omnibox.cs` | The field, suggestions, completion, and the breathing glow (composition `DropShadow`) |
| `Stage.cs` | Page host, the snapshot while waking, "never came" text |
| `Bars.cs` | Bottom bars |
| `AppMenu.cs` | The hamburger menu: New/Private/Reopen, History, Bookmarks, View, Page, Passwords, Settings, Welcome, Feedback |
| `Shortcuts.cs` | Every key binding in one place |
| `KeyHook.cs` | `WH_KEYBOARD_LL` hook: WebView2 keeps keys in its own process, so shortcuts are caught globally while the window is in front |
| `Design.cs` | `Palette` (light/dark brush pairs repainted in place), `Icons` (Segoe Fluent), `Paths`, `Tone`, sizes |
| `Kit.cs` | Shared building blocks: rounded borders, shadows, rows |
| `Press.cs` | One pointer-handling base for everything pressable |
| `UI.cs` | `UI.Queue`: the dispatcher, from anywhere |
| `Panels\*` | Settings, History, Downloads, Bookmarks, Passwords (+ `AccountList`), Hidden, Welcome, Extensions (`ExtensionSlot`, `ExtensionsPage`, `StoreOffer`), Spaces (`SpaceDot`, `SpaceSwipe`, `NewSpaceCard`) |

## Data on disk

`%LOCALAPPDATA%\Search\` (a test world is `%LOCALAPPDATA%\Search (<world>)\`):

- `settings.json`, `session*.json`, `history.json`, `bookmarks.json`,
  `downloads.json`, `hidden.json`, `spaces.json`, an icons folder
- `WebView2\`: the engine's profiles (cookies, caches); one profile per space
- `Extensions\`: unpacked extensions
- Passwords are **not** on disk. They're in Credential Manager as generic credentials `Search:…`.

Every file format matches what the planned C++ build will read (see [[Roadmap]]).

## Other files in the repo

- `build.ps1`, `publish-aot.cmd`: see [[Build and Release]]
- `Installer\Search.nsi`: see [[Build and Release#Installer]]
- `bench.ps1`: see [[Testing#The bench]]
- `Icon\icon.ps1`: makes `Search.ico`
- `PLAN.md`: the source for [[Roadmap]]
