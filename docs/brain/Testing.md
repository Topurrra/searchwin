---
tags: [searchwin, testing]
updated: 2026-09-24
---

# Testing

Back to [[README]]

## Test worlds: never test in the real profile

`Store.World` decides where everything lives:

| How it's run | World | Folder |
|---|---|---|
| Installed / published copy | none (the real one) | `%LOCALAPPDATA%\Search\` |
| From `bin\Debug`, or `SEARCH_PROBE=1` | `test` | `%LOCALAPPDATA%\Search (test)\` |
| `SEARCH_PROBE=<name>` | `<name>` | `%LOCALAPPDATA%\Search (<name>)\` |

A test world has its own files and WebView2 profile, its own Credential Manager
prefix, its own single-instance key and its own bench pipe
(`search-bench-<world>`). It can run **next to** the real Search. It's the only
place that writes `debug.log`.

```powershell
$env:SEARCH_PROBE="look1"; Start-Process .\build\Search\Search.exe
```

Use a fresh name for a clean first run (you get the Welcome pages).

> Leftover world folders (`Search (menucrash)`, `Search (final…)`, `Search (measure)`…)
> can be deleted by hand in Explorer. Claude's `Remove-Item` guard blocks paths with parentheses.

## The bench

The app can be driven by a script. Turn on **Settings › General › Let a script
drive Search**. The app then listens on a named pipe that only the current
Windows user can open (`PipeOptions.CurrentUserOnly`). `bench.ps1` (PowerShell 7)
speaks the same commands as the Mac's `./bench`:

```powershell
./bench.ps1 open https://example.com     # → tab id
./bench.ps1 wait <id>                    # until loaded
./bench.ps1 text <id>                    # page text
./bench.ps1 shot <id> out.png            # screenshot
./bench.ps1 click <id> "button.go"       # also: type, submit
./bench.ps1 probe                        # window state: panels open, dialog, window
./bench.ps1 close all
```

Add `--test` to talk to a debug build. Bench URLs may be `http(s)` or `chrome-extension://`.

### Testing FishCatcher (scam warnings)

- `tabs`, `wait` and `open` answer with `"fish"`: the verdict's level, score,
  reasons (`reasons`, and the extension's keys in `keys`), `realSite`, the
  quiet line's text (`caution`) and how long the address check took (`ms`).
  `"trouble": "Scam"` means the warning page is up.
- `./bench.ps1 fish <id> back|continue|real|ok` presses the warning's buttons
  on that tab (test runs only; it brings the tab to the front).
- **Was the site contacted?** In a test run, `SEARCH_HOST_RULES` passes
  Chromium's `--host-resolver-rules` (e.g. `MAP paypa1-secure-login.com
  127.0.0.1:8767`), so a lookalike name can point at a small local server that
  logs every connection and request. `SEARCH_NET_LOG=C:/path/net.json` writes
  the engine's network log. Both only work when `Store.Testing`.
- `Get-DnsClientCache | Where-Object Entry -match <name>` shows whether a
  name was looked up at all (a made-up name leaves a "9003" entry).

## Verifying on screen

The bench can't see WinUI chrome (menus, dialogs, flyouts). To test those,
drive the app with the **computer-use** tools:

1. Launch the **release** build in a test world.
2. `request_access` for the app.
3. Click, hover and screenshot.

Hover every submenu. Crashes can come from **hovering** a menu, not only from clicking it.

## Release smoke test

Run this against `build\Search\Search.exe` (the Native AOT output), not the debug build:

- [ ] `build\Search\Search.pri` exists
- [ ] The first window appears; the field glows steadily, without flickering
- [ ] Exactly one set of window buttons; min/max/close work; you can drag the strip
- [ ] Hamburger menu: hover **every** submenu (History, Bookmarks, **View**, View › Tabs Wear, Page)
- [ ] View › Light / Dark / System switch the look; Show Tabs in Sidebar switches the layout
- [ ] Space menu (dot at the column's foot): checkmarks show; the Icon submenu opens
- [ ] New Space dialog: rounded Fluent style, dark fields in dark mode; Create is disabled until a name is typed
- [ ] Settings: every page opens
- [ ] Open a page, find (`Ctrl+F`), private tab, bookmark, download
- [ ] Close and reopen: the session comes back

## What has been verified

On screen, in the debug build (2026-09-24):
- extension install, popup, and the current-tab fix
- bookmarks: add, dropdown, panel
- downloads, find, private tab, tab rename, editing a pin's letter
- spaces: create and switch
- Welcome pages, every Settings page
- hide elements and the Hidden panel
- sleep and wake, crash recovery

In the native build: the menus (View radios, toggles, space menu + Icon
submenu), the Fluent dialogs, and switching between light/dark and strip/sidebar.

**Not verified:**
- the camera/mic prompt (no camera here)
- saving and filling passwords with real accounts
- importing real browser data

The last two were deliberately left to the user.
