# Search for Windows

A small, fast, quiet web browser — the Windows port of [Search](../README.md), by
[Office Commun](https://officecommun.com). Same window, same tabs, same field,
same keys (with Ctrl where the Mac has ⌘), rewritten in **C#**, **WinUI 3** and
**WebView2**.

The Mac app is made of what macOS already carries — WebKit, AppKit, SwiftUI.
This one is made of what Windows carries: **WebView2** for pages (the Chromium
engine Windows 10 and 11 already have installed, kept up to date by Windows),
**WinUI 3** for everything drawn around them. Nothing else.

## What it is

A row of tabs — across the top or down the left, your choice — and the page.
No toolbar, no start page, no account. You type an address or a few words in
one field and you are on the page.

- **One field.** Type an address and you go there; type words and you search.
  It finishes addresses from your own history, and never sends what you type
  anywhere until you press Enter. `Ctrl+K` lists your open tabs by name.
- **Tabs that stay out of the way.** Pin the pages you keep open all day and
  they shrink to a letter or their icon. Tabs from your last session come back
  instantly and cost nothing until you click them. Tabs you haven't looked at
  for half an hour are suspended and give their memory back.
- **Across the top or down the side**, `Ctrl+Shift+S`; fold the column away
  with `Ctrl+S` and push the left edge for it.
- **Reading mode**, **hide anything for good**, **an ad blocker that runs
  before the page**, **video that follows you**, **passwords in Windows'
  Credential Manager**, **bookmarks, history, downloads**, **Chrome
  extensions**, **spaces** — as on the Mac.
- **Light, dark, or Windows' own.** The frame and the pages follow.

## Keyboard

| | |
|---|---|
| `Ctrl+L` address · `Ctrl+K` switch tab · `Ctrl+T` new tab · `Ctrl+W` close · `Ctrl+Shift+T` reopen | `Alt+←` `Alt+→` or `Ctrl+[` `Ctrl+]` back, forward · `Ctrl+Tab` `Ctrl+Shift+Tab` next, previous tab · `Ctrl+1`–`Ctrl+9` jump |
| `Ctrl+Shift+S` tabs across the top or down the left · `Ctrl+S` fold the sidebar away · `Ctrl+Shift+B` bookmark this page | `Ctrl+Shift+R` reading mode · `Ctrl+Shift+P` float the video · `Ctrl+Shift+H` hide something · `Ctrl+Shift+U` what is hidden here |
| `Ctrl+F` find · `Ctrl+D` duplicate tab · `Ctrl+Shift+C` copy address · `Ctrl+Shift+V` paste and go | `Ctrl+Y` (or `Ctrl+H`) history · `Ctrl+Shift+J` (or `Ctrl+J`) downloads · `Ctrl+,` settings · `Ctrl+Alt+L` passwords |

Windows' own keys work too: `F5` reload, `F6` / `Alt+D` address, `F3` find
next, `F11` full screen, `F12` developer tools, the mouse's back and forward
buttons. `Esc` puts away whatever is open.

## Privacy, concretely

| What | Where it is | Who can read it |
|---|---|---|
| Passwords | Windows Credential Manager, as generic credentials named `Search:…` | Your Windows account. |
| History, bookmarks, open tabs, hidden elements, settings | Small JSON files in `%LOCALAPPDATA%\Search\` | You. |
| Cookies and site data | WebView2's profile in `%LOCALAPPDATA%\Search\WebView2\` | The sites that set them, as in any browser. |
| Extensions | Unpacked in `%LOCALAPPDATA%\Search\Extensions\` | Each extension, within the permissions it asked for. |
| Anything else | Nowhere. There is no server. | — |

Edge's own extras that report to Microsoft (SmartScreen, shopping, hub
panels) are switched off in the engine, and crash dumps stay on the machine.

## Building it

- Windows 10 (1903) or later, x64 or Arm64
- The .NET 9 SDK — `winget install Microsoft.DotNet.SDK.9`, or Microsoft's
  `dotnet-install.ps1 -Channel 9.0` into your profile
- The WebView2 runtime — already on every Windows 11 and up-to-date Windows 10

```powershell
dotnet build Search\Search.csproj        # a debug build: Search\bin\Debug\...\Search.exe
.\build.ps1                               # release, self-contained: build\Search\Search.exe
.\build.ps1 -Zip                          # + build\Search-<version>-x64.zip
```

A debug build — anything run from `bin\Debug`, or with `SEARCH_PROBE` set —
keeps its own folder, `%LOCALAPPDATA%\Search (test)\`, and never touches the
session, history or passwords of the Search you actually use.

## How it's put together

It mirrors the Mac app file for file wherever it can, so the two can be read
side by side:

- `Core\Browser.cs` is `Browser.swift`: which tabs exist, which one is showing,
  whether the field is up. Its features keep their parts in
  `Core\Browser.*.cs` (Page, Passwords, Spaces, Float, Sleep, Zoom…), hooked in
  through partial methods in `Browser.Features.cs`.
- `Core\Tab.cs` is one tab: a WebView2 built the first time anyone asks for it,
  so twenty restored tabs are twenty objects, not twenty renderers.
- `Core\Web.cs` holds the one WebView2 environment every tab shares — one
  browser process, warm renderers — started before the window has drawn.
- `UI\` is everything drawn, in C# rather than XAML: `Design.cs` has every
  colour as a light/dark pair (brushes repaint in place when the look turns
  over), `Kit.cs` the pieces every panel is made of, `TabBar.cs`, `SideBar.cs`,
  `Omnibox.cs`, `Stage.cs` the window's parts.
- `UI\KeyHook.cs`: a focused WebView2 keeps its keys in its own process, so the
  shortcuts are caught from Windows while the window is in front — what the
  Mac's local event monitor does.

### Where Windows differs from the Mac, on purpose

- **The window's three buttons** are the app's own, at the far end of the
  strip, or in the column's corner — where Windows people look for them — and
  they fold away with the column the way the traffic lights do.
- **Sleeping tabs** are suspended by WebView2 rather than torn down and
  rebuilt: they come back exactly as they were, instantly, with nothing
  re-fetched.
- **Zoom** (`Ctrl+=`, `Ctrl+-`, `Ctrl+0`) is remembered per site, as on the
  Mac; `Ctrl` and the mouse wheel do the engine's own zoom as well.
- **Video that follows you** uses Chromium's own picture-in-picture window:
  always on top, native controls, no ±15 s skip buttons.
- **Passwords from other browsers** come over as the CSV each browser exports.
  Chrome and Edge lock their saved passwords to themselves on Windows
  (app-bound encryption), so only their bookmarks, history and icons are read
  straight from their files.
- **Chrome extensions** run in WebView2's own extension engine. What it doesn't
  expose to the app isn't there yet: badges and changing icons on the buttons,
  an extension's own right-click items, extensions without a popup reacting to
  their button, keyboard commands other than opening the popup, native
  messaging.
- **Spaces** switch with `Alt+1`–`Alt+9` (the Mac's `⌃1`–`⌃9`), the space's
  icon, or a sideways swipe over the column that changes space once it is long
  enough, rather than sliding under the fingers.
- **Updates**: there is no update server for Windows builds yet, so this build
  doesn't look for one.

### Testing it without closing it

Turn on **Settings › General › Let a script drive Search** and the running app
listens on a named pipe only your Windows user can open. `bench.ps1` (PowerShell
7) speaks it, with the same commands as the Mac's `./bench`:

```powershell
./bench.ps1 open https://example.com     # a tab of its own, at the end of your row → its id
./bench.ps1 wait 2e7e7e89                # until it has loaded
./bench.ps1 text 2e7e7e89                # the page's text
./bench.ps1 shot 2e7e7e89 out.png        # a picture of it
./bench.ps1 click 2e7e7e89 "button.go"   # click, type, submit — through the page's own events
./bench.ps1 probe                        # the window's state: open panels, a dialog, the window
./bench.ps1 close all
```

Add `--test` to talk to a debug build instead of the Search you use.

## License

MIT — see [LICENSE](LICENSE). "Search" and the app icon are Office Commun's.
