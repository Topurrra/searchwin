# SearchWin Remaining Work Plan

> **For agentic workers:** Use superpowers:executing-plans or superpowers:subagent-driven-development when implementing an approved work package. This is a project-level roadmap; split each package into a focused implementation plan before changing runtime behavior.

**Goal:** Finish and optimize the existing browser and tools, then deliver the remaining features in dependency order.

**Architecture:** Keep the native C#/WinUI browser, portable Search.Kit logic, lazy Rust engine, and Svelte tool pages. Extend those boundaries rather than introducing a replacement shell or duplicate command/index systems.

**Tech stack:** .NET 9 / Native AOT / WinUI 3 / WebView2; Rust; Svelte / TypeScript; named-pipe IPC.

**Spec:** `docs/brain/Master Plan.md`, `Decisions.md` (later decisions supersede earlier ones), `Fixes.md`, `Optimization.md`, and the Ideas notes. Reviewed against the separate Obsidian vault and repository HEAD `904708d` on 2026-09-26. Some notes are dated 2026-09-27; their dates are not proof of completion.

## Scope and evidence

This is a documentation and source review of the main architecture, feature wiring, known follow-ups, packaging, and test infrastructure. It is not a line-by-line review of every imported tool, a security audit, or a live UI/performance certification. No runtime implementation was changed.

Fresh checks:

- Search.Kit: **758 passed**, with two existing xUnit analyzer warnings.
- Browser JavaScript: **125 passed**.
- Tool tests: not completed. The available pnpm wrapper attempted dependency reconciliation and stopped without a TTY; direct Vitest invocation then failed reading its package configuration with an operation-not-permitted error. Preserve the lockfile/modules and reproduce with the intended toolchain before treating this as an application failure.
- Rust tests, AOT UI checks, startup/latency/memory measurements, and installer checks were not rerun. Previous log results remain historical evidence only.

## Current state

| Area | Evidence-backed status |
|---|---|
| Browser | Native tabs/spaces, profiles, history/bookmarks, passwords, downloads, reader, extensions and sleeping-tab code exist. Real-account/import/device checks remain in the older roadmap. |
| Foundation | Rust helper, named-pipe client, tool host/shim and command registry implemented. |
| Protection | FishCatcher and Shields implemented; a documented live YouTube ad regression remains unverified/unresolved. |
| Universal field | Converters, bangs, command routing, indexed search, clipboard and file/media routes implemented. |
| Tools | Existing generated catalog has 32 entries: Utilities 7, Files 6, Privacy 3, Images 5, Documents 2, Development 4, Media 2, Focus 3. This includes the broken Screen Recorder entry. Nested Dev panels are not separate catalog tools. |
| Packs | Installer/updater/remover framework exists. `Search.Kit/Packs/packs.json` contains **one downloadable pack: FFmpeg**. Eight tool categories are not eight independently downloadable packs; `build.ps1` bundles the tool pages. |
| Media | Player, conversion cache, subtitles, range serving, extract/compress tools implemented. Non-native playback waits for complete conversion. |
| Future systems | Imported voice/snippet/model-related engine code is groundwork, not proof of an integrated Search experience. Phases 4–7 remain open. |

**Assessment:** phases 0–2 have substantial implemented foundations; phase 3 is implemented with release-blocking follow-ups. Do not describe it as fully verified and finished.

## Global constraints

- Browser first; first window ≤300 ms; local field work <5 ms; indexed results target <50 ms.
- Engine idle target <60 MB; core installer target ≤45 MB. Measure these separately from the full WebView2 process tree.
- Nothing heavy before the first window; optional models load on use and release when idle.
- Native AOT safety and source-generated serialization remain required.
- Background/tray mode stays off by default. Test worlds keep clipboard recording off unless explicitly opted in.
- Follow the current protection-update exceptions and user-started download policy. AI/network features need explicit settings and send behavior consistent with the privacy plan.
- Keep Workspace and the Mac source repositories untouched. No KeepItLocal Redact code or file/audio/video redaction integration.
- Windows OCR in Core, PaddleOCR optional: D39 supersedes the Tesseract-pack proposal.
- Word ⇄ PDF uses Word automation, with LibreOffice fallback; no bespoke document conversion engine.
- No yt-dlp. C++ shell replacement is a separate, optional long-term project.

## Review focus

1. Test worlds must not modify another world's scheduled tasks, profile, clipboard history or files.
2. A hidden tool must not retain unintended privileged command access through the shared bridge.
3. Cancellation, app exit and pack updates must not leave converter processes or locked files behind.
4. Tool-tab sleeping must not suspend active jobs or lose unsaved state.
5. Imported engine functionality must work under Search's process lifecycle, arguments and native UI, not assume the old Tauri shell.

## Part A — fix and optimize what exists

### A1. Close release blockers first

**Owners:** `Engine/src/commands/reminders.rs`, `cron_tasks.rs`, `Tools/src/lib/offInSearch.ts`, `appScreens.ts`, `Search.Kit/Web/ToolCalls.cs`, `Search/Core/ToolsHost.cs`.

- [ ] Isolate reminder and Cron tasks under distinct Search/world folders. Reconcile only tasks owned by the exact current namespace; never wildcard the shared KeepItLocal tree.
- [ ] Verify reminder activation end to end. Creation currently uses `current_exe()` inside the engine, so the scheduled program is the engine, not necessarily the browser that can present the reminder. Define and test the correct activation path as part of this fix.
- [ ] Hide Screen Recorder consistently from index, field and direct routes until supported. Contain Reminders until task isolation and activation are verified.
- [ ] Enforce unavailable/disallowed operations at the tool bridge, including hidden hardening and recorder commands. UI visibility alone is insufficient.
- [ ] Replace shell-open behavior in Cleaner/Analyzer and Duplicate Preview with reveal or the browser's intended file-opening route. “Open location” must not execute the selected installer/script.
- [ ] Resolve ownerless recycle/permanent-delete prompts from the headless file-manager engine.
- [ ] Restrict FFmpeg/probe/subtitle input protocols and force known subtitle formats. Protocol restrictions reduce network access but do not, by themselves, prevent all unwanted local-file access; test playlist/disguised-input behavior too.

**Done when:** regression tests fail on old code and pass on the changes; a synthetic world cannot remove another world's tasks; unavailable routes and commands are refused; opening locations does not execute files; delete confirmation is visible and cancellable. Use disposable fixtures, never real scheduled tasks for the destructive regression.

### A2. Repair the media experience

**Owners:** `Search/Core/Player.cs`, `Search.Kit/Media/Remux.cs`, `Search/Core/ToolsHost.cs`, `Search/Core/Browser.Field.cs`, `Tools/src/routes/play/+page.svelte`.

- [ ] Reproduce controls disappearing in a normal tab; constrain video and stage height so controls remain reachable in small windows and portrait media.
- [ ] Measure direct playback before rewriting it. The Svelte page already assigns the direct URL immediately and runs `prepare(..., 'check')` asynchronously; “remove a blocking probe” is not the demonstrated fix.
- [ ] Measure file metadata/open time and first Range response. Preserve codec verification: an MP4 extension does not guarantee usable audio/video.
- [ ] For incompatible containers/codecs, prototype incremental fragmented-MP4 playback. The current `Player.Make` awaits FFmpeg completion before publishing a playable file. Replace this whole-file barrier with a bounded buffer and explicit stream lifecycle.
- [ ] Define seek restart, cancellation, subtitle availability, unsupported hardware encoder fallback, concurrent tabs and disk-space behavior before committing to the streaming implementation.
- [ ] Reuse file-identity keyed probe results; verify cache invalidation and pack update interaction.
- [ ] Make File Manager and field use the same `FileKinds.Opening` decision, including MKV when FFmpeg is installed.
- [ ] Finish Media Utility trim/convert and diagnose compress-to-size overshoot after basic playback is reliable.

**Done when:** short and two-hour fixtures cover direct playback, MKV with incompatible audio, subtitles, seeking beyond buffered output, cancellation and reopen. Targets from Optimization: direct first frame <300 ms, remuxed <1 s; report measured hardware and percentile results rather than promising universal timings.

### A3. Reproduce and fix YouTube protection

**Owners:** `Search/Assets/js/youtube-shield.js`, `Search/Core/Shield.cs`, script injection in `Tab.cs`, JS tests.

- [ ] Capture a failing case in an isolated profile, including initial load, in-page navigation and restored tabs.
- [ ] Trace which response/event path leaves the advertisement active; inspect current upstream behavior only after obtaining the reproduction.
- [ ] Add a minimal regression fixture, then change the affected handling and verify ordinary playback remains intact.

**Important correction:** the current script already matches both `/youtubei/v1/player` and `/next`. The Optimization note's “only player” suspicion is stale. Server-side insertion and injection timing are hypotheses, not established causes. Passing JSON-pruning unit tests does not establish live ad blocking.

### A4. Improve settings and pack lifecycle

**Owners:** `Search/UI/Panels/SettingsPanel.cs`, `Search/Core/Packs.cs`, `Search.Kit/Packs/PackStore.cs`, `Engine/src/commands/ffmpeg.rs`, `Search/Core/Commands.cs`, `Installer/Search.nsi`.

- [ ] Replace full Settings redraw on each 250 ms progress tick with updates to the affected pack row. Preserve focus and scroll.
- [ ] Match the field's hover behavior; improve grouping/descriptions, then evaluate settings search against actual navigation difficulty.
- [ ] Make a user's selected FFmpeg win over pack discovery consistently.
- [ ] Coordinate running media jobs with pack updates/removal; await termination where necessary before file swaps.
- [ ] Sweep interrupted removal leftovers and test interrupted install, same-version repair, hash failure and locked files.
- [ ] Show license/source links before download; fix NOTICE tag wording.
- [ ] Make bare `>ffmpeg` open pack information rather than start the download.
- [ ] Remove regenerable pack/play-cache files on uninstall according to the chosen profile-retention behavior.

**Done when:** keyboard focus survives download progress; interrupted operations recover without corrupting the working version; selecting a command does not unexpectedly download a large pack.

### A5. Measure field/index and resource behavior at scale

**Owners:** `Search.Kit/Field/*`, `Search/Core/FileIndex.cs`, `Engine/src/commands/search.rs`, `Engine/src/core/throttle.rs`, `Search/Core/Browser.Sleep.cs`, `Search.Kit/Engine/EngineLauncher.cs`.

- [ ] Record cold/warm startup, local keystroke latency, asynchronous result latency and cancellation under indexing load.
- [ ] Use a deterministic 100k-file corpus: prefix recall, multiword content queries, renames/deletes and watcher behavior under excluded/ignored folders.
- [ ] Measure idle engine and full process-tree memory with the index loaded, clipboard enabled and several tool tabs open. Optimize measured hotspots.
- [ ] Validate tool-tab suspend/resume against conversion, focus timers, unsaved documents and active file operations. Sleeping-tab support already exists; extend its activity rules where needed.
- [ ] Verify engine-wide resource limits separately from indexing-worker limits. Worker Job Objects exist in `search.rs`; the shell launcher inspected does not establish the planned resident-engine hard cap.

**Done when:** a repeatable benchmark report distinguishes UI time from result delivery and engine memory from renderer memory, with no regressions in search recall or active tasks.

### A6. Make release verification authoritative

**Owners:** `.github/workflows/ci.yml`, `build.ps1`, `publish-aot.cmd`, tool scripts, browser test harness and brain notes.

- [ ] Remove obsolete `continue-on-error` from engine/tools CI once green; run Rust, tool and browser-JS tests rather than only checking/building those components.
- [ ] Pin the intended Node/pnpm toolchain and use the frozen lockfile. Remove stale Tauri-only scripts where unused.
- [ ] Build shell, engine and tools from the same revision; fail release packaging if a required component/toolchain is absent. Current build script can continue without engine or tools.
- [ ] Run the AOT smoke checklist and bench-field against a fresh test world: all available tools, pack lifecycle, MKV/audio/subtitle seeking, real Explorer drag/drop, menus, permissions and startup timing.
- [ ] Verify core browser flows: password save/fill/import with controlled accounts, browser import fixtures, camera/microphone, spaces/private isolation, downloads and session recovery.
- [ ] Correct conflicting notes: phase 3 status, category counts vs panels, bundled categories vs downloadable packs, outdated Tesseract and old C++/updater priorities. Keep a clear completed/implemented-unverified/planned distinction.

**Gate before broad feature expansion:** blockers resolved, required suites green, fresh AOT evidence recorded, and measured regressions understood. Do not use an older installer or test count as proof for new code.

## Part B — remaining feature roadmap

### B1. Finish promised core integrations

- [ ] Windows OCR in Core, shared by Image to Text and CamScanner. Route through a native host adapter; surface available language support clearly. PaddleOCR follows as an optional pack, not Tesseract.
- [ ] Complete the promised media operations from A2.
- [ ] Add browser-to-tool entry points where already planned: page/selection Markdown, image OCR, QR actions, screenshot editing and image conversion. Reuse existing tools rather than building a second implementation.

**Exit:** these existing surfaces work on a clean Core install or explain the precise optional dependency required.

### B2. Phase 4 — background foundation, then voice and documents

- [ ] Define opt-in tray lifecycle, single engine ownership, wake/reconnect, shutdown and resident memory limits.
- [ ] Add global hotkeys and snippets using the existing registry; protect sensitive inputs and keep all persistence scoped to Search.
- [ ] Add Vosk runtime/model installation and push-to-talk, then dictation into other apps, cancellation/listening UI, safety checks and idle model unloading. A downloaded model alone cannot enable voice in an engine compiled without its feature.
- [ ] Implement Word ⇄ PDF through a bounded Word worker, then isolated LibreOffice fallback. Handle missing dependencies, timeout and cleanup without orphan processes.
- [ ] Restore Screen Recorder only after the engine feature and Search-native capture/region controls work together.

**Exit:** background mode is off by default; closing the browser behaves predictably both ways; background tools work without the old Tauri windows.

### B3. Phase 5 — Ask

- [ ] On-demand page-to-Markdown extraction, source metadata, copy/export to Obsidian, and selected-tab context.
- [ ] Local endpoint and BYOK model settings, secure key storage, streaming, cancellation and context limits.
- [ ] Disclosure Firewall on every outward model send, with inspectable receipts showing destination and content.
- [ ] Ship Ask Page first, then compare selected tabs and summarize available video transcripts with citations.

**Exit:** browsing incurs no model work when unused; private/sensitive content is excluded or deliberately selected; all model traffic follows one checked send path. Provider support and cost reporting must be verified during that implementation, not assumed from old idea notes.

### B4. Phase 6 — Act, then remember and repeat

- [ ] Agent space, visible task/step log and reliable stop/takeover.
- [ ] Convert bench primitives into production action interfaces, using accessibility/DOM data first. Bench access is groundwork, not a completed permission system.
- [ ] Enforce task/site grants, consequential-action confirmations and cross-site transfer controls across every entry point.
- [ ] Keep page text untrusted; add hostile-page regression cases before allowing consequential automation.
- [ ] Opt-in semantic history and agent memory in the shared index, with space separation, private-page exclusions and per-site deletion/export.
- [ ] Add watchers and recipes after cancellation, scheduling, permissions and task isolation work.
- [ ] Add MCP server/client using the same registry and enforcement.
- [ ] Plan T3 Code integration separately: project/file/process permissions, local lifecycle, workspace selection and stop behavior need their own design and optimization note. Do not assume embedding its UI solves those boundaries.

### B5. Remaining optional packs

- [ ] Vosk, PaddleOCR, semantic search/MiniLM, background-removal model and PDF runtime as needed by verified workflows.
- [ ] For each: supported architectures, runtime feature availability, provenance, install/repair/update/remove, offline behavior and idle-memory budget.
- [ ] Distinguish category toggles, compiled engine features and downloaded runtime assets in code and documentation.

### B6. Phase 7 — distribution and accessibility

- [ ] Prototype MSIX/full-trust helper behavior early, then finish Store packaging and update ownership after stabilization.
- [ ] Validate Arm64 end to end: shell, Rust target, every native runtime and pack. `build.ps1 -Arch arm64` alone is insufficient; current Rust invocation does not select an Arm64 target and FFmpeg manifest is win64.
- [ ] Complete keyboard/screen-reader/high-contrast/DPI coverage, profile migration/recovery and clean-machine installation/uninstallation.
- [ ] Verify runtime/license notices and current Store requirements at release time. Earlier notes are product intent, not current certification guidance.

## Recommended execution order

1. A1 containment and correctness, with A6 CI corrections.
2. A2 media and A3 YouTube reproduction/fix.
3. A4 settings/packs, A5 measured optimization, then the full A6 release gate.
4. B1 core completion; B2 background/voice/documents.
5. B3 Ask; B4 Act and memory; optional packs alongside their consumers.
6. B6 release readiness, with early packaging feasibility checks.

Defer the C++ shell rewrite. The older size estimates predate the integrated engine/tools and exclude much of the total browser cost. Reconsider only after measurements show native chrome is the limiting factor and the rewrite justifies its input/accessibility/migration cost.

Do not assign calendar estimates from the old phase tables: their completion dates and scope no longer match this checkout. Estimate each bounded work package after its first reproduction or technical spike.
