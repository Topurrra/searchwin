# Tool Quality Audit — 2026-08-30 (code-first pass)

Executes step 4 of `tool-excellence-audit.md` as a code-reading audit (owner call:
usage triage skipped — owner uses everything). 11 parallel reviewers read every
tool's Svelte frontend + Rust backend. **No app runs — findings are from code.**

**Tally: 13 EXCELLENT · 20 GOOD · 6 NEEDS WORK** across 39 surfaces.
~25 concrete bugs found (each with file:line below). Verdict meanings:
EXCELLENT = hard to improve meaningfully · GOOD = solid with specific gaps ·
NEEDS WORK = real defects or missing table stakes.

---

## Priority fix list (severity-ordered, per plan P0–P2 scale)

| # | Sev | Surface | Defect (file:line) |
|---|-----|---------|--------------------|
| 1 | P0 | Search | Synonym expansion inflates `query_keywords`, then `required=(len+1)/2` drops exact-match files — "invoice report" can drop `invoice report.pdf` (search.rs:9340, 10366-10371) |
| 2 | P0 | Semantic | Rebuild clears vector store but the mtime/size reuse path never re-stores vectors — every unchanged file loses semantic recall after any rebuild with ≥1 change (search.rs:6398-6400 vs 6616-6652); watcher upserts never embed (7749-7825) |
| 3 | P0 | Search+Palette | Non-ASCII (Georgian!) normalized via `is_ascii_alphanumeric` → "" — apps scope returns nothing for Georgian queries; exact/prefix/phrase credit lost in search (search.rs:11905, 11944; byte-based edit_distance 10141-10166) |
| 4 | P0 | Folder Diff & Merge | `sync_folders` copies only on size mismatch — same-size modified files (which the diff itself flags) silently never sync (diff_merge.rs:413); mirror deletes bypass Recycle/recovery/safe_path (:461) |
| 5 | P0 | Cleaner/Analyzer | "jetbrains-cache" target = all of `%LOCALAPPDATA%\JetBrains` incl. Toolbox-installed IDE **binaries** — clean deletes installed IDEs (cleaner.rs:611) |
| 6 | P0 | Clipboard | History images written plaintext (no DPAPI) and skip sensitive scanning — password screenshots sit unencrypted (clipboard_history.rs:1598, 1621); `write_text_to_clipboard` error path returns without CloseClipboard → wedges system clipboard (:2517) |
| 7 | P0 | Document Scanner | EXIF orientation: backend decodes unoriented, overlay maps corners via oriented naturalWidth/Height → 90°-wrong warp on phone photos, the primary input (camscan.rs:66-71, 394-399; CamScanner.svelte:252-268) |
| 8 | P1 | Watermark | Text-mode scale slider dead — `_scale_percent` ignored, fixed 96px render; no overwrite guard: empty suffix silently destroys the original (image_extra_tools.rs:716-732, 120-135) |
| 9 | P1 | Snippets | Hook swallows terminator before worker exclusion check → space/tab/enter eaten in excluded apps (snippet_expand.rs:261 vs 310-312); no modifier-state check (:272-295); no password-field avoidance |
| 10 | P1 | Screen Recorder | Encoder death with audio: Err before mux/promote leaves `.kiltmp` partial while toast claims "saved" (encode.rs:352-354; ScreenRecorder.svelte:206-212); tray-Quit mid-take orphans ffmpeg + temps (lib.rs:3544-3547) |
| 11 | P1 | Time Tracker | Sleep/hibernate gap booked as active time on last app — no gap cap on `cur.end_ms = now` (time_tracker.rs:495-499; focusMode.ts:232 solves exactly this); idle start not backdated (461-463) |
| 12 | P1 | Focus Mode | Enforcement on webview `setInterval(3000)` — throttled to ~1/min while minimized to tray, which is the whole scenario (focusMode.ts:274); move to the Rust tick |
| 13 | P1 | CSV Toolkit | Merge cancel clears flag mid-file → truncated file marked `success:true` and merging continues (spreadsheet.rs:2405-2407) |
| 14 | P1 | Format Converter | `Event::Empty` dropped — self-closing XML tags vanish; root-array JSON→XML emits non-well-formed output; zero tests (format.rs:113, 124, 132-138) |
| 15 | P1 | My Commands | Any target containing `{query}` treated as bang → URL-encoded substitution into shell commands (myCommands.ts:380-382; PaletteV2.svelte:4473); `child.kill()` doesn't kill the tree (my_shell.rs:66-69) |
| 16 | P1 | Encrypt/Decrypt | Decrypt trusts header KDF params unclamped — crafted .kenc → multi-GB Argon2 allocation (crypto_tool.rs:266-273); no zeroization |
| 17 | P1 | Hash Check | Sync command on main thread — multi-GB file freezes UI and cancel can never fire (hash.rs:51); use `(async)` + spawn_blocking like archive.rs:234 |
| 18 | P1 | Voice | cpal stream error swallowed to eprintln — dead mic, UI says running (voice.rs:1300); no voice-error event / device re-resolve |
| 19 | P2 | Dev Toolkit | JWT sample token/key mismatch fails its own verify demo (JwtPanel.svelte:20,291,301); secret-scan spans are Rust byte offsets used as JS UTF-16 indices (SecretScanPanel.svelte:135-144); regex flags box stale (RegexPanel.svelte:374); ReDoS guard insufficient (:36-38) |
| 20 | P2 | Password Generator | `/(.)\\1/` regex-literal bug — adjacent-duplicate audit never fires; modulo bias in raw mode (PasswordGenerator.svelte:191, 93) |
| 21 | P2 | Privacy Audit | `is_sensitive_domain` includes bare `microsoft.com` — privacy hosts lists get High "malware" findings (privacy_audit.rs:1315); mic/cam = 2 of ~15 ConsentStore caps (:588) |
| 22 | P2 | Automations (hidden) | `strip_metadata` silently dropped by serde — "Clean metadata" is a no-op toggle (image_tools.rs:2463-2476; automationRecipes.ts:399); step progress contract broken (:182) |
| 23 | P2 | Media Utility | Audio bitrate always subtracted even for silent video (undershoot); NaN target accepted; MP3 extract never probes duration → no progress (media_utility.rs:625, 572; MediaUtility.svelte:216-223) |
| 24 | P2 | SSH Key Manager | icacls result ignored (`let _=`) — failed ACL lock unwarned, OpenSSH will refuse key (ssh_keys.rs:174-179); config rewrite without temp+rename (:390-423) |
| 25 | P2 | Reminders | "remind me to look at the report at 3pm" → text "look" (first-marker split, reminders.ts:225-236); reminder text plaintext localStorage (:27,53) |

---

## Per-tool verdicts

### Pillars & core surfaces
- **Search / File Search — GOOD.** Architecture is excellent (`rank.rs` fused scorer with ordering-invariant tests; layered query passes; content-hash OCR cache makes staleness structurally impossible). Held back by ranking edge cases: #1 synonym dilution, #3 Georgian, apostrophe treated as quote char (9285), edit-distance-1 fuzzy floor (ntpd→notepad fails), ~12s worst-case freshness, watcher upsert omits `sensitive_kinds`. Beats Everything/Windows Search on breadth already; subsequence/abbreviation matching is the single biggest class jump.
- **Semantic search (beta) — NEEDS WORK.** Sound int8 quantization with drift tests, best-chunk scoring, robust store — gutted by rebuild vector loss (#2) and no incremental embedding. Also full-corpus clone per query (vector_cache.rs:211). Fix those two and it's genuinely class-leading (no free competitor has local semantic).
- **Command Palette V2 — GOOD.** Race discipline is genuinely strong (requestId guards everywhere, atomic commits, live-grep with CAS-bounded caps, Georgian-aware snippets). Gaps: launcher has zero fuzzy (exact/prefix/contains only), Georgian apps scope dead (#3), 16k-line PaletteV2.svelte untested beyond the extracted 34-line state module, Tab scope-cycling silently dies when chips hidden (4903).
- **Clipboard History — EXCELLENT.** Best-in-class paste-back (UIPI detection, thread-attach refocus, settle poll); real image budgets; tested retention. Must-fix: #6 (image encryption + clipboard wedge). Scanner misses AWS secret keys/`glpat-` (:97); search ignores pin labels/source app.
- **Voice to Text — EXCELLENT.** Mic arbitration chokepoint, grammar-constrained commands with no-drift tests, tested safety gate, real resource discipline (idle backstop, working-set trim). Fix #18 (swallowed stream errors). Common-word literals ("copy","zip") are risky exact-match commands.
- **Notes + Quick Notes — EXCELLENT.** Loss-proof save path (fs2 lock, BLAKE3 revision check, temp+fsync+rename, conflict banners), tested trash/restore, regression-tested export fidelity. Gaps: no notes-dir watcher (external edits/list staleness), quicknote-created notes don't appear in open Notes page, attachments never GC'd.
- **Snippets — GOOD.** Clean tested matcher, sentinel anti-loop, DPAPI storage. Fix #9 — the eaten terminator in excluded apps is user-visible daily; modifier handling and password-field avoidance are the class-raisers (espanso lacks the latter too).
- **My Commands — GOOD.** Coherent trust model (DPAPI store, sane shell flags, double-gated destructive actions, correct bang escaping). Fix #15; bang defaults duplicated in TS and Rust (drift risk); shell warning is informational, not a gate.
- **Time Tracker — GOOD.** Real privacy story (DPAPI day blobs, "(private)" exclusions, retention + wipe). Fix #11 — it's data correctness, and the fix already exists in focusMode.ts. Elevated windows silently extend prior session (:279-281); week view ships 7-day timeline over IPC every 20s unused.
- **Privacy Audit — EXCELLENT.** 8 scan classes (way beyond mic/cam), correct ConsentStore FILETIME in-use detection, genuinely thoughtful FP handling. Fix microsoft.com hosts FP (#21); extending ConsentStore caps (location etc.) is a one-line-per-cap class-raiser; `reveal_in_explorer` quote-breaking (:180).

### Utilities pack
- **Hash Check — GOOD.** Streaming 4-algo single pass, tests incl. cancel — but see #17 (sync main-thread). No byte progress, no drag-drop.
- **Encoders — GOOD.** 9 round-trip tests; but all decodes force UTF-8 (binary payloads error), hex rejects spaced input while binary cleans it, no debounce.
- **QR Code — EXCELLENT.** Best-engineered small tool (stale-guards, all states, path-validated export). Dead `margin` width param (qr.rs:38,54); QR *decoder* would add a second use case.
- **Format Converter — NEEDS WORK.** See #14. Also no CSV mode; XML→* stringifies scalars.
- **Password Generator — GOOD.** CSPRNG + honest EFF entropy math; see #20; no clipboard auto-clear (copied passwords land in own Clipboard History!).
- **Color Picker — GOOD.** Proper EyeDropper feature-detect. Drag floods recents (save on `onchange`); no alpha; WCAG contrast readout would beat PowerToys.
- **Calculator — EXCELLENT.** Genuine Soulver-class engine (Pratt parser, units, DST-proof date math, 30 tests). `1,000` breaks parse; currency is the flagship missing feature.

### File pack
- **File Manager — GOOD.** guard_write everywhere, Recycle-Bin deletes, tests. Robocopy backup lacks /R /W flags — one locked file stalls ~forever (file_manager.rs:969-980); no mid-file progress/cancel (50GB file uncancellable); no long-path handling.
- **Archive Utility — EXCELLENT.** Zip-slip + archive-bomb triple guard + worker-process staging. Sync `inspect_archive` decompresses whole tar on main thread (:1239); 7z always `encrypted:false` (:1297).
- **Cleaner / Analyzer — EXCELLENT** (one mis-scoped target — #5 is the fix). Allowlist + typed-CONFIRM server-side, streaming walks, RAM-aware concurrency. Clean phase lacks progress/cancel; permanent delete, no Recycle fallback.
- **Duplicate Finder — EXCELLENT.** Never deletes — transactional move-to-review with undo; keyed hash cache; perceptual mode. Auto-selects ALL dupes on completion (aggressive); indeterminate progress; hardlinks counted as reclaimable.
- **Bulk Rename — EXCELLENT.** Mandatory preview, reserved-name checks, two-phase temp rename handling A↔B swaps, undo. Undo banner hides after 60s though the transaction persists; folder+children in one batch confuses phase-1 (order deepest-first).
- **Folder Diff & Merge — NEEDS WORK.** Diff half solid (content-verified, OCR image diff, diff3). Sync half is #4 — the one place the house safety pattern was skipped.

### Image pack
- **Image Studio — GOOD.** Best-in-repo encoder stack (mozjpeg/oxipng/ravif), EXIF orientation with regression test, partial-failure reporting. ICC profiles dropped (wide-gamut shift); animated GIF flattened silently; batch name collisions overwrite (`a.jpg`+`a.png` → one `a_studio.webp`).
- **Image to Text (OCR) — GOOD.** Robust engine resolution, real subprocess cancel, stroke-safe preprocessing. No EXIF orientation (rotated phone photos → garbage); skips `validate_user_path` (convention break); tool ignores the existing OCR cache.
- **Document Scanner — NEEDS WORK.** Good pure-Rust pipeline (Canny→quad→tested homography) but #7 breaks the primary input; temp previews never cleaned; no PDF/multi-page export; mojibake in pane titles ("1 ? Adjust source").
- **Favicon Generator — GOOD.** Complete pack in one pass + manifest + snippet. UI promises SVG but picker/loader can't (Studio has the rasterizer — wire it); non-ICO-size selection writes zero-image favicon.ico; manifest JSON unescaped background.
- **Watermark — NEEDS WORK.** See #8. Also no EXIF orientation; WebP lossless-only; baseline JPEG not mozjpeg.

### Media pack
- **Screen Recorder — EXCELLENT.** Serious engineering (preflight disk+encoder checks, fragmented-MP4 crash safety, watchdog stop, pre-encoder redaction). Fix #10; primary-monitor only; stale doc comment (screenrec_cmds.rs:58-61).
- **Media Utility — GOOD.** Robust cancel incl. pre-spawn race, µs progress quirk unit-tested, unwatchable-target refusal. See #23 + shared app-exit ffmpeg orphan with recorder.

### Documents pack
- **Markdown Converter — GOOD** (backend EXCELLENT — exact byte-reuse of the tested Notes export engine). Regex HTML→MD eats single-line pre/code; stale source-dir for pasted markdown images; table headers unstyled in DOCX/PDF.
- **CSV Toolkit — GOOD.** Fully streaming with per-file partial results — see #13; whole-workbook RAM for csv→excel with no 1,048,576-row precheck; no CP1251/Windows-1252 transcoding (dies on non-UTF-8); no overwrite check.
- **Word Converter [hidden] — EXCELLENT, unhide candidate.** Hidden as "superseded by Markdown Converter" but converts DOCX→MD — the *reverse* direction; nothing else covers it. Real OOXML engine, 18 tests. Text boxes/footnotes/headers dropped; media re-extracted per reference; silent overwrite.
- **Remove Password [hidden] — GOOD, hiding reasonable (niche).** Correct approach + tests. Encrypted-CFBF files die at ZipArchive before the honest error can fire (sniff `D0 CF 11 E0` upfront); fragile `strip_self_closing_tag`.

### Development pack
- **Developer Tools hub — GOOD.** Real engines (grex, sqlparser AST lint, checksum-verified secret scanner, tested cronEngine with honest Task Scheduler translation). See #19; SQL locked to GenericDialect (valid T-SQL flagged invalid).
- **SSH Key Manager — GOOD.** Private key material never crosses to JS; pure-Rust; async keygen. See #24; stale-index known_hosts removal (TOCTOU); hashed entries render as gibberish.
- **Encrypt / Decrypt — EXCELLENT.** Textbook: AES-256-GCM STREAM/BE32, Argon2id above OWASP floor, header-stored params, keyfile pepper, streaming cancel, cleanup, tamper tests. See #16; text variants are sync commands.
- **Image to Base64 — GOOD.** Proper batch plumbing with per-file isolation. No size cap and double payload (raw + data_uri) per file → 300MB TIFF ≈ 800MB across layers; MIME by extension only.

### Privacy pack
- **File Shredder — GOOD.** SSD honesty is real (UI recommends FDE, warns wear-leveling). Locked files → raw os error 32 with no guidance; cancel mid-batch under-reports history; free-space wipe misses MFT-resident slack (SDelete-style small-file pass).
- **Screenshot Redact — EXCELLENT.** Genuinely destroys pixels; EXIF gone by construction; honest UI copy. Pixelate description overpromises (depix attacks); preview blur ≠ export blur (WYSIWYG drift); dead `strip_metadata` param.
- **Privacy Hardening [hidden] — GOOD; mechanism is un-hide quality.** Best-in-repo reversibility (snapshot-before-apply, exact revert, id-only command surface). Blockers are breadth (~13 vs ShutUp10 50+) and two fixables: export path skips the write-target gate; backup exports *current* (hardened) state without warning. With "curated safe subset" framing + ~10 safe HKCU additions it's shippable.

### Time & Focus pack + hidden Automation
- **Reminders — GOOD.** Fires when app closed via one-shot Task Scheduler tasks with orphan reconcile — clever and solid. See #25; no recurrence; bare "at 3" = 03:00.
- **Focus Mode — GOOD.** Genuinely non-destructive (WM_CLOSE, ALWAYS_ALLOWED safety set, snooze). See #12; TOCTOU on window action; session lost on restart; no nudge escalation.
- **Automations [hidden pack] — NEEDS WORK; redesign instinct confirmed by code.** Steps are cosmetic over one hardcoded image pass; #22; recipes schema is image-only; in-memory job queue; the tested cron_tasks engine sits disconnected in devkit. As "image preset batch" it's ~shippable; as "Automations" it needs the planned v2 redesign (generic step contract + triggers).

---

## Cross-cutting themes (fix once, benefit many)

1. **EXIF orientation** handled only in Image Studio — OCR, Document Scanner, Watermark, Favicon all ignore it. One shared `load_oriented()` helper fixes four tools.
2. **Sync `#[tauri::command]` on the main thread** for heavy work: hash, format, qr, `inspect_archive`, `crypto_*_text`, `preview_duplicate_file`. The repo knows the right pattern (archive.rs:234); one sweep.
3. **Non-ASCII/Georgian text** is a systematic blind spot in search/launcher normalization — notable given the app bundles Georgian OCR and the owner types Georgian.
4. **Overwrite guards inconsistent**: Image Studio/Archive do it right; Watermark, Word Converter, CSV outputs, batch collisions don't.
5. **House safety pattern** (safe_path/guard_write/recovery/Recycle) applied everywhere except `sync_folders`, `export_hardening_backup`, `notes_export_*`, `ocr_image`.
6. **App-exit orphan sweep** missing: ffmpeg children + temps (recorder, media utility) survive tray-Quit; no startup temp sweep.
7. **Encryption-at-rest gaps** against the DPAPI brand promise: clipboard images, reminder text.

## Answer to "are they the best already?"

No — and also closer than "almost done" usually means. 13 of 39 surfaces are
genuinely hard to improve. The architecture layer (safety gates, cancel
registries, atomic writes, test discipline on pure engines) is above most
commercial utility suites. What stands between GOOD and EXCELLENT is almost
never missing features — it's the ~25 findings above, of which ~7 are P0.
Fix list feeds `tool-excellence-audit.md` step 5 as the initial finding log
(caps: top-3 per weekly tool per cycle; ledger top-10 override applies).
