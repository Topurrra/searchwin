---
tags: [searchwin, lessons]
updated: 2026-09-25
---

# Lessons Learned

Back to [[README]]

Each lesson is written as **symptom → cause → fix**, with the file or API where
the fix lives. *(uncertain)* marks things that weren't proven.

## The top five

1. **Always test the release (Native AOT) build.** The View-menu crash, the
   doubled window buttons, the square dialogs and a cast crash were all
   invisible in debug.
2. **Zero AOT/trim warnings does not mean AOT-safe.** WinRT casts and type
   tests still fail at runtime.
3. **A focused WebView2 swallows keys in its own process.** Host key handlers
   never see them. See [[#Input and keyboard]].
4. **WebView2 must start visible**, or it may never paint.
5. **Test in isolated worlds** (`SEARCH_PROBE`), and measure in brand-new ones.

---

## Build and Native AOT

### The missing resource index
- **Symptom:** in the installed build, hovering ☰ → **View** killed the app
  with no crash.log and no Event Log entry. Dialogs had square corners and
  white fields. The debug build was fine.
- **Cause:** an unpackaged `dotnet publish` doesn't copy `Search.pri` or
  `App.xbf` to the publish folder. Without the resource index, WinUI can't
  find `ms-appx:///Microsoft.UI.Xaml/Themes/themeresources.xaml`.
  `RadioMenuFlyoutItem` and `ToggleMenuFlyoutItem` then fast-fail when their
  menu opens.
- **Fix:** the `PublishResourceIndex` target in `Search.csproj` copies
  `$(OutDir)$(TargetName).pri` and `*.xbf` to `$(PublishDir)` (`b7916e4`).
  The first workaround (plain items with "✓") was reverted once the real
  cause was found. **Find the root cause before settling on a workaround.**

### XamlControlsResources timing
- **Symptom:** startup failed with **0xC000027B**, in debug too, after adding
  `XamlControlsResources` in the `App()` constructor.
- **Fix:** keep an empty `<ResourceDictionary/>` in `App.xaml`, and add the
  resources in `OnLaunched` inside try/catch, before any window exists.

### The MSVC linker for AOT
- **Symptom:** `'vswhere.exe' is not recognized`, then `link` exited with 9009.
- **Cause:** ILCompiler couldn't find MSVC. Also, `cmd /c "vcvarsall && …%PATH%…"`
  expands `%PATH%` before vcvarsall runs.
- **Fix:** `publish-aot.cmd` calls `vcvarsall.bat x64` first and then
  `dotnet publish -p:PublishAot=true -p:IlcUseEnvironmentalTools=true …`.
  `build.ps1` detects MSVC with vswhere and falls back to ReadyToRun.

### WinRT casts under AOT
- **Symptom:** `InvalidCastException` in `Kit.Field` with 0 AOT warnings.
- **Cause:** a C# cast of a WinRT object from `XamlReader.Load` has no reflection to rely on under AOT.
- **Fix:** `WinRT.CastExtensions.As<ControlTemplate>(…)`. Use `.As<T>()` for every WinRT cast.

### Type tests on system-supplied WinRT objects
- **Symptom:** doubled min/max/close buttons, only in AOT.
- **Cause:** `if (AppWindow.Presenter is OverlappedPresenter p)` quietly
  evaluated false, so `SetBorderAndTitleBar(true,false)` never ran.
- **Fix:** create the presenter yourself: `App.Overlapped()` =
  `OverlappedPresenter.Create()` + `SetBorderAndTitleBar(true,false)` + minimum
  640×420, applied with `AppWindow.SetPresenter`. **Don't type-test WinRT
  objects the system hands you.**

### Other AOT rules
- `XamlBindingHelper.ConvertValue(typeof(Geometry), …)` is reflection-based.
  Replaced with our own path parser, `Paths.Parse` (`Design.cs`).
- **CsWinRT1028:** classes deriving from WinUI types must be `partial`. A regex
  pass missed the generic `Segmented<T>`.
- Reflection-based System.Text.Json isn't AOT-safe. Use the source-generated
  `Json` context (`Core/Json.cs`) and `Store.Shape<T>()`. Anonymous-type
  `Serialize(new{…})` calls became `JsonObject`.
- `Store.Read` used to quarantine good user files on *any* exception. Now it does so only on `JsonException`.
- A 73 MB `Search.pdb` still appeared despite `DebugType=none` *(uncertain why)*. `build.ps1` moves `*.pdb` to `build\`.

### Size facts that drove the decisions
- The `Microsoft.WindowsAppSDK` metapackage pulls in onnxruntime, DirectML
  and AI features. Use the component packages instead (debug output went from 236 to 170 MB).
- ReadyToRun: 215 MB = .NET 74 + WinRT projections 58 + WinUI 71. Our own code is 2.2 MB.
- AOT: 73 MB folder, 14.7 MB exe, ~250 ms first window, ~100 MB private memory.
- GC tuning (`GCgen0size`, `GCConserveMemory`) changed nothing, because WinUI dominates.
- The Windows App Runtime 2.5 redistributable is 114.6 MB. Windows 11's
  built-in `WindowsAppRuntime.CBS.2` can't be used by third-party apps.

### Small compile gotchas
- **MSB3027/MSB3021 "file is locked"**: stop `Search` before building.
- **CS0028**: a method named `UI.Main(Action)` was treated as an entry point. Renamed to `UI.Do`.
- `WMC1509 No LocalAssembly` is a harmless XAML compiler warning.

## WinUI and XAML

- **`Border` and `Rectangle` are sealed** (CS0509). Derive from `Grid`.
- **`CoreWebView2Deferral`** doesn't exist in the WinUI projection. Use `Windows.Foundation.Deferral`.
- **Lambda discard trap** (hit twice): in `(_, e) => { _ = SomethingAsync(); }`,
  the single `_` is a real parameter, so the line assigns to it (CS0029).
  Name it `(sender, e)`.
- **Ambiguous types:** `DispatcherQueueTimer` needs full qualification (Microsoft.UI.Dispatching vs Windows.System).
- **The address completion undid itself:** `TextBox.TextChanged` fires
  *asynchronously*, so a bool guard around the setter never sees it. Use an
  "expected text" guard instead (`Omnibox.Put`/`Typed`).
- **A popup clipped by its parent:** hang it on a `Canvas`, which never clips.
- **`SelectionHighlightColor` ignores alpha.** Use a pre-blended solid (`Tone.Selection`).
- **"No installed components were detected" COMException:** an element was
  added to a new parent while still inside the old one. Remove it from its old
  parent first.
- **Glow flicker:** 7 rings at 1.2% alpha quantise into visible steps, and a
  Storyboard restarted on every `Loaded`. Fix: a composition `SpriteVisual` +
  `DropShadow` (BlurRadius 52) with a compositor animation that repeats forever
  and alternates (`Omnibox.Breath()`).
- **Segoe Fluent Icons has no bookmark ribbon.** It's drawn as a `PathIcon`
  (`Icons.Element`). A sentinel glyph handed to `FontIcon` rendered as boxes.
- **ContentDialog:** it needs `XamlRoot`, and only one can be open at a time.
  Keep the primary button disabled until the input is valid, otherwise a
  placeholder like "Work" makes Create silently do nothing.
- **Hidden icons still take spacing** in Auto grid columns. Put them in one collapsible panel.
- **Sizes copied from the Mac** (tab width 186) read large at Windows' 125% scaling. Tabs are now 160.

## Window chrome

- **Doubled buttons:** see [[#Type tests on system-supplied WinRT objects]].
- **Right-click opened the system menu over sidebar rows:** the
  `InputNonClientPointerSource` rects were computed before layout finished.
  Recompute on `LayoutUpdated`, pass them to Windows only when they change,
  and scale them by `XamlRoot.RasterizationScale` (`MainWindow.Regions`).
- **An overlay needs its own caption rect** if it should be draggable (Welcome), minus its own buttons.

## Input and keyboard

- **Shortcuts didn't work while a page had focus.** WebView2 handles keys in
  its own process, so `PreviewKeyDown` and `KeyboardAccelerator` never see
  them. Fix: `KeyHook.cs`, a `WH_KEYBOARD_LL` hook that:
  - acts only when our main window is in the foreground (not PiP, not DevTools);
  - swallows the key-up of any key-down it took;
  - keeps its delegate in a static field (or the GC collects it);
  - skips AltGr (Ctrl+Alt with `VK_RMENU` down) and anything with the Win key held.
- **Esc during a rename closed the panel.** Fields tagged `KeyHook.OwnEscape` keep their Esc.
- IME/CJK input is untested.

## WebView2

- **Pages loaded but never painted.** A WebView2 created `Collapsed` or at
  `Opacity=0` may never draw. Fix:
  - create it visible, underneath (`Children.Insert(0, …)`), and raise it with `Canvas.SetZIndex`;
  - set `WEBVIEW2_DEFAULT_BACKGROUND_COLOR` before creation, which also removes the white flash.
  - An early bisect suspected the title-bar settings; that was a red herring.
- **NullReferenceException at startup:** restored tabs were built before the stage existed. Create `Web.Stage` before `new Browser()`.
- **Scripts could miss the first page.** Collect every `AddScriptToExecuteOnDocumentCreatedAsync`
  call and await them (`arming`) before the first `Navigate`. To change a
  script later, remove it by id and re-add it.
- **Zoom:** the WinUI control doesn't expose `ZoomFactor`, so zoom is CSS
  `zoom` per site. `devicePixelRatio` includes the display scale; divide by
  `RasterizationScale` (this caused a phantom "125%").
- **Sleep:** `TrySuspendAsync` only works on a view that isn't visible. Set
  `MemoryUsageTargetLevel` to Low before suspending, and back to Normal plus `Resume()` on show.
- **Crash recovery:** handle `ProcessFailed` (RenderProcessExited, RenderProcessUnresponsive, FrameRenderProcessExited). The active tab recovers at once; others are marked stale.
- **Find:** `ActiveMatchIndex` is already 1-based. Next/Previous finish
  asynchronously, so follow the `ActiveMatchIndexChanged` and `MatchCountChanged` events.
- **Picture-in-picture** needs a user gesture. Use CDP `Runtime.evaluate` with
  `userGesture:true, awaitPromise:true`. The video should be playing *(uncertain whether that's strictly required)*.
- **Blocker:** use the 3-argument `AddWebResourceRequestedFilter` with
  `RequestSourceKinds.Document` (the 2-argument one is deprecated). Answer 403
  only for third-party requests. Tracking prevention stays Balanced, because Strict breaks sign-ins.
- **`ExecuteScriptAsync` doesn't await promises:** an async IIFE returns `{}`.
  Store the result on `window` and read it in a second call.
- **Hide-mode picker got stuck** after the page redrew: it checked `live` but
  not whether its frame was still in the page. Check `frame.isConnected`.
- **Spellcheck off:** set `spellcheck=false` on `documentElement` from a
  document-created script, waiting with a MutationObserver if the root isn't there yet.
- One page's engine is ~250 MB (browser, GPU ~113 MB, network, storage,
  renderers). `--disable-features=SpareRendererForSitePerProcess` was tried
  and not adopted.

## Extensions

- `AddBrowserExtensionAsync` rejects folders starting with `_` (E_ACCESSDENIED). Strip `_metadata` from the CRX.
- WebView2 drops an extension whose files change, so each version is unpacked into its own folder *(reasoned, not isolated)*.
- **Keep Chrome's extension id** by writing the CRX public key into
  `manifest.json` as `key`. The id is the SHA-256 of the key, first 16 bytes,
  mapped to a–p. CRX3 signatures are verified.
- **The store's live "Add to Chrome" did nothing.** On Chromium the store
  shows an *enabled* button, and the Mac script only replaced a disabled one.
  `StoreRelay` now also matches `/^(add to|remove from) chrome$/i`.
- **A popup's `chrome.tabs.query({active, currentWindow})` returned the popup
  itself**, because each WebView2 is its own window. Fix:
  `ExtensionSlot.CurrentTab` overrides `chrome.tabs.query`/`getCurrent`
  before the popup navigates.
- A popup must use the active tab's profile, and that profile must be
  prepared (`Extensions.Adopt`) before the popup navigates. Private tabs get no extensions.
- **Not possible through WebView2:** badges and live icons, context-menu
  items, action clicks for extensions without a popup, commands other than
  `_execute_action`, native messaging.

## Passwords and import

- Chrome and Edge passwords use **app-bound ("v20") encryption**, so they
  can't be read from their files. Import passwords from CSV only. Bookmarks,
  history and icons are still read directly. Keep the UI text honest about this.
- Read another browser's SQLite from a **copy of the main file only**. Copying
  `-wal` without `-shm` makes the read-only open fail. The file is read with `winsqlite3.dll`.
- **Vault:** Credential Manager `Search:<host>:<user>`, with `Search (<world>):`
  in test worlds. The "last used" date lives in the credential comment.
  Windows Hello is used through `UserConsentVerifierInterop` and falls back when it isn't set up.

## Installer

- The WebView2 bootstrapper is downloaded once and **rejected unless it's
  Authenticode-valid and signed by O=Microsoft Corporation**.
- The NSIS install directory is fixed and not read from the registry, because
  the folder is cleared before install. The installer asks you to close a
  running Search instead of killing it.
- **Default browser:** an app can't make itself the default. Write the HKCU
  registration, open Default apps, then poll `UserChoiceLatest` (newer
  Windows 11) and `UserChoice`.
- Unsigned installer, so SmartScreen warns. Signing is in [[Roadmap]].

## Testing and the bench

- **Fast-fail crashes leave no trace** (no crash.log, no Event Log entry).
  Reproduce them in the release build in a test world, and wrap suspects in try/catch → `Links.Trouble`.
- **Measure in brand-new worlds.** A reused world restores old tabs and inflates the numbers.
- **AF_UNIX sockets failed under `%LOCALAPPDATA%`** ("invalid argument" on
  connect) but worked under `C:\Temp`, so the problem stayed hidden until
  integration *(cause unknown)*. Fix: a named pipe with `CurrentUserOnly`.
- You can set up a test world by writing its `settings.json` (`welcomed`, `bench`, `spaces`, `look`).
- WebView2 only paints while visible. Bench tabs sit at zero opacity behind the live page.

## Tooling and process

- **Claude's Remove-Item guard** blocks some deletes of paths with
  parentheses, such as `Search (world)`. Keep deletes simple, or leave them to the user.
- **PowerShell traps:**
  - `` `n `` inside a single-quoted replace string is inserted literally;
  - variable names are case-insensitive (`$zip` clashed with `-Zip`);
  - curly quotes act as string delimiters inside double-quoted strings;
  - `cmd /c` expands `%VAR%` early.
- **Line endings:** the repo uses `core.autocrlf false` and LF. Watch for tools turning files into CRLF.
- **Bash:** `cat > file` with no heredoc hangs waiting on stdin.
- **Parallel porting worked:**
  - feature stubs plus `partial` hooks in `Browser.Features.cs`;
  - contracts committed first, then one git worktree per agent, merged with `--no-ff`.
  - Agents can't run the app, so integration still found the store button,
    the popup tab and the AF_UNIX bugs. **Check commit identity** after agents
    run (one used a typo'd email).
- **Screen testing with computer-use:**
  - grant `msedgewebview2.exe` as well, or pages look blank;
  - move the test window clear of other windows;
  - PowerShell `CopyFromScreen` needs `SetProcessDPIAware()`.
- **UI text must match what Windows actually does** (import wording, "pipe" not "socket").

## Sprint 0 (2026-09-25)

### Engine and Rust
- **Re-hosting a Tauri app:** a stand-in `tauri` crate (a path dependency
  with the same name) and a proc macro is far cheaper than rewriting the commands.
  Put the generated entry in a **module with the command's name**: `use
  a::cmd;` then imports it too, since modules and functions are different
  namespaces.
- **`State<'_, T>` in sync commands** run on the blocking pool must be
  `'static`. The stand-in's `State` owns an `Arc<T>`, so the macro takes a `'static` one.
- **Named pipes:** the default DACL lets Everyone read. Set an SDDL
  `D:P(A;;GA;;;<user SID>)`. `first_pipe_instance(true)` on a name already
  taken fails with **Access is denied**, not "already exists".
- **`rav1e` needs NASM:** build `ravif` with `default-features = false,
  features = ["threading"]` when NASM isn't installed (slower AVIF encoding).
- `windows` 0.54's `HLOCAL` wraps `*mut c_void`, not `isize`.

### WebView2
- **`SetVirtualHostNameToFolderMapping` doesn't serve `index.html` for `/`**:
  it gives `ERR_ACCESS_DENIED`. Always name the file.
- The WebView2 message's `Source` is the top-level document, so a bridge can
  trust it and refuse framed pages. `NavigationKind` tells back/forward/reload apart from a new document.

### SvelteKit
- The hash router (`kit.router.type = "hash"`) forbids page options:
  delete `+layout.ts`'s `ssr = false`.
- Vite `resolve.alias` with exact regexes swaps every `@tauri-apps/*` import for a shim.

### C#
- **Namespace hiding:** a class `Search.Kit` hides a namespace `Search.Kit`
  (CS0437). Name the library namespace `SearchKit`.
- CS8126: `Rest` can't be a tuple element name.
- **The discard trap again:** `using var _ = …` declares a variable called `_`. Name it.
- **A reader closing the pipe mid-write** throws `ObjectDisposedException`
  from the writer. Map it to "engine went away" (a test caught this).

### Tooling
- **`… | Select -First N` stops the command upstream.** It cut a `cargo build`
  off halfway. Filter with `Where-Object` instead.
- **PowerShell aliases beat functions:** a helper called `R` ran
  `Invoke-History`. Don't name helpers after aliases (`r`, `h`, `ls`…).
- **Mixed line endings across the repo** (CRLF in some files, LF in others):
  scripted replaces must detect the newline first, or use the Edit tool.
- **computer-use:** app grants reset each session, so request `search.exe` and
  `msedgewebview2.exe` again. A window launched minimized needs `ShowWindow(SW_RESTORE)`.
### Search's own pages (2026-09-25)
- **A Rust exe links the VC++ runtime dynamically by default.** Check with
  `dumpbin /dependents`, and use `+crt-static` for anything shipped to clean PCs.
- **Cancelling a certificate error reads as "cancelled"** to
  `NavigationCompleted`. Show the warning in `ServerCertificateErrorDetected`
  itself, where the reason is known.
- **Retry by navigating to the tab's address, not `Reload()`.** When the
  engine never got to the new page, reload repeats whatever document it last had.
- **`ContentLoading.IsErrorPage`** is the moment to cover the engine's error
  page. Clearing the cover at navigation start flashes the old error page.
- **Cross-origin `fetch()` from tool pages to `files.search` needs CORS**
  (`<img>`/`<video>` don't). A suffix range (`bytes=-100`) isn't CORS-safelisted, so answer the `OPTIONS` preflight.
- Chromium refuses some ports outright (9, 25…) as unsafe. That's a "Broken" page, not "Refused".

## Round 4–5: Engine index and Kit dedup (2026-09-26)

### redb whole-file locks
- **Symptom:** phase 2's state DB migration left the file locked for the process's lifetime. Worker processes couldn't open it. **Cause:** `get_db` caches the handle, and a migration that uses `get_db` keeps it cached and locked. **Fix:** Normalize the config once, write it with `write_search_config_for_state` (which uses write_json_shared), and track shared opens in a registry so `get_db` refuses a path already held elsewhere. `local_db.rs` now has a SHARED_DBS map and a guard on `get_db` in release builds too.

### ignore::WalkBuilder and hidden filters
- **Symptom:** `hidden(false)` turned off both dot-name filtering AND FILE_ATTRIBUTE_HIDDEN checking on Windows. **Cause:** the ignore crate's hidden filter combines both checks. **Fix:** keep `hidden(false)` for dot-folders (still controlled by include_hidden), and add a filter_entry that skips entries with FILE_ATTRIBUTE_HIDDEN. This also makes excluded folders a real Skip, not a Continue. Nested chosen roots below exclusions are now re-walked from themselves via `walked_roots`, which dedupes against the outer walk only if it actually reaches the nested root.

### Tantivy RangeQuery scoring and word order
- **Symptom:** regression tests for prefix pass read-ahead relied on doc order after scoring, but RangeQuery scores are flat. **Cause:** without a total order on ties, tests would pass or fail depending on internal doc numbering. **Fix:** set `writer_with_num_threads(1, ...)` in the test so docs are inserted in order and later docs lose ties naturally. A test depending on read-ahead truncation must count on this determinism.

### Windows default exclusions and scope
- **Lesson:** Default exclusion patterns like `desktop.ini` and `Thumbs.db` must be added to both the engine's defaults and Search.Kit's IndexPlan.DefaultExcludes. The Kit sends its own list, so SearchKil reads config from the engine or legacy JSON, normalizes once per startup, and the watcher events do not re-check the hidden attribute — watcher-updated entries must be covered by rules too.

### test.rs: walker tests and %TEMP%
- **Lesson:** %TEMP% is a default exclusion (appdata/local), so a test that wants the defaults must be aware that the test folder itself might match. Walker tests need to drop entries that match the test folder's own path. Nested roots in reconcile walks must use walked_roots to reopen excluded ancestors only when they're actually traversed.

### PowerShell 7 and UTF-8 BOM
- **Symptom:** `[IO.File]::ReadAllText / WriteAllText` in PowerShell 7 silently drops a UTF-8 BOM. A file with a BOM (like search.rs) read with these functions and then written back loses the BOM, and the diff gains a first-line change. **Fix:** write with `New-Object System.Text.UTF8Encoding($true)` + `[IO.File]::WriteAllText($file, $content, $encoding)` to preserve the BOM, or use the Edit tool for bulk edits.

### GitHub push protection (2026-09-25)
- **A test fixture that looks like a secret blocks the push.** A fake Slack
  webhook in `sensitive_scan.rs`'s tests stopped a push with GH013. Fix:
  build such strings at run time (`format!` / `repeat`), or allow it with the link GitHub gives.
- **If the commits aren't pushed yet, fix the history:** `filter-branch
  --tree-filter` over `origin/main..main`, with a backup branch first.
- **`sed -i` in Git Bash strips CRLF.** Use `sed -b -i` to keep a Windows
  file's line endings, or the whole file shows as changed.

### FishCatcher in the browser (2026-09-25)
- **`NavigationStarting` is not a gate for the network.** A local server got
  `GET /secret-path` 17 ms after `location.href=…`, while the handler was
  still running (it slept 5 s to prove it); the netlog shows a preconnect and
  the main-frame request starting alongside the event. Cancelling stops the
  page, not the request. Fix: `WebResourceRequested` on document requests
  (`Fish.cs`) answers the stopped request with an empty 204 before it leaves.
- **`CoreWebView2.Navigate()` looks the name up before `NavigationStarting`**
  too (a made-up name showed in `Get-DnsClientCache`). Fix: check before
  handing the engine an address (`Tab.Forewarn`); then there is no lookup at all.
- **Engine-started navigations still get a speculative TCP/TLS connection**
  before any event reaches the host. `--disable-features=SpeculativePreconnect`
  (gone from Chromium 153) and `LoadingPredictor` didn't stop it *(which
  feature does is unknown)*.
- **`--host-resolver-rules`** (test runs: `SEARCH_HOST_RULES`) points a
  lookalike name at a local server, which then shows exactly which connections
  and requests a site would have seen. `*.localhost` is no substitute: FishCatcher never scores it.
- **Tools:** in this shell, `\x` inside a bash heredoc reached the file as a
  real character (a Segoe glyph became "çBA"); put such constants in C# with the
  Edit tool. The Edit tool also once dropped a space after `=` in a line it
  matched. Check `git diff` for line endings after scripted edits: a CRLF file
  (`Bench.cs`) came back LF.

### Shields 2.0 (2026-09-25)
- **Every `WebResourceRequested` handler hears every request that matches
  any filter on that view.** FishCatcher's document filter delivers to
  Shield's handler and the other way round; each handler checks the context
  itself.
- **Real lists are mostly names:** 93k of EasyList + EasyPrivacy's 110k
  network rules are `||domain^`. A name set instead of rule objects halved the
  memory. Measure with the real list, not a synthetic one.
- **One bad selector drops its whole CSS rule.** Chunk generic selectors (100
  a rule) and refuse extended syntax (`:-abp-`, `:has-text(`…) and anything
  that could close the rule (`{`, `}`, `;`, comments).
- **`adoptedStyleSheets` works at document creation**, before
  `documentElement` exists, and isn't blocked by a page's CSP the way an
  injected `<style>` is.
- **Auto-reject must not be clever.** Substring matching would click a
  headline "Senate to reject the bill"; an unconditional CMP `RejectAll`
  overrides the choice a person made on the site. Whole text, inside a
  consent container, and only while consent is pending.
- **Cleaning a URL is a navigation Search starts**, and those are allowed into
  `tools.search`; never let a tidied (or unwrapped) address lead there.
- **`pwsh -File x.ps1 -Modes a,b` passes one string.** A hashtable lookup on
  it gave `$null`, the test world got an empty `settings.json`, and Search
  quarantined it (`settings.unreadable-*.json`).
- Stopping `Get-Process Search` would also stop a real Search; stop test
  runs by PID.

## Round 2: Phase 2 fixes and integration (2026-09-26)

### Workflow isolation
- **Worktree creation:** three parallel agents built fixes on branches p2-field, p2-clip, p2-player. The session started in the parent `Search` repo instead of the `searchwin` subdirectory, so `git worktree add` created worktrees there (`../searchwin-wt/{branch}` from the parent). The agents then lost their branches when they `cd` back to the searchwin folder without pointing to the worktrees explicitly. **Fix:** create worktrees *by hand* with explicit target and branch name: `git worktree add ../searchwin-wt/branch-name branch-name` from the searchwin folder itself, and point each agent at the full absolute path `C:\Users\user\Projects\Search\searchwin-wt\branch-name`.

### Engine timing and the clipboard
- **WM_CLIPBOARDUPDATE arrives before data is ready.** PowerShell's `.SetText()` and other OLE-delayed copies were failing ~1–2% when the engine read the clipboard on the message itself. **Symptom:** copies were dropped or truncated; silence because the listener runs on a background thread. **Fix:** defer the read with `SetTimer(hwnd, 1, 60ms, NULL)` and read in `WM_TIMER` instead. The `WM_CLIPBOARDUPDATE` message records who copied (GetClipboardOwner's process, or foreground app as fallback) and the exclusion markers (no clipboard data read at this point); only the data itself waits the 60 ms. Result: 0% failure rate on 360 burst copies and 25 cross-process copies. **Also:** GetClipboardSequenceNumber before and after the read confirms no intervening copy corrupted the record.

### Test isolation: clipboard history opt-in
- **Symptom:** bench scripts created made-up clipboard strings (e.g. `p2clip-guid-world`) for testing. Early runs never printed or echoed these strings, but test worlds defaulted `clip.history` on like production, so the test strings were captured to disk. If a test used the real clipboard *anywhere* (saved it, printed it, or read it), the test would have corrupted the user's real clipboard history with these garbage entries.
- **Root cause:** no test explicitly checked clipboard history recording; the kit tests covered the logic, but no integration test read what was actually saved to disk. The bench's clip and field reports only showed rows captured during the current session (`RunStartedMs` filter), so pre-test leftover garbage was invisible.
- **Fix:** test worlds now opt in with `{"clip.history": true}` in their `settings.json`. Off by default in all test worlds means the real clipboard is never touched unless a test explicitly enables history recording. The default production (real profile) stays on. **Lesson:** *an integration test that touches the user's environment (filesystem, clipboard, credentials) must not do so in a default/production world. Require explicit opt-in.*

### SvelteKit hash router and media playback
- **SvelteKit's hash router (`kit.router.type = "hash"`) does not expose hash-embedded query strings in `page.url.searchParams`.** The router only extracts the path portion for route matching; query strings like `#/play?path=...` never reach `$app/state` or the stores. **Symptom:** the player route tried to read `page.url.searchParams.get('path')` and got null. **Fix:** read `location.hash` directly with `new URL(hash, origin)`, cache the path in local state, and resync on `popstate` and `hashchange` events. **Also:** `goto()` inside SvelteKit silently drops query strings in the hash; use `history.pushState()` directly for navigation.
- **CoreWebView2.Navigate unescapes percent-encoded fragments.** A path `%5C%3A` (backslash and colon, percent-encoded) arrives at the browser as literal `\:`. This is harmless when a component encodes with `encodeURIComponent`, but breaks assumptions about having percent-encoded URLs end-to-end. *Not critical, but surprising.*
- **Short synthetic test audio (1 s) makes bench-driven testing unreliable.** A file that reaches `ended` and auto-advances to the next track between bench calls makes a manual seek look like it reverted when it was actually the player's own next-track logic. *Use a longer test clip (e.g. 12 s) or pause playback before exercising seek/volume/mute.*

### Engine and the build process
- **Building the engine from source (`cargo build --release`) is not implicit.** The `publish-aot.cmd` script copies `Engine/target/release/kil-engine.exe` if it exists, without checking whether it's current. Leftover binaries from hours-old builds will be shipped with the app, silently losing any engine fixes committed since then. **Symptom:** a native-AOT build used a 14-hour-old engine, so secrets weren't marked with exclusion markers and the `clear_file_search_index` command didn't exist, even though both fixes were in the current source. **Fix:** publish-aot should run `cargo build --release` first, or at minimum pick the newer of the release and debug binaries (as `Engine.Executable` does in the running app).

### Row deduplication and the pointer
- **Rows are redrawn on every result batch, so a new row appearing under a still mouse pointer gets no `PointerEntered` event.** The fix was to track "pointer over the list" on the list container itself (the `Border`), not on individual rows. But the tracking is only reset by `PointerExited`, `PointerCanceled` and `PointerCaptureLost` — if any of those are missed (e.g. the window is deactivated), the flag sticks and every later question keeps unfilled top-hit spacers. **The FieldBoard.PointerOver guard needs to also clear on window `Deactivated` and when `Editing` turns false.**

### File operations on the UI thread
- **File.Exists() and ToolsHost.ServeFile's range-request checking both block on the UI thread.** For files on a network share or an offline mapped drive, each call can block for seconds. During media playback in ServeFile, this happens once per chunk request. **Fix:** move existence checks into `Task.Run` and report results via `UI.Do`, or drop the check and rely on `GetFileFromPathAsync` throwing `FileNotFoundException` for the 404 response.

### Omnibox state and the field model integration
- **Cause:** Omnibox.Put() remembered only the last text it put into a box and forgot it after the first change report. When the browser's `Recompose()` sets text twice in one keystroke (e.g., Show→Recompose puts '>dark', then Picked=0 puts the command's title 'Dark'), the TextBox reports two changes after the fact. Both changes read the final text 'Dark', so the second was mistaken for user typing. **Symptom:** >command and !bang became plain Google searches. **Root cause analysis:** the one-slot `expected` echo suppression is too simple. **Fix:** FieldEcho keeps the most recent text put into the box and updates it only when the box itself reports different text.
- **A popup inherits the gate of the address it's about to have.** Setting a
  new window's address optimistically let a web page's `window.open` pass the
  tools.search gate. Gate on the document that actually loaded.
- **Bench on a bug before believing the obvious cause.** "The seek on load does
  nothing" was really "the position was never saved": a backward seek, a pause
  of paused media and a track's end all skipped the save.
- **A copied binary goes stale silently.** publish-aot shipped an engine from
  hours earlier and fixes vanished only in AOT. Build what you ship, or refuse
  when it's older than its sources.
- **`robocopy /MIR` deletes what only the repo has.** A note written straight
  into docs/brain was wiped by the next vault → repo sync. Write notes in the
  vault only.
- **Allowlists for "open with its app".** A blocklist of runnable types always
  misses some (msix, rdp, iso, vhdx, py…). Open known documents, reveal the rest.
- **The old WinRT pickers fail in an unpackaged app.** `Windows.Storage.Pickers`
  show the dialog (hosted by `PickerHost.exe`), then the result is E_FAIL. Use
  the App SDK's `Microsoft.Windows.Storage.Pickers` with `AppWindow.Id`
  (`Search/UI/Pick.cs`). A bare `catch { }` hid this for months: log what
  you swallow.
- **Test the path the user takes.** Unit tests passed and bench verbs set
  folders directly, so neither the picker bug nor the content-search leak
  showed until the field and Settings were driven for real.

## Phase 1: Security fixes (2026-09-25)

### ABP pattern matching
- **Exponential wildcard backtracking:** a pattern like `/ad*a*a*a*a*zz` on a long URL took over 5 seconds (timeout). The old recursive Search would explore all possible split points of the URL. Fix: place each piece between '*' at its earliest match, left to right. Each piece has a fixed length ('^' is one char, or none only at the very end), and a '*' always follows, so an earlier end can only help later. This is exact, not a heuristic. No memo or backtracking needed; the new matcher is O(n).
- **Token index missed whole-word matches:** a word 'banner' was indexed if it appeared anywhere in the pattern, but matched only as a substring in URLs. When the pattern was `-ad-*banner`, the index filed it under 'banner' even though that word only appears in compound words like '-ad-bigbanner' in the URLs it should match. Similarly, 'adserver' missed 'myadserver.com' and exceptions were never tried. Fix: a token is only usable for indexing if it is bounded on both sides in the pattern — by a non-alphanumeric literal, by '^', or by a '|' anchor. A run at an unanchored end or next to '*' is not indexable; it goes to genericPlain. Lower MinToken from 3 to 2 so rules like `-ad-` or `/ad/` get their own bucket. On the real lists, 127 of 4,732 non-domain rules had been mis-filed.

### Signature verification
- **A blocklist rode along unsigned:** FeedBundle.cs accepted a blocklist in the remote feed even though the signature only covered the other fields (version, generated, sources, count, bloom). The unsigned blocklist then replaced the bundled one, deleting protection. Fix: only accept a blocklist when it is inside the signed v2 payload (v1 fields + blocklist, all signed together). A v1-only signature gives the Bloom filter and BlockList=null. `WithFeed` merges instead of replaces, so even a signed but empty blocklist can't empty the bundled one. Until the registry signs v2, only the Bloom is used from the feed.
- **Rollback was missing:** a malicious feed sent an older 'generated' date to roll back the protection. Fix: store the signed 'generated' date in the state file and refuse any feed whose date is earlier than the saved one. Forget clears both files, so rollback is reset.

### Cookie banner rejections
- **Consent container match too broad:** bare 'cmp' and 'privacy' substrings inside many non-CMP elements (AEM's cmp-button class, pages with a "privacy policy" link) were treated as consent banners. A button with text "Deny" anywhere on the page (like in a "Senate to deny the bill" headline) would be clicked. Fix: CONSENT_NAME now matches only known CMP names word-bounded, and the container must be a `dialog` tag or role, or have `position: fixed/sticky`. Dropped 'deny'/'deny all'/'no thanks' entirely from the fallback phrases.
- **Password pages broke the logic:** when a page has a visible password field anywhere, `cookie-reject.js` was settling the whole script (no CMP handlers, no leftover-banner CSS). Fix: apply the password guard only to the fallback text-button finder; keep the CMP APIs and CSS running.

### FishCatcher: Forge protection
- **Page-supplied facts reached Search's UI:** the page's own `chrome.webview.postMessage` with `fish.facts` could include forged gsbThreat, domain age, form-action text or other keys. These strings appeared in the warning page and could instruct users to call a fake number. Fix: `PageFacts.FromProbe()` reads only aitm and scam, never the fields that come from Safe Browsing or RDAP. Analyzer only accepts a GsbThreat when `Messages.IsSafeBrowsing` is true. FormAction.Run re-checks every destination with `IsHost`. All strings are capped to probe limits; resourceHosts at 40, formActions at 10, and counts are clamped to 1..100000.
- **Message budget starvation:** the rate limit of 6 fish.facts per document could be used up by the page before the probe even spoke, silencing the real warnings. Fix: `ProbeBudget` is reset in `ContentLoading`, which fires before any page scripts run. (Also: the probe now reads innerText once per look, not twice; the DOMContentLoaded look is structural only; the settled look runs at idle or 1.2 s after load or 5 s final; the overlay check uses `elementsFromPoint` at the window centre instead of measuring every div.)

### Parallel worktree merges
- **Clean separation by file set** allowed three parallel fixes to merge with zero textual conflicts: FishCatcher (Fish.cs, FeedBundle.cs, PageFacts.cs), Shields matcher (AbpPattern.cs, NetworkRule.cs, RegistrableDomain.cs), and browser wiring (Shield.cs, page scripts, lists defaults). Each agent worked in distinct files despite touching the same namespace.
- **Cross-branch integration gaps still need a grep:** the matcher branch added `LiveRegistrableDomain` and wired it into `SearchKit.Shields.FilterList`, but `Search.Core.Shield.cs` (owned by the browser branch) still called `SimpleRegistrableDomain` directly. The merge didn't detect this because `Shield.cs`'s line never conflicted. After merging, a `grep -rn SimpleRegistrableDomain` found the gap, and a follow-up commit fixed it.
- **Check commit identity:** one worktree had a typo'd committer email. Verify `git log --format=%an` after agents run.

### Testing in parallel
- **Unit tests caught the forgery immediately:** a test feeding `FromProbe` a page with 20 forged fields (gsbThreat 'Your PC is infected. Call 1-888...', youngDomainDays, form actions like 'call support now') showed every forged value was dropped. The resulting `Reasons` contained only fixed sentences from `Messages.cs`.
- **Fuzz testing for the index:** 60,000 random requests against a brute-force pass over every parsed rule (with mixed separators, `*`, `^`, `|`, options like `$third-party/$1p`, exceptions, trailing-dot hosts) gave 0 mismatches. The real lists showed 0 mismatches too.
- **End-to-end verification without hand-testing:** test worlds with `SEARCH_HOST_RULES` (--host-resolver-rules pointing lookalike domains at a local server) and `SEARCH_PROBE=worldname` let us verify the full path: a page posting forged facts got the expected low verdict; a real phishing-like form got Critical or High; the probe's budget worked; etc.
### Phase 1, round 2 (2026-09-25)
- **Count every failing shape before calling a matcher gap fixed.** Reviving
  17 odd `||` rules hid 125 more (names ending in a dot). Grep the real lists for everything that fails the parse gate.
- **Never exempt a request because of headers a page can set** (Accept is
  CORS-safelisted). Say "navigation" only when the engine does, or when the
  URL is the one a tab is loading.
- **A value captured at document start is only safe if it's called safely:**
  `send.call(...)` reaches for `Function.prototype.call` again. Bind once with
  the original `bind`, and capture every built-in you use later.
- **`\b` fails on real class names** (`cookie_notice`, `cookieBanner`):
  underscores and letters are both word characters. When a structural check
  already bounds false positives, a plain substring match is right.
- **Agents mirroring docs one way can overwrite each other's docs.** The notes
  agent's vault → repo `/MIR` reverted the integration agents' committed
  `docs/brain`; recovered with `git checkout -- docs/brain`. Sync from the newer side only.

## Phase 3: Tools as pages + packs (2026-09-27)

### JSON and AOT
- **`JsonArray.Add(JsonObject)` binds to generic `Add<T>` under AOT,** triggering IL2026/IL3050 warnings. Only a full AOT publish shows them; Debug is silent. **Fix:** type values as `JsonNode`, not `JsonObject`. Apply this to packs.json parsing.

### Media and Chromium
- **Chromium plays AC-3 audio in MP4 silently instead of raising an error.** A remux check using playback readyState and currentTime can't detect the codec mismatch. **Fix:** probe FFmpeg streams (ffprobe) before trusting a playable container. Player.cs does this; live checked: a problem AC-3 MP4 probed with 'ac3' audio codec, got converted, and played correctly after.
- **A hidden or minimised WebView2 tab never loads.** `readyState` stays 0, `play()` never settles, media tests hang. **Fix:** ShowWindow(SW_RESTORE) + SetForegroundWindow on the Search process itself before driving media. (Other agents' test windows can also minimise yours; it's a shared screen.) A bench tab's minimise point is the frame value (-32000).

### FFmpeg and security
- **BtbN daily FFmpeg builds are deleted after ~2 weeks.** Month-end autobuilds are kept long-term (back to 2024-10) with a `checksums.sha256` file. **Fix:** always pin a month-end build tag (e.g. `autobuild-2026-08-31-13-27`), never 'latest' or a daily tag. The NOTICE should say this, but said '~30 MB' instead (actually 144 MB unpacked); updated on merge.
- **FFmpeg can be tricked by file content, not name.** If a user downloads a crafted HLS playlist as 'film.mkv', FFmpeg's demuxer reads the content and fetches from `file://` URLs inside it, potentially reading other local files. **Fix:** add `-protocol_whitelist file,pipe` before every `-i`, and force sidecar subtitle demuxers from their extension (ffprobe doesn't get the protocol list, so it can also be tricked). Kit RemuxTests has no case for this yet; it's a real gap.

### Drag and drop with WebView2
- **WebView2's `postMessageWithAdditionalObjects` only accepts disk-backed `File` objects.** Synthetic Files (created in JS) are rejected ('not a file on the disk'). **Fix:** the bench and unit tests can't exercise real drag-drop; wrap the page's postMessage in the test and answer `host:drop.paths` with the objects it sends, so no native dialog is needed.

### SvelteKit and routing
- **Setting `location.hash` under SvelteKit's hash router (`kit.router.type = "hash"`) triggers `location.reload()`.** It's treated as a user edit to the address bar. **Fix:** navigate using goto() or click a link. For playback, read `location.hash` directly and use `history.pushState()` for navigation; listen to `popstate` and `hashchange` events.
- **SvelteKit hash router refuses `+server.ts` routes entirely,** even with prerender. A build-time JSON file needs a post-build script using Vite's `ssrLoadModule` to import and run the server code at build time.

### Theme and design tokens
- **Tool pages already follow the browser's theme for free.** Every tab gets the same `CoreWebView2Profile.PreferredColorScheme` from the browser (set per profile), so `@media (prefers-color-scheme: dark)` switches live. No host call or event needed; theme token changes are instant across all open tabs.
- **Per-tool inline `<style>` blocks with hard-coded fallback colors contradict a central token layer.** Examples: CSV Toolkit's pink icon tile `var(--color-accent, #6d7cff)` versus the actual `--color-accent: #b5352c` (red) from styles.css. These fallbacks are now unreachable; the central mapping defines all token names. Future: a tool audit could remove the mismatched fallbacks.

### Tauri shim and window labelling
- **The Tauri shim labels every tool tab `window.label = 'tool'`,** so any Workspace layout code gated on `if (label !== 'overlay')` was running in every tool tab (voice listeners, push-to-talk, command mode). **Lesson:** check for window-label gates when porting Workspace code. The solution is to either remove the code (if it belongs in the browser only) or add a real gate (if it's per-pack).

### Favicons and tabs
- **All tool tabs at `https://tools.search/…` shared one favicon** because they're all on the same host. HistoryPanel, Tab.AdoptIcon, and Favicons.Fetch all keyed by hostname. **Fix:** key by `Address.IconKey` (hostname, or tools.search-<id>) so each tool gets its own per-tool favicon (a Lucide SVG in the browser's muted colour, drawn per tool).
- **A bench `shot` screenshot times out if the test window is minimised** (frame -32000). `probe` command shows the frame value; call ShowWindow(SW_RESTORE) on your own test process to fix it. Don't assume one agent's test windows don't minimise yours.

### Svelte 5 event handling
- **Svelte 5 delegates `onkeydown` and similar handlers to the document root.** Synthetic test events need `bubbles: true` to reach the handler. Unit tests (vitest, node --test) failed to trigger key handlers until this was fixed.

### Git workflow and CRLF files
- **`git diff --check` flags every added line in a CRLF file as trailing whitespace** because `core.whitespace` isn't set to `cr-at-eol`. This is a repo-wide false positive for any CRLF file edit. Run `git -c core.whitespace=cr-at-eol diff --check` instead. Don't set `core.whitespace` globally (out of scope; touches shared config). A Git Bash `sed -i` can silently normalize CRLF to LF on read, even without the `-i` flag; use PowerShell `[System.IO.File]::ReadAllBytes/GetString` for reliable byte inspection on Windows.
- **A bench stray file can be accidentally swept into a merge-resolution commit** via `git add -A`. After committing a conflict resolution, re-check `git status --short` to catch unintended additions.

### Integration testing and safety
- **Parallel builds by multiple agents can race on a test world.** If Builder A and Builder B both use a SEARCH_PROBE world with the same name, they interfere. Use machine-unique names (UUID or timestamp) and communicate the name through the task description or a shared file, never as a side effect of builder A's run.
- **Rebuild the engine before shipping.** A leftover binary from an hours-old cargo build gets shipped silently; fixes committed since then are lost in AOT only. The publish-aot.cmd script should run `cargo build --release` first or refuse to ship when the binary is older than its source files.

### File operations and latency
- **File.Exists() and ToolsHost.ServeFile's range-request checking block on the UI thread.** For network shares or offline mapped drives, each check can block for seconds. During media playback (one check per chunk request), this multiplies. **Fix:** move checks into `Task.Run` and report via `UI.Do`, or drop the check and rely on exceptions for the 404 response. The player (Server-side ToolsHost.ServeFile) still has this issue; it's not critical for local files but noticeable on network storage.

### Cut lists and hidden screens
- **A screen marked `hidden: true` in appScreens.ts is not the same as removing it from the index catalog.** When two different code paths manage visibility (appScreens.ts's inline `hidden: true` and offInSearch.ts filtering), the last write wins silently. Tools that should be hidden but appear in the field, index and catalog.json are a subtle bug. **Fix:** a single source of truth (offInSearch.ts only) and one test asserting all offInSearch ids are actually excluded (not just checking that the ones in offInSearch don't appear; also checking that tools with stale `hidden: true` don't appear either).
