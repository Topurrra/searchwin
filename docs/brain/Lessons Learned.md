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
