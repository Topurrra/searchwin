---
tags: [searchwin, decisions, adr]
updated: 2026-09-26
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

### D20 · A stand-in Tauri crate, not a rewrite of the commands (2026-09-25)
- **Decision:** `Engine/compat/tauri` + `tauri-macros` provide the few Tauri
  APIs the engine's commands use. `#[tauri::command]` generates a hidden module
  with a JSON entry point; `generate_handler!` builds the name → entry table.
- **Why:** 390 commands and 650 Tauri references would take weeks to rewrite,
  and would drift from Workspace. With the stand-in, the command code is
  unchanged: only 3 import lines moved (the throttle module).

### D21 · The engine is a separate process, over a user-only named pipe (2026-09-25)
- **Why:**
  - the first window stays fast;
  - a crash in a heavy tool doesn't take the browser down;
  - the engine can later stay resident alone (tray mode).
- **Security:** a protected DACL with a single entry (the current user), remote
  clients rejected, and the first instance claims the name (a second engine exits with code 2).

### D22 · Tool pages: hash routing, served from a folder mapping (2026-09-25)
- **Decision:** `https://tools.search/index.html#/tool/<id>`, with WebView2
  `SetVirtualHostNameToFolderMapping`, and a bridge over `chrome.webview.postMessage`.
- **Why:** there's no server to answer deep links (hash routing), and there's no
  custom scheme to register. The bridge answers only pages whose top-level
  source is tools.search, and web pages can't navigate a tab there.

### D23 · Portable logic lives in `Search.Kit`, namespace `SearchKit` (2026-09-25)
- **Why:** it builds and tests without WinUI, on any OS. The namespace isn't
  `Search.Kit`, because the browser's UI class `Kit` would hide it.

### D24 · Word ⇄ PDF through Word or LibreOffice, never our own converter (2026-09-25)
- **Decision:**
  - Microsoft Word first, driven silently through its automation interface
    (hidden, no alert dialogs, always quits, with a hard timeout);
  - then LibreOffice headless, with its own profile folder;
  - neither installed: Settings offers LibreOffice as a download.
- **Why:** the user's month on a pdfium-based converter came nowhere near
  iLovePDF's quality. The technique follows KeepItLocal Redact's desktop app
  (`privacy_core/office/convert.rs`), **re-written** for Search, not copied (D18).

### D25 · No yt-dlp (2026-09-25)
- **Why:** Microsoft Store rejects YouTube downloaders, and the Store is the
  distribution plan. Revisit only for a direct-download-only edition.

### D26 · Search's own pages and dialogs, never Edge's (2026-09-25)
- **Decision:**
  - a page that fails gets Search's trouble page (`PageTrouble`, drawn in `Stage`);
  - a bad certificate gets Search's warning, with "continue anyway" for that site for the session;
  - `alert`/`confirm`/`prompt`/"leave page?" and HTTP sign-in get Search's dialogs.
- **How:** `ContentLoading.IsErrorPage` covers the engine's error page the
  moment it arrives. `ServerCertificateErrorDetected` → Cancel shows our
  warning. `AreDefaultScriptDialogsEnabled = false` + `ScriptDialogOpening`, and `BasicAuthenticationRequested`.

### D27 · FishCatcher warns by default; its feed is opt-in (2026-09-25)
- **Decision:** "Warn about scam and phishing sites" is on for everyone: the
  check is all on the computer and asks nobody. The daily signed feed is off
  until turned on in Settings › Privacy, because it's a download (the
  no-network rule; the extension made it opt-in too).
- **Warn, never block:** High/Critical get Search's warning page with a quiet
  "Continue anyway" (that host, until quit); Elevated gets a line at the
  bottom; frames are left alone. Fail open on anything unexpected.
- **How:** checked before `Navigate()`, in `NavigationStarting`, and at the
  document request (`WebResourceRequested`), since the event alone doesn't
  hold the request back (see [[Lessons Learned]]).

### D28 · Shields: one "*" filter and our own matcher; the full lists are opt-in (2026-09-25)
- **Decision:** the engine gets one request filter (`*`, all contexts, the
  page's own requests) and every request is decided in `Shield.cs` against a
  compiled list (`SearchKit.Shields.FilterList`), the spike's option B. One
  filter per domain (the old way) costs more to add with every filter: 37 s
  a tab at 10k filters, about an hour at 100k. The MV3 extension (option C)
  stays the fallback if UI-thread contention ever shows.
- **The lists are opt-in:** "Full ad and tracker lists" (EasyList +
  EasyPrivacy from easylist.to, checked once a day) is **off** until turned on
  in Settings › Privacy, because it's a download (the no-network rule in
  CLAUDE.md; the Cloud Agent Brief calls filter lists "user-started
  downloads"; D27 did the same for FishCatcher's feed). Without them the
  built-in 44 domains and 9 slot selectors block, as before. **To make them
  the default, flip `shield.lists` in `Prefs.cs`** — nothing else changes.
- **On by default** (all on this computer, nothing asked of anyone): blocking,
  "Remove tracking from links" (tags, redirect wrappers, AMP) and "Dismiss
  cookie banners" (reject only, never accept).
- **Cosmetics:** one shared stylesheet per list (the same for every page,
  handed over once per tab and adopted by the page) plus a small per-site one
  swapped in at each navigation. Pausing a site turns all of it off there.

### D32 · Tests follow SKILL.md (test-audit) (2026-09-26)
- **Decision:** every new or changed test passes its authoring gate (the four
  questions in SKILL.md, test-audit skill: does it test production or only
  test code? Is it a known regression? Does it cover the boundary? Would it
  catch a break?). A bug's regression test must fail on the pre-fix code. Dead
  tests (no production caller exists) are removed. Adopted test commands map to
  the repo's own: `dotnet test Search.Kit.Tests`, `node --test Search/Assets/js/tests/`,
  `pnpm vitest run` in `Tools/`, `cargo test --no-default-features` in `Engine/`.
- **Why:** the round 4–5 audit removed 226 test rows that tested dead code paths
  (LocalSources, FieldLayout.Build, FieldBoard.Walk, etc.). The browser's
  FieldModel constructor never passed `local: []` in production, so entire
  families of tests never exercised live paths. A test that only tests test
  scaffolding has no contract to own. **Consequence:** every test survives as
  long as its owner does, and a failing test failure must be real.
- **$autoreview** in CLAUDE.md means a pass over the diff that verifies: every
  added/changed test fails on pre-fix code (git checkout before each run),
  tests never import test-only mock code that production doesn't call, and
  `git diff --check` shows no tab/trailing-space issues.

### D29 · Protection updates on by default (2026-09-25)
- **Decision:** after phase 1's security hardening, the two protection updates
  now ship **on by default** instead of opt-in:
  - **"Full ad and tracker lists"** (`Prefs.shieldLists`): default `true`. Downloads EasyList + EasyPrivacy daily from easylist.to, compiled and cached.
  - **"Daily list of reported scam sites"** (FishCatcher feed, `Prefs.scamFeed`): default `true`. Downloads the signed blocklist/Bloom filter daily from FishCatcher's registry.
- **Why:** both go only to their publishers (no browsing data sent), send no
  requests if the user is offline or has disabled Shields, and have been
  hardened against tampering (signed v2 payload, rollback protection, rate
  limits). User-side blocking (EasyList) and on-device detection (FishCatcher)
  are the baseline. A fresh install now protects against ads and known scams
  from day one.
- **Settings text:** "Full ad and tracker lists: Updated in the background from
  the EasyList publishers — nothing about your browsing is sent, and only the
  signed parts of what comes back are ever used." "Daily list of reported scam
  sites: Updated in the background from FishCatcher's registry — nothing about
  your browsing is sent, and only the signed parts of what comes back are ever
  used."
- **Revisit:** if a user's bandwidth is severely constrained or if a download
  ever fails to verify, make an offline-first build available for distribution
  with the lists pre-built (`lists.bin` 3.6 MB) and the feed pre-signed
  (fishcatcher-feed.json 791 KB). That would reduce fresh-install network use
  from ~4 MB to ~500 KB while keeping updates automatic.

### D30 · Clipboard history on by default (2026-09-26)
- **Decision:** clipboard history is on by default for real users. Settings ›
  Clipboard turns it off (and offers to clear what's kept, pins aside), sets how
  long it's kept, keeps pictures or not, pauses, and lists excluded apps
  (password managers). The engine starts about 3 s after the first window is
  activated, never before it.
- **Why:** the user's call: history is "much better and useful" on by default.
- **Secrets:** the engine tags copies it recognises (API keys, tokens, cards)
  and keeps them only briefly. They show as "Hidden: {kind}" and are never put
  in a field row. Shift+Enter never searches or opens them. When Search puts a
  secret back on the clipboard, or its own tool pages copy a password, it sets
  the markers that tell Windows' clipboard history (Win+V), the cloud clipboard
  and other clipboard managers to skip it (QuietCopy).
- **Revisit:** if people are surprised by it. A one-time line in the popup and
  on the Welcome page says what's kept and where to turn it off.

### D31 · Test worlds keep clipboard history off (2026-09-26)
- **Decision:** in test worlds (`SEARCH_PROBE`) clipboard history defaults to
  off; a test opts in with `{"clip.history": true}`. The real profile keeps D30.
- **Why:** the engine's listener records every copy on the machine, not just
  Search's. With history on in test worlds, agents' test runs captured the
  user's real copies from other apps, and bench output (`clip`, `field`)
  could print them. Tests must never read the real clipboard.
- **How:** `ClipGuard.OnByDefault(testing)`; bench clip/field output also drops
  entries captured before the run started (`RunStartedMs`).

### D33 · Word ⇄ PDF moves to phase 4 (2026-09-26)
- **Decision:** phase 3 is tool pages plus the packs framework and the FFmpeg
  pack. Word ⇄ PDF (D24: Word silently, else LibreOffice) moves to phase 4.
- **Why:** the user's call, to keep phase 3 focused.
- **Until then:** the Word converter tool page stays out of the tools index.

### D34 · Tools organize by packs; packs are downloads (2026-09-27)
- **Decision:** Search ships **32 tools** across **8 packs** (Utilities, Files, Privacy, Images, Documents, Development, Media, Focus). Tool pages are restyled with Search's design tokens, not Workspace's. Each pack can be installed, updated and removed through Settings › Packs.
- **Core pack (always on):** field, shields, FishCatcher, converters, bangs, clipboard, file search. Core never downloads.
- **Optional packs:** Utilities, Files, Privacy, Images, Documents, Development, Media, Focus are all optional and download on demand. Each tool is a web page at `https://tools.search/tool/<id>`.
- **Packs framework:** manifest (embedded at build, specifies id, version, size, SHA-256, https URL, licence, build page, exact source commit), download + stream + hash while downloading, stage atomically, verify before unpacking, list only specified files, move into place if complete. Remove is also atomic; old version stays until new one is in place, crash leftover swept on next install.
- **Why:** keeps the binary lean (no tools bundled); every tool update is optional; each tool is independently removable. Manifest is the source of truth; the exact source commit is always in version control.
- **Revisit:** when more packs land (Voice/Tesseract/OCR/semantic search).

### D35 · FFmpeg pack: BtbN month-end build, LGPL, pinned commit (2026-09-27)
- **Decision:** the FFmpeg pack uses BtbN's month-end autobuild (`autobuild-2026-08-31-13-27`, ffmpeg-n8.1.2-50-g1a748fe2cd), win64 LGPL shared, without x264/x265. Download is 70.8 MB; installed is 144 MB.
- **Why:** month-end builds are kept long-term; daily builds are deleted after ~2 weeks, causing broken links. LGPL (not GPL or AGPL) allows redistribution. Shared (not static) is smaller. h264_mf (Windows hardware encoder) and no x264 save 50+ MB.
- **Exact source:** commit 1a748fe2cd43e3ead22fafb1b5b7d77f153898a8 (verified on GitHub). Licence LGPL 3.0-or-later in `third_party/FFmpeg-NOTICE.md`. Settings › Packs shows the licence, build page and source link.
- **How:** pack manifest specifies version, size, SHA-256, URL, licence URL, build URL, and source. Player (remux, transcode), Media Utility (extract MP3, compress) use it when installed. Without pack: gate shows "plays with the FFmpeg pack" + Settings › Packs button.
- **Updates:** if a newer month-end build exists, update packs.json version, size, sha256, url, source. Test with `pnpm build` + `cargo test` + live in SEARCH_PROBE world. No force re-download; installed versions persist until removed.
- **Revisit:** when FFmpeg's monthly builds expire or when a significant codec is added/removed.

### D36 · OCR: Windows Core + Tesseract pack (deferred post-phase-3) (2026-09-27)
- **Decision:** following the Master Plan, OCR should use Windows' built-in OCR in Core, with Tesseract as an optional pack. Neither is wired yet; OcrTool and CamScanner show a "locate Tesseract" gate (shell-out only).
- **Why:** Windows OCR doesn't require a download; Tesseract adds ~80 MB for users who need other languages or batch OCR. Splitting them (Core + pack) keeps the binary small.
- **Current:** shell-out pattern in `Engine/src/commands/ocr.rs` (bundled exe, PATH, or Program Files). No pack integration, no Windows-OCR fallback.
- **Next step:** Builder A's packs framework (core/packs.rs, bin_dir()) is ready; sync with Builder A on a shared `resolve_*` helper (for FFmpeg and Tesseract) before implementing either pack. Cross-team coordination needed (task 2591b36a).
- **Revisit:** after Tesseract pack is wired and Windows-OCR fallback is added.

### D37 · Reminders and Cron task scoping bug (known issue, not fixed in phase 3) (2026-09-27)
- **Decision:** the Reminders tool is listed and ships in phase 3 despite a known bug: opening Reminders calls `reconcile_reminder_tasks`, which deletes all `\KeepItLocal\*` scheduled tasks — including Cron panel tasks and Workspace's own Reminders. The scoping is wrong.
- **Why shipped:** the bug was discovered during merge verification and documented in [[Log/2026-09-27#Blockers]]. Shipping phase 3 and fixing this in a follow-up is faster than rolling back. The bug only manifests if someone opens the Reminders tool; it's not a silent issue.
- **Workaround:** don't open Reminders in a Workspace world. Test worlds should not have real Workspace reminders/tasks anyway (D31 keeps them isolated).
- **Fix:** match exact folder `'\\KeepItLocal\\'` instead of wildcard, or move Search's tasks to `\\Search\\<world>\\Reminders\\` per world. For now, add 'reminders' to `offInSearch.ts` (hide it) until Cron is scoped.
- **Revisit:** in phase 4 when background mode (Cron for global hotkeys) is wired, scope both Reminders and Cron to their own per-world folders.

### D38 · Screen Recorder shipped but broken (known issue in phase 3 installer) (2026-09-27)
- **Decision:** Screen Recorder is listed in the tools index and field despite a known blocker: the engine is built without screenrec feature, and its Tauri toolbar/region-picker/redaction windows don't work in the WebView2 shim.
- **What happened:** Builder A set `hidden: true` in appScreens.ts and added it to offInSearch.ts. Builder B's change to appScreens.ts line 834 (`tool.hidden = tool.id in offInSearch`) overwrites the inline `hidden: true`, and searchTools.ts filters only on offInSearch membership. Screen Recorder was never added to offInSearch.ts.
- **Result:** it's in dist/catalog.json, the tools index, and the field. Users can open it but get a "Screen recorder is not available in this build" message.
- **Fix:** add `'screen-recorder': '…'` to `offInSearch.ts`, add a regression test in index.test.ts asserting it's not listed, rebuild and re-publish the installer.
- **Revisit:** in phase 4+ when the engine is built with screenrec and native Search overlays replace the Tauri windows.

### D39 · PaddleOCR, not Tesseract, as the OCR pack (2026-09-27)
- **Decision:** Windows OCR stays in core; the optional OCR pack is PaddleOCR.
- **Why:** the user's call. Tesseract was only needed for Georgian, which
  PaddleOCR doesn't support; Search is international, so that's acceptable.

### D40 · Each feature keeps its own optimization note (2026-09-27)
- **Decision:** [[Optimization]] covers what exists now; every future feature
  or phase gets its own optimization note, linked from it.
- **Also planned:** T3 Code inside Search, for the agentic browser (Master Plan › Future).

### Review note — 2026-09-26 (no new product decision)

The [remaining-work roadmap](../superpowers/plans/2026-09-26-remaining-roadmap.md)
reconciles current source with the vault and proposes stabilization before further
feature phases. Existing decisions remain authoritative; the proposed sequence
has not yet been adopted as a new product decision.
