---
tags: [searchwin, protocol, engine]
updated: 2026-09-26
---

# Engine Protocol and Command Reference

Back to [[README]] · Also see [[Engine Protocol]] (setup and connection overview)

This document describes the JSON-RPC commands that `kil-engine.exe` (the headless helper process) accepts over the named pipe. The browser and tools call these commands to index files, search, manage clipboard history, generate instant answers, launch apps, and more.

**Protocol basics:** JSON-RPC 2.0 over a named pipe (`search-engine[-world]`). One JSON line per message. Asynchronous events arrive on the same connection with an `"event"` key instead of `"id"`.

## Connection and Metadata

### engine.hello
**Request:** none.

**Response:** `{engine: "kil-engine", version: "X.Y.Z", pid: N, commands: [...]}`

Identifies the engine and returns the list of callable command names. Safe to call repeatedly.

### engine.methods
**Request:** none.

**Response:** `["search.query", "clipboard.list", ...]`

Lists callable command names (same as `engine.hello`'s commands field).

### engine.exit
**Request:** none.

Stops the engine. Sends no response; the pipe closes.

---

## File Index (search.rs)

The file index has two parts: a filename index (fast, word-based search in file names) and a content index (slower, word-based search inside files). Both are built from index options (roots, includeHidden, maxContentKb, excludes…) and watched for file changes.

### save_file_search_index_options
**Request:** `{options: FileSearchIndexOptions}`

**FileSearchIndexOptions** (all fields):
- `roots: [string]` — folders to index (absolute paths, e.g., ["C:\\Users\\…", "D:\\Projects"])
- `filenameRoots: [string]` — folders to search by filename only (optional; if omitted, same as roots)
- `includeHidden: boolean` — index .dotfiles and folders (e.g., .git, .ssh)
- `indexContent: boolean` — build the content index (set true for everything now; phase 3 may make this false)
- `maxContentKb: number | null` — cap file size for content indexing (e.g., 10000 for 10 MB; null = no cap)
- `watcherEnabled: boolean` — watch folders for changes and re-index incrementally (saves a rebuild on file changes)
- `watcherPaused: boolean` — temporarily pause the watcher (keeps watching but doesn't update the index)
- `commitEvery: number | null` — commit index changes every N file changes (null = no limit; incremental indexing uses this)
- `excludeFolders: [string]` — folder names to skip, ordered with last-match-wins semantics (e.g., ["temp", "build", "node_modules"]; the engine has defaults including `.git`, `node_modules`, `build`, `.vscode`, `appdata/local`). A `!` prefix (e.g., `"!build"`) acts as a keep: it un-excludes a folder and everything below it. Repeated rules keep the last one; an outer keep can be overridden by an inner exclude. Pattern ending in a name ends at a folder boundary (e.g., `build` matches `build/` and all its contents, but `appdata/local` only matches folders literally named `local` inside folders named `appdata`). Also match `*` patterns in folder names (e.g., `*cache*` matches any folder with "cache" in its name).
- `includeHidden: boolean` — when true, index `.dotfiles` and Windows-hidden entries (FILE_ATTRIBUTE_HIDDEN flag). When false, also excludes `AppData` below the chosen folder (AppData/Roaming is a privacy boundary). A dot-folder chosen explicitly as its own folder stays hidden until includeHidden is on; a nested chosen root inside a dot-folder turns includeHidden on (phase 4 A1 fix).
- `excludeExtensions: [string]` — extensions to skip (e.g., ["exe", "dll", "so"])
- `performanceMode: boolean` — (advanced) skip some analyses
- `ocr*` fields — OCR settings (phase 3+)
- `contentIndexingEnabled, semanticSearchEnabled` — feature flags (phase 3+)

**Response:** null (async; events follow)

Call this before `start_*_search_index`. The browser's `IndexPlan.Options` builds this JSON.

### start_search_services
**Request:** `{}`

**Response:** null (async; events follow)

Initializes the file search scheduler and prewarms engines. Safe to call once roots exist. This was missing from the command table until phase 2 (see [[Lessons Learned#Phase 2: The universal field]]).

### start_filename_search_index
**Request:** `{options: FileSearchIndexOptions}` (same shape as save_file_search_index_options)

**Response:** `boolean` (async)

Builds the filename index (names only, not contents). Emits `file-search-index-progress` events during the build.

### start_content_search_index
**Request:** `{options: FileSearchIndexOptions}`

**Response:** `FileSearchBuildResult` (async)

Builds the content index (text inside files). Slower than filename indexing. Emits `file-search-index-progress` events.

**FileSearchBuildResult:**
- `success: boolean` — build completed without errors
- `partial: boolean` — some files were skipped due to errors
- `canceled: boolean` — build was cancelled
- `message: string` — status or error summary

### start_file_search_index
**Request:** `{options: FileSearchIndexOptions}`

**Response:** `FileSearchBuildResult` (async)

Builds **both** filename and content indexes (calls start_filename_search_index internally). Emits `file-search-index-progress` events with `indexKind: "filename"` and `"content"` on separate events.

### get_file_search_status
**Request:** `{}`

**Response:** `FileSearchStatus`

**FileSearchStatus:**
- `roots: [string]` — current index roots
- `filenameRoots: [string]` — folders searched by name only
- `indexedFiles: number` — total files in the content index
- `filenameIndexedFiles: number` — total files in the filename index
- `indexing: boolean` — content index is currently building
- `filenameIndexing: boolean` — filename index is currently building
- `watching: boolean` — file watcher is active
- `watcherEnabled: boolean` — watcher setting (may be paused)
- `watcherPaused: boolean` — watcher is paused (no real-time updates)
- `contentIndexBytes: number` — disk size of the content index
- `filenameIndexBytes: number` — disk size of the filename index
- `scannedEntries: number` — files scanned in the last build
- `skippedFiles: number` — files skipped due to errors or exclusions
- `deletedFiles, updatedFiles: number` — incremental changes since last status
- `lastError: string | null` — last error message
- `lastIndexedAtMs: number` — timestamp of the last build
- `diagnostics: {indexWorker, watcherWorker, watcherStrategy, performanceMode, lastWorkerMessage, lastStatusAtMs}`
- `rebuildSchedule: string | null` — next scheduled rebuild time (phase 3+)
- `excludeFolders, excludeExtensions: [string]` — current exclude lists

Safe to poll repeatedly; internally ensures the engine is loaded.

### search_local_files
**Request:** `{options: {query: string, limit?: number, offset?: number, extensionFilter?: [string], pathFilter?: string, naturalLanguage?: boolean}}`

**Response:** `{query: string, results: [{path, fileName, extension, size, modifiedMs, score, entryType, matchReason, matchedKeywords, sensitiveKinds}], returned: number, totalHits: number, tookMs: number}`

Searches the filename index for file names matching the query. Results are scored and sorted. `entryType` is "file" or "folder". `sensitiveKinds` lists flagged secrets found in the path.

**Notes on single-word search (phase 2 findings):**
- A single common word like "notes" alone may rank a "notes" folder first (BM25 sees "notes" in both path and filename), pushing "notes.txt" out of view. Multi-word queries or "notes.txt" work reliably.
- The always-on prefix pass (phase 4) runs on `file_name` only, skips words under 3 letters, reads ahead page*2 documents (not 8x), and includes extension/entry-type filters as query clauses. It is skipped when a page of name-matches already exists.
- The field's live query path may differ from the direct `search_local_files` call. Phase 2 review found related-term-only admission failing in the field's natural-language path. Before shipping, verify `build_result_item` / `typed_keyword_hits` counting from the field's query path (not just the unit test).

### search_file_contents
**Request:** `{options: {query: string, limit?: number, offset?: number, extensionFilter?: [string], pathFilter?: string, naturalLanguage?: boolean}}`

**Response:** `{query: string, results: [{path, fileName, extension, size, modifiedMs, score, snippet, snippets, matchCount, matchedKeywords, sensitiveKinds, semanticScore, semanticOnly}], returned: number, totalHits: number, semanticActive: boolean, tookMs: number}`

Searches the content index for text inside files. `snippet` is an HTML excerpt with highlighted matches. `snippets` is an array of all matches. `semanticScore` and `semanticOnly` are phase 3+ features. **Important:** the reader caches data for ~5–6 seconds after a build; queries before that lag shows 0 hits.

### stop_file_search_index_watcher
**Request:** `{}`

**Response:** null

Stops the file watcher (incremental re-indexing). Call when removing all folders.

### clear_file_search_index
**Request:** `{mode?: "content" | "all"}`

**Response:** null

Clears the file search indexes. `mode: "content"` clears only the content index (keeps the filename index); `mode: "all"` or omitted clears both. Emits a `file-search-index-progress` event with `current: 0`. Used when folders are removed or a rebuild is needed. **Important:** options must be saved via `save_file_search_index_options` before clearing, or the watcher loses its configuration.

### read_file_preview
**Request:** `{path: string, maxBytes?: number}`

**Response:** `string` (plain text preview of the file)

Reads the first N bytes of a file as plain text. Used to preview .txt, .md, .json, etc. in the file rows.

### open_search_result_path
**Request:** `{path: string}`

**Response:** null

**Security gate:** validates the path against core::safe_path::validate_user_path (rejects traversal/system paths). Then **launches the OS-associated app** with `open::that_detached(path)`. This is a real, irreversible action: if you pass a .exe, it runs. Use with caution in tests; never call this on untrusted paths.

---

## Apps (search.rs + launcher_icons.rs)

The app launcher cache searches the Start Menu, PATH, and registry for installed programs and their icons.

### refresh_launch_target_cache
**Request:** `{}`

**Response:** null

Clears the in-memory app cache. The next search rebuilds it (scans real system data).

### search_launch_targets
**Request:** `{options: {query: string, browseAll?: boolean, limit?: number}}`

**Response:** `{query: string, results: [{id, name, path, kind, source, score}], returned: number, totalHits: number, tookMs: number, cacheBuiltAtMs: number}`

Searches for installed apps by name. `source` is "StartMenu", "PATH", or "Registry". `kind` is "exe", "msi", "appref-ms", etc. `id` is a stable identifier (returned by later commands).

### ensure_launcher_icon
**Request:** `{path: string, kind?: string}`

**Response:** `string | null` (file path to a PNG on disk, or null if extraction failed)

Extracts an icon from the executable or app and caches it as a PNG file. The PNG path is under the engine's data dir (`icon_cache_dir`). Safe to call repeatedly; the cache is checked first.

### launch_cached_target
**Request:** `{path: string}`

**Response:** null (async)

Launches an app from the cache. **Security gate:** the path must be in the verified launch target cache (checked with `normalize_launch_path_for_match`). Rejects untrusted/nonexistent paths with an error.

### list_processes
**Request:** `{}`

**Response:** `[{name: string, exePath: string, pids: [number], windowCount: number, memoryBytes: number}]` (async)

Lists running processes by name. Read-only; safe for building a list of active apps. **Note:** names like "notepad" may collide across instances; pids distinguishes them.

### launch_targets_running
**Request:** `{paths: [string]}`

**Response:** `[string]` (which of the given paths have a running process)

Checks which of the given app paths are currently running.

### kill_process
**Request:** `{pids: [number], confirmed?: boolean}`

**Response:** `string | null` (null on success, error message otherwise)

**Not exercised in phase 2.** Kills processes by PID; requires `confirmed: true` to actually kill them (unconfirmed calls return an error).

---

## Instant Answers

These commands compute quick results (calculator, unit conversions, encoding, hashing, color conversion) without searching the web.

### evaluate_quick_query
**Request:** `{query: string, webSearchEnabled?: boolean}`

**Response:** `QuickAction | null`

**QuickAction:** tagged with `type`:
- `"calculator"` (e.g., "23*47") — `{type: "calculator", expression: string, result: string}`
- `"unitConversion"` (e.g., "5 km to mi") — `{type: "unitConversion", from: string, to: string, result: string}`
- `"systemCommand"` (e.g., "lock", "shutdown") — **The field rejects this type** (see [[Lessons Learned#Instant Answers]])
- `"openUrl"` (e.g., "ipconfig") — **The field rejects this type**
- `"webSearch"` — fallback when nothing matches

**Important:** the field's `AnswerSource.Suggest` accepts only calculator and unitConversion, dropping systemCommand and openUrl. This prevents a keystroke from accidentally launching a system command.

**No currency conversion:** the UNITS table in quick_actions.rs supports Length, Mass, DataBinary (binary sizes), Time, Temperature — but not currency/FX rates. "128 usd to gel" returns null (phase 2 limitation).

### execute_system_command
**Request:** `{id: string, confirmed?: boolean}`

**Response:** `string | null` (command output or error)

Runs a system command by id (from list_system_commands). Destructive commands (shutdown, restart, etc.) require `confirmed: true`; unconfirmed calls return an error.

### list_system_commands
**Request:** `{}`

**Response:** `[{id, name, description, group, requiresConfirmation}]`

Lists available system commands. Read-only; safe for building a command menu.

### encode_decode
**Request:** `{algorithm: "base64" | "hex" | "url", mode: "encode" | "decode", input: string}`

**Response:** `string` (encoded or decoded result)

Encodes or decodes text with the given algorithm.

### compute_hashes
**Request:** `{path: string, operationId?: string}`

**Response:** `{md5: string, sha1: string, sha256: string, blake3: string}` (async)

**Important:** hashes a **file at a path**, not arbitrary input text. The Master Plan's example "sha256 hello" (hashing typed text) is **not** what this command does. The browser/Kit must implement text-hashing separately (e.g., client-side via C# SHA256.HashData, or a new engine command for phase 3).

### cancel_hash_operation
**Request:** `{operationId: string}`

**Response:** null

Cancels an in-flight hash by id.

### convert_format
**Request:** `{from: "json" | "yaml" | "toml" | "xml", to: "json" | "yaml" | "toml" | "xml", input: string}`

**Response:** `string` (converted output)

Converts data from one format to another.

---

## Clipboard History

The engine's clipboard listener runs on a background thread with Win32 `AddClipboardFormatListener`. It captures copies from any app and stores them in an encrypted (DPAPI) history file. Secrets are auto-detected and tagged; pictures are thumbnailed.

### start_clipboard_listener
**Request:** `{}`

**Response:** null

Idempotent. Spawns the Win32 listener thread and a 500 ms debounced save thread. Emits `"clipboard-history-updated"` events on every capture/mutation. If the listener is already running, this call is a no-op.

### get_clipboard_history
**Request:** `{}`

**Response:** `[ClipboardEntry]`

**ClipboardEntry:**
- `id: number` — stable identifier for later commands
- `capturedAtMs: number` — capture timestamp (since 1970)
- `kind: "text" | "image"`
- `text: string` (text entries only) — the text content
- `sourceApp: string` (optional) — app that owned focus when copied
- `sourceAppPath: string` (optional) — path to the app's exe
- `sensitiveKinds: [string]` (e.g., ["sensitive", "github_token"]) — flagged secrets
- `category: string` (e.g., "password") — semantic category
- `isPinned: boolean` — user pinned this entry (keeps it longer)
- `pinLabel: string` (optional) — user-assigned label
- `imagePath: string` (images only) — disk path to the full image
- `thumbnailPath: string` (images only) — disk path to a small thumbnail
- `imageWidth, imageHeight: number` (images only)
- `imageSizeBytes: number` (images only)
- `imageFormat: string` (images only, e.g., "png", "jpeg")

### pin_clipboard_entry
**Request:** `{id: number, pinned: boolean}`

**Response:** null

Pins or unpins an entry. Pinned entries appear first in the history and have a longer retention window.

### pin_clipboard_entries
**Request:** `{ids: [number], pinned: boolean}`

**Response:** null

Bulk pin/unpin.

### label_clipboard_entry
**Request:** `{id: number, label: string}`

**Response:** null

Assigns a user-visible label to an entry.

### delete_clipboard_entry
**Request:** `{id: number}`

**Response:** null

Deletes one entry. Emits `"clipboard-history-updated"`.

### delete_clipboard_entries
**Request:** `{ids: [number]}`

**Response:** null

Bulk delete. Emits `"clipboard-history-updated"`.

### clear_clipboard_history
**Request:** `{}`

**Response:** null

Clears all non-pinned entries. Pinned entries are kept. Emits `"clipboard-history-updated"`.

### copy_clipboard_entry_to_clipboard
**Request:** `{id: number}`

**Response:** null

Copies an entry back to the OS clipboard (plain text for text entries; the image file for pictures). Does not add a new history entry (marked internally to suppress re-capture). For secrets, the browser should not use this; instead, insert the text directly into the focused field or page.

### set_clipboard_paused
**Request:** `{paused: boolean}`

**Response:** null

Pauses clipboard listening (doesn't stop it, just stops saving new entries).

### get_clipboard_paused
**Request:** `{}`

**Response:** `boolean`

Checks if the listener is paused.

### get_clipboard_retention_days
**Request:** `{}`

**Response:** `number`

Returns how long entries are kept (default 14).

### set_clipboard_retention_days
**Request:** `{days: number}`

**Response:** null

Sets retention (e.g., 7 for 1 week). Affects only new entries.

### get_clipboard_image_retention_days
**Request:** `{}`

**Response:** `number`

Returns how long pictures are kept (default 2).

### set_clipboard_image_retention_days
**Request:** `{days: number}`

**Response:** null

Sets picture retention separately.

### get_clipboard_images_enabled
**Request:** `{}`

**Response:** `boolean`

Checks if picture capture is on.

### set_clipboard_images_enabled
**Request:** `{enabled: boolean}`

**Response:** null

Turns picture capture on/off.

### get_clipboard_exclusions
**Request:** `{}`

**Response:** `[string]` (app names to exclude, case-insensitive)

Returns the list of apps whose copies are never captured. Defaults include password managers (KeePassXC, 1Password, Bitwarden, etc.).

### set_clipboard_exclusions
**Request:** `{apps: [string]}`

**Response:** null

Replaces the exclusion list. App names are matched case-insensitively against the foreground process name at copy time. **Note:** the match is made 60 ms after the copy (see [[Lessons Learned#Clipboard and sensitive data]]), so a manager that minimizes immediately may already have lost focus.

### reset_clipboard_exclusions_to_defaults
**Request:** `{}`

**Response:** null

Resets to the built-in list of password managers.

### take_clipboard_recovery_notice
**Request:** `{}`

**Response:** `string | null` (user-facing message, or null if nothing happened)

One-shot retrieval of a "history was corrupt, recovered" notice. Returns null after the first call.

### get_clipboard_text
**Request:** `{}`

**Response:** `string | null` (current OS clipboard contents)

Reads the live OS clipboard, not the history. Use with caution; this is a side-effectful read of the user's data.

### remember_foreground_window
**Request:** `{}`

**Response:** null

**Internal.** Captures the current foreground window HWND so paste/type-out commands can later find the target. Called by the browser before showing a clipboard or voice UI.

---

## Paste and Type-Out (Special Cases)

These commands interact with the previous foreground app (the app that had focus before the browser opened a clipboard or voice UI).

### paste_clipboard_entry
**Request:** `{id: number, autoPaste?: boolean}`

**Response:** null (or event `"clipboard-paste-fallback"` if nothing worked)

Writes the entry back to the OS clipboard, then (if `autoPaste`, default true) tries to refocus `PREVIOUS_FOREGROUND_HWND` and `SendInput` a synthetic Ctrl+V.

**Headless behavior:** if no foreground window was ever captured (in a headless test or if `remember_foreground_window` was never called), the target HWND stays 0, and `paste_clipboard_entry` **degrades safely**: no keystrokes are injected; instead, it emits `"clipboard-paste-fallback"` {payload: "Text is on your clipboard — switch to your target app and press Ctrl+V."}.

### type_out_text
**Request:** `{text: string}`

**Response:** null (or event `"clipboard-paste-fallback"`)

Copies text to the clipboard with a one-off marker so it doesn't re-enter history, then tries to `SendInput` the synthetic Ctrl+V. If no target window exists, emits the fallback message.

### paste_snippet_text
**Request:** `{text: string, autoPaste?: boolean, restoreClipboard?: boolean}`

**Response:** null

Same shape as `paste_clipboard_entry` but for pre-expanded snippet text. `restoreClipboard` puts the user's prior clipboard text back ~400 ms after the paste (only meaningful when autoPaste actually injects).

---

## Events

Asynchronous events arrive on the same pipe connection as responses (a line with an `"event"` key). The browser must be prepared to skip lines with "event" when reading the response to a synchronous call.

### file-search-index-progress
**Payload:** `{indexKind: "filename" | "content", stage: string, indexedFiles: number, scannedEntries: number, skippedFiles: number, message: string, finished: boolean, success: boolean, canceled: boolean, partial: boolean, currentPath: string}`

Emitted during a file index build. An initial "started" event arrives on the same connection before the RPC result. Caller must be prepared to see "event" lines interleaved with the response.

### clipboard-history-updated
**Payload:** (empty, or no payload)

Emitted when the clipboard history changes (a new entry captured, pinned/deleted/labeled, or paused/resumed).

### clipboard-paste-fallback
**Payload:** `string` (a user-facing toast message)

Emitted when `paste_clipboard_entry` or `type_out_text` could not find the target window and fell back to copying to the clipboard.

---

## Error Handling

All commands return either a value or an error. In JSON-RPC 2.0, an error is `{error: {code: number, message: string}}`. Common codes:

- `2` — `Invalid Request` (malformed JSON)
- `-32601` — `Method not found` (command doesn't exist; see D20 for a registration gotcha)
- `null` or a typed response on success

If a command fails mid-stream (e.g., during an index build), events may show the error, and the final response is still sent.

---

## Test and Integration Notes

**Phase 2 verification (headless over named pipe):** 30 live checks passed, including index build/search, clipboard lifecycle, instant answers, and app launch. Commands verified with exact param shapes from this reference. Engine built with `--no-default-features` and tested in a throwaway %TEMP% data dir with synthetic test files.

**Phase 2 known gaps:**
- Filename search quirk: single common words ("notes", "sample") alone may miss hits; multi-word or full-filename queries work (see search_local_files note).
- No currency/FX conversion in evaluate_quick_query.
- compute_hashes only hashes files, not typed text; text-hashing needs a new code path.
- Text-hashing in the Kit (SHA256 of typed input) is **not** via this engine.

**Headless paste/type-out:** remember_foreground_window is not called automatically; it's meant for the browser to call before showing a clipboard UI. Without it, paste and type-out emit the fallback message.
