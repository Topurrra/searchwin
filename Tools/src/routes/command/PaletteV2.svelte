<script lang="ts">
    /*
      Unified Command Palette — Phase 3.6 shell.

      Replaces (eventually) the three current overlays (`/overlay`,
      `/clipboard-overlay`, `/voice-overlay`) with ONE shell that
      switches between modes:

        - default   → search tools / files / apps / quick actions
                      (current `/overlay` behavior, redesigned)
        - clipboard → recent copies, paste-on-Enter
                      (current `/clipboard-overlay`, mode-pilled)
        - voice     → dictation (Transcribe) or control (Command)
                      (current `/voice-overlay`, sub-toggle)

      Workspace identity preserved: this is the FRONT DOOR. Big tools
      (image convert, duplicate finder, etc.) still open inside the
      workspace pages; the palette routes you there. Small things
      (calculator, paste, system commands) execute inline.

      Status (2026-05-20):
        - 3.6.1 [wip]  Shell + default mode wired. Clipboard / Voice
                       modes stubbed (header chrome ready; mode-specific
                       bodies land in 3.6.2 / 3.6.3).
        - 3.6.2 [todo] Clipboard mode body + paste-on-Enter.
        - 3.6.3 [todo] Voice mode body + Transcribe / Command sub-toggle.
        - 3.6.4 [todo] Polish + macOS feel pass.
        - 3.6.5 [todo] Preview hook for in-app testing.
        - 3.6.6 [todo] Backend hand-off — Tauri window + hotkey routing.

      Old overlays preserved untouched per user rule until 3.6.6 ships.

      Visual design per the user's mockups (2026-05-20):
        - Rounded 16-px panel, transparent edges (no DWM acrylic).
        - Top bar: optional mode pill on the left, scoped search input,
          mic button on the right.
        - Body: section labels at 10-px uppercase + frecency-ranked
          rows. Hotkey chips on CORE pillars (Win+V, Ctrl+Alt+V) for
          discoverability.
        - Footer: contextual hints + persistent Offline badge.
        - macOS feel: elegant, soft shadows, accent reserved for
          indicators, never decoration.
    */
    import { onMount, onDestroy, tick } from 'svelte';
    import { invoke, convertFileSrc } from '@tauri-apps/api/core';
    import { emitTo, listen } from '@tauri-apps/api/event';
    import { getVersion } from '@tauri-apps/api/app';
    import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
    import { currentMonitor } from '@tauri-apps/api/window';
    import { LogicalSize, LogicalPosition } from '@tauri-apps/api/dpi';
    import { goto } from '$app/navigation';
    import { pendingNavigation } from '$lib/stores/pendingNavigation';
    import {
        Search,
        ExternalLink,
        FolderOpen,
        Sparkles,
        AppWindow,
        Folder,
        FileText,
        Calculator,
        Power,
        Settings,
        Terminal,
        Clock,
        Wrench,
        Globe,
        X,
        Mic,
        Lock,
        Clipboard,
        Code2,
        ArrowLeft,
        Zap,
        Mail,
        Braces,
        Palette,
        NotebookPen,
        Hash as HashIcon,
        Type as TypeIcon,
        Image as ImageIcon,
        Eye,
        EyeOff,
        Maximize2,
        X as XIcon,
        Copy,
        Pin,
        PinOff,
        Tag,
        Trash2,
        ChevronDown,
        ChevronRight,
        AlertTriangle,
        Timer,
        Target,
        Bell,
        TerminalSquare,
        Cpu,
        Activity,
        Keyboard,
        Radio,
        Star,
        Smile,
    } from '@lucide/svelte';
    import { searchEmoji } from '$lib/emojiSearch';
    import type { EmojiEntry } from '$lib/emojiData';
    import {
        cyclePaletteScope,
        defaultScopeForPaletteMode,
        modeForPaletteScope,
        type CommandPaletteMode,
        type CommandPaletteScope,
    } from '$lib/commandPaletteState';
    import { emojiRecents, rememberEmoji } from '$lib/stores/emojiRecents';
    // Continuous command mode lives in the MAIN window's coordinator; these
    // are the cross-webview request + the mirrored read-only state.
    import {
        commandMode,
        requestToggleCommandMode,
        initCommandModeMirror,
    } from '$lib/stores/commandMode';
    import { formatBytes, formatDateTime, revealFileSearchResult } from '$lib/stores/fileSearch';
    import { hasHighTierKind, sensitiveKindLabel } from '$lib/types/sensitive';
    import { installedTools, enabledPackIds } from '$lib/stores/toolPacks';
    // Wave F (2026-05-27): chip empty-states need to read notes + create
    // new notes from inside the palette. The notes store is the same one
    // the Notes pillar page reads from.
    import { notes, refreshNotes } from '$lib/stores/notes';
    // Wave J (2026-05-27): polished `.ki` note preview — parse frontmatter
    // out, render markdown body as safe HTML, show updated/tags/pinned in a
    // clean header. Replaces the raw `<pre>` dump that previously showed
    // frontmatter + literal `# heading` syntax for notes.
    import { parseNoteMarkdown, renderNoteHtml } from '$lib/notes/preview';
    import { parseReminderInput } from '$lib/stores/reminders';
    import { settings, THEME_OPTIONS, type Theme } from '$lib/stores/settings';
    import {
        commandAppearance,
        setCommandOpacity,
        setCommandAccent,
        setCommandDesktopBlur,
        setCommandDensity,
        setCommandWidth,
        setCommandPosition,
        setCommandAnimationLevel,
        setCommandAccentGlow,
        setCommandShowChips,
        toggleHiddenSection,
        applyAppearancePatch,
        resetCommandAppearance,
        ACCENT_PRESETS as CMD_ACCENT_PRESETS,
        DENSITY_OPTIONS,
        WIDTH_OPTIONS,
        ANIMATION_OPTIONS,
        HIDABLE_SECTIONS,
        type PaletteSectionId,
        type PalettePosition,
        type PaletteWidth,
        type PaletteAnimationLevel,
        OPACITY_MIN,
        OPACITY_MAX,
    } from '$lib/stores/commandAppearance';
    import {
        voiceSession,
        armVoice,
        disarmVoice,
        onVoicePartial,
        onVoiceTranscript,
    } from '$lib/stores/voiceSession';
    import { executeVoiceCommand, voiceModelLocale } from '$lib/stores/commandRegistry';
    // Cleanup Wave 1 (2026-05-28): dictation pipeline ported from the
    // legacy /voice-overlay. Used in the new `'dictate'` voice sub-mode:
    // normalize transcript → apply editing commands (comma / new line /
    // scratch that / undo) → paste into the previously focused app via
    // paste_snippet_text or type_out_text depending on voiceOutputMode.
    import { processDictation } from '$lib/stores/dictationCommands';
    import { normalizeTranscript } from '$lib/stores/transcriptNormalizer';
    import { recordActivity } from '$lib/stores/activityLog';
    import {
        evaluateFrontendQuickAction,
        type RenderableQuickAction,
    } from '$lib/stores/quickActionEvaluators';
    import type { SettingsSectionId } from '$lib/stores/settingsTarget';
    import {
        myCommands,
        addCommand,
        reloadMyCommands,
        isBang,
        expandBang,
        type MyCommand,
        type MyCommandType,
    } from '$lib/stores/myCommands';
    // Cleanup Wave 1 (2026-05-28): user-snippet expansion ported from the
    // legacy /clipboard-overlay. Typing `/sig` in clipboard mode surfaces
    // matching snippets above the history; Enter expands the template
    // (with variables like {{clipboard}}) and pastes the result into the
    // previously focused app via the same SendInput Ctrl+V infra used
    // for clipboard entries.
    import {
        snippets,
        findSnippets,
        previewSnippet,
        recordSnippetUse,
        refreshSnippets,
        type Snippet,
    } from '$lib/stores/snippets';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    // ToastContainer needs to be mounted on the standalone palette window
    // too — without it, `toast()` calls write to the palette's local toast
    // store but no subscriber renders them (the main window's
    // ToastContainer is on a different webview). When the palette is
    // mounted as an overlay inside the main window, the main window's
    // existing ToastContainer handles output, so we skip ours via
    // `isCommandWindow` to avoid double-rendering toasts. (Wave 2.2 bug
    // fix, 2026-05-26.)
    import ToastContainer from '$lib/ToastContainer.svelte';
    // Cleanup Wave 1.1 (2026-05-28): per-row inline actions on clipboard
    // rows — the wand-popover transform menu the workspace ClipboardHistory
    // page already exposes. Same component, dropped straight in.
    import ClipboardActionMenu from '$lib/components/ClipboardActionMenu.svelte';
    import { _ } from 'svelte-i18n';
    import { t } from '$lib/i18n';

    /* ──────────────────────────────────────────────────────────────
       Props — only set when the palette is rendered as an OVERLAY
       inside the main window (Phase 3.6.5 test binding). When accessed
       as a standalone route (`/command` URL) or a future dedicated
       Tauri overlay window, both default and the component falls back
       to its route/window dismissal logic.

         asOverlay  — render over the live workspace: translucent scrim
                      instead of a solid background, and dismissal /
                      tool-open route through `onClose` + the main
                      route's already-mounted `navigate-tool` listener.
         onClose    — called to tear down the overlay (the parent sets
                      the commandPaletteOpen store to false).
       ────────────────────────────────────────────────────────────── */
    let {
        asOverlay = false,
        onClose,
    }: { asOverlay?: boolean; onClose?: () => void } = $props();

    /** True when running as the REAL dedicated Tauri overlay window
     *  (label 'command', Phase 3.6.6) — as opposed to the embedded-in-
     *  main overlay (`asOverlay`) or the standalone dev route (label
     *  'main'). The real window is `transparent: true` at the OS level,
     *  so its `.cmd-root` must be transparent too — a solid background
     *  would fill the window and hide the desktop/workspace behind it.
     *  Derived (not onMount-set) so the very first paint already has the
     *  right background — no solid-bg flash. */
    let isCommandWindow = $derived(!asOverlay && !isEmbeddedInMain());

    /* ──────────────────────────────────────────────────────────────
       Types
       (Mirror the shapes used by the existing /overlay so we can
       reuse the same backend invokes without translation.)
       ────────────────────────────────────────────────────────────── */

    type LaunchTargetItem = {
        id: string;
        name: string;
        path: string;
        kind: string;
        source: string;
        score: number;
    };

    type FileSearchResultItem = {
        path: string;
        fileName: string;
        entryType: 'file' | 'folder' | string;
        extension: string;
        size: number;
        modifiedMs: number;
        score: number;
        matchedKeywords: string[];
        matchReason: string;
        /** Backend's content-scan tags — present when the indexer
         *  found patterns hinting at sensitive content (private keys,
         *  .env, credentials, etc.). Optional because the filename-
         *  only index path returns null. Surfaced as a "Sensitive"
         *  badge inline next to the title. */
        sensitiveKinds?: string[] | null;
    };

    type ContentSearchResultItem = {
        path: string;
        fileName: string;
        extension: string;
        size: number;
        modifiedMs: number;
        score: number;
        snippet: string;
        snippets: string[];
        matchCount: number;
        matchedKeywords: string[];
    };

    type FileSearchQueryResult = {
        query: string;
        totalHits: number;
        returned: number;
        tookMs: number;
        results: FileSearchResultItem[];
    };

    type ContentSearchQueryResult = {
        query: string;
        totalHits: number;
        returned: number;
        tookMs: number;
        results: ContentSearchResultItem[];
    };

    type LaunchTargetSearchResult = {
        query: string;
        totalHits: number;
        returned: number;
        tookMs: number;
        cacheBuiltAtMs: number | null;
        results: LaunchTargetItem[];
    };

    type KeepItLocalMatch = {
        id: string;
        name: string;
        description: string;
        score: number;
    };

    type RecentItem = {
        kind: 'app' | 'file' | 'folder' | 'tool';
        path: string;
        displayName: string;
        launchCount: number;
        lastLaunchedMs: number;
        score: number;
        // Wave I (2026-05-27): backend now stat's `file` / `folder`
        // kinds after the per-kind truncate so the preview pane can
        // show real Size + Modified values. Both are optional — a
        // deleted / locked / offline path comes through as undefined,
        // which the preview pane treats as "metadata unavailable"
        // rather than literal zero.
        sizeBytes?: number;
        modifiedMs?: number;
    };

    type RecentItemsResult = {
        apps: RecentItem[];
        files: RecentItem[];
        folders: RecentItem[];
        tools: RecentItem[];
    };

    /** Inline quick-action result returned by `evaluate_quick_query`.
     *  Same tagged union the existing /overlay uses — keeping the shape
     *  identical means we can drop in the same backend with zero new
     *  Rust. We only RENDER `calculator` and `unitConversion` in the
     *  palette for now (system commands need a confirm dialog flow that
     *  belongs to a future polish phase; URL / web-search actions need
     *  the `webSearchEnabled` opt-in path which we deliberately don't
     *  pass through here yet). */
    type QuickAction =
        | { type: 'calculator'; expression: string; result: string }
        | { type: 'unitConversion'; original: string; result: string }
        | {
              type: 'systemCommand';
              id: string;
              name: string;
              description: string;
              requiresConfirmation: boolean;
          }
        | { type: 'openUrl'; url: string; display: string }
        | { type: 'webSearch'; provider: string; url: string; query: string };

    /** `RenderableQuickAction` is imported from `quickActionEvaluators`.
     *  Shape-compatible with the backend's calculator + unitConversion
     *  variants, so backend results assign cleanly into state typed as
     *  `RenderableQuickAction`. */

    /** Top-level mode. The mode determines header chrome (mode pill,
     *  scoped placeholder) AND body content. Default opens with no
     *  pill and shows the unified search. */
    type Mode = CommandPaletteMode;
    /** Voice has THREE sub-modes per user spec:
     *    - transcribe: dictation fills the input → search runs
     *    - command:    voice control (scroll up, open chrome, …) —
     *                  does NOT transcribe to the input.
     *    - dictate:    Cleanup Wave 1 (2026-05-28) — speech is processed
     *                  via dictationCommands (comma / new line / scratch
     *                  that) then pasted into the PREVIOUSLY focused app
     *                  (Word, Notes, etc.). The "press hotkey from inside
     *                  Word, speak a sentence, watch it land in the
     *                  document" workflow ported from /voice-overlay. */
    type VoiceSubMode = 'transcribe' | 'command' | 'dictate';

    /* ──────────────────────────────────────────────────────────────
       Mode state
       ────────────────────────────────────────────────────────────── */

    let mode = $state<Mode>('default');
    let voiceSubMode = $state<VoiceSubMode>('transcribe');

    function switchMode(
        next: Mode,
        scope: PaletteScope = defaultScopeForPaletteMode(next),
    ) {
        mode = next;
        paletteScope = scope;
        // Reset query when switching modes — each mode owns its own
        // search scope; carrying text across would be confusing.
        query = '';
        resetPaletteSelection();
        // Preview is only meaningful in clipboard mode (default / voice
        // don't have an inspectable "current item" worth a 320 px pane).
        // Snap it shut whenever we leave clipboard so reentering default
        // doesn't reopen the preview from a previous clipboard visit.
        if (next !== 'clipboard') {
            previewOpen = false;
            previewAutoOpened = false;
        }
    }

    function goBackToDefault() {
        switchMode('default');
    }

    /* ──────────────────────────────────────────────────────────────
       Shared search state (default mode owns these — clipboard / voice
       modes add their own state in 3.6.2 / 3.6.3)
       ────────────────────────────────────────────────────────────── */

    let query = $state('');
    let searching = $state(false);
    let selectedIndex = $state(0);
    let inputEl = $state<HTMLInputElement | null>(null);
    let scrollEl = $state<HTMLDivElement | null>(null);
    let appVersion = $state('');

    // Default-mode search results.
    let fileResults = $state<FileSearchResultItem[]>([]);
    let fileTotalHits = $state(0);
    let queryMs = $state<number | null>(null);
    let launchResult = $state<LaunchTargetSearchResult | null>(null);
    /** Offset for the NEXT pagination request. After the initial fetch
     *  this is FILE_PAGE_SIZE (or whatever count came back). Bumped on
     *  every successful loadMore call. Reset to 0 when the query or
     *  mode changes. */
    let fileNextOffset = $state(0);
    let isLoadingMoreFiles = $state(false);
    /** How many results we request per backend call. /overlay buffers
     *  50 + shows 5; /command shows all received rows immediately and
     *  fetches another page on scroll, so the page size doubles as
     *  "results visible per scroll". */
    const FILE_PAGE_SIZE = 12;
    /** Content-search results — populated only when `searchMode === 'content'`.
     *  Same backend (`search_file_contents`) the /overlay and the workspace
     *  FileSearch page use, so behavior carries through identically. */
    let contentResults = $state<ContentSearchResultItem[]>([]);
    let contentTotalHits = $state(0);

    // Wave 3.3 — Live grep (un-indexed content search fallback).
    // When Tantivy returns nothing for a content query, the palette
    // offers a "Search in a folder live" CTA that runs ripgrep's
    // engine over a user-picked folder. Hits stream in via the
    // `live-grep-hit-{opId}` Tauri event; UI shows them in a
    // dedicated section above content results.
    interface LiveGrepHit {
        path: string;
        lineNumber: number;
        line: string;
        matchStart: number;
        matchEnd: number;
    }
    interface LiveGrepSummary {
        matches: number;
        filesScanned: number;
        cancelled: boolean;
        truncated: boolean;
    }
    let liveGrepHits = $state<LiveGrepHit[]>([]);
    let liveGrepBusy = $state(false);
    let liveGrepFolder = $state<string | null>(null);
    let liveGrepOpId = $state<string | null>(null);
    let liveGrepSummary = $state<LiveGrepSummary | null>(null);
    let liveGrepHitUnlisten: (() => void) | null = null;
    /** Pattern controls (Wave 3.3.3 — invisible to user, 2026-05-27).
     *  Live grep ALWAYS runs literal + case-insensitive — the casual-
     *  query expectation for any "find this" box. Regex mode lives in
     *  the dedicated Regex Tool for power users; we don't expose it
     *  in the palette because mixing the two semantics in one box
     *  confuses people (typing `2+2` should find "2+2", not "match
     *  one+ 2s followed by 2"). The Tantivy index path uses its own
     *  tokenizer; this pair is just for the ripgrep fallback. */
    const LIVE_GREP_LITERAL = true;
    const LIVE_GREP_CASE_SENSITIVE = false;

    // Wave 4.1b-3 (2026-05-27) — Shell command output streaming.
    // When a `shell`-type My Command runs, the backend streams stdout/
    // stderr line-by-line via `my-shell-output-<opId>` Tauri events.
    // The palette renders a streaming output panel (monospace,
    // colored stderr) so the user sees long output (a full `git log`,
    // a `npm test` run) live instead of being limited to a 200-char
    // toast preview.
    interface ShellOutputLine {
        stream: 'stdout' | 'stderr';
        line: string;
    }
    interface ShellExecState {
        opId: string;
        label: string;
        command: string;
        shellKind: 'powershell' | 'pwsh' | 'cmd';
        lines: ShellOutputLine[];
        running: boolean;
        exitCode: number | null;
        cancelled: boolean;
        durationMs: number | null;
        hadStderr: boolean;
        startedAt: number;
    }
    /** The currently displayed shell run (if any). Null when no shell
     *  command has been run this session, or after the user dismisses
     *  the panel with Esc / the close button. We only render ONE at a
     *  time — the previous run is replaced when a new one starts. */
    let shellExec = $state<ShellExecState | null>(null);
    /** Hard cap on streamed lines kept in memory (and rendered). A
     *  runaway `Get-ChildItem -Recurse C:\` could otherwise emit tens
     *  of thousands of paths and the DOM would die. Past the cap we
     *  drop the oldest lines (FIFO) so the user still sees what's
     *  happening at the tail. */
    const SHELL_OUTPUT_CAP = 2000;
    let shellExecUnlisten: (() => void) | null = null;

    /** Which file-search backend we hit on every keystroke. `files` searches
     *  filenames + paths (fast). `content` searches inside files (full-text
     *  index — heavier, gets a slightly longer debounce). Mirrors /overlay's
     *  same-named toggle. */
    let searchMode = $state<'files' | 'content'>('files');

    /* ─── Scope filter chips (Palette Appearance Wave A) ────────────
       A row of chips under the search input lets the user narrow the
       palette to a single category. "All" (default) shows everything;
       picking a chip filters non-matching sections AND, for Clipboard
       / Voice, flips the underlying mode so the user lands on the
       right surface. The active chip is accent-tinted using the
       user's chosen accent color (commandAppearance.accent or the
       theme default). Esc returns scope to 'all' first, then closes
       palette (so the chips never trap the user).                       */

    type PaletteScope = CommandPaletteScope;

    // The Web scope only appears once browser search is switched on — it's
    // off by default, and a destination that always returns nothing is worse
    // than no destination.
    const SCOPE_CHIPS: { id: PaletteScope; label: string }[] = [
        { id: 'all', label: 'All' },
        { id: 'tools', label: 'Tools' },
        { id: 'files', label: 'Files' },
        { id: 'notes', label: 'Notes' },
        { id: 'clipboard', label: 'Clipboard' },
        { id: 'voice', label: 'Voice' },
        { id: 'apps', label: 'Apps' },
        { id: 'windows', label: 'Windows' },
        { id: 'emoji', label: 'Emoji' },
        { id: 'commands', label: 'Commands' },
        { id: 'browser', label: 'Web' },
    ];
    const PRIMARY_SCOPE_IDS = new Set<PaletteScope>([
        'all',
        'files',
        'notes',
        'apps',
        'clipboard',
        'voice',
    ]);

    let paletteScope = $state<PaletteScope>('all');
    let moreScopesOpen = $state(false);
    let moreScopeIndex = $state(0);
    let moreScopesEl = $state<HTMLDivElement | null>(null);

    /** Available scopes drive the visual hierarchy. Keyboard cycling follows
     *  the visible primary row first, then the destinations under More. */
    let availableScopeChips = $derived(
        SCOPE_CHIPS.filter((c) => c.id !== 'browser' || $settings.browserSearchEnabled),
    );
    let primaryScopeChips = $derived(
        availableScopeChips.filter((chip) => PRIMARY_SCOPE_IDS.has(chip.id)),
    );
    let moreScopeChips = $derived(
        availableScopeChips.filter((chip) => !PRIMARY_SCOPE_IDS.has(chip.id)),
    );
    let activeMoreScope = $derived(
        moreScopeChips.find((chip) => chip.id === paletteScope) ?? null,
    );

    /** True when the given section should render given the current scope.
     *  Pass 'always' for things that ignore the scope (the input bar,
     *  the empty state's CTA, etc.). Anything else is shown only when
     *  the active scope is 'all' OR exactly matches `category`. */
    function inScope(category: PaletteScope | 'always'): boolean {
        if (category === 'always') return true;
        if (paletteScope === 'all') return true;
        return paletteScope === category;
    }

    /** Which scopes have a meaningful Files-vs-Inside axis. Apps /
     *  Tools / Clipboard / Voice all do their own ad-hoc search and
     *  don't read `searchMode`; if the user picked Inside under Files
     *  and then switched to Apps, the previous searchMode would carry
     *  over and the backend would still fire `search_file_contents`,
     *  leaving Apps results stale. This helper drives the reset in
     *  `setPaletteScope` so the search engine and the visible picker
     *  always agree. */
    function supportsContentSearch(scope: PaletteScope): boolean {
        // Notes is NOT here on purpose: notes search (search_note_bodies)
        // already covers name AND body in one pass, so the Files/Inside axis
        // is meaningless there — no toggle, no file-content search.
        return scope === 'all' || scope === 'files';
    }

    function setPaletteScope(scope: PaletteScope) {
        moreScopesOpen = false;
        const targetMode = modeForPaletteScope(scope);
        if (mode !== targetMode) {
            switchMode(targetMode, scope);
        } else {
            paletteScope = scope;
            resetPaletteSelection();
        }
        // Commands ←/→ drill-in: any scope change exits the preview-pane
        // item focus so we never carry a drilled-in cursor across chips.
        commandItemsFocused = false;
        // Wave I (2026-05-27): clamp searchMode to what the new scope
        // actually supports. If the user picked Inside under Files /
        // Notes and then hopped to Apps / Tools, the global searchMode
        // would still be 'content' — but those scopes ignore it and
        // would render an empty list while the engine pointlessly ran
        // a content-search. Reset to 'files' so the next runSearch
        // hits the right backend. We also clear stale content results
        // and bump selectedIndex back to the top so arrow-nav doesn't
        // land on a hidden row.
        if (!supportsContentSearch(scope) && searchMode === 'content') {
            searchMode = 'files';
            contentResults = [];
            contentTotalHits = 0;
            // Wave 7.6 (2026-05-28): also clear live-grep state when
            // forced out of content mode — same fix as in setSearchMode.
            liveGrepHits = [];
            liveGrepSummary = null;
            liveGrepAutoFiredFor = null;
            if (liveGrepBusy && liveGrepOpId) {
                void cancelLiveGrep();
            }
            launchResult = null;
            fileNextOffset = 0;
            isLoadingMoreFiles = false;
            if (query.trim()) {
                void runSearch(query);
            }
        }
        // Auto-open the preview (master-detail) for the scopes where it's
        // the primary surface: Commands (category items live in the pane),
        // Files (filename + inside/content), Notes, and Clipboard. We only
        // FORCE it open on scope/mode ENTER — if the user then presses
        // Ctrl+P to close it within the scope, we don't fight them. 'all',
        // 'tools', 'apps', 'voice' keep the manual-toggle behavior.
        if (
            scope === 'commands' ||
            scope === 'files' ||
            scope === 'notes' ||
            scope === 'clipboard'
        ) {
            previewOpen = true;
            // Mark this open as auto-opened so a later switch to a non-auto
            // chip can close it (a manually Ctrl+P'd preview clears this flag
            // in togglePreview and is therefore left alone).
            previewAutoOpened = true;
            // Scope transitions always reset selection, so the first item is
            // immediately ready for preview instead of inheriting a stale row.
        } else if (previewAutoOpened) {
            // Switching from an auto-open chip ('all'/'tools'/'apps'/'voice')
            // closes the auto-opened preview — it shouldn't stick around on a
            // surface that never asked for it. A manually-opened preview has
            // previewAutoOpened === false, so it survives the switch.
            previewOpen = false;
            previewAutoOpened = false;
        }
        // Wave F (2026-05-27): refresh the notes list when the user
        // switches into the Notes chip — they expect the freshest list,
        // and refreshing is cheap (one file walk under the Notes dir).
        if (scope === 'notes') {
            void refreshNotes();
        }
        // Build the browser snapshot while the user is still typing, so the
        // one-time DB copy cost is paid before the first query rather than
        // inside it. Best-effort — a failure just means query #1 pays.
        if (scope === 'browser' && $settings.browserSearchEnabled) {
            void invoke('warm_browser_search', {
                includeHistory: $settings.browserHistoryEnabled,
            }).catch(() => {});
        }
        // Wave G (2026-05-27): when the user clicks Apps, eagerly load
        // EVERY installed app (cache-resident, no filesystem walk) so
        // the empty-state body shows the full list instead of just
        // recents + a hint. browseAll=true + empty query hits the
        // dedicated cache-read branch in search_launch_targets.
        if (scope === 'apps' && !browseAllAppsRaw.length) {
            void loadAllInstalledApps();
        }
        // Commands chip (2026-06-13): eager-load all three sources. The
        // running-apps list is refreshed EVERY time the chip is entered
        // (processes go stale fast); the static system-action list loads
        // once. The system-info snapshot loads once here too so the card
        // is ready before the user looks for it.
        // Windows chip: always re-enumerate on entry. Unlike the app list
        // this cannot be cached — titles change as the user works and a
        // closed window's HWND is a dead row.
        if (scope === 'windows') {
            void loadOpenWindows();
        }
        if (scope === 'commands') {
            void loadProcessGroups();
            if (!systemInfo) void loadSystemInfo();
            if (!systemCommands.length) void loadSystemCommands();
        }
    }

    function choosePaletteScope(scope: PaletteScope) {
        setPaletteScope(scope);
        void tick().then(() => inputEl?.focus());
    }

    function focusMoreScope(index: number) {
        void tick().then(() => {
            document
                .querySelector<HTMLButtonElement>(`[data-more-scope-index="${index}"]`)
                ?.focus();
        });
    }

    function openMoreScopes(index?: number) {
        if (!moreScopeChips.length) return;
        actionsOpen = false;
        const activeIndex = moreScopeChips.findIndex((chip) => chip.id === paletteScope);
        moreScopeIndex = index ?? (activeIndex >= 0 ? activeIndex : 0);
        moreScopesOpen = true;
        focusMoreScope(moreScopeIndex);
    }

    function closeMoreScopes(refocusInput = false) {
        if (!moreScopesOpen) return;
        moreScopesOpen = false;
        if (refocusInput) void tick().then(() => inputEl?.focus());
    }

    function toggleMoreScopes() {
        if (moreScopesOpen) {
            closeMoreScopes();
        } else {
            openMoreScopes();
        }
    }

    function moveMoreScope(delta: number) {
        const count = moreScopeChips.length;
        if (!count) return;
        moreScopeIndex = (moreScopeIndex + delta + count) % count;
        focusMoreScope(moreScopeIndex);
    }

    function onWindowPointerDown(event: PointerEvent) {
        if (!moreScopesOpen || !moreScopesEl || !(event.target instanceof Node)) return;
        if (!moreScopesEl.contains(event.target)) closeMoreScopes();
    }

    function onMoreScopesFocusOut(event: FocusEvent) {
        const container = event.currentTarget as HTMLElement;
        const next = event.relatedTarget;
        if (!(next instanceof Node) || !container.contains(next)) closeMoreScopes();
    }

    /** Wave G: cached full list of installed apps for the "Apps" chip
     *  empty-state. Populated lazily on first chip click. RAW = whatever
     *  the backend returned (.exe + every Start Menu / Desktop shortcut
     *  pointing at it). The `browseAllApps` derived strips duplicates
     *  by name, mirroring `dedupedLaunchResults` — without this the
     *  Apps chip shows "KeepItLocal" three times. */
    let browseAllAppsRaw = $state<LaunchTargetItem[]>([]);
    let browseAllAppsLoading = $state(false);
    let browseAllApps = $derived.by<LaunchTargetItem[]>(() => {
        if (!browseAllAppsRaw.length) return [];
        const byName = new Map<string, LaunchTargetItem>();
        for (const item of browseAllAppsRaw) {
            const key = item.name.toLowerCase().trim();
            const existing = byName.get(key);
            // Keep the highest-scoring entry — for the browse-all path
            // that's the frecency-boost-weighted score the backend
            // attaches, so the .exe in Program Files (which the user
            // has actually launched) wins over the cold .lnk shortcut.
            if (!existing || item.score > existing.score) {
                byName.set(key, item);
            }
        }
        return Array.from(byName.values()).sort(
            (a, b) =>
                b.score - a.score ||
                a.name.toLowerCase().localeCompare(b.name.toLowerCase()),
        );
    });

    async function loadAllInstalledApps() {
        if (browseAllAppsLoading) return;
        browseAllAppsLoading = true;
        try {
            const result = await invoke<LaunchTargetSearchResult>('search_launch_targets', {
                options: { query: '', limit: 300, browseAll: true },
            });
            browseAllAppsRaw = result?.results ?? [];
        } catch (error) {
            console.warn('loadAllInstalledApps failed:', error);
        } finally {
            browseAllAppsLoading = false;
        }
    }

    /** Per-path resolved icon URLs. Populated lazily on result arrival via
     *  the `ensure_launcher_icon` Tauri command, then the row uses the real
     *  Windows shell icon instead of a generic Lucide. Marker value `null`
     *  means "we asked but the backend couldn't extract one" — UI falls back
     *  to the Lucide icon for that path forever in the current session. */
    let entryIcons = $state<Record<string, string | null>>({});

    /* ──────────────────────────────────────────────────────────────
       Commands scope (2026-06-13): a "Commands" chip surfacing running
       apps (kill), system actions (lock/sleep/…), and a read-only
       system-info card. Backed by four locked backend commands. All
       camelCase per the Tauri serde contract.
       ────────────────────────────────────────────────────────────── */
    interface ProcessGroup {
        name: string;
        exePath: string | null;
        pids: number[];
        memoryBytes: number;
        windowCount: number;
    }
    interface SystemDisk {
        mount: string;
        freeBytes: number;
        totalBytes: number;
    }
    interface SystemInfo {
        osName: string;
        osVersion: string;
        cpuModel: string;
        logicalCores: number;
        ramUsedBytes: number;
        ramTotalBytes: number;
        batteryPercent: number | null;
        uptimeSecs: number;
        disks: SystemDisk[];
    }
    interface SystemCommandItem {
        id: string;
        name: string;
        description: string;
        requiresConfirmation: boolean;
        group: string;
    }

    /** Running GUI apps grouped by application (kill rows). Refreshed
     *  every time the Commands chip is entered — they go stale fast. */
    let processGroups = $state<ProcessGroup[]>([]);
    /** Read-only machine snapshot for the system-info card. Loaded once
     *  lazily; cheap enough that a stale RAM figure is acceptable. */
    let systemInfo = $state<SystemInfo | null>(null);
    /** Backend-defined system actions (lock / sleep / shutdown / …).
     *  Static list — loaded once. Dispatched via the existing
     *  `execute_system_command` path (reusing `activateQuickAction`). */
    let systemCommands = $state<SystemCommandItem[]>([]);

    /** Commands chip (2026-06-13 redesign): the 4 category rows shown in
     *  the Commands scope. Selecting a row drives the master-detail
     *  preview pane (the items live there), never dismissing the palette.
     *  Keys are `cmd-cat:<id>`; the preview pane routes on `<id>`. */
    type CommandCategoryId =
        | 'system-info'
        | 'system-actions'
        | 'control-panel'
        | 'running-apps';
    const COMMAND_CATEGORIES: { id: CommandCategoryId; label: string }[] = [
        { id: 'system-info', label: 'System Info' },
        { id: 'system-actions', label: 'System Actions' },
        { id: 'control-panel', label: 'Control Panel' },
        { id: 'running-apps', label: 'Running apps' },
    ];

    async function loadProcessGroups() {
        try {
            processGroups = await invoke<ProcessGroup[]>('list_processes');
        } catch (error) {
            console.warn('list_processes failed:', error);
        }
    }

    /* ──────────────────────────────────────────────────────────────
       Windows chip — a real window switcher (2026-07-29). Lists every
       alt-tab-eligible window; Enter focuses the chosen one.

       Distinct from the Commands chip's "Running apps": that groups by
       APPLICATION and its action is kill. This lists individual WINDOWS
       and its action is switch — so a browser with six tabs-as-windows
       shows six rows here and one row there.

       Note the pre-existing voice command "switch window" (which fires
       Alt+Tab) is untouched — it's a blind toggle, not a switcher.
       ────────────────────────────────────────────────────────────── */
    interface WindowInfo {
        hwnd: number;
        title: string;
        pid: number;
        app: string;
        exePath: string | null;
        isForeground: boolean;
    }

    /** Open windows, refreshed every time the Windows chip is entered —
     *  window titles and z-order go stale within seconds. */
    let openWindows = $state<WindowInfo[]>([]);
    let openWindowsLoading = $state(false);
    let openWindowsLoadError = $state(false);

    async function loadOpenWindows() {
        openWindowsLoading = true;
        openWindowsLoadError = false;
        try {
            openWindows = await invoke<WindowInfo[]>('list_windows');
        } catch (error) {
            console.warn('list_windows failed:', error);
            openWindows = [];
            openWindowsLoadError = true;
        } finally {
            openWindowsLoading = false;
        }
    }

    /** Windows to show. Windows-scope only: in 'all' these rows would
     *  double every running app already surfaced by the Apps section.
     *  Matches on title AND app name so both "chrome" and a page title
     *  find the same row. */
    let windowMatches = $derived.by<WindowInfo[]>(() => {
        if (paletteScope !== 'windows') return [];
        return openWindows.filter((w) => commandQueryMatches(`${w.title} ${w.app}`));
    });

    /** Enter on a window row: focus it, then get out of the way. Focus is
     *  a plain switch, never a toggle — a switcher that minimized the thing
     *  you just picked would be a trap. (Toggle is per-app hotkeys' job.) */
    async function focusWindowRow(win: WindowInfo) {
        try {
            await invoke('focus_window', { hwnd: win.hwnd });
            await hidePalette();
        } catch (error) {
            console.warn('focus_window failed:', error);
            errorToast("Couldn't switch to that window", error, {
                hint: 'The window may have been closed. Reopen the palette to refresh the list.',
            });
        }
    }
    /* ──────────────────────────────────────────────────────────────
       Emoji chip — an offline emoji picker (2026-07-29). Enter copies
       the emoji to the clipboard and dismisses the palette, so the next
       keystroke is Ctrl+V wherever the user was already typing.

       The dataset is BUNDLED (src/lib/emojiData.ts — Unicode Character
       Database names, Unicode License). Nothing is fetched, ever, at
       build time or runtime. Search + ranking live in $lib/emojiSearch
       so they can be unit-tested; recents are a small localStorage list
       ($lib/stores/emojiRecents), deliberately NOT the Rust frecency
       system — this is one section's ordering hint, not global ranking.
       ────────────────────────────────────────────────────────────── */

    /** Emoji to show. Emoji-scope only: these rows have nothing to do
     *  with files/apps/tools and would drown the 'all' list. No limit:
     *  searchEmoji's default cap is 60 (a "few suggestions" default), but
     *  this is a full picker — the bundled table (~1.9k glyphs) is small
     *  and the grid scrolls, so show all of it. */
    let emojiMatches = $derived.by<EmojiEntry[]>(() => {
        if (paletteScope !== 'emoji') return [];
        return searchEmoji(query, $emojiRecents, Infinity);
    });

    /** Column count of the emoji grid. Fixed rather than `auto-fill` so
     *  ↑/↓ can move by exactly one row — with auto-fill the key handler
     *  would have to measure the rendered width every keystroke. Kept in
     *  sync with `.cmd-emoji-grid`'s `repeat(12, ...)` below. */
    const EMOJI_COLS = 12;

    /** Name of the highlighted glyph, shown in the section header. The
     *  grid cells are glyph-only, so this is the only place the name is
     *  readable without hovering. */
    let selectedEmojiName = $derived.by(() => {
        const key = selectables[selectedIndex]?.key;
        if (!key?.startsWith('emoji:')) return '';
        return emojiMatches.find((e) => e.c === key.slice('emoji:'.length))?.n ?? '';
    });

    /** Enter on an emoji row: clipboard, remember, get out of the way.
     *  Mirrors `activateQuickAction`'s copy path (navigator.clipboard +
     *  errorToast) rather than adding a Rust command for static text.
     *  The palette closing IS the confirmation — no flash needed, the
     *  user's next keystroke is the paste. */
    async function copyEmojiRow(entry: EmojiEntry) {
        try {
            await navigator.clipboard.writeText(entry.c);
            rememberEmoji(entry.c);
            await hidePalette();
        } catch (error) {
            console.warn('emoji copy failed:', error);
            errorToast("Couldn't copy that emoji", error, {
                hint: 'Another app may be holding the clipboard. Try copying again.',
            });
        }
    }
    async function loadSystemInfo() {
        try {
            systemInfo = await invoke<SystemInfo>('system_info');
        } catch (error) {
            console.warn('system_info failed:', error);
        }
    }
    async function loadSystemCommands() {
        try {
            systemCommands = await invoke<SystemCommandItem[]>('list_system_commands');
        } catch (error) {
            console.warn('list_system_commands failed:', error);
        }
    }

    /** Confirm before force-closing a group of processes. Sets the shared
     *  confirm modal with a destructive (red) action that kills every pid
     *  in the group, then refreshes the running-apps list. */
    function openKillConfirm(group: ProcessGroup, refresh = loadProcessGroups) {
        pendingConfirm = {
            title: 'Close ' + group.name + '?',
            description:
                'Force-closes ' +
                group.name +
                (group.windowCount > 1 ? ' (' + group.windowCount + ' windows)' : '') +
                '. Unsaved work will be lost.',
            danger: true,
            onConfirm: async () => {
                try {
                    await invoke('kill_process', { pids: group.pids, confirmed: true });
                    await refresh();
                } catch (error) {
                    console.warn('kill_process failed:', error);
                    toast(`Couldn't close ${group.name}: ${error}`, 'error');
                }
            },
        };
        void tick().then(() => confirmCancelEl?.focus());
    }

    /** AND-token match over a haystack — mirrors `timeFocusMatches`'
     *  every-token-must-appear contract so the Commands filters feel
     *  identical to the rest of the palette. Empty query → matches all. */
    function commandQueryMatches(haystack: string): boolean {
        const raw = query.trim().toLowerCase();
        if (!raw) return true;
        const tokens = raw.split(/\s+/).filter(Boolean);
        const hay = haystack.toLowerCase();
        return tokens.every((tok) => hay.includes(tok));
    }

    /** Running apps to show as kill rows. In the Commands scope: ALL
     *  groups, filtered by the typed query over the app name. In 'all':
     *  ONLY when the query starts with "kill " — the text after it
     *  filters by name. Keeps "kill noise" out of All unless asked for. */
    let killRows = $derived.by<ProcessGroup[]>(() => {
        if (paletteScope === 'commands') {
            return processGroups.filter((g) => commandQueryMatches(g.name));
        }
        if (paletteScope === 'all') {
            const raw = query.trim();
            const m = /^kill\s+(.*)$/i.exec(raw);
            if (!m) return [];
            const term = m[1].trim().toLowerCase();
            if (!term) return processGroups;
            const tokens = term.split(/\s+/).filter(Boolean);
            return processGroups.filter((g) => {
                const hay = g.name.toLowerCase();
                return tokens.every((tok) => hay.includes(tok));
            });
        }
        return [];
    });

    /** Whether the system-info data is wanted. Click-only now (2026-06-13):
     *  reachable solely via Commands → System Info, never from a keyword in
     *  the All scope. Drives the lazy-load $effect below. */
    let showSystemInfo = $derived.by<boolean>(() => {
        if (mode !== 'default') return false;
        return paletteScope === 'commands';
    });
    // Lazy-load the system snapshot the first time it's actually wanted.
    $effect(() => {
        if (showSystemInfo && !systemInfo) void loadSystemInfo();
    });

    /** Uptime seconds → "Xh Ym" (or "Ym" under an hour). */
    function formatUptime(secs: number): string {
        const total = Math.max(0, Math.floor(secs));
        const h = Math.floor(total / 3600);
        const m = Math.floor((total % 3600) / 60);
        return h > 0 ? `${h}h ${m}m` : `${m}m`;
    }

    /** Build the synthetic quick-action shape `activateQuickAction`
     *  expects for a system command, so the Commands scope reuses the
     *  exact same confirm-gated dispatch the typed path already uses. */
    function activateSystemAction(item: SystemCommandItem) {
        void activateQuickAction({
            type: 'systemCommand',
            id: item.id,
            name: item.name,
            description: item.description,
            requiresConfirmation: item.requiresConfirmation,
        });
    }

    /** Click handler for a Commands category row: move the selection to it
     *  (so `previewItem` recomputes to that category) and make sure the
     *  master-detail preview is open. The palette never dismisses — these
     *  rows are `opens:false` and just drive the detail pane. */
    function selectCommandCategory(key: string) {
        const idx = selectables.findIndex((s) => s.key === key);
        if (idx >= 0) selectedIndex = idx;
        previewOpen = true;
    }

    /** Deduplicated launch results — backend often returns BOTH the
     *  actual app (`.exe` in Program Files) AND a desktop / Start Menu
     *  shortcut to it. From the user's perspective they're the same
     *  thing ("Doom" + "Doom shortcut" = noise), so we collapse by
     *  name (case-insensitive) and keep the highest-scoring entry.
     *  Genuinely different apps with overlapping names (rare) stay
     *  separate because they have different `name` strings. */
    let dedupedLaunchResults = $derived.by<LaunchTargetItem[]>(() => {
        if (!launchResult?.results.length) return [];
        const byName = new Map<string, LaunchTargetItem>();
        for (const item of launchResult.results) {
            const key = item.name.toLowerCase().trim();
            const existing = byName.get(key);
            if (!existing || item.score > existing.score) {
                byName.set(key, item);
            }
        }
        return Array.from(byName.values()).sort((a, b) => b.score - a.score);
    });

    /** Paths of app launch targets that map to a currently-running windowed app
     *  (Tinycast-style "see what's running" dot). Populated by an effect that
     *  asks the backend which of the VISIBLE app rows are running — bounded to
     *  the shown handful, never the whole app catalog. Latest-wins guarded so
     *  fast typing can't paint a stale dot from an earlier query. */
    let runningAppPaths = $state<Set<string>>(new Set());
    let runningQueryToken = 0;
    $effect(() => {
        const paths = dedupedLaunchResults.map((a) => a.path);
        if (paths.length === 0) {
            if (runningAppPaths.size) runningAppPaths = new Set();
            return;
        }
        const token = ++runningQueryToken;
        void invoke<string[]>('launch_targets_running', { paths })
            .then((running) => {
                if (token !== runningQueryToken) return; // a newer query won
                runningAppPaths = new Set(running);
            })
            .catch(() => {
                // Non-fatal: the dot is a nicety, not a feature the row needs.
                if (token === runningQueryToken) runningAppPaths = new Set();
            });
    });

    /** File / content results can carry duplicate paths in rare cases:
     *  pagination offsets can overlap when the index re-builds between
     *  pages, the backend can surface the same path through two index
     *  paths (filename + USN mirror), and symlinks can resolve to the
     *  same target from two different recorded paths. Keyed `{#each}`
     *  blocks require unique keys, so we dedup by path and keep the
     *  highest-scoring entry — same recipe as `dedupedLaunchResults`. */
    let dedupedFileResults = $derived.by<FileSearchResultItem[]>(() => {
        if (!fileResults.length) return [];
        const byPath = new Map<string, FileSearchResultItem>();
        for (const item of fileResults) {
            const existing = byPath.get(item.path);
            if (!existing || item.score > existing.score) {
                byPath.set(item.path, item);
            }
        }
        // Preserve original ordering by walking the source list and
        // emitting the chosen entry on first encounter. Pure sort-by-
        // score would change the visible order, which is set by the
        // backend's relevance ranking already.
        const seen = new Set<string>();
        const out: FileSearchResultItem[] = [];
        for (const item of fileResults) {
            const winner = byPath.get(item.path);
            if (winner && !seen.has(item.path)) {
                out.push(winner);
                seen.add(item.path);
            }
        }
        return out;
    });

    let dedupedContentResults = $derived.by<ContentSearchResultItem[]>(() => {
        if (!contentResults.length) return [];
        const byPath = new Map<string, ContentSearchResultItem>();
        for (const item of contentResults) {
            const existing = byPath.get(item.path);
            if (!existing || item.score > existing.score) {
                byPath.set(item.path, item);
            }
        }
        const seen = new Set<string>();
        const out: ContentSearchResultItem[] = [];
        for (const item of contentResults) {
            const winner = byPath.get(item.path);
            if (winner && !seen.has(item.path)) {
                out.push(winner);
                seen.add(item.path);
            }
        }
        return out;
    });
    let keepitlocalMatches = $state<KeepItLocalMatch[]>([]);
    let recentItems = $state<RecentItemsResult | null>(null);
    let toolFrecencyBoosts = $state<Record<string, number>>({});

    /** Total hits visible in the unified INSIDE FILES section (Wave
     *  3.3.3): Tantivy results + capped live-grep hits. The "+"
     *  suffix in the header (rendered when liveGrepSummary.truncated)
     *  hints that more matches exist; the user doesn't need to know
     *  WHICH engine produced them. */
    let insideHitCount = $derived(
        dedupedContentResults.length + Math.min(liveGrepHits.length, 200),
    );

    // ─── Settings search ─────────────────────────────────────────────
    // Type a setting name/keyword in default mode → jump straight to that
    // Settings section. Section ids match Settings.svelte's SectionId; the
    // deep-link is carried to the main window via navigate-tool's
    // `settingsSection` field (or pendingNavigation in embedded test mode).
    const SETTINGS_SECTIONS: { id: SettingsSectionId; label: string; keywords: string }[] = [
        { id: 'system', label: 'System', keywords: 'system startup launch login boot autostart' },
        { id: 'appearance', label: 'Appearance', keywords: 'appearance theme font language dark light color palette' },
        { id: 'shortcuts', label: 'Shortcuts', keywords: 'shortcuts hotkeys keys keyboard chord voice push to talk command palette clipboard paste autopaste bang web search sticky note recording start stop' },
        { id: 'voice', label: 'Voice', keywords: 'voice vosk dictation speech model microphone mic push to talk' },
        { id: 'fileIndex', label: 'File Index', keywords: 'index indexing file name filename search rebuild watcher drives' },
        { id: 'contentIndex', label: 'Content Index', keywords: 'index indexing content text inside file search rebuild' },
        { id: 'toolpacks', label: 'Tool Packs', keywords: 'tool packs install enable modules' },
        { id: 'onboarding', label: 'Onboarding', keywords: 'onboarding tour welcome tips replay' },
        { id: 'storage', label: 'Storage & Privacy', keywords: 'storage privacy disk wipe data delete usage' },
        { id: 'logs', label: 'Activity & Diagnostics', keywords: 'activity diagnostics logs errors audit' },
        { id: 'reset', label: 'Reset', keywords: 'reset defaults restore' },
    ];
    /** Settings sections matching the current query (label or keyword
     *  substring). Default mode only, ≥2 chars to avoid noise, capped at 4. */
    let settingsMatches = $derived.by(() => {
        if (mode !== 'default') return [];
        const q = query.trim().toLowerCase();
        if (q.length < 2) return [];
        return SETTINGS_SECTIONS.filter(
            (s) => s.label.toLowerCase().includes(q) || s.keywords.includes(q),
        ).slice(0, 4);
    });

    // ─── My Commands matching ────────────────────────────────────────
    // User-defined quicklinks / bangs / app-or-file openers. Default mode.
    //   - bang: first word === keyword → the rest is the {query} arg.
    //   - others: keyword prefix match OR label substring.
    interface MyCommandMatch {
        cmd: MyCommand;
        arg: string; // the bang query (empty for non-bangs)
    }
    let myCommandMatches = $derived.by<MyCommandMatch[]>(() => {
        if (mode !== 'default') return [];
        const raw = query.trim();
        if (!raw) return [];
        const lower = raw.toLowerCase();
        const firstWord = lower.split(/\s+/)[0];
        const rest = raw.slice(firstWord.length).trim();
        const out: MyCommandMatch[] = [];
        for (const cmd of $myCommands) {
            if (isBang(cmd)) {
                if (firstWord === cmd.keyword) {
                    out.push({ cmd, arg: rest });
                } else if (
                    // Prefix-attached bangs with no space — "r/rust", "?foo".
                    // When the keyword ends in a non-alphanumeric char it can
                    // glue straight onto the query (mirrors the backend bang
                    // parser). Such a keyword can't collide with a normal
                    // word, so this never false-matches plain text.
                    /[^a-z0-9]$/i.test(cmd.keyword) &&
                    lower.startsWith(cmd.keyword) &&
                    raw.length > cmd.keyword.length
                ) {
                    out.push({ cmd, arg: raw.slice(cmd.keyword.length).trim() });
                }
            } else if (cmd.keyword.startsWith(lower) || cmd.label.toLowerCase().includes(lower)) {
                out.push({ cmd, arg: '' });
            }
        }
        return out.slice(0, 6);
    });

    /* ── Time & Focus palette integration (pack-gated) ──────────────────
       The Time & Focus tools' state lives in the MAIN window; the palette
       drives them via the timeFocusBridge (emit → main). Only surfaced when
       the pack is enabled. */
    let timeFocusEnabled = $derived($enabledPackIds.includes('time-focus'));

    // "remind me to X at 3pm" → a one-Enter reminder (created in the main
    // window). Parsing reused from the Reminders tool.
    interface ReminderMatch {
        text: string;
        dueMs: number;
    }
    let reminderMatch = $derived.by<ReminderMatch | null>(() => {
        if (mode !== 'default' || !timeFocusEnabled) return null;
        const raw = query.trim();
        if (!/^remind(er)?\b/i.test(raw)) return null;
        return parseReminderInput(raw);
    });

    // "note: buy milk" → a one-Enter quick note (created in the main window via
    // the palette→main bridge). Needs `note:` or `note ` so it never shadows a
    // file/tool search like "notepad" or "notes". Notes is a core pillar, so
    // this isn't pack-gated.
    let noteMatch = $derived.by<string | null>(() => {
        if (mode !== 'default') return null;
        const m = query.trim().match(/^note[:\s]+(.+)/i);
        const text = m?.[1]?.trim();
        return text ? text : null;
    });

    // "new note" / "create note" / "quick note" / "sticky" / bare "note" →
    // open the lightweight sticky quick-note window (a separate, RAM-minimal
    // window that saves a `.ki` file). Distinct from the `note: <text>` inline
    // quick-add above. Notes is a core pillar, so this isn't pack-gated.
    let quickNoteMatch = $derived.by<boolean>(() => {
        if (mode !== 'default') return false;
        if (noteMatch) return false; // `note: text` quick-add owns that input
        const raw = query.trim().toLowerCase();
        if (raw.length < 3) return false;
        return /^(notes?|new note|create( a)? note|quick ?note|sticky( note)?)$/.test(raw);
    });

    // Focus control verbs, filtered by the typed query.
    interface TimeFocusAction {
        id: string;
        label: string;
        keywords: string;
        event: 'palette-focus';
        action: string;
        icon: any;
    }
    const TIME_FOCUS_ACTIONS: TimeFocusAction[] = [
        { id: 'focus-start', label: 'Start Focus session', keywords: 'focus distraction website block', event: 'palette-focus', action: 'start', icon: Target },
        { id: 'focus-stop', label: 'Stop Focus', keywords: 'focus', event: 'palette-focus', action: 'stop', icon: Target },
    ];
    let timeFocusMatches = $derived.by<TimeFocusAction[]>(() => {
        if (mode !== 'default' || !timeFocusEnabled) return [];
        const raw = query.trim().toLowerCase();
        if (raw.length < 3) return [];
        if (/^remind(er)?\b/i.test(raw)) return []; // reminder quick-add owns these
        const tokens = raw.split(/\s+/).filter(Boolean);
        return TIME_FOCUS_ACTIONS.filter((a) => {
            const hay = `${a.label} ${a.keywords}`.toLowerCase();
            return tokens.every((tok) => hay.includes(tok));
        }).slice(0, 5);
    });

    async function createReminderFromPalette(m: ReminderMatch) {
        try {
            await emitTo('main', 'palette-create-reminder', { text: m.text, dueMs: m.dueMs });
            toast(`Reminder set: ${m.text}`, 'success', 3000);
        } catch (error) {
            errorToast("Couldn't set that reminder", error, {
                hint: 'Try a simpler phrase like "tomorrow 9am" or "in 2 hours".',
            });
        }
        await hidePalette();
    }

    async function createNoteFromPalette(text: string) {
        try {
            await emitTo('main', 'palette-create-note', { text });
            const preview = text.length > 40 ? `${text.slice(0, 40)}…` : text;
            toast(`Note saved: ${preview}`, 'success', 3000);
        } catch (error) {
            errorToast("Couldn't save that note", error, {
                hint: 'The Notes folder may be unavailable — try opening the Notes tool to verify access.',
            });
        }
        await hidePalette();
    }

    /** Open the lightweight sticky quick-note window, then dismiss the palette. */
    async function openQuickNote() {
        try {
            await invoke('show_quick_note');
        } catch (error) {
            errorToast("Couldn't open the quick-note", error, {
                hint: 'The Notes folder may be unavailable — try opening the Notes tool to verify access.',
            });
        }
        await hidePalette();
    }
    async function runTimeFocusAction(a: TimeFocusAction) {
        try {
            await emitTo('main', a.event, { action: a.action });
        } catch (error) {
            errorToast("Couldn't run that quick-action", error, {
                hint: 'Try a simpler expression — quick-actions support math, conversions, and a few utilities (press Ctrl+/ for the full list).',
            });
        }
        await hidePalette();
    }

    /** Format a due timestamp for the reminder preview row. */
    function formatReminderDue(ms: number): string {
        const d = new Date(ms);
        const now = new Date();
        const tomorrow = new Date(now);
        tomorrow.setDate(now.getDate() + 1);
        const time = d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
        if (d.toDateString() === now.toDateString()) return `Today ${time}`;
        if (d.toDateString() === tomorrow.toDateString()) return `Tomorrow ${time}`;
        return `${d.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' })} ${time}`;
    }

    /* ──────────────────────────────────────────────────────────────
       "Apps" = installed Windows programs + KeepItLocal tools. The three
       PILLARS — Search (file-search), Clipboard (clipboard-history) and
       Voice (voice-to-text) — are NOT tools; they live in the CORE
       section and their own overlays, so they're excluded from the
       Suggested Apps / Recent Apps lists. "Recent" always means items the
       user actually OPENED (resolved from the frecency log) — never the
       raw search text they typed (that's how the search overlay works).
       ────────────────────────────────────────────────────────────── */
    const EXCLUDED_PILLAR_TOOL_IDS = new Set([
        'file-search',
        'clipboard-history',
        'voice-to-text',
    ]);

    /** Recent "apps": recently-opened KeepItLocal tools (minus pillars)
     *  merged with recently-launched Windows programs, ordered by most
     *  recent use. Items carry their `kind` ('tool' | 'app') so the row
     *  renders the right icon + activation. */
    let recentApps = $derived.by<RecentItem[]>(() => {
        if (!recentItems) return [];
        const tools = recentItems.tools.filter(
            (t) => !EXCLUDED_PILLAR_TOOL_IDS.has(t.path),
        );
        return [...tools, ...recentItems.apps]
            .sort((a, b) => b.lastLaunchedMs - a.lastLaunchedMs)
            .slice(0, 4);
    });

    /* ──────────────────────────────────────────────────────────────
       User-tunable appearance (Settings → Command Palette). Applied as
       CSS variables on `.cmd-root` so they cascade through the palette
       without touching any other surface. Opacity drives the window
       panel's color-mix percentage; the accent override also recomputes
       a soft tint + a readable contrast color so accent-on text stays
       legible whatever color the user picks.
       ────────────────────────────────────────────────────────────── */
    /** Pick black/white for text sitting ON the accent, from the accent's
     *  luminance. Accent comes from a color input so it's always #rgb/#rrggbb. */
    function accentContrast(hex: string): string {
        const m = hex.trim().replace(/^#/, '');
        let r = 0;
        let g = 0;
        let b = 0;
        if (m.length === 3) {
            r = parseInt(m[0] + m[0], 16);
            g = parseInt(m[1] + m[1], 16);
            b = parseInt(m[2] + m[2], 16);
        } else if (m.length === 6) {
            r = parseInt(m.slice(0, 2), 16);
            g = parseInt(m.slice(2, 4), 16);
            b = parseInt(m.slice(4, 6), 16);
        } else {
            return '#ffffff';
        }
        const luminance = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
        return luminance > 0.55 ? '#0a0a0a' : '#ffffff';
    }

    let cmdRootStyle = $derived.by(() => {
        const a = $commandAppearance;
        const parts: string[] = [`--cmd-panel-opacity: ${Math.round(a.opacity * 100)}%`];
        if (a.accent) {
            parts.push(`--color-accent: ${a.accent}`);
            parts.push(`--color-accent-soft: color-mix(in srgb, ${a.accent} 16%, transparent)`);
            parts.push(`--color-accent-contrast: ${accentContrast(a.accent)}`);
        }
        // Wave B (2026-05-27): density → row height + vertical gap.
        // Pull the canonical pixel value from the DENSITY_OPTIONS table so
        // the picker UI and the rendered surface can never drift apart.
        const densityRow =
            DENSITY_OPTIONS.find((d) => d.id === a.density)?.rowPx ?? 32;
        parts.push(`--cmd-row-height: ${densityRow}px`);
        parts.push(`--cmd-row-pad-y: ${Math.max(2, Math.round((densityRow - 22) / 2))}px`);
        // Wave D (2026-05-27, fixed in Wave F): animation level →
        // override the global motion tokens AT THE PALETTE ROOT so
        // every transition / animation reading `var(--dur-micro)` or
        // `var(--dur-enter)` automatically uses the scaled duration.
        // The default tokens live in `styles.css` (130ms / 190ms).
        // Multiplying once here is far more reliable than calc()-ing
        // at every transition rule.
        const motionMult =
            ANIMATION_OPTIONS.find((m) => m.id === a.animationLevel)?.mult ?? 1;
        parts.push(`--cmd-motion-mult: ${motionMult}`);
        if (motionMult === 0) {
            // Reduced — collapse everything to instant.
            parts.push(`--dur-micro: 0ms`);
            parts.push(`--dur-enter: 0ms`);
        } else if (motionMult !== 1) {
            // Lively (or any non-default) — scale up.
            parts.push(`--dur-micro: ${Math.round(130 * motionMult)}ms`);
            parts.push(`--dur-enter: ${Math.round(190 * motionMult)}ms`);
        }
        return parts.join('; ');
    });

    /* Track B: real desktop blur via DWM acrylic on the command WINDOW.
       Only meaningful in the dedicated window (the backend command targets
       the 'command' window). Applied on mount + whenever the user flips the
       toggle (which reaches this window via the cross-window storage event).
       Backend no-ops gracefully if unsupported; the catch swallows the rest. */
    $effect(() => {
        const enabled = $commandAppearance.desktopBlur;
        if (!isCommandWindow) return;
        void invoke('set_command_window_blur', { enabled }).catch(() => {});
    });

    /** Active quick-action result for the current query (calculator,
     *  unit conversion). Backend returns it in < 1 ms so we evaluate
     *  every keystroke alongside the search invokes — no separate
     *  debounce path. Null when the query is empty or doesn't match
     *  any quick-action pattern. */
    let quickAction = $state<RenderableQuickAction | null>(null);

    /** Brief "Copied!" flash on the quick-action row when the user
     *  activates a calculator / unit-conversion result. Self-clears
     *  after 1.4 s so the feedback doesn't linger. */
    let quickActionCopied = $state(false);
    let quickActionCopiedTimer: ReturnType<typeof setTimeout> | null = null;

    /* ──────────────────────────────────────────────────────────────
       Action panel (Ctrl+Space) — Raycast-style "what can I do with
       this item?" surface. Anchored bottom-right of the palette,
       shows the verbs available for the currently selected row.
       Default verb (Enter on the row) is also listed inside the
       panel so the user can see every option in one place.
       ────────────────────────────────────────────────────────────── */
    let actionsOpen = $state(false);
    let actionsSelectedIndex = $state(0);
    /** Brief "Copied!" line shown when an action's outcome is a
     *  clipboard write. Same 1.4 s self-clear as quickActionCopied. */
    let actionFeedback = $state<string | null>(null);
    let actionFeedbackTimer: ReturnType<typeof setTimeout> | null = null;

    /** Show a transient feedback line on the action panel (or as a
     *  toast if the panel is closed). Used so the user knows a
     *  silent action (like "Copy path") actually fired. */
    function flashActionFeedback(message: string) {
        actionFeedback = message;
        if (actionFeedbackTimer) clearTimeout(actionFeedbackTimer);
        actionFeedbackTimer = setTimeout(() => {
            actionFeedback = null;
            actionFeedbackTimer = null;
        }, 1400);
    }

    async function copyTextSilent(text: string, feedback: string) {
        try {
            await navigator.clipboard.writeText(text);
            flashActionFeedback(feedback);
        } catch (error) {
            console.warn('copy failed:', error);
            errorToast("Couldn't copy to clipboard", error, {
                hint: 'Another app may be holding the clipboard. Try copying again.',
            });
        }
    }

    /** Extract the file/folder name from a path. Handles both Windows
     *  (backslash) and POSIX (forward slash) separators since paths
     *  on Windows can carry either depending on the source. */
    function basename(path: string): string {
        const cleaned = path.replace(/[\\\/]+$/, '');
        const idx = Math.max(cleaned.lastIndexOf('\\'), cleaned.lastIndexOf('/'));
        return idx >= 0 ? cleaned.slice(idx + 1) : cleaned;
    }

    const STUDIO_IMAGE_EXTENSIONS = new Set([
        'jpg',
        'jpeg',
        'png',
        'webp',
        'bmp',
        'tiff',
        'tif',
        'gif',
        'svg',
        'ico',
    ]);
    const OCR_IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'webp', 'bmp', 'tif', 'tiff', 'gif']);
    const REDACT_IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'webp']);

    function extensionForPath(path: string): string {
        const name = basename(path);
        const dot = name.lastIndexOf('.');
        return dot > 0 ? name.slice(dot + 1).toLowerCase() : '';
    }

    async function copySha256(path: string) {
        try {
            const result = await invoke<{ sha256: string }>('compute_hashes', { path });
            await copyTextSilent(result.sha256, 'SHA-256 copied');
        } catch (error) {
            errorToast("Couldn't calculate SHA-256", error, {
                hint: 'The file may have moved, been deleted, or be unavailable.',
            });
        }
    }

    function toolActionsForFile(path: string): ItemAction[] {
        const extension = extensionForPath(path);
        const actions: ItemAction[] = [
            {
                id: 'copy-sha256',
                label: 'Copy SHA-256',
                icon: HashIcon,
                hint: 'Calculate a local file fingerprint',
                activate: () => copySha256(path),
            },
        ];

        if (toolForId('encrypt-decrypt')) {
            actions.push({
                id: 'encrypt-file',
                label: 'Encrypt file',
                icon: Lock,
                hint: 'Open with this file ready',
                activate: () => openMainAtTool('encrypt-decrypt', { targetFile: path }),
            });
        }
        if (toolForId('image-studio') && STUDIO_IMAGE_EXTENSIONS.has(extension)) {
            actions.push({
                id: 'tool-image-studio',
                label: 'Open in Image Studio',
                icon: ImageIcon,
                hint: 'Resize, crop, compress, or remove the background',
                activate: () => openMainAtTool('image-studio', { targetFile: path }),
            });
        }
        if (toolForId('ocr-image-to-text') && OCR_IMAGE_EXTENSIONS.has(extension)) {
            actions.push({
                id: 'tool-ocr',
                label: 'Extract text (OCR)',
                icon: TypeIcon,
                hint: 'Read text on this device',
                activate: () => openMainAtTool('ocr-image-to-text', { targetFile: path }),
            });
        }
        if (toolForId('screenshot-redact') && REDACT_IMAGE_EXTENSIONS.has(extension)) {
            actions.push({
                id: 'tool-screenshot-redact',
                label: 'Redact image',
                icon: EyeOff,
                hint: 'Black out sensitive regions',
                activate: () => openMainAtTool('screenshot-redact', { targetFile: path }),
            });
        }
        if (toolForId('word-converter') && extension === 'docx') {
            actions.push({
                id: 'tool-word-markdown',
                label: 'Convert to Markdown',
                icon: FileText,
                hint: 'Open with this document ready',
                activate: () => openMainAtTool('word-converter', { targetFile: path }),
            });
        }
        return actions;
    }

    /** Browse scopes reuse the same resources as query results with a
     *  scope-specific key prefix. Normalize only those aliases before action
     *  resolution so Preview, Enter, Ctrl+Space, and right-click keep the
     *  same verbs without duplicating every resource branch. */
    function canonicalResultKey(key: string): string {
        if (key.startsWith('tool-all:')) return `keepitlocal:${key.slice('tool-all:'.length)}`;
        if (key.startsWith('app-all:')) return `launch:${key.slice('app-all:'.length)}`;
        if (key.startsWith('recent-app-all:')) return `recent-app:${key.slice('recent-app-all:'.length)}`;
        if (key.startsWith('recent-file-all:')) return `recent-file:${key.slice('recent-file-all:'.length)}`;
        if (key.startsWith('note-all:')) return `file:${key.slice('note-all:'.length)}`;
        if (key.startsWith('note-hit:')) return `file:${key.slice('note-hit:'.length)}`;
        return key;
    }

    type ItemAction = {
        id: string;
        label: string;
        icon: typeof Copy;
        /** Short description shown under the label inside the panel. */
        hint?: string;
        activate: () => void | Promise<void>;
    };

    /** Build the action list for whatever item is selected right now.
     *  Returns an empty list if no selection (panel stays closed).
     *  The FIRST action in the list is always the "default" verb
     *  that Enter triggers on the row itself — having it in the
     *  panel too keeps the list comprehensive without forcing the
     *  user to remember "Enter does the default thing." */
    function actionsForSelected(): ItemAction[] {
        const sel = selectables[selectedIndex];
        if (!sel) return [];
        const originalKey = sel.key;
        const key = canonicalResultKey(originalKey);

        // ── Open windows (Windows chip) ────────────────────────────
        // Added 2026-07-29 with the window switcher: without a branch here
        // Ctrl+Space did nothing on a window row, which reads as broken next
        // to every other row type. `toggle_window` was registered by the
        // switcher work but had no caller until now.
        if (key.startsWith('window:')) {
            const hwnd = Number(key.slice('window:'.length));
            const win = openWindows.find((w) => w.hwnd === hwnd);
            if (!win) return [];
            return [
                {
                    id: 'focus-window',
                    label: 'Focus window',
                    icon: ExternalLink,
                    hint: win.title || win.app,
                    activate: () => void focusWindowRow(win),
                },
                {
                    id: 'toggle-window',
                    label: win.isForeground ? 'Minimize window' : 'Focus / minimize (toggle)',
                    icon: AppWindow,
                    hint: 'Same behaviour as a per-app hotkey',
                    activate: async () => {
                        try {
                            await invoke('toggle_window', { hwnd: win.hwnd });
                        } catch (error) {
                            errorToast("Couldn't toggle that window", error, {
                                hint: 'The window may have closed already.',
                            });
                        }
                        await hidePalette();
                    },
                },
                {
                    id: 'copy-window-title',
                    label: 'Copy window title',
                    icon: Copy,
                    hint: win.title || win.app,
                    activate: () => void copyTextSilent(win.title || win.app, 'Title copied'),
                },
                {
                    id: 'kill-window-app',
                    label: 'Force close',
                    icon: X,
                    hint: 'Unsaved work is lost',
                    // Same confirm modal the Commands chip uses — it only
                    // reads name/windowCount/pids, all of which a window row
                    // has. Refreshes the window list instead of the process
                    // list, since that's what's on screen here.
                    activate: () =>
                        openKillConfirm(
                            {
                                name: win.app,
                                exePath: win.exePath,
                                pids: [win.pid],
                                memoryBytes: 0,
                                windowCount: 1,
                            },
                            loadOpenWindows,
                        ),
                },
            ];
        }

        // ── Emoji ──────────────────────────────────────────────────
        if (key.startsWith('emoji:')) {
            const glyph = key.slice('emoji:'.length);
            const entry = emojiMatches.find((e) => e.c === glyph);
            if (!entry) return [];
            return [
                {
                    id: 'copy-emoji',
                    label: 'Copy emoji',
                    icon: Copy,
                    hint: entry.c,
                    activate: () => void copyEmojiRow(entry),
                },
                {
                    id: 'copy-emoji-name',
                    label: 'Copy name',
                    icon: Copy,
                    hint: entry.n,
                    activate: () => void copyTextSilent(entry.n, 'Name copied'),
                },
            ];
        }

        // ── Settings sections ──────────────────────────────────────
        if (key.startsWith('settings:')) {
            const id = key.slice('settings:'.length) as SettingsSectionId;
            const section = SETTINGS_SECTIONS.find((s) => s.id === id);
            return [
                {
                    id: 'open-settings',
                    label: section ? `Open Settings → ${section.label}` : 'Open Settings',
                    icon: Settings,
                    hint: 'Jump to this settings section',
                    activate: () => void openMainAtSettings(id),
                },
            ];
        }

        // ── My Commands (user-defined quicklinks / bangs) ──────────
        if (key.startsWith('mycmd:')) {
            const id = key.slice('mycmd:'.length);
            const cmd = $myCommands.find((c) => c.id === id);
            if (!cmd) return [];
            const match = myCommandMatches.find((m) => m.cmd.id === id);
            return [
                {
                    id: 'run',
                    label: 'Run',
                    icon: ExternalLink,
                    hint: cmd.target,
                    activate: () => void runMyCommand(cmd, match?.arg ?? ''),
                },
                {
                    id: 'edit-command',
                    label: 'Edit command',
                    icon: Settings,
                    hint: 'Open the My Commands manager',
                    activate: () => void openMainAtTool('my-commands'),
                },
            ];
        }

        // ── Apps (launch_target + recent app) ──────────────────────
        if (key.startsWith('launch:') || key.startsWith('recent-app:')) {
            const path = key.startsWith('launch:') ? key.slice(7) : key.slice(11);
            const name = basename(path);
            return [
                {
                    id: 'launch',
                    label: 'Launch',
                    icon: ExternalLink,
                    hint: 'Open the app',
                    activate: () => launchApp(path),
                },
                {
                    id: 'reveal',
                    label: 'Reveal in Explorer',
                    icon: FolderOpen,
                    hint: 'Show the .exe / .lnk in its folder',
                    activate: () => void revealFileSearchResult(path),
                },
                {
                    id: 'copy-path',
                    label: 'Copy path',
                    icon: Copy,
                    hint: path,
                    activate: () => void copyTextSilent(path, 'Path copied'),
                },
                {
                    id: 'copy-name',
                    label: 'Copy name',
                    icon: Copy,
                    hint: name,
                    activate: () => void copyTextSilent(name, 'Name copied'),
                },
                {
                    id: 'add-command',
                    label: 'Add to My Commands',
                    icon: Zap,
                    hint: 'Save as a keyword you can run from here',
                    activate: () => quickAddCommand('app', path, name),
                },
            ];
        }

        // ── Browser bookmarks + history ───────────────────────────
        if (key.startsWith('browser:')) {
            const url = key.slice('browser:'.length);
            const hit = browserHits.find((item) => item.url === url);
            if (!hit) return [];
            return [
                {
                    id: 'open-browser-hit',
                    label: 'Open in browser',
                    icon: Globe,
                    hint: hit.displayUrl || hit.url,
                    activate: () => void openBrowserHit(hit),
                },
                {
                    id: 'copy-url',
                    label: 'Copy URL',
                    icon: Copy,
                    hint: hit.url,
                    activate: () => void copyTextSilent(hit.url, 'URL copied'),
                },
            ];
        }

        // ── Files & content matches + live-grep hits ───────────────
        // live-grep: keys carry a `:lineNumber` suffix we strip when
        // deriving the file path. Action set is the same as for
        // content matches (open / reveal / copy path / copy name /
        // add to commands) since the underlying resource IS the file.
        if (
            key.startsWith('file:') ||
            key.startsWith('content:') ||
            key.startsWith('recent-file:') ||
            key.startsWith('live-grep:')
        ) {
            let path: string;
            if (key.startsWith('file:')) {
                path = key.slice(5);
            } else if (key.startsWith('content:')) {
                path = key.slice(8);
            } else if (key.startsWith('recent-file:')) {
                path = key.slice(12);
            } else {
                // live-grep:<path>:<lineNumber> — strip the trailing
                // :<digits>. The path itself may contain colons
                // (Windows drive letter), so we anchor on the last
                // colon followed by digits.
                const rest = key.slice('live-grep:'.length);
                const m = rest.match(/^(.*):(\d+)$/);
                path = m ? m[1] : rest;
            }
            const name = basename(path);
            const matchedFile = fileResults.find((result) => result.path === path);
            const toolActions = matchedFile?.entryType === 'folder' ? [] : toolActionsForFile(path);
            return [
                {
                    id: 'open',
                    label:
                        originalKey.startsWith('note-all:') || originalKey.startsWith('note-hit:')
                            ? 'Open in Notes'
                            : 'Open',
                    icon: ExternalLink,
                    hint:
                        originalKey.startsWith('note-all:') || originalKey.startsWith('note-hit:')
                            ? 'Open in the KeepItLocal editor'
                            : 'Open with the default app',
                    activate: () => openFile(path),
                },
                {
                    id: 'fullscreen',
                    label: 'Expand to fullscreen',
                    icon: Maximize2,
                    hint: 'Big preview · Esc to exit',
                    activate: () => {
                        previewOpen = true;
                        previewAutoOpened = false;
                        openPreviewFullscreen();
                    },
                },
                ...toolActions,
                {
                    id: 'reveal',
                    label: 'Reveal in Explorer',
                    icon: FolderOpen,
                    hint: 'Highlight the file in its folder',
                    activate: () => void revealFileSearchResult(path),
                },
                {
                    id: 'copy-path',
                    label: 'Copy path',
                    icon: Copy,
                    hint: path,
                    activate: () => void copyTextSilent(path, 'Path copied'),
                },
                {
                    id: 'copy-name',
                    label: 'Copy name',
                    icon: Copy,
                    hint: name,
                    activate: () => void copyTextSilent(name, 'Name copied'),
                },
                {
                    id: 'add-command',
                    label: 'Add to My Commands',
                    icon: Zap,
                    hint: 'Save as a keyword you can run from here',
                    activate: () => quickAddCommand('file', path, name),
                },
            ];
        }

        // ── Folder (recent) ────────────────────────────────────────
        if (key.startsWith('recent-folder:')) {
            const path = key.slice(14);
            const name = basename(path);
            return [
                {
                    id: 'open',
                    label: 'Open in Explorer',
                    icon: FolderOpen,
                    hint: 'Open the folder',
                    activate: () => openFile(path),
                },
                {
                    id: 'copy-path',
                    label: 'Copy path',
                    icon: Copy,
                    hint: path,
                    activate: () => void copyTextSilent(path, 'Path copied'),
                },
                {
                    id: 'copy-name',
                    label: 'Copy name',
                    icon: Copy,
                    hint: name,
                    activate: () => void copyTextSilent(name, 'Name copied'),
                },
                {
                    id: 'add-command',
                    label: 'Add to My Commands',
                    icon: Zap,
                    hint: 'Save this folder as a keyword you can run from here',
                    activate: () => quickAddCommand('folder', path, name),
                },
            ];
        }

        // ── KeepItLocal tools (matched / suggested / recent) ───────
        if (
            key.startsWith('keepitlocal:') ||
            key.startsWith('suggested:') ||
            key.startsWith('recent-tool:')
        ) {
            const toolId = key.startsWith('keepitlocal:')
                ? key.slice(12)
                : key.startsWith('suggested:')
                  ? key.slice(10)
                  : key.slice(12);
            const tool = toolForId(toolId);
            return [
                {
                    id: 'open',
                    label: 'Open in Workspace',
                    icon: ExternalLink,
                    hint: tool?.name ?? toolId,
                    activate: () => openMainAtTool(toolId),
                },
                {
                    id: 'copy-name',
                    label: 'Copy tool name',
                    icon: Copy,
                    hint: tool?.name ?? toolId,
                    activate: () => void copyTextSilent(tool?.name ?? toolId, 'Name copied'),
                },
            ];
        }

        // ── Core pillars (Clipboard / Snippets) ────────────────────
        if (key.startsWith('core:')) {
            const pillarId = key.slice(5);
            const pillar = corePillars.find((p) => p.id === pillarId);
            if (!pillar) return [];
            return [
                {
                    id: 'open',
                    label: pillar.name,
                    icon: ExternalLink,
                    hint: pillar.description,
                    activate: () => void pillar.activate(),
                },
            ];
        }

        // ── Snippets ──────────────────────────────────────────────
        if (key.startsWith('snip:')) {
            const id = Number(key.slice('snip:'.length));
            const snippet = matchingSnippets.find((item) => item.id === id);
            if (!snippet) return [];
            return [
                {
                    id: 'paste-snippet',
                    label: 'Paste snippet',
                    icon: ExternalLink,
                    hint: `/${snippet.trigger}`,
                    activate: () => void pasteSnippet(snippet),
                },
                {
                    id: 'manage-snippets',
                    label: 'Manage snippets',
                    icon: Settings,
                    hint: 'Open the Snippets workspace',
                    activate: () => void openMainAtTool('snippets'),
                },
            ];
        }

        // ── Clipboard entries (the rich variant — multiple verbs) ──
        if (key.startsWith('clip:')) {
            const id = parseInt(key.slice(5), 10);
            const entry = clipboardEntries.find((e) => e.id === id);
            if (!entry) return [];
            const isText = entry.kind === 'text';
            const acts: ItemAction[] = [
                {
                    id: 'paste',
                    label: 'Paste',
                    icon: ExternalLink,
                    hint: 'Send into the previous app',
                    activate: () => pasteClipboardEntry(entry),
                },
                {
                    id: 'pin',
                    label: entry.isPinned ? 'Unpin' : 'Pin',
                    icon: Pin,
                    hint: entry.isPinned ? 'Remove from pinned' : 'Keep it at the top of history',
                    activate: () => void toggleClipboardPin(entry),
                },
                {
                    id: 'label',
                    label: 'Label…',
                    icon: Tag,
                    hint: 'Name + pin this entry',
                    activate: () => openClipboardLabelEditor(entry),
                },
            ];
            if (isText) {
                acts.push({
                    id: 'copy',
                    label: 'Copy to clipboard',
                    icon: Copy,
                    hint: 'Put it back on the clipboard without pasting',
                    activate: () => void copyTextSilent(entry.text, 'Copied'),
                });
                // Open as URL if the entry is a link.
                if (entry.category === 'url') {
                    acts.push({
                        id: 'open-url',
                        label: 'Open URL',
                        icon: Globe,
                        hint: entry.text,
                        activate: async () => {
                            try {
                                await invoke('open_external_url', { url: entry.text });
                                await hidePalette();
                            } catch (error) {
                                errorToast("Couldn't open that item", error, {
                    hint: 'The file may have been moved or deleted since the result was indexed.',
                });
                            }
                        },
                    });
                }
                // For file_path entries, "Reveal in Explorer" makes sense.
                if (entry.category === 'file_path') {
                    acts.push({
                        id: 'reveal',
                        label: 'Reveal in Explorer',
                        icon: FolderOpen,
                        hint: entry.text,
                        activate: () => void revealFileSearchResult(entry.text),
                    });
                }
            } else {
                // Image entries: copy the actual image back to the clipboard
                // via the backend (handles non-text payloads).
                acts.push({
                    id: 'copy',
                    label: 'Copy image',
                    icon: Copy,
                    hint: 'Put the image back on the clipboard without pasting',
                    activate: () => void copyClipboardEntry(entry),
                });
                if (entry.imagePath) {
                    acts.push(
                        ...toolActionsForFile(entry.imagePath).filter(
                            (action) =>
                                action.id === 'tool-image-studio' ||
                                action.id === 'tool-ocr' ||
                                action.id === 'tool-screenshot-redact',
                        ),
                    );
                }
            }
            // Destructive action last.
            acts.push({
                id: 'delete',
                label: 'Delete',
                icon: Trash2,
                hint: 'Remove from clipboard history',
                activate: () => void deleteClipboardEntry(entry),
            });
            return acts;
        }

        // ── Quick action ───────────────────────────────────────────
        // The variant determines the actions: calc/conv get copy ops,
        // openUrl/webSearch get open + copy-link.
        if (key === 'quick-action' && quickAction) {
            // Capture into a non-null local so the action closures keep
            // TypeScript-narrowed access — without this, the closures
            // see `quickAction` as `RenderableQuickAction | null` at
            // invocation time (the state could be reset by then).
            const qa = quickAction;
            if (qa.type === 'calculator' || qa.type === 'unitConversion') {
                const expr = qa.type === 'calculator' ? qa.expression : qa.original;
                return [
                    {
                        id: 'copy-result',
                        label: 'Copy result',
                        icon: Copy,
                        hint: qa.result,
                        activate: () => void activateQuickAction(qa),
                    },
                    {
                        id: 'copy-expression',
                        label: 'Copy expression',
                        icon: Copy,
                        hint: expr,
                        activate: () => void copyTextSilent(expr, 'Expression copied'),
                    },
                    {
                        id: 'copy-both',
                        label: 'Copy as "expr = result"',
                        icon: Copy,
                        hint: `${expr} = ${qa.result}`,
                        activate: () =>
                            void copyTextSilent(
                                `${expr} = ${qa.result}`,
                                'Expression + result copied',
                            ),
                    },
                ];
            }
            // Bang variants — open in browser, plus a copy-link option
            // so the user can stash it elsewhere instead of navigating.
            if (qa.type === 'openUrl') {
                return [
                    {
                        id: 'open-url',
                        label: 'Open in browser',
                        icon: Globe,
                        hint: qa.url,
                        activate: () => void activateQuickAction(qa),
                    },
                    {
                        id: 'copy-url',
                        label: 'Copy URL',
                        icon: Copy,
                        hint: qa.url,
                        activate: () => void copyTextSilent(qa.url, 'URL copied'),
                    },
                ];
            }
            if (qa.type === 'webSearch') {
                return [
                    {
                        id: 'open-url',
                        label: `Search ${qa.provider}`,
                        icon: Globe,
                        hint: qa.url,
                        activate: () => void activateQuickAction(qa),
                    },
                    {
                        id: 'copy-url',
                        label: 'Copy search URL',
                        icon: Copy,
                        hint: qa.url,
                        activate: () => void copyTextSilent(qa.url, 'URL copied'),
                    },
                    {
                        id: 'copy-query',
                        label: 'Copy query',
                        icon: Copy,
                        hint: qa.query,
                        activate: () => void copyTextSilent(qa.query, 'Query copied'),
                    },
                ];
            }
            if (qa.type === 'systemCommand') {
                // System commands have a single primary action — Run.
                // The confirm dialog (if any) is wired inside
                // activateQuickAction so the action panel doesn't need
                // a separate confirm flow.
                return [
                    {
                        id: 'run',
                        label: qa.requiresConfirmation ? `${qa.name} (confirms first)` : qa.name,
                        icon: ExternalLink,
                        hint: qa.description,
                        activate: () => void activateQuickAction(qa),
                    },
                ];
            }
        }

        // ── One-shot creation + focus controls ────────────────────
        if (key === 'reminder-create' && reminderMatch) {
            return [
                {
                    id: 'create-reminder',
                    label: 'Set reminder',
                    icon: Bell,
                    hint: formatReminderDue(reminderMatch.dueMs),
                    activate: () => void createReminderFromPalette(reminderMatch),
                },
            ];
        }
        if (key === 'note-create' && noteMatch) {
            return [
                {
                    id: 'create-note',
                    label: 'Save note',
                    icon: NotebookPen,
                    hint: noteMatch,
                    activate: () => void createNoteFromPalette(noteMatch),
                },
            ];
        }
        if (key === 'note-new' || key === 'quick-note-open') {
            return [
                {
                    id: 'open-quick-note',
                    label: 'Create a note',
                    icon: NotebookPen,
                    hint: 'Open a floating sticky note',
                    activate: () => void openQuickNote(),
                },
            ];
        }
        if (key.startsWith('tf:')) {
            const id = key.slice('tf:'.length);
            const action = timeFocusMatches.find((item) => item.id === id);
            if (!action) return [];
            return [
                {
                    id: action.id,
                    label: action.label,
                    icon: action.icon,
                    hint: 'Run this Time & Focus action',
                    activate: () => void runTimeFocusAction(action),
                },
            ];
        }
        if (key.startsWith('kill:')) {
            const target = key.slice('kill:'.length);
            const group = killRows.find((item) => (item.exePath ?? item.name) === target);
            if (!group) return [];
            return [
                {
                    id: 'force-close',
                    label: 'Force close',
                    icon: X,
                    hint: 'Unsaved work is lost',
                    activate: () => openKillConfirm(group),
                },
            ];
        }

        return [];
    }

    /** Derived action list — recomputes when the selection changes
     *  so the panel stays in sync as the user arrows up/down. In default
     *  mode with an active query we also append a query-level "Open
     *  results in main window" action (moved here from the top bar) so
     *  it's reachable no matter which row is selected. */
    let availableActions = $derived.by<ItemAction[]>(() => {
        const actions = actionsForSelected();
        if (mode === 'default' && query.trim()) {
            actions.push({
                id: 'open-in-main',
                label: 'Open results in main window',
                icon: ExternalLink,
                hint: 'See all matches in the workspace',
                activate: () => void openInMain(),
            });
        }
        return actions;
    });

    let primarySelectedAction = $derived.by<ItemAction | null>(
        () => availableActions[0] ?? null,
    );

    function toggleActionsPanel() {
        if (actionsOpen) {
            closeActionsPanel();
        } else {
            openActionsPanel();
        }
    }

    function openActionsPanel() {
        if (availableActions.length === 0) return;
        moreScopesOpen = false;
        cheatsheetOpen = false;
        appearanceOpen = false;
        shortcutsOpen = false;
        actionsSelectedIndex = 0;
        actionsOpen = true;
    }

    function closeActionsPanel() {
        actionsOpen = false;
        actionsSelectedIndex = 0;
        // Don't reset actionFeedback here — let it self-expire so the
        // "Copied!" line stays briefly visible after the panel closes.
    }

    /** Keep native Tab focus and the palette's selected row in sync. Rows are
     *  rendered in many scope-specific branches, so one delegated handler is
     *  safer than repeating this on every result button. */
    function onBodyFocusIn(event: FocusEvent) {
        const target = event.target as HTMLElement | null;
        const rowEl = target?.closest<HTMLElement>('[data-cmd-key]');
        const key = rowEl?.dataset.cmdKey;
        if (!key) return;
        const index = selectables.findIndex((item) => item.key === key);
        if (index >= 0) selectedIndex = index;
    }

    /** Right-click any result row → open its action panel. Mouse parity
     *  for the Ctrl+Space keyboard path. Delegated on the results
     *  container (.cmd-body) so it works for EVERY row type without
     *  per-row handlers or nested buttons — rows already carry a
     *  `data-cmd-key` that maps back to a selectable. Selecting the row
     *  first means `availableActions` reflects it when the panel opens. */
    function onRowContextMenu(event: MouseEvent) {
        const target = event.target as HTMLElement | null;
        const rowEl = target?.closest('[data-cmd-key]') as HTMLElement | null;
        if (!rowEl) return;
        const key = rowEl.getAttribute('data-cmd-key');
        if (!key) return;
        const idx = selectables.findIndex((s) => s.key === key);
        if (idx < 0) return;
        event.preventDefault();
        selectedIndex = idx;
        openActionsPanel();
    }

    // Reset the panel-internal selection whenever the underlying row
    // selection changes — keeps the "first action highlighted" invariant
    // so Enter always activates the most useful default. Without this,
    // arrowing through rows while the panel is open would leave the
    // panel's index pointing at a stale, possibly out-of-range slot.
    $effect(() => {
        // Reference for reactivity — we only care that selectedIndex
        // changed; the body re-runs the reset.
        void selectedIndex;
        actionsSelectedIndex = 0;
    });

    // Close the panel automatically if the selection becomes empty
    // (e.g. user clears the query mid-keystroke).
    $effect(() => {
        if (availableActions.length === 0 && actionsOpen) {
            actionsOpen = false;
        }
    });

    /* ──────────────────────────────────────────────────────────────
       Derived: KeepItLocal tool catalog (for matching against the
       user's query). Same shape as /overlay so the matching logic
       behaves identically.
       ────────────────────────────────────────────────────────────── */
    /** Natural-language aliases per tool — extra words appended to each
     *  tool's match tokens so conversational queries find the right tool
     *  even when they don't use the tool's literal name ("shrink" →
     *  Image Compressor, "checksum" → Hash Check, "unzip" → Extract
     *  Archive). Purely additive: existing name/description/category
     *  matching is unchanged, this only ADDS more substrings that can
     *  match. Tool ids match appScreens.ts. */
    const TOOL_ALIASES: Record<string, string> = {
        'hash-check': 'checksum md5 sha sha256 blake3 fingerprint verify integrity',
        encoders: 'base64 hex url html binary rot13 encode decode escape',
        'qr-code': 'qr barcode',
        'format-converter': 'json yaml toml xml beautify prettify minify tree',
        'password-generator': 'password passphrase pwd generate secure strong',
        'util-calculator': 'calc calculator math arithmetic units convert measurement metric imperial percent percentage soulver notepad',
        'file-search': 'find files search index lookup locate',
        'cleaner-analyzer': 'clean cleanup cache disk space junk bloat ram analyze',
        'duplicate-finder': 'duplicate dupe copies identical',
        'bulk-rename': 'rename batch mass',
        'automation-recipes': 'automation recipe workflow macro',
        'file-shredder': 'shred wipe secure erase destroy delete',
        'screenshot-redact': 'redact blur censor screenshot hide black',
        'word-converter': 'docx word convert markdown md text',
        'csv-toolkit': 'csv merge clean split json jsonl tsv excel xlsx xls ods spreadsheet sheet convert',
        'doc-password': 'remove password unlock word docx restrict editing protection',
        'dev-toolkit': 'developer dev tools toolkit jwt token decode bearer regex regexp pattern match test explain breakdown replace substitution named groups sql format query beautify lint diff compare difference markdown preview render html uuid ulid nanoid guid identifier id generator fake mock test data generate sample fixtures cron schedule crontab scheduler recurring periodic secret leak credential pii scan exposed',
        'image-studio': 'image studio resize convert compress format png jpg webp svg scale dimensions shrink optimize smaller crop',
        'img-base64': 'image base64 data uri',
        'img-favicon': 'favicon icon',
        'img-watermark': 'watermark stamp brand',
    };

    let keepitlocalTargets = $derived(
        $installedTools
            // `hidden` tools are withheld from the palette too. Without this the
            // hide would be cosmetic: the sidebar entry disappears but typing the
            // tool's name still launches it, which is worse than not hiding at all
            // (the user meets a surface we deliberately judged not ready).
            .filter((tool) => tool.available && !tool.hidden)
            .map((tool) => ({
                id: tool.id,
                name: tool.name,
                description: tool.description,
                tokens: `${tool.name} ${tool.description} ${tool.category} ${TOOL_ALIASES[tool.id] ?? ''}`.toLowerCase(),
            })),
    );

    /* ──────────────────────────────────────────────────────────────
       Tool lookup helper — resolves an `id` back to the full
       ToolScreen object (so we can render the tool's actual icon
       instead of the generic Wrench). Used by the KeepItLocal tool-
       matches section, Recent Tools section, and the voice Transcribe
       results — each of those carries only a string id and would
       otherwise fall back to a generic icon.

       Returns `undefined` when the id is unknown (e.g. a stale recent
       entry for a tool whose pack was disabled). Callers must guard.
       ────────────────────────────────────────────────────────────── */
    function toolForId(id: string) {
        return $installedTools.find((tool) => tool.id === id);
    }

    /* ──────────────────────────────────────────────────────────────
       Content-search snippet highlighting — mirrors the FileSearch
       page's identical helper (Phase 3.3.11). The backend already
       returns `<mark>`-wrapped snippets for most extractors, but some
       paths give plain text + a `matchedKeywords` list — we wrap each
       keyword in `<mark>` here.

       Always HTML-escape user-derived text first, then re-introduce
       the safe `<mark>` tags. Returning a string used with `{@html}`
       in the row template; without escape this would be an XSS hole.
       ────────────────────────────────────────────────────────────── */
    const HTML_ESCAPE_RE = /[&<>"']/g;
    const HTML_ESCAPE_MAP: Record<string, string> = {
        '&': '&amp;',
        '<': '&lt;',
        '>': '&gt;',
        '"': '&quot;',
        "'": '&#39;',
    };
    function escapeHtml(text: string): string {
        return text.replace(HTML_ESCAPE_RE, (c) => HTML_ESCAPE_MAP[c] ?? c);
    }
    function escapeRegex(text: string): string {
        return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    }
    function highlightSnippet(snippet: string, matchedKeywords: string[]): string {
        if (!snippet) return '';
        // If the backend has already wrapped matches, pass through —
        // its highlighting is more accurate than our keyword splat
        // (it knows tokenization rules).
        if (/<mark[\s>]/i.test(snippet)) return snippet;
        const escaped = escapeHtml(snippet);
        if (!matchedKeywords?.length) return escaped;
        // Sort longest-first so "annual report" matches before "annual"
        // and we don't end up double-wrapping the inner word.
        const terms = matchedKeywords
            .filter((kw) => kw && kw.length >= 2)
            .sort((a, b) => b.length - a.length);
        if (terms.length === 0) return escaped;
        const pattern = new RegExp(
            `(${terms.map(escapeRegex).join('|')})`,
            'gi',
        );
        return escaped.replace(pattern, '<mark>$1</mark>');
    }

    /** 2026-05-27 polish — robust query-driven highlight helper.
     *
     *  Drop-in for places where we have the RAW user query rather than
     *  the backend's matchedKeywords array (titles, paths, free-text
     *  fields). Differs from `highlightSnippet`:
     *    • Takes the raw query string, tokenizes ourselves.
     *    • Strips surrounding quotes from phrase tokens (so `"q3
     *      budget"` matches the literal phrase).
     *    • Dedupes case-insensitively.
     *    • Sorts longest-first so phrases match before their member
     *      words.
     *    • Escapes regex specials safely — handles `.`, `+`, `(`, `)`,
     *      `[`, `]`, `?`, `*`, `^`, `$`, `|`, `\`.
     *    • Output is HTML-escaped; `<mark>` is the only injected tag.
     *      Safe to render via `{@html}`.
     *    • Works with Unicode (Cyrillic, Georgian, etc.) because the
     *      regex engine handles full code-points natively.
     *    • Returns the input as escaped HTML when no tokens / no
     *      matches — never returns raw user input.
     */
    function highlightText(text: string, query: string | null | undefined): string {
        if (!text) return '';
        const escaped = escapeHtml(text);
        if (!query) return escaped;
        const tokens = tokenizeQueryForHighlight(query);
        if (tokens.length === 0) return escaped;
        const pattern = new RegExp(
            `(${tokens.map(escapeRegex).join('|')})`,
            'gi',
        );
        return escaped.replace(pattern, '<mark>$1</mark>');
    }

    /** Split a search query into highlight-ready tokens. Phrases in
     *  `"double quotes"` come out as one token; bare words are split on
     *  whitespace. Tokens shorter than 2 chars are dropped (would
     *  light up the whole page on a single letter). Case-insensitive
     *  dedupe + longest-first sort so phrase matches win over their
     *  member words. */
    function tokenizeQueryForHighlight(query: string): string[] {
        const tokens: string[] = [];
        const re = /"([^"]+)"|(\S+)/g;
        let m: RegExpExecArray | null;
        while ((m = re.exec(query)) !== null) {
            const tok = (m[1] ?? m[2] ?? '').trim();
            if (tok.length >= 2) tokens.push(tok);
        }
        // Case-insensitive dedupe — keep the first occurrence so we
        // preserve any "natural" casing from the query, but match
        // case-insensitively anyway because of the `i` flag.
        const seen = new Set<string>();
        const unique = tokens.filter((t) => {
            const key = t.toLowerCase();
            if (seen.has(key)) return false;
            seen.add(key);
            return true;
        });
        return unique.sort((a, b) => b.length - a.length);
    }

    /* ──────────────────────────────────────────────────────────────
       Real Windows shell icon extraction — mirrors /overlay's
       `fetchEntryIcon` exactly so the same path produces the same
       cached icon. Each `ensure_launcher_icon` call is bounded by:
         - kind = "app":     unique per path (each .exe → its embedded icon)
         - kind = "folder":  one shared icon for all folders
         - kind = "file":    one shared icon per extension
       The backend writes the PNG to a per-cache-key file under the app's
       data dir, then we serve it through `convertFileSrc` so the webview
       can load it as a local resource.

       Icons fade in as they resolve — we don't await before rendering,
       and the Lucide fallback shows for the few ms before the PNG lands.
       ────────────────────────────────────────────────────────────── */
    async function fetchEntryIcon(path: string, kind: 'app' | 'folder' | 'file') {
        if (!path) return;
        // Already resolved or in-flight — dedup so a re-search doesn't
        // re-invoke the backend for paths we've already asked about.
        if (path in entryIcons) return;
        entryIcons = { ...entryIcons, [path]: null };
        try {
            const iconPath = await invoke<string | null>('ensure_launcher_icon', { path, kind });
            if (iconPath) {
                entryIcons = { ...entryIcons, [path]: convertFileSrc(iconPath) };
            }
        } catch {
            // Leave the entry as `null` so we don't retry — keeps the
            // Lucide fallback visible without hammering the shell.
        }
    }

    /** Fan out icon fetches whenever new search results arrive. Apps
     *  ask for 'app', files for 'file', folders for 'folder' — the
     *  backend's cache strategy then dedup-by-extension automatically
     *  so a thousand `.docx` files share one cached icon. */
    $effect(() => {
        const apps = launchResult?.results;
        if (apps?.length) {
            for (const item of apps) {
                void fetchEntryIcon(item.path, 'app');
            }
        }
        if (fileResults.length) {
            for (const item of fileResults) {
                const kind = item.entryType === 'folder' ? 'folder' : 'file';
                void fetchEntryIcon(item.path, kind);
            }
        }
        if (contentResults.length) {
            for (const item of contentResults) {
                void fetchEntryIcon(item.path, 'file');
            }
        }
        if (recentItems) {
            for (const item of recentItems.apps) {
                void fetchEntryIcon(item.path, 'app');
            }
            for (const item of recentItems.files) {
                void fetchEntryIcon(item.path, 'file');
            }
            for (const item of recentItems.folders) {
                void fetchEntryIcon(item.path, 'folder');
            }
        }
        // Wave G/I (2026-05-27): the "Apps" chip's empty-state browse
        // list loads its app paths via search_launch_targets with
        // browseAll=true. Those records bypass the search path that
        // normally drives icon resolution, so without this branch the
        // chip rendered every app with the generic Lucide AppWindow
        // fallback instead of the real Windows shell icon. Fetching
        // here is cheap — the backend dedups in-flight requests + caches
        // resolved PNGs per app path.
        if (browseAllApps.length) {
            for (const app of browseAllApps) {
                void fetchEntryIcon(app.path, 'app');
            }
        }
        // Commands scope (2026-06-13): kill rows reuse the same Windows
        // shell-icon pipeline keyed by the grouped app's exe path. Many
        // groups have no exe path (system/elevated) — those keep the
        // Lucide fallback; we only fetch for resolvable paths.
        if (processGroups.length) {
            for (const group of processGroups) {
                if (group.exePath) void fetchEntryIcon(group.exePath, 'app');
            }
        }
        // Windows chip (2026-07-29): same shell-icon pipeline, keyed by the
        // owning process's exe path. Windows whose exe we couldn't resolve
        // (protected processes) keep the Lucide fallback.
        if (openWindows.length) {
            for (const win of openWindows) {
                if (win.exePath) void fetchEntryIcon(win.exePath, 'app');
            }
        }
    });

    /* ──────────────────────────────────────────────────────────────
       Frecency-ranked Suggested Tools — top N installed tools by the
       backend's frecency signal. Used in default mode when the query
       is empty (the user's mockup shows this).
       ────────────────────────────────────────────────────────────── */
    let suggestedTools = $derived.by(() => {
        const tools = $installedTools.filter(
            (tool) => tool.available && !EXCLUDED_PILLAR_TOOL_IDS.has(tool.id),
        );
        // Sort by frecency boost (high → low), fall back to name.
        const ranked = [...tools].sort((a, b) => {
            const boostA = toolFrecencyBoosts[a.id] ?? 0;
            const boostB = toolFrecencyBoosts[b.id] ?? 0;
            if (boostB !== boostA) return boostB - boostA;
            return a.name.localeCompare(b.name);
        });
        return ranked.slice(0, 2);
    });

    /* ──────────────────────────────────────────────────────────────
       Recent items resolution — same logic as /overlay. Pulled from
       the backend's frecency log; lets the empty-query state show
       "where you were last".
       ────────────────────────────────────────────────────────────── */
    async function refreshRecentItems() {
        try {
            // Same backend command the search overlay uses — the palette
            // was previously calling a non-existent `list_recent_frecency_targets`
            // (the command isn't registered) so recents never loaded.
            const result = await invoke<RecentItemsResult>('get_recent_items', {
                limitPerKind: 6,
            });
            recentItems = result;
        } catch (error) {
            console.warn('recent items load failed:', error);
            recentItems = null;
        }
    }

    async function refreshToolFrecency() {
        try {
            // Correct registered command is `get_frecency_boosts` (with a
            // `kind`) — `get_tool_frecency_boosts` doesn't exist, which is why
            // Suggested Apps never frecency-ranked.
            const boosts = await invoke<Record<string, number>>('get_frecency_boosts', {
                kind: 'tool',
            });
            toolFrecencyBoosts = boosts ?? {};
        } catch (error) {
            console.warn('frecency boosts fetch failed:', error);
        }
    }

    /* ──────────────────────────────────────────────────────────────
       KEEPITLOCAL CORE pillar list — the always-installed surfaces
       that get a privileged top placement in default mode. Each row
       includes a hotkey chip pointing at the underlying mode (a
       teaching cue: "you can press this hotkey to jump here").
       ────────────────────────────────────────────────────────────── */

    interface CorePillar {
        id: 'clipboard' | 'voice';
        name: string;
        description: string;
        icon: any;
        /** Display string for the keyboard shortcut chip. Hotkey text is
         *  no longer rendered on the pillar rows (per design), but the
         *  field is kept so the data is available if a chip returns. */
        hotkey?: string;
        /** Action when activated — clipboard / voice switch mode in place. */
        activate: () => void | Promise<void>;
    }

    let corePillars = $derived<CorePillar[]>([
        {
            id: 'clipboard',
            name: 'Clipboard',
            description: 'Search and paste your local history',
            icon: Clipboard,
            hotkey: formatShortcut(
                $settings.clipboardOverlayShortcut || 'CommandOrControl+Shift+V',
            ),
            activate: () => switchMode('clipboard'),
        },
        {
            id: 'voice',
            name: 'Voice',
            description: 'Dictate or control with your voice',
            icon: Mic,
            hotkey: formatShortcut(
                $settings.voiceOverlayShortcut || 'CommandOrControl+Alt+V',
            ),
            activate: () => switchMode('voice'),
        },
    ]);

    /* ──────────────────────────────────────────────────────────────
       Utility: format the user's hotkey shortcut into display chips.
       Mirrors the same formatting the existing overlay uses so the
       same strings produce the same on-screen chips.
       ────────────────────────────────────────────────────────────── */
    function formatShortcut(shortcut: string): string {
        if (!shortcut) return '';
        return shortcut
            .replace(/CommandOrControl/gi, 'Ctrl')
            .replace(/\bCmd\b/gi, 'Ctrl')
            .replace(/\bSuper\b/gi, 'Win')
            .replace(/\bMeta\b/gi, 'Win');
    }

    /* ──────────────────────────────────────────────────────────────
       Navigation — jump to a workspace page (main window) for "big"
       tools. The Tauri main window listens to `navigate-tool` and
       routes accordingly.
       ────────────────────────────────────────────────────────────── */
    async function openMainAtTool(
        toolId: string,
        extras?: { notePath?: string; targetFile?: string },
    ) {
        // Frecency update is fire-and-forget — the user is already
        // landing on the new tool, no need to block on it. Backend
        // `launch_cached_target` (apps) and `open_file_search_result`
        // (files) already record frecency themselves; tool launches
        // skip the backend entirely (pure frontend emit) so we have
        // to call this manually or the frecency boosts never update.
        void invoke('record_frecency_launch', { kind: 'tool', path: toolId })
            .then(() => refreshToolFrecency())
            .catch(() => {});

        // Wave K (2026-05-28): payload may carry a `notePath` so the
        // Notes tool opens that specific `.ki` file on mount (Enter on
        // a note row in the palette). Empty/undefined is fine — it
        // means "open the Notes tool, let it pick its default note".
        const payload: { toolId: string; notePath?: string; targetFile?: string } = { toolId };
        if (extras?.notePath) payload.notePath = extras.notePath;
        if (extras?.targetFile) payload.targetFile = extras.targetFile;

        if (asOverlay) {
            // Overlay-in-main mode: the main route is mounted right
            // behind us, so its `navigate-tool` listener is live —
            // emit to it, then close the overlay.
            try {
                await emitTo('main', 'navigate-tool', payload);
            } catch (error) {
                console.warn('navigate to tool failed:', error);
            }
            onClose?.();
            return;
        }

        if (isEmbeddedInMain()) {
            // Standalone-route test mode: the cross-window navigate-tool
            // event has no listener (the main route isn't mounted while
            // we occupy the webview), so stash the target + navigate home
            // where the main route reads `pendingNavigation` on mount.
            pendingNavigation.set(payload);
            try {
                await goto('/');
            } catch (error) {
                console.warn('navigate to tool failed:', error);
            }
            return;
        }

        try {
            await emitTo('main', 'navigate-tool', payload);
            // Bring the main window to the front. The palette is usually
            // summoned by global hotkey while the user is in another app, so
            // the main window is hidden/behind — without this the tool
            // navigates invisibly and looks like nothing happened.
            await invoke('show_main_window_command');
            // After dispatching, dismiss the overlay window so the user
            // lands on the tool in the main window.
            await hidePalette();
        } catch (error) {
            console.warn('navigate to tool failed:', error);
        }
    }

    /** Jump to a Settings section in the main window. Mirrors
     *  openMainAtTool's three navigation modes; the section is carried
     *  via the navigate-tool event's `settingsSection` field (or
     *  pendingNavigation in embedded test mode), and the main route
     *  applies it through `requestSettingsSection`. */
    async function openMainAtSettings(section: SettingsSectionId) {
        if (asOverlay) {
            try {
                await emitTo('main', 'navigate-tool', {
                    toolId: 'settings',
                    settingsSection: section,
                });
            } catch (error) {
                console.warn('navigate to settings failed:', error);
            }
            onClose?.();
            return;
        }
        if (isEmbeddedInMain()) {
            pendingNavigation.set({ toolId: 'settings', settingsSection: section });
            try {
                await goto('/');
            } catch (error) {
                console.warn('navigate to settings failed:', error);
            }
            return;
        }
        try {
            await emitTo('main', 'navigate-tool', {
                toolId: 'settings',
                settingsSection: section,
            });
            // Bring the main window forward (see openMainAtTool) so Settings
            // is actually visible after the palette dismisses.
            await invoke('show_main_window_command');
            await hidePalette();
        } catch (error) {
            console.warn('navigate to settings failed:', error);
        }
    }

    /** True when the palette is being tested EMBEDDED inside the main
     *  window (Phase 3.6.5 binding from the Home search) rather than
     *  running in its own Tauri overlay window. Detected by the webview
     *  window label: the main window is labelled 'main'; a dedicated
     *  overlay window (Phase 3.6.6) will have its own label. Defaults to
     *  embedded if the label can't be read (plain browser dev). */
    function isEmbeddedInMain(): boolean {
        try {
            return getCurrentWebviewWindow().label === 'main';
        } catch {
            return true;
        }
    }

    async function hidePalette() {
        // Closing the palette must also kill any playing media — a hidden Tauri
        // window keeps its <audio>/<video> running otherwise.
        stopPreviewMedia();
        // If the mic was used this session, release the Vosk recognizer +
        // models on close so an idle palette doesn't keep speech models in
        // RAM (matches the voice overlay's release-on-close). Runs in every
        // dismiss path below. Idempotent on the backend.
        if (micWasUsed) {
            micWasUsed = false;
            disarmVoice('search-overlay');
            void invoke('voice_release_models').catch(() => {});
        }
        // Overlay-in-main mode (Phase 3.6.5): just tear down the overlay
        // via the parent's callback. The workspace is already mounted
        // behind us, so there's nothing to navigate to.
        if (asOverlay) {
            onClose?.();
            return;
        }
        if (isEmbeddedInMain()) {
            // Standalone-route test mode — return to the workspace so
            // Esc / post-activation dismissal takes the user back.
            try {
                await goto('/');
            } catch {
                // best effort
            }
            return;
        }
        // Production: hide the dedicated command-palette overlay window
        // via Tauri (label 'command'). This is the real Phase 3.6.6
        // window summoned by the Ctrl+Alt+K global hotkey.
        try {
            await invoke('hide_command_window_command').catch(() => {});
        } catch {
            // ignored — best effort.
        }
    }

    /** Push the current query into the workspace File Search page
     *  with mode preserved, then close the palette. For when the
     *  user wants advanced filters, pagination beyond what the
     *  palette shows, or the full workspace chrome (status bar,
     *  index controls, etc.). Mirrors /overlay's "Open in main"
     *  affordance — keeps the palette focused on quick wins while
     *  giving a clean handoff for deeper work. */
    async function openInMain() {
        if (asOverlay) {
            // Overlay-in-main mode: emit to the live main route listener
            // (it sets the file-search stores + selects the screen), then
            // close the overlay.
            try {
                await emitTo('main', 'navigate-tool', {
                    toolId: 'file-search',
                    searchQuery: query,
                    searchMode,
                });
            } catch (error) {
                console.warn('open in main failed:', error);
            }
            onClose?.();
            return;
        }
        if (isEmbeddedInMain()) {
            // Standalone-route test mode: stash the file-search target
            // (query + mode) and navigate home where the main route
            // applies it.
            pendingNavigation.set({
                toolId: 'file-search',
                searchQuery: query,
                searchMode,
            });
            try {
                await goto('/');
            } catch (error) {
                console.warn('open in main failed:', error);
            }
            return;
        }
        try {
            // Show the main window FIRST so the navigate-tool event has
            // a recipient that's actually visible. The .catch swallows
            // any "main window already visible" no-op the backend may
            // surface — best-effort is enough.
            await invoke('show_main_window_command').catch(() => {});
            await emitTo('main', 'navigate-tool', {
                toolId: 'file-search',
                searchQuery: query,
                searchMode,
            });
            await hidePalette();
        } catch (error) {
            console.warn('open in main failed:', error);
            errorToast("Couldn't open results in the main window", error, {
                hint: 'Try opening the main window from the tray icon first, then run the search again.',
            });
        }
    }

    /* ──────────────────────────────────────────────────────────────
       Search execution — debounced like the existing /overlay so
       fast typing doesn't spam the backend.
       ────────────────────────────────────────────────────────────── */

    let searchTimer: ReturnType<typeof setTimeout> | null = null;
    let activeRequestId = 0;

    /* ── Notes-scope body search ─────────────────────────────────────────
       The Notes scope only ever matched title/preview/tags, and `preview`
       is a ~200-char snippet — so a word deep in a note body (the user's
       "ivermectin") was invisible here even though the Files/Inside content
       search found it. This mirrors the Notes tool's own body search
       (Notes.svelte): a debounced `search_note_bodies` feeding a path set
       that `notesForQuery` merges in. Deliberately NOT routed through the
       content index — that silently no-ops until the index is built (see
       notes.rs), which would make the Notes scope quietly depend on an
       unrelated setting. Runs only in the Notes scope: 'all' already
       surfaces note bodies via the content index. */
    const NOTE_BODY_SEARCH_DEBOUNCE_MS = 200;
    let noteBodyMatchPaths = $state<Set<string>>(new Set());
    let noteBodySearchTimer: ReturnType<typeof setTimeout> | null = null;
    let noteBodySearchSeq = 0;

    $effect(() => {
        const q = query.trim();
        const active = paletteScope === 'notes';
        if (noteBodySearchTimer) clearTimeout(noteBodySearchTimer);

        // Clear synchronously so a stale set can't outlive its query (or its
        // scope — leaving Notes must drop the matches immediately).
        if (!active || q.length < 2) {
            noteBodySearchSeq++;
            if (noteBodyMatchPaths.size) noteBodyMatchPaths = new Set();
            return;
        }

        const seq = ++noteBodySearchSeq;
        noteBodySearchTimer = setTimeout(() => {
            noteBodySearchTimer = null;
            void invoke<Array<{ path: string }>>('search_note_bodies', { query: q })
                .then((hits) => {
                    if (seq !== noteBodySearchSeq) return; // superseded
                    noteBodyMatchPaths = new Set(hits.map((hit) => hit.path));
                })
                // Degrade to title/preview/tags, never break the palette.
                .catch(() => {
                    if (seq === noteBodySearchSeq) noteBodyMatchPaths = new Set();
                });
        }, NOTE_BODY_SEARCH_DEBOUNCE_MS);

        return () => {
            if (noteBodySearchTimer) clearTimeout(noteBodySearchTimer);
        };
    });

    /* ── Browser (Web scope) ─────────────────────────────────────────────
       Bookmarks + optional history across every installed browser/profile.
       Queried live against a short-lived temp snapshot — deliberately never
       indexed, because an index would survive the user clearing their
       browsing data. Same debounce+seq shape as the notes body search. */
    interface BrowserHit {
        url: string;
        title: string;
        displayUrl: string;
        kind: 'bookmark' | 'history';
        browser: string;
        profile: string;
        folder: string;
        visitCount: number;
        lastVisitMs: number;
        score: number;
    }
    const BROWSER_SEARCH_DEBOUNCE_MS = 200;
    let browserHits = $state<BrowserHit[]>([]);
    let browserSearching = $state(false);
    let browserSearchTimer: ReturnType<typeof setTimeout> | null = null;
    let browserSearchSeq = 0;

    $effect(() => {
        const q = query.trim();
        const active = paletteScope === 'browser' && $settings.browserSearchEnabled;
        if (browserSearchTimer) clearTimeout(browserSearchTimer);

        if (!active || q.length < 2) {
            browserSearchSeq++;
            browserSearching = false;
            if (browserHits.length) browserHits = [];
            return;
        }

        const seq = ++browserSearchSeq;
        browserSearching = true;
        browserSearchTimer = setTimeout(() => {
            browserSearchTimer = null;
            void invoke<{ results: BrowserHit[] }>('search_browser', {
                query: q,
                includeHistory: $settings.browserHistoryEnabled,
            })
                .then((res) => {
                    if (seq !== browserSearchSeq) return; // superseded
                    browserHits = res.results ?? [];
                    browserSearching = false;
                })
                .catch(() => {
                    if (seq !== browserSearchSeq) return;
                    browserHits = [];
                    browserSearching = false;
                });
        }, BROWSER_SEARCH_DEBOUNCE_MS);

        return () => {
            if (browserSearchTimer) clearTimeout(browserSearchTimer);
        };
    });

    /** Open a hit in the default browser, then dismiss. Deliberately does NOT
     *  record frecency: that persists to frecency.json, which would durably
     *  store visited URLs and outlive the browser's own history — the exact
     *  retention problem we avoid by not indexing. */
    async function openBrowserHit(hit: BrowserHit) {
        try {
            await invoke('open_external_url', { url: hit.url });
            await hidePalette();
        } catch (error) {
            console.warn('open browser hit failed:', error);
        }
    }

    /** Notes matching the current query — title/preview/tags (instant) plus
     *  body matches from `noteBodyMatchPaths`. Only meaningful in the Notes
     *  scope (the effect above only populates the set there). */
    let notesForQuery = $derived.by(() => {
        const q = query.trim().toLowerCase();
        if (!q) return [];
        const bodies = noteBodyMatchPaths;
        return $notes.filter(
            (n) =>
                n.title.toLowerCase().includes(q) ||
                n.preview.toLowerCase().includes(q) ||
                n.tags.some((t) => t.toLowerCase().includes(q)) ||
                bodies.has(n.path),
        );
    });

    function onQueryInput(event: Event) {
        const value = (event.target as HTMLInputElement).value;
        query = value;
        selectedIndex = 0;
        if (searchTimer) clearTimeout(searchTimer);
        // Mark "searching" immediately, not inside the debounced runSearch.
        // Why: between a keystroke and the debounce firing, `searching` was
        // false and the previous query's results stale. If the new query
        // happened to yield zero matches on every section, the "No matches"
        // empty state would flash briefly in the debounce window. Flipping
        // searching=true here keeps the previous results visible (or the
        // intro state if it was empty) until the new search resolves.
        if (value.trim()) {
            searching = true;
        }
        searchTimer = setTimeout(() => {
            void runSearch(value);
        }, searchDelayMs());
    }

    function searchDelayMs(): number {
        // Mirror /overlay's adaptive debounce — content search hits a
        // heavier index path so it gets a slightly longer settle window.
        // Filename search is fast enough that 80ms feels imperceptible
        // while still cutting redundant queries on fast typing.
        if (searchMode === 'content') return 140;
        return query.length <= 2 ? 120 : 80;
    }

    /** Wave 3.3.1 — auto-fire live grep across the user's CONFIGURED
     *  indexed folders. No folder picker — we already know where the
     *  user wants to search (they set up the index roots already).
     *  Used as the fallback when content search returns nothing for
     *  a non-trivial query. Subscribes to the per-operation hit-event
     *  stream so the results section fills in as the walk progresses. */
    async function startLiveGrep() {
        if (liveGrepBusy) {
            // Already running — let the user cancel from the dedicated
            // Cancel button rather than start something parallel.
            return;
        }
        const trimmed = query.trim();
        if (!trimmed) return;

        // Read the user's configured indexed folders from the search
        // backend. These are the same roots the Tantivy index walks,
        // so live-grep over them gives the user the "results from where
        // I told you to look" experience — exactly matching the
        // indexed-content surface they expect.
        let folders: string[] = [];
        try {
            const status = await invoke<{ roots?: string[]; filenameRoots?: string[] }>(
                'get_file_search_status',
            );
            // Prefer content-index roots; if empty, fall back to
            // filename-index roots (some users only configure one set).
            folders = (status.roots && status.roots.length > 0)
                ? [...status.roots]
                : ([...(status.filenameRoots ?? [])]);
        } catch (error) {
            console.warn('could not read indexed folders:', error);
        }
        if (folders.length === 0) {
            // No configured roots — let the user know with a clear
            // toast instead of silently doing nothing. Pointing them at
            // File Index settings is the natural next step.
            toast(
                'Live grep needs at least one indexed folder. Add one in Settings → File Index.',
                'info',
                5000,
            );
            return;
        }

        // Tear down any previous run's event listener BEFORE starting
        // the new one — each run has its own operation-scoped event
        // name so a stale listener wouldn't fire on the new events
        // anyway, but cleaning up keeps the listener registry tidy.
        liveGrepHitUnlisten?.();
        liveGrepHitUnlisten = null;
        liveGrepHits = [];
        liveGrepSummary = null;
        // Display label: "N folders" rather than a single path now.
        liveGrepFolder =
            folders.length === 1 ? folders[0] : `${folders.length} indexed folders`;

        const opId = `live-grep-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
        liveGrepOpId = opId;
        liveGrepBusy = true;
        try {
            liveGrepHitUnlisten = await listen<LiveGrepHit>(`live-grep-hit-${opId}`, (event) => {
                liveGrepHits = [...liveGrepHits, event.payload];
            });
            const summary = await invoke<LiveGrepSummary>('live_grep_in_folders', {
                options: {
                    folders,
                    query: trimmed,
                    operationId: opId,
                    literal: LIVE_GREP_LITERAL,
                    caseSensitive: LIVE_GREP_CASE_SENSITIVE,
                },
            });
            liveGrepSummary = summary;
        } catch (error) {
            console.warn('live grep failed:', error);
            errorToast("Couldn't search inside files", error, {
                hint: 'Some folders may be unavailable or the search was cancelled. Try a smaller scope or rebuild the content index from Settings.',
            });
        } finally {
            liveGrepBusy = false;
            liveGrepOpId = null;
            liveGrepHitUnlisten?.();
            liveGrepHitUnlisten = null;
        }
    }

    /** The last query we auto-fired live grep for. Prevents an
     *  auto-fire loop where each keystroke restarts the grep before
     *  the previous walk finishes. Reset when the user changes the
     *  query (so re-entering the same query later still triggers). */
    let liveGrepAutoFiredFor = $state<string | null>(null);

    /** Auto-fire trigger: after a content search completes with zero
     *  results for a non-trivial query, kick off the live-grep
     *  fallback across configured indexed folders. The 3+ char gate
     *  avoids wasting an SSD-saturating walk on a half-typed query.
     *  $effect runs after `contentResults` / `searching` / `query`
     *  settle, so we react to the post-search state, not the
     *  mid-debounce one.
     *
     *  Wave 3.3.2 auto-cancel: when the user keeps typing while a
     *  walk is in flight, the previous query's grep is no longer
     *  useful — cancel it so the worker frees up immediately for
     *  the new query. */
    $effect(() => {
        const trimmed = query.trim();
        // Reset the latch when the query changes so a *new* search
        // can fire even if the previous one already ran.
        if (trimmed !== liveGrepAutoFiredFor && liveGrepAutoFiredFor !== null) {
            liveGrepAutoFiredFor = null;
            // The previous query's grep hits are now stale — wipe them the
            // moment the query changes. The auto-fire below only re-runs when
            // the index returns nothing, so without this the old hits linger
            // under a new query that DOES have indexed results (the reported
            // "search society, still see outsystems hits" bug).
            liveGrepHits = [];
            liveGrepSummary = null;
            // Auto-cancel: if a previous walk is mid-flight, kill it
            // so the next iteration's auto-fire isn't fighting for
            // CPU + disk against the stale query.
            if (liveGrepBusy && liveGrepOpId) {
                void cancelLiveGrep();
            }
        }
        // Only auto-fire when: in content mode, query is long enough,
        // the index returned nothing, we're not mid-search, and we
        // haven't already auto-fired this exact query.
        if (
            searchMode === 'content' &&
            trimmed.length >= 3 &&
            !searching &&
            contentResults.length === 0 &&
            !liveGrepBusy &&
            liveGrepAutoFiredFor !== trimmed
        ) {
            liveGrepAutoFiredFor = trimmed;
            void startLiveGrep();
        }
    });

    async function cancelLiveGrep() {
        const opId = liveGrepOpId;
        if (!opId) return;
        try {
            await invoke('cancel_live_grep', { operationId: opId });
        } catch {
            // Best-effort — the search worker checks the flag at each
            // walker entry, so the cancel takes effect at the next file
            // boundary even if the invoke itself errors.
        }
    }

    /** Reveal the file containing a live-grep hit in Explorer (with the
     *  file selected). Same UX as the regular file-search and content-
     *  search result rows. Future polish: also navigate to the matched
     *  line if the user has VS Code / their editor's CLI registered. */
    async function openLiveGrepHit(hit: LiveGrepHit) {
        await revealFileSearchResult(hit.path);
    }

    /** Wave 3.3.2 — windowed snippet around a match with the matched
     *  span highlighted. matchStart/matchEnd are BYTE offsets into
     *  the UTF-8 representation of the line, so we go through
     *  TextEncoder/TextDecoder to slice correctly even when the line
     *  contains multi-byte UTF-8 (Georgian, emoji, accented chars).
     *
     *  Window: ~60 bytes before the match + the match + up to 160
     *  bytes after, clamped to the actual line. If we trimmed off
     *  the start or end, prepend / append `…`. Result is a {pre,
     *  match, post, hasMatch} struct the template renders. */
    function liveGrepHitSnippet(line: string, matchStart: number, matchEnd: number) {
        if (matchEnd <= matchStart) {
            // No match offset (rare — happens when the matcher couldn't
            // locate the match inside the trimmed line, e.g. a regex
            // anchored at start-of-line). Fall back to a plain head
            // snippet so the user still sees the line content.
            return {
                pre: line.length > 220 ? line.slice(0, 220) + '…' : line,
                match: '',
                post: '',
                hasMatch: false,
            };
        }
        const bytes = new TextEncoder().encode(line);
        const len = bytes.length;
        const winStart = matchStart > 60 ? matchStart - 60 : 0;
        const winEnd = Math.min(len, Math.max(matchEnd + 160, winStart + 220));
        const decoder = new TextDecoder();
        const pre = decoder.decode(bytes.slice(winStart, matchStart));
        const match = decoder.decode(bytes.slice(matchStart, matchEnd));
        const post = decoder.decode(bytes.slice(matchEnd, winEnd));
        return {
            pre: winStart > 0 ? '…' + pre : pre,
            match,
            post: winEnd < len ? post + '…' : post,
            hasMatch: true,
        };
    }

    async function runSearch(value: string) {
        const requestId = ++activeRequestId;
        const trimmed = value.trim();
        // Notes scope owns its own search (search_note_bodies — name + body in
        // one pass) and does not touch the file/app pipeline. Bail with the
        // same clean slate as an empty query, so no per-keystroke index hit
        // runs and no stray file match leaks into the shared empty-state guard.
        if (
            !trimmed ||
            paletteScope === 'notes' ||
            paletteScope === 'browser' ||
            paletteScope === 'windows'
        ) {
            fileResults = [];
            fileTotalHits = 0;
            contentResults = [];
            contentTotalHits = 0;
            launchResult = null;
            keepitlocalMatches = [];
            quickAction = null;
            queryMs = null;
            fileNextOffset = 0;
            isLoadingMoreFiles = false;
            searching = false;
            return;
        }
        searching = true;
        // Pagination resets on every new search — the next loadMore
        // request picks up after whatever we get back from this fetch.
        fileNextOffset = 0;
        isLoadingMoreFiles = false;

        // Flicker fix (Phase 3.6.4.11): compute the SYNCHRONOUS pieces
        // (KeepItLocal tool match, quick action) into LOCAL variables —
        // do NOT update state yet. If we updated keepitlocalMatches and
        // quickAction here immediately, those sections would unmount /
        // remount instantly while file/content/app sections kept their
        // STALE previous-query content during the async wait. That
        // desync (some sections updated, others stale) reads as flicker.
        // Instead, hold all new values and apply them ATOMICALLY when
        // the backend search resolves — so the entire body transitions
        // as one frame.
        const nextKeepitlocalMatches = computeKeepItLocalMatches(trimmed);
        // evaluateQuickAction handles BOTH frontend (percentage / base
        // conversion / date math) AND backend (plain math / units) — it
        // returns the first matching RenderableQuickAction or null.
        const quickActionPromise = evaluateQuickAction(trimmed);

        // Track wall-clock so the meta strip can show "found in 23 ms"
        // — backend `tookMs` measures only the index hit, this captures
        // round-trip including IPC overhead which is what the user
        // actually feels.
        const started = performance.now();

        // The file-search command depends on mode. Content search has
        // its own backend command (`search_file_contents`) returning
        // ContentSearchQueryResult; filename search hits `search_local_files`.
        // App search is skipped entirely in content mode — apps don't
        // have searchable contents and we don't want to dilute focus.
        const fileCommand =
            searchMode === 'content' ? 'search_file_contents' : 'search_local_files';

        try {
            const [fileSearch, launchSearch, nextQuickAction] = await Promise.all([
                invoke<FileSearchQueryResult | ContentSearchQueryResult>(fileCommand, {
                    options: {
                        query: trimmed,
                        limit: FILE_PAGE_SIZE,
                        offset: 0,
                        extensionFilter: null,
                        pathFilter: null,
                        naturalLanguage: true,
                    },
                }).catch(() => null),
                searchMode === 'content'
                    ? Promise.resolve(null)
                    : invoke<LaunchTargetSearchResult>('search_launch_targets', {
                          options: { query: trimmed, limit: 8 },
                      }).catch(() => null),
                quickActionPromise,
            ]);

            if (requestId !== activeRequestId) return;

            // ─── ATOMIC STATE COMMIT ─────────────────────────────────
            // All sections update together so the body never shows a
            // half-transitioned mix (old-query files + new-query tools).
            keepitlocalMatches = nextKeepitlocalMatches;
            // A My Command bang (built-in or custom) is the single source of
            // truth for bangs in the palette — if one matched this query,
            // drop the backend's web-search suggestion for the same keyword
            // so the user never sees the bang twice (only relevant when the
            // Web-search opt-in is also on; bangs work either way now).
            quickAction =
                nextQuickAction?.type === 'webSearch' &&
                myCommandMatches.some((m) => isBang(m.cmd))
                    ? null
                    : nextQuickAction;

            if (fileSearch) {
                if (searchMode === 'content') {
                    const contentSearch = fileSearch as ContentSearchQueryResult;
                    contentResults = contentSearch.results ?? [];
                    contentTotalHits = contentSearch.totalHits ?? 0;
                    fileResults = [];
                    fileTotalHits = 0;
                    fileNextOffset = contentResults.length;
                } else {
                    const fileResp = fileSearch as FileSearchQueryResult;
                    fileResults = fileResp.results ?? [];
                    fileTotalHits = fileResp.totalHits ?? 0;
                    contentResults = [];
                    contentTotalHits = 0;
                    fileNextOffset = fileResults.length;
                }
            }
            queryMs = Math.round(performance.now() - started);
            if (launchSearch) {
                launchResult = launchSearch;
            } else if (searchMode === 'content') {
                // Content mode hides apps — clear the previous list so a
                // stale set doesn't linger when we re-enter file mode.
                launchResult = null;
            }
        } catch (error) {
            console.warn('search failed:', error);
        } finally {
            if (requestId === activeRequestId) {
                searching = false;
            }
        }
    }

    /** Switch between filename and content search. Clears stale results
     *  immediately so the body never shows a mixed state, then re-runs
     *  the search against the current query.
     *
     *  Wave 7.6 bug fix (2026-05-28): also clears `liveGrepHits` and
     *  cancels any in-flight live grep. Without this, switching from
     *  content mode (where live grep may have populated 200+ hits)
     *  back to file mode left the INSIDE FILES section rendering
     *  those stale hits forever. */
    function setSearchMode(next: 'files' | 'content') {
        if (searchMode === next) return;
        searchMode = next;
        fileResults = [];
        fileTotalHits = 0;
        contentResults = [];
        contentTotalHits = 0;
        // Live-grep cleanup — see Wave 7.6 comment above.
        liveGrepHits = [];
        liveGrepSummary = null;
        liveGrepAutoFiredFor = null;
        if (liveGrepBusy && liveGrepOpId) {
            void cancelLiveGrep();
        }
        launchResult = null;
        queryMs = null;
        fileNextOffset = 0;
        isLoadingMoreFiles = false;
        selectedIndex = 0;
        if (query.trim()) {
            void runSearch(query);
        }
    }

    /** Fetch the next page of file results when the user scrolls near
     *  the bottom of the body. Appends to the existing list rather
     *  than replacing it so the user's scroll position is preserved.
     *  Bails out early when there's nothing more to load, when a
     *  fetch is already in flight, or when the search context has
     *  changed (different query / mode). */
    async function loadMoreFileResults() {
        if (isLoadingMoreFiles) return;
        const trimmed = query.trim();
        if (!trimmed) return;
        const currentCount =
            searchMode === 'content' ? contentResults.length : fileResults.length;
        const totalHits = searchMode === 'content' ? contentTotalHits : fileTotalHits;
        if (currentCount === 0) return; // initial fetch hasn't landed
        if (currentCount >= totalHits) return; // exhausted

        isLoadingMoreFiles = true;
        const requestId = activeRequestId;
        const fileCommand =
            searchMode === 'content' ? 'search_file_contents' : 'search_local_files';
        try {
            const response = await invoke<FileSearchQueryResult | ContentSearchQueryResult>(
                fileCommand,
                {
                    options: {
                        query: trimmed,
                        limit: FILE_PAGE_SIZE,
                        offset: fileNextOffset,
                        extensionFilter: null,
                        pathFilter: null,
                        naturalLanguage: true,
                    },
                },
            );
            // Bail if the user has moved on to a different query / mode
            // since we issued the request — otherwise we'd splice stale
            // results into the current list.
            if (requestId !== activeRequestId) return;
            if (searchMode === 'content') {
                const newRows = (response as ContentSearchQueryResult).results ?? [];
                contentResults = [...contentResults, ...newRows];
                fileNextOffset = contentResults.length;
            } else {
                const newRows = (response as FileSearchQueryResult).results ?? [];
                fileResults = [...fileResults, ...newRows];
                fileNextOffset = fileResults.length;
            }
        } catch (error) {
            console.warn('load more failed:', error);
        } finally {
            isLoadingMoreFiles = false;
        }
    }

    /** Scroll handler on `.cmd-body` — fires loadMore when the user
     *  reaches the bottom ~240 px of the scrollable area. Mirrors the
     *  /overlay pattern. Lightweight (no IntersectionObserver) since
     *  we already have the scrollEl bound. */
    function onBodyScroll() {
        if (!scrollEl) return;
        if (isLoadingMoreFiles) return;
        const threshold = 240;
        const distanceToBottom =
            scrollEl.scrollHeight - scrollEl.scrollTop - scrollEl.clientHeight;
        if (distanceToBottom < threshold) {
            void loadMoreFileResults();
        }
    }

    /* ──────────────────────────────────────────────────────────────
       Frontend quick-action evaluators live in
       `$lib/stores/quickActionEvaluators` — see that module for the
       full list of supported patterns (percentage, base conversion,
       date math, day-of-week, age, workdays, time arithmetic,
       color conversion, color contrast, bitwise ops, string ops,
       UUID, random, tip / discount, quick stats, advanced math).
       We import the orchestrator + type only; the page never reaches
       into individual evaluator functions.
       ────────────────────────────────────────────────────────────── */

    /* ──────────────────────────────────────────────────────────────
       Combined quick-action evaluator — frontend evaluators first
       (sync, no IPC), then fall through to the backend's generic
       `evaluate_quick_query`. The backend handles plain math (`23 *
       47`) and base unit conversions (`50 mi to km`); the imported
       `evaluateFrontendQuickAction` covers the rest.
       ────────────────────────────────────────────────────────────── */
    async function evaluateQuickAction(value: string): Promise<RenderableQuickAction | null> {
        const frontend = evaluateFrontendQuickAction(value);
        if (frontend) return frontend;
        try {
            const action = await invoke<QuickAction | null>('evaluate_quick_query', {
                query: value,
                // Bang shortcuts (g foo, ?foo, gh repo, etc.) and the
                // generic "search the web for X" suggestion are gated on
                // the user's explicit opt-in. When `webSearchEnabled` is
                // false the backend ignores bang prefixes and never
                // returns openUrl / webSearch results, so the palette
                // stays offline-pure for users who want it that way.
                webSearchEnabled: $settings.webSearchEnabled === true,
            });
            // Accept every variant the palette knows how to render.
            // systemCommand carries `requiresConfirmation` which the
            // activator routes through the in-app confirm modal before
            // invoking `execute_system_command`.
            if (
                action &&
                (action.type === 'calculator' ||
                    action.type === 'unitConversion' ||
                    action.type === 'openUrl' ||
                    action.type === 'webSearch' ||
                    action.type === 'systemCommand')
            ) {
                return action;
            }
        } catch {
            // Backend unreachable / errored — fall through to null.
        }
        return null;
    }

    /** Activate the current quick-action — for calculator and unit
     *  conversion this means "copy the result to the system clipboard".
     *  Matches Spotlight / Raycast's behaviour: the value the user
     *  most likely wants is the result, ready to paste anywhere. */
    async function activateQuickAction(action: RenderableQuickAction) {
        // Calculator / unit conversion → copy result to clipboard.
        if (action.type === 'calculator' || action.type === 'unitConversion') {
            try {
                await navigator.clipboard.writeText(action.result);
                // Flash a "Copied!" affordance on the row for ~1.4 s so the
                // user gets immediate feedback without a full toast.
                quickActionCopied = true;
                if (quickActionCopiedTimer) clearTimeout(quickActionCopiedTimer);
                quickActionCopiedTimer = setTimeout(() => {
                    quickActionCopied = false;
                    quickActionCopiedTimer = null;
                }, 1400);
            } catch (error) {
                console.warn('quick-action copy failed:', error);
                errorToast("Couldn't copy to clipboard", error, {
                hint: 'Another app may be holding the clipboard. Try copying again.',
            });
            }
            return;
        }
        // openUrl / webSearch → open in the user's default browser via
        // the shared backend command. After firing we dismiss the palette
        // so the user lands directly in the browser. Both shapes carry a
        // `url` field so the handler is single-branch.
        if (action.type === 'openUrl' || action.type === 'webSearch') {
            try {
                await invoke('open_external_url', { url: action.url });
                await hidePalette();
            } catch (error) {
                console.warn('open url failed:', error);
                errorToast("Couldn't open that item", error, {
                    hint: 'The file may have been moved or deleted since the result was indexed.',
                });
            }
            return;
        }
        // systemCommand → execute via `execute_system_command`. Gated
        // on a native confirm dialog when the backend marks the command
        // destructive (shutdown / restart / signout). Non-destructive
        // ones (lock / sleep / open terminal / open settings) run
        // immediately. Closes the palette after firing — the user's
        // expectation is "thing happens and the window goes away."
        if (action.type === 'systemCommand') {
            // Destructive commands (shutdown / restart / sign out) are gated
            // behind an IN-APP confirmation modal. A native confirm() dialog
            // is unreliable from this transparent, always-on-top window — it
            // can render BEHIND the palette, and the focus it steals can trip
            // hide-on-blur — which is why "no confirmation window" appeared.
            // The modal also satisfies the backend's `confirmed: true` gate.
            // Non-destructive commands (lock / sleep / terminal …) run now.
            if (action.requiresConfirmation) {
                pendingConfirm = {
                    title: action.name + '?',
                    description: action.description + '.',
                    danger: true,
                    onConfirm: () => runSystemCommand(action),
                };
                void tick().then(() => confirmCancelEl?.focus());
                return;
            }
            await runSystemCommand(action);
            return;
        }
    }

    /* ──────────────────────────────────────────────────────────────
       Destructive-action confirmation (in-app modal)

       Generic confirm prompt (2026-06-13). Was system-command-only
       (`pendingSystemCommand`); now any destructive action can request
       confirmation by setting `pendingConfirm` with its own onConfirm
       callback. System commands and the Commands-scope "close app" rows
       both flow through this one modal + keyboard guard.
       ────────────────────────────────────────────────────────────── */

    /** A destructive action waiting on the in-app confirm modal. Null =
     *  no prompt open. `onConfirm` runs the action; `danger` styles the
     *  primary button red. */
    let pendingConfirm = $state<{
        title: string;
        description: string;
        danger: boolean;
        onConfirm: () => void | Promise<void>;
    } | null>(null);
    /** Destructive prompts start on Cancel. Enter then remains safe until the
     *  user explicitly tabs to the destructive action. */
    let confirmCancelEl: HTMLButtonElement | null = $state(null);
    let confirmDialogEl: HTMLElement | null = $state(null);

    /** Execute against the backend. `confirmed: true` is sent for destructive
     *  commands so the backend safety gate (which independently refuses
     *  shutdown/restart/signout without it) lets them through.
     *
     *  Wave 2.2 (2026-05-26): backend now returns `Option<String>` — a
     *  status message for actions that have user-visible output
     *  ("Wi-Fi turned ON", "Local IP: 192.168.1.42", "DNS cache flushed"),
     *  or `null` for actions that complete silently. We toast the message
     *  on success when present. */
    async function runSystemCommand(
        action: Extract<RenderableQuickAction, { type: 'systemCommand' }>,
    ) {
        try {
            const message = await invoke<string | null>('execute_system_command', {
                id: action.id,
                confirmed: action.requiresConfirmation ? true : false,
            });
            if (message) {
                // Wave 2.2 fix (2026-05-26): when the action returned a
                // status message ("Local IP: …", "Wi-Fi turned ON", "DNS
                // resolver cache flushed"), KEEP THE PALETTE OPEN so the
                // toast actually has a window to render in. If we hid
                // the palette here, the ToastContainer would unmount
                // before the toast rendered — exactly the symptom of
                // "type 'ip' / press Enter / nothing happens". The user
                // can dismiss the palette manually via Esc or by
                // clicking outside; that explicit dismiss feels right
                // for an info-style action anyway.
                toast(message, 'success');
            } else {
                // Silent actions (lock, sleep, terminal launch, …) hide
                // the palette right away — the user's expectation is
                // "thing happens, window goes away."
                await hidePalette();
            }
        } catch (error) {
            console.warn('system command failed:', error);
            toast(`${action.name} failed: ${error}`, 'error');
        }
    }

    function confirmPendingSystemCommand() {
        const pending = pendingConfirm;
        pendingConfirm = null;
        void tick().then(() => inputEl?.focus());
        if (pending) void pending.onConfirm();
    }

    function cancelPendingSystemCommand() {
        pendingConfirm = null;
        void tick().then(() => inputEl?.focus());
    }

    /* ──────────────────────────────────────────────────────────────
       KeepItLocal tool match scorer — same logic as /overlay so the
       same query produces the same matches.
       ────────────────────────────────────────────────────────────── */

    const STOPWORDS = new Set([
        'a', 'an', 'the', 'of', 'and', 'or', 'to', 'from', 'in', 'on',
        'at', 'for', 'with', 'by', 'file', 'files', 'folder', 'folders',
        'search', 'find', 'show', 'my', 'me', 'just', 'only', 'tool', 'tools',
        'app', 'apps',
    ]);

    function normalizeSearchText(input: string) {
        return input.toLowerCase().replace(/[^a-z0-9]+/g, ' ').trim();
    }

    function computeKeepItLocalMatches(queryText: string): KeepItLocalMatch[] {
        const normalized = normalizeSearchText(queryText);
        if (normalized.length < 2) return [];
        const tokens = normalized
            .split(/\s+/)
            .filter((tok) => tok.length >= 2 && !STOPWORDS.has(tok));
        if (tokens.length === 0) return [];
        const ranked = keepitlocalTargets
            .map((tool) => {
                let score = 0;
                if (tool.name.toLowerCase() === normalized) score += 400;
                else if (tool.name.toLowerCase().startsWith(normalized)) score += 250;
                else if (tool.tokens.includes(normalized)) score += 120;
                for (const tok of tokens) {
                    if (tool.tokens.includes(tok)) score += 30;
                }
                return { id: tool.id, name: tool.name, description: tool.description, score };
            })
            .filter((m) => m.score >= 60)
            .sort((a, b) => b.score - a.score || a.name.localeCompare(b.name));
        return ranked.slice(0, 8);
    }

    /* ──────────────────────────────────────────────────────────────
       Selectable list — every row that can be Enter-activated. Used
       by keyboard navigation (↑↓ + Enter).
       ────────────────────────────────────────────────────────────── */

    type SelectableItem = {
        key: string;
        label: string;
        activate: () => void | Promise<void>;
        /** Smart-Enter contract (search/default mode): does activating this
         *  row OPEN / launch / navigate / dispatch an action? `true` → the
         *  palette dismisses after Enter (the user is done). `false` → it's a
         *  copy-style result (calculator / unit conversion) that the user reads
         *  or copies, so Enter keeps the palette open to keep working. The
         *  activate functions already encode this (copy-style ones never call
         *  hidePalette); the flag makes the contract explicit, drives the
         *  footer "Enter" hint, and refocuses the input on stay-open rows. */
        opens: boolean;
    };

    let selectables = $derived.by<SelectableItem[]>(() => {
        const items: SelectableItem[] = [];

        // Clipboard mode: snippets first (so a `/sig`-style trigger
        // match is the default Enter action — the Pro pull from the
        // legacy /clipboard-overlay), then every visible entry. Enter
        // expands+pastes snippets via `paste_snippet_text` and pastes
        // entries via `paste_clipboard_entry`.
        if (mode === 'clipboard') {
            for (const snippet of matchingSnippets) {
                items.push({
                    key: `snip:${snippet.id}`,
                    label: snippet.trigger,
                    activate: () => pasteSnippet(snippet),
                    opens: true,
                });
            }
            for (const entry of filteredClipboard) {
                items.push({
                    key: `clip:${entry.id}`,
                    label:
                        entry.kind === 'image'
                            ? `Image (${entry.imageFormat?.toUpperCase() ?? 'IMG'})`
                            : entry.text,
                    activate: () => pasteClipboardEntry(entry),
                    opens: true,
                });
            }
            return items;
        }

        if (mode !== 'default') return items;

        if (!query.trim()) {
            // Wave F (2026-05-27): scope-aware empty-state selectables.
            // The rendered body switches on `paletteScope` — selectables
            // must match or arrow-nav lands on invisible rows.
            if (paletteScope === 'tools') {
                for (const tool of $installedTools) {
                    items.push({
                        key: `tool-all:${tool.id}`,
                        label: tool.name,
                        activate: () => openMainAtTool(tool.id),
                        opens: true,
                    });
                }
                return items;
            }
            if (paletteScope === 'files') {
                if (recentItems) {
                    for (const r of recentItems.files) {
                        items.push({
                            key: `recent-file-all:${r.path}`,
                            label: r.displayName,
                            activate: () => openFile(r.path),
                            opens: true,
                        });
                    }
                }
                return items;
            }
            if (paletteScope === 'notes') {
                items.push({
                    key: 'note-new',
                    label: 'Create new note',
                    activate: () => openQuickNote(),
                    opens: true,
                });
                for (const note of $notes) {
                    items.push({
                        key: `note-all:${note.path}`,
                        label: note.title || 'Untitled',
                        activate: () => openFile(note.path),
                        opens: true,
                    });
                }
                return items;
            }
            if (paletteScope === 'apps') {
                // Wave G (2026-05-27): use the cached "all apps" list
                // when it's loaded; the data is identical to what the
                // rendered body shows, so arrow nav stays in sync.
                for (const app of browseAllApps) {
                    items.push({
                        key: `app-all:${app.path}`,
                        label: app.name,
                        activate: () => launchApp(app.path),
                        opens: true,
                    });
                }
                return items;
            }
            if (paletteScope === 'emoji') {
                // Emoji chip with no query: recents first, then the table.
                // Same list `emojiMatches` renders, so arrow-nav matches
                // exactly what's on screen.
                for (const e of emojiMatches) {
                    items.push({
                        key: `emoji:${e.c}`,
                        label: e.n,
                        activate: () => copyEmojiRow(e),
                        opens: true, // copies, then dismisses
                    });
                }
                return items;
            }
            if (paletteScope === 'commands') {
                // Commands chip (2026-06-13 redesign): the main list is the
                // 4 category rows. Selecting a category never dismisses the
                // palette — it just drives the master-detail preview pane,
                // where the actual items (system-info card, action rows,
                // running apps) are rendered and clicked.
                for (const cat of COMMAND_CATEGORIES) {
                    items.push({
                        key: `cmd-cat:${cat.id}`,
                        label: cat.label,
                        activate: () => {},
                        opens: false,
                    });
                }
                return items;
            }
            if (paletteScope === 'windows') {
                for (const win of windowMatches) {
                    items.push({
                        key: `window:${win.hwnd}`,
                        label: win.title || win.app,
                        activate: () => focusWindowRow(win),
                        opens: true,
                    });
                }
                return items;
            }
            if (paletteScope === 'browser') return items;

            // paletteScope === 'all' — original empty state: CORE
            // pillars + suggested tools + recents.
            for (const pillar of corePillars) {
                items.push({
                    key: `core:${pillar.id}`,
                    label: pillar.name,
                    activate: pillar.activate,
                    opens: false, // switches mode inside the palette — stays open
                });
            }
            for (const tool of suggestedTools) {
                items.push({
                    key: `suggested:${tool.id}`,
                    label: tool.name,
                    activate: () => openMainAtTool(tool.id),
                    opens: true,
                });
            }
            // Recent Apps — merged tools + Windows programs (pillars
            // excluded), most-recent first. Keys keep their kind so the
            // existing recent-tool/recent-app action branches still apply.
            for (const r of recentApps) {
                items.push({
                    key: r.kind === 'tool' ? `recent-tool:${r.path}` : `recent-app:${r.path}`,
                    label: r.displayName,
                    activate:
                        r.kind === 'tool'
                            ? () => openMainAtTool(r.path)
                            : () => launchApp(r.path),
                    opens: true,
                });
            }
            if (recentItems) {
                for (const r of recentItems.files.slice(0, 3)) {
                    items.push({
                        key: `recent-file:${r.path}`,
                        label: r.displayName,
                        activate: () => openFile(r.path),
                        opens: true,
                    });
                }
                for (const r of recentItems.folders.slice(0, 3)) {
                    items.push({
                        key: `recent-folder:${r.path}`,
                        label: r.displayName,
                        activate: () => openFile(r.path),
                        opens: true,
                    });
                }
            }
            return items;
        }

        // Query-active list — quick action (calculator / unit
        // conversion) goes FIRST since the user typed a math/conversion
        // expression and is clearly looking for that result. Then
        // launch apps (deduped), KeepItLocal tool matches, file results.
        // Order matches /overlay so muscle memory holds.
        // EXCEPTION — My Commands first: a user-defined command is the
        // user's own explicit shortcut, so it outranks apps, tools, and
        // even quick actions. It only matches on an exact keyword/label,
        // so it never displaces a calculator/URL/system result the user
        // typed (those inputs don't match a keyword).
        if (reminderMatch) {
            const rm = reminderMatch;
            items.push({
                key: 'reminder-create',
                label: `Remind me: ${rm.text}`,
                activate: () => createReminderFromPalette(rm),
                opens: true, // completes the action + confirms, then dismisses
            });
        }
        if (noteMatch) {
            const nt = noteMatch;
            items.push({
                key: 'note-create',
                label: `New note: ${nt}`,
                activate: () => createNoteFromPalette(nt),
                opens: true, // captures + confirms, then dismisses
            });
        }
        if (quickNoteMatch) {
            items.push({
                key: 'quick-note-open',
                label: 'Create a note',
                activate: () => openQuickNote(),
                opens: true, // opens the sticky window, then dismisses
            });
        }
        for (const m of myCommandMatches) {
            items.push({
                key: `mycmd:${m.cmd.id}`,
                label: m.cmd.label,
                activate: () => runMyCommand(m.cmd, m.arg),
                opens: true,
            });
        }
        for (const a of timeFocusMatches) {
            items.push({
                key: `tf:${a.id}`,
                label: a.label,
                activate: () => runTimeFocusAction(a),
                opens: true, // dispatches a timer/focus action, then dismisses
            });
        }
        if (quickAction) {
            // Label is descriptive enough to identify the row in a
            // screen-reader announcement — actual rendering is handled
            // by the template branches on `type`.
            const qa = quickAction;
            const label =
                qa.type === 'calculator'
                    ? qa.expression
                    : qa.type === 'unitConversion'
                      ? qa.original
                      : qa.type === 'openUrl'
                        ? qa.display
                        : qa.type === 'webSearch'
                          ? `${qa.provider}: ${qa.query}`
                          : qa.name; // systemCommand
            items.push({
                key: 'quick-action',
                label,
                activate: () => activateQuickAction(qa),
                // Calculator / unit conversion just COPY their result — Enter
                // keeps the palette open. URL / web-search / system commands
                // open or run something, so they dismiss it.
                opens: qa.type !== 'calculator' && qa.type !== 'unitConversion',
            });
        }
        // Commands scope (2026-06-13 redesign): with a query present the
        // list is still the 4 category rows — the typed query filters the
        // ITEMS shown inside the preview pane, not the category list itself.
        if (paletteScope === 'commands') {
            for (const cat of COMMAND_CATEGORIES) {
                items.push({
                    key: `cmd-cat:${cat.id}`,
                    label: cat.label,
                    activate: () => {},
                    opens: false,
                });
            }
        }
        // 'all' scope kill type-to-find: when the user types "kill chrome"
        // we still surface running-app rows inline so Enter closes them.
        // `killRows` only returns rows in 'all' when the query starts with
        // "kill " (and in 'commands' — but there the categories own the
        // preview, so this push is gated to 'all' to avoid duplicates).
        if (paletteScope === 'all') {
            for (const group of killRows) {
                items.push({
                    key: `kill:${group.exePath ?? group.name}`,
                    label: group.name,
                    activate: () => openKillConfirm(group),
                    opens: false, // opens the confirm modal, palette stays
                });
            }
        }
        // Windows chip — one selectable per open window. Must be pushed
        // here, not only rendered: a row that isn't in `selectables` renders
        // but can't be reached with the arrow keys.
        for (const win of windowMatches) {
            items.push({
                key: `window:${win.hwnd}`,
                label: win.title || win.app,
                activate: () => focusWindowRow(win),
                opens: true,
            });
        }
        // Emoji chip — one selectable per matching emoji. Same rule as the
        // Windows section above: a row that isn't in `selectables` renders
        // but can't be reached with the arrow keys.
        for (const e of emojiMatches) {
            items.push({
                key: `emoji:${e.c}`,
                label: e.n,
                activate: () => copyEmojiRow(e),
                opens: true,
            });
        }
        if (dedupedLaunchResults.length) {
            for (const app of dedupedLaunchResults) {
                items.push({
                    key: `launch:${app.path}`,
                    label: app.name,
                    activate: () => launchApp(app.path),
                    opens: true,
                });
            }
        }
        for (const tool of keepitlocalMatches) {
            items.push({
                key: `keepitlocal:${tool.id}`,
                label: tool.name,
                activate: () => openMainAtTool(tool.id),
                opens: true,
            });
        }
        for (const s of settingsMatches) {
            items.push({
                key: `settings:${s.id}`,
                label: `Settings: ${s.label}`,
                activate: () => openMainAtSettings(s.id),
                opens: true,
            });
        }
        for (const file of dedupedFileResults) {
            items.push({
                key: `file:${file.path}`,
                label: file.fileName || file.path,
                activate: () => openFile(file.path),
                opens: true,
            });
        }
        for (const file of dedupedContentResults) {
            items.push({
                key: `content:${file.path}`,
                label: file.fileName || file.path,
                activate: () => openFile(file.path),
                opens: true,
            });
        }
        // Notes-scope body/title/tag matches — only in the Notes scope, so
        // 'all' doesn't double-list notes it already shows via content hits.
        if (paletteScope === 'notes') {
            for (const note of notesForQuery) {
                items.push({
                    key: `note-hit:${note.path}`,
                    label: note.title || 'Untitled',
                    activate: () => openFile(note.path),
                    opens: true,
                });
            }
        }
        // Push order must mirror the markup order below, or arrow-nav
        // selects a different row than the one highlighted.
        if (paletteScope === 'browser') {
            for (const hit of browserHits) {
                items.push({
                    key: `browser:${hit.url}`,
                    label: hit.title,
                    activate: () => openBrowserHit(hit),
                    opens: true,
                });
            }
        }
        // Wave 3.3.2 — live-grep hits participate in selectables so they
        // get keyboard nav + Ctrl+Space action panel (Reveal in Explorer,
        // Copy path, etc.). Cap at 200 matches in the selectable list
        // for the same reason we cap the rendered list — beyond that,
        // arrow-navigating a thousand hits stops being useful.
        for (const hit of liveGrepHits.slice(0, 200)) {
            const key = `live-grep:${hit.path}:${hit.lineNumber}`;
            items.push({
                key,
                label: `${hit.path.split(/[\\/]/).pop()}:${hit.lineNumber}`,
                activate: () => void openLiveGrepHit(hit),
                opens: true,
            });
        }

        // Wave F (2026-05-27): scope-aware filter for the query-active
        // list. The render-side guards already hide non-matching sections;
        // this strips their items from the navigable list too so arrow
        // keys can't land on hidden rows. Map each `key` prefix to its
        // category, then keep only items in the current scope.
        if (paletteScope !== 'all') {
            const keyScope = (k: string): PaletteScope | null => {
                if (k.startsWith('core:')) return null; // visible only on 'all'
                if (k.startsWith('suggested:')) return null;
                if (k.startsWith('recent-folder')) return null;
                if (
                    k.startsWith('mycmd:') ||
                    k.startsWith('keepitlocal:') ||
                    k.startsWith('settings:') ||
                    k.startsWith('tf:') ||
                    k.startsWith('reminder-create') ||
                    k.startsWith('quick-action') ||
                    k.startsWith('recent-tool')
                )
                    return 'tools';
                if (
                    k.startsWith('file:') ||
                    k.startsWith('content:') ||
                    k.startsWith('live-grep:') ||
                    k.startsWith('recent-file')
                )
                    return 'files';
                if (
                    k.startsWith('note-create') ||
                    k.startsWith('quick-note-open') ||
                    k.startsWith('note-all') ||
                    k.startsWith('note-hit:') ||
                    k === 'note-new'
                )
                    return 'notes';
                if (
                    k.startsWith('launch:') ||
                    k.startsWith('recent-app') ||
                    k.startsWith('app-all:')
                )
                    return 'apps';
                if (k.startsWith('cmd-cat:') || k.startsWith('kill:'))
                    return 'commands';
                if (k.startsWith('emoji:')) return 'emoji';
                // 2026-07-29: `window:` was missing here, so with a query
                // typed in the Windows scope the rows rendered but every
                // one of them was filtered out of `selectables` — exactly
                // the arrow-key trap the comment below warns about.
                if (k.startsWith('window:')) return 'windows';
                // Mandatory: an unmapped prefix returns null and the row
                // becomes unreachable by arrow key in a non-'all' scope —
                // it renders but can't be selected.
                if (k.startsWith('browser:')) return 'browser';
                return null;
            };
            return items.filter((it) => keyScope(it.key) === paletteScope);
        }

        return items;
    });

    /** The row Enter will act on. Drives the footer hint so the user sees
     *  whether Enter will "Copy" (calculator / unit conversion) or "Execute"
     *  (open / launch / run) before they press it. */
    const selectedSelectable = $derived(selectables[selectedIndex] ?? null);
    const selectionAnnouncement = $derived.by(() => {
        if (!selectedSelectable) {
            return selectables.length ? `${selectables.length} results` : 'No results';
        }
        const label = selectedSelectable.label.replace(/\s+/g, ' ').trim();
        const summary = label.length > 80 ? `${label.slice(0, 77)}…` : label;
        return `${selectedIndex + 1} of ${selectables.length}: ${summary}`;
    });

    async function launchApp(path: string) {
        try {
            await invoke('launch_cached_target', { path });
            await hidePalette();
        } catch (error) {
            console.warn('launch failed:', error);
            errorToast("Couldn't launch that app", error, {
                hint: 'The app may have moved or been uninstalled. Try opening it from the Start menu, or re-run the app cache from Settings → Search Overlay.',
            });
        }
    }

    async function openFile(path: string) {
        // Wave K (2026-05-28): `.ki` notes are KeepItLocal's own format
        // — Windows has no associated app for them, so the previous
        // `open_search_result_path` path resulted in either a "no app
        // associated" dialog or a silent no-op. Route them into the
        // Notes editor instead, which can actually edit them. Detection
        // is case-insensitive so `.KI` / `.Ki` (rare but possible) work
        // too.
        if (/\.ki$/i.test(path)) {
            await openMainAtTool('notes', { notePath: path });
            return;
        }
        try {
            // Registered command is `open_search_result_path` (same one the
            // search overlay uses) — `open_file_search_result` doesn't exist,
            // so opening a file/folder result silently did nothing.
            await invoke('open_search_result_path', { path });
            await hidePalette();
        } catch (error) {
            console.warn('open file failed:', error);
        }
    }

    /** Run a user-defined My Command. Dispatches by type to the same
     *  backend commands the palette already uses (open_external_url /
     *  launch_cached_target / open_search_result_path). For bangs, the
     *  trailing query is substituted into the {query} template. */
    /** Per-session latch so the shell-trust warning fires once, not
     *  on every shell-command activation. Reset on palette restart
     *  (it's `let`, not persisted) — a fresh KIL run re-warns. */
    let myShellWarned = $state(false);

    async function runMyCommand(cmd: MyCommand, arg: string) {
        const target = isBang(cmd) ? expandBang(cmd, arg) : cmd.target;
        if (!target) return;
        try {
            if (cmd.type === 'url' || cmd.type === 'bang') {
                await invoke('open_external_url', { url: target });
            } else if (cmd.type === 'app') {
                await invoke('launch_cached_target', { path: target });
            } else if (cmd.type === 'shell') {
                // Wave 4.1 + 4.1b shell-type: first-use-per-session
                // warning, then route to the streaming runner. Streaming
                // means we DON'T hide the palette on success — the user
                // wants to see the output.
                if (!myShellWarned) {
                    myShellWarned = true;
                    toast(
                        'Heads up: shell commands run with your account permissions. Only run commands you trust — same as installing any Windows app.',
                        'info',
                        6000,
                    );
                }
                await runShellCommand(cmd, target);
                return;
            } else {
                // file / folder
                await invoke('open_search_result_path', { path: target });
            }
            await hidePalette();
        } catch (error) {
            console.warn('run my command failed:', error);
            errorToast(`Couldn't run "${cmd.label}"`, error, {
                hint: 'Check the command target in Settings → My Commands — the path or URL may have changed.',
            });
        }
    }

    /** Wave 4.1b-3 (2026-05-27): launch a shell command in streaming
     *  mode. Sets up the output panel state, subscribes to the per-op
     *  output event, fires the invoke, and updates the panel with the
     *  final summary (or cancelled state) when the invoke resolves. */
    async function runShellCommand(cmd: MyCommand, target: string) {
        // Tear down any previous run's listener (only one shell run on
        // screen at a time).
        shellExecUnlisten?.();
        shellExecUnlisten = null;

        const opId = `myshell-${Date.now().toString(36)}-${Math.random()
            .toString(36)
            .slice(2, 8)}`;
        const shellKind = cmd.shellKind ?? 'powershell';
        shellExec = {
            opId,
            label: cmd.label,
            command: target,
            shellKind,
            lines: [],
            running: true,
            exitCode: null,
            cancelled: false,
            durationMs: null,
            hadStderr: false,
            startedAt: Date.now(),
        };

        try {
            shellExecUnlisten = await listen<ShellOutputLine>(
                `my-shell-output-${opId}`,
                (event) => {
                    // Guard against late events arriving after the user
                    // dismissed or replaced the panel.
                    if (!shellExec || shellExec.opId !== opId) return;
                    const next = [...shellExec.lines, event.payload];
                    if (next.length > SHELL_OUTPUT_CAP) {
                        next.splice(0, next.length - SHELL_OUTPUT_CAP);
                    }
                    shellExec = { ...shellExec, lines: next };
                },
            );

            const result = await invoke<{
                outputPreview: string | null;
                hadStderr: boolean;
                exitCode: number | null;
                durationMs: number;
                cancelled: boolean;
                lineCount: number;
            }>('run_my_shell_command', {
                options: {
                    command: target,
                    operationId: opId,
                    shellKind,
                },
            });

            if (shellExec && shellExec.opId === opId) {
                shellExec = {
                    ...shellExec,
                    running: false,
                    exitCode: result.exitCode,
                    cancelled: result.cancelled,
                    durationMs: result.durationMs,
                    hadStderr: result.hadStderr,
                };
            }
        } catch (error) {
            console.warn('shell run failed:', error);
            const errMsg = String(error);
            if (shellExec && shellExec.opId === opId) {
                shellExec = {
                    ...shellExec,
                    running: false,
                    // Backend returns Err for non-zero exit; if there's
                    // no exitCode in the panel, surface the message as
                    // a synthetic stderr line so it's visible even when
                    // no stream emitted anything.
                    exitCode: shellExec.exitCode ?? -1,
                    durationMs: Date.now() - shellExec.startedAt,
                    hadStderr: true,
                    lines:
                        shellExec.lines.length === 0
                            ? [{ stream: 'stderr', line: errMsg }]
                            : shellExec.lines,
                };
            } else {
                errorToast(`Couldn't run "${cmd.label}"`, errMsg, {
                    hint: 'Check the shell command in Settings → My Commands — the executable or arguments may have changed.',
                });
            }
        } finally {
            shellExecUnlisten?.();
            shellExecUnlisten = null;
        }
    }

    /** Cancel an in-flight shell run. Best-effort: the backend kills
     *  the subprocess; the panel updates to "cancelled" when the
     *  invoke promise resolves. */
    async function cancelShellExec() {
        if (!shellExec || !shellExec.running) return;
        try {
            await invoke('cancel_my_shell_command', { operationId: shellExec.opId });
        } catch (error) {
            console.warn('cancel shell failed:', error);
        }
    }

    /** Dismiss the shell output panel. Safe to call while still
     *  running — the backend will keep running in the background until
     *  it exits naturally; we just stop showing it. Most users will
     *  hit cancel first. */
    function dismissShellExec() {
        shellExecUnlisten?.();
        shellExecUnlisten = null;
        shellExec = null;
    }

    /** One-tap capture: save the selected app/file/folder as a My Command
     *  with an auto-derived keyword (the user renames it in Settings →
     *  My Commands). The store persists to localStorage immediately, so
     *  it survives restart and shows up in the palette right away. */
    function quickAddCommand(type: MyCommandType, target: string, label: string) {
        const base =
            (label || target).toLowerCase().replace(/[^a-z0-9]+/g, '').slice(0, 16) || 'cmd';
        const created = addCommand({ keyword: base, label: label || base, type, target });
        toast(
            `Added "${created.keyword}" to My Commands — rename it in Settings → My Commands.`,
            'success',
            4500,
        );
    }

    /* ──────────────────────────────────────────────────────────────
       Keyboard handling — global Esc + ↑↓ + Enter.
       ────────────────────────────────────────────────────────────── */
    function onKeydown(event: KeyboardEvent) {
        // Destructive prompts own the keyboard while open. Escape cancels;
        // Enter is left to the focused native button so Cancel can never
        // accidentally confirm an action.
        if (pendingConfirm) {
            if (trapModalFocus(event)) return;
            if (event.key === 'Enter') {
                if (document.activeElement !== confirmCancelEl) {
                    // Do not let Enter leak to a focused search row behind the
                    // dialog. Native button behavior handles both modal buttons.
                    const active = document.activeElement;
                    const isConfirmButton =
                        active instanceof HTMLButtonElement &&
                        active.classList.contains('cmd-confirm-btn');
                    if (!isConfirmButton) event.preventDefault();
                }
            } else if (event.key === 'Escape') {
                event.preventDefault();
                cancelPendingSystemCommand();
            }
            return;
        }

        // Clipboard label editor owns the keyboard while open. Enter saves
        // only from its text field, so a focused Cancel button stays safe.
        if (labelEditEntry) {
            if (trapModalFocus(event)) return;
            if (event.key === 'Enter' && document.activeElement === labelInputEl) {
                event.preventDefault();
                void saveLabel();
            } else if (event.key === 'Escape') {
                event.preventDefault();
                cancelLabel();
            }
            return;
        }

        // Fullscreen is a true modal surface. Let its native media and button
        // controls own their keys, and never let a hidden search row receive
        // an accidental Enter or arrow key.
        if (previewFullscreen) {
            if (trapModalFocus(event)) return;
            if (event.key === 'Escape') {
                event.preventDefault();
                event.stopPropagation();
                closePreviewFullscreen();
            }
            return;
        }

        // The action surface intentionally leaves focus on the search input so
        // paste still targets the app behind the palette. It owns every key
        // while open, however, so typing cannot alter the hidden query.
        if (actionsOpen) {
            if (
                (event.ctrlKey || event.metaKey) &&
                !event.altKey &&
                !event.shiftKey &&
                event.code === 'Space'
            ) {
                event.preventDefault();
                closeActionsPanel();
                return;
            }
            if (event.key === 'Escape') {
                event.preventDefault();
                closeActionsPanel();
                inputEl?.focus();
                return;
            }
            if (event.key === 'ArrowDown') {
                event.preventDefault();
                if (availableActions.length === 0) return;
                actionsSelectedIndex =
                    (actionsSelectedIndex + 1) % availableActions.length;
                return;
            }
            if (event.key === 'ArrowUp') {
                event.preventDefault();
                if (availableActions.length === 0) return;
                actionsSelectedIndex =
                    (actionsSelectedIndex - 1 + availableActions.length) %
                    availableActions.length;
                return;
            }
            if (event.key === 'Enter') {
                event.preventDefault();
                const action = availableActions[actionsSelectedIndex];
                if (action) {
                    void action.activate();
                    if (!action.id.startsWith('copy')) {
                        closeActionsPanel();
                    }
                }
                return;
            }
            // Preserve ordinary focus traversal for the action buttons.
            if (event.key === 'Tab') return;
            event.preventDefault();
            return;
        }

        // Ctrl/Cmd + / → toggle the syntax cheatsheet. Universal help
        // shortcut — Raycast and many other tools use the same binding.
        if (
            (event.ctrlKey || event.metaKey) &&
            !event.altKey &&
            !event.shiftKey &&
            event.key === '/'
        ) {
            event.preventDefault();
            cheatsheetOpen = !cheatsheetOpen;
            if (cheatsheetOpen) {
                moreScopesOpen = false;
                appearanceOpen = false;
                shortcutsOpen = false;
            }
            return;
        }

        // Ctrl+Alt+A → toggle the live appearance editor. Uses event.code
        // (physical KeyA) because Ctrl+Alt is AltGr on Windows and would
        // garble event.key. Available in every mode — appearance is global.
        if (
            event.ctrlKey &&
            event.altKey &&
            !event.shiftKey &&
            !event.metaKey &&
            event.code === 'KeyA'
        ) {
            event.preventDefault();
            toggleAppearancePanel();
            return;
        }

        // Ctrl+Alt+I → toggle the keyboard-shortcut reference. Same
        // event.code reasoning as Ctrl+Alt+A above (Ctrl+Alt is AltGr on
        // Windows, which garbles event.key). Available in every mode.
        if (
            event.ctrlKey &&
            event.altKey &&
            !event.shiftKey &&
            !event.metaKey &&
            event.code === 'KeyI'
        ) {
            event.preventDefault();
            toggleShortcutsPanel();
            return;
        }

        // Escape always closes the highest visible surface before changing
        // search state. This prevents a panel from being left onscreen while
        // a scope, command drill-in, or palette state changes underneath it.
        if (event.key === 'Escape') {
            if (shortcutsOpen) {
                event.preventDefault();
                closeShortcutsPanel();
                return;
            }
            if (appearanceOpen) {
                event.preventDefault();
                closeAppearancePanel();
                return;
            }
            if (moreScopesOpen) {
                event.preventDefault();
                closeMoreScopes(true);
                return;
            }
            if (cheatsheetOpen) {
                event.preventDefault();
                cheatsheetOpen = false;
                return;
            }
            if (shellExec) {
                event.preventDefault();
                if (shellExec.running) {
                    void cancelShellExec();
                } else {
                    dismissShellExec();
                }
                return;
            }
            if (paletteScope === 'commands' && commandItemsFocused) {
                event.preventDefault();
                commandItemsFocused = false;
                return;
            }
            if (document.activeElement !== inputEl) {
                event.preventDefault();
                inputEl?.focus();
                return;
            }
            if (mode === 'default' && paletteScope !== 'all') {
                event.preventDefault();
                setPaletteScope('all');
                return;
            }
        }

        // More scopes is a compact menu, not a second result list. Keep its
        // arrow and Enter keys from moving or activating the palette result
        // underneath it.
        if (moreScopesOpen && !actionsOpen && !shortcutsOpen && !appearanceOpen) {
            const active = document.activeElement;
            const isMoreScopeOption =
                active instanceof HTMLButtonElement && active.dataset.moreScopeIndex !== undefined;
            if (event.key === 'ArrowDown') {
                event.preventDefault();
                moveMoreScope(2);
                return;
            }
            if (event.key === 'ArrowUp') {
                event.preventDefault();
                moveMoreScope(-2);
                return;
            }
            if (event.key === 'ArrowRight') {
                event.preventDefault();
                moveMoreScope(1);
                return;
            }
            if (event.key === 'ArrowLeft') {
                event.preventDefault();
                moveMoreScope(-1);
                return;
            }
            if (event.key === 'Home') {
                event.preventDefault();
                moreScopeIndex = 0;
                focusMoreScope(moreScopeIndex);
                return;
            }
            if (event.key === 'End') {
                event.preventDefault();
                moreScopeIndex = Math.max(0, moreScopeChips.length - 1);
                focusMoreScope(moreScopeIndex);
                return;
            }
            if (isMoreScopeOption && (event.key === 'Enter' || event.key === ' ')) {
                return;
            }
            if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                const chip = moreScopeChips[moreScopeIndex];
                if (chip) choosePaletteScope(chip.id);
                return;
            }
        }

        // Cycle scopes from the search field without taking Tab away from
        // actual controls in menus, dialogs, or action panels.
        if (
            event.key === 'Tab' &&
            !event.altKey &&
            !event.ctrlKey &&
            !event.metaKey &&
            $commandAppearance.showChips &&
            !cheatsheetOpen &&
            !appearanceOpen &&
            !shortcutsOpen &&
            !moreScopesOpen &&
            (document.activeElement === inputEl || document.activeElement === document.body)
        ) {
            const nextScope = cyclePaletteScope(
                [...primaryScopeChips, ...moreScopeChips].map((chip) => chip.id),
                paletteScope,
                event.shiftKey ? -1 : 1,
            );
            if (nextScope) {
                event.preventDefault();
                choosePaletteScope(nextScope);
                return;
            }
        }

        // Ctrl/Cmd + Space → toggle the action panel. Available in every
        // mode whenever there's a selection, so the user can always reach
        // the "what can I do with this?" surface without hunting.
        //
        // Cleanup Wave 1.2 (2026-05-28): in clipboard mode with a TEXT
        // entry selected, Ctrl+Space opens the row's wand transform
        // popover instead — that's the matching "actions for this item"
        // surface for clipboard (Format JSON / base64 / case / sort /
        // Send to note / etc.). The generic action panel doesn't carry
        // those transforms, only Paste/Pin/Label/Copy/Open URL. Falls
        // through to the action panel for image entries, snippet rows,
        // and non-clipboard modes — where the wand doesn't render.
        if (
            (event.ctrlKey || event.metaKey) &&
            !event.altKey &&
            !event.shiftKey &&
            event.code === 'Space'
        ) {
            if (mode === 'clipboard') {
                const sel = selectables[selectedIndex];
                if (sel?.key.startsWith('clip:')) {
                    const id = Number.parseInt(sel.key.slice('clip:'.length), 10);
                    const entry = clipboardEntries.find((e) => e.id === id);
                    if (entry?.kind === 'text') {
                        event.preventDefault();
                        // The wand trigger button is `.action-trigger`
                        // inside this row's `.cmd-row-actions` strip.
                        // Sibling-selector (~) reaches it from the
                        // row's main button via its data-cmd-key.
                        const wandBtn = document.querySelector<HTMLButtonElement>(
                            `[data-cmd-key="clip:${id}"] ~ .cmd-row-actions .action-trigger`,
                        );
                        wandBtn?.click();
                        return;
                    }
                }
            }
            event.preventDefault();
            toggleActionsPanel();
            return;
        }

        // Ctrl/Cmd + P → toggle the preview pane. Available in clipboard mode
        // (inspect copied text/images) AND default/search mode (Raycast-style
        // file preview: scrollable text/PDF/Office content, image thumbs, and
        // an in-palette audio/video player). Voice has its own visualization,
        // so the shortcut is inert there; outside these modes we let the
        // browser keep Ctrl+P (the listener is component-scoped anyway).
        if (
            (event.ctrlKey || event.metaKey) &&
            event.key.toLowerCase() === 'p' &&
            (mode === 'clipboard' || mode === 'default')
        ) {
            event.preventDefault();
            togglePreview();
            return;
        }

        // Cleanup Wave 1 (2026-05-28): Alt+P / Alt+D / Alt+L on the
        // selected clipboard entry → pin / delete / label. Alt-letter
        // combos don't type into the always-focused search input and
        // don't collide with Ctrl+P (preview) or webview accelerators.
        // Only fires in clipboard mode and only when the focused
        // selectable is a clipboard ENTRY (snippet rows have no pin /
        // delete / label semantics).
        if (
            mode === 'clipboard' &&
            event.altKey &&
            !event.ctrlKey &&
            !event.metaKey &&
            !event.shiftKey &&
            (event.key === 'p' ||
                event.key === 'P' ||
                event.key === 'd' ||
                event.key === 'D' ||
                event.key === 'l' ||
                event.key === 'L')
        ) {
            const sel = selectables[selectedIndex];
            // Map back from the selectable key to a clipboard entry. Keys
            // are `clip:<id>` for entries and `snip:<id>` for snippets;
            // we only act on the former.
            if (sel && sel.key.startsWith('clip:')) {
                // ClipboardEntry.id is numeric; the selectable key is a
                // `clip:<id>` string, so parseInt before the lookup.
                const id = Number.parseInt(sel.key.slice('clip:'.length), 10);
                const entry = clipboardEntries.find((e) => e.id === id);
                if (entry) {
                    event.preventDefault();
                    if (event.key === 'p' || event.key === 'P') {
                        void toggleClipboardPin(entry);
                    } else if (event.key === 'd' || event.key === 'D') {
                        void deleteClipboardEntry(entry);
                    } else {
                        openClipboardLabelEditor(entry);
                    }
                    return;
                }
            }
        }

        // Esc behaviour is mode-dependent (USER RULE):
        //   - If in a sub-mode (clipboard / voice), Esc goes BACK to
        //     default mode (not hide the palette).
        //   - If in default mode, Esc dismisses the palette entirely.
        // Backspace stays as a normal text-edit key in the input.
        if (event.key === 'Escape') {
            event.preventDefault();
            if (mode !== 'default') {
                goBackToDefault();
            } else if (query) {
                query = '';
            } else {
                void hidePalette();
            }
            return;
        }

        // Ctrl+Alt+← / Ctrl+Alt+→ — segmented-control toggle. The exact
        // same gesture switches the active sub-toggle in whichever mode
        // has one: default mode flips Files / Inside search scope; voice
        // mode flips Transcribe / Command. An explicit modifier combo
        // (vs cursor-boundary bare ←/→, which was too fragile) works
        // regardless of cursor position and never fights text editing.
        // Direction mirrors the on-screen pill order: left = first chip
        // (Files / Transcribe), right = second chip (Inside / Command).
        if (
            event.ctrlKey &&
            event.altKey &&
            !event.shiftKey &&
            !event.metaKey &&
            (event.key === 'ArrowLeft' || event.key === 'ArrowRight')
        ) {
            if (mode === 'default') {
                event.preventDefault();
                setSearchMode(event.key === 'ArrowLeft' ? 'files' : 'content');
                return;
            }
            if (mode === 'voice') {
                event.preventDefault();
                // Cleanup Wave 1 (2026-05-28): 3-way cycle through
                // transcribe → command → dictate (right) or reverse (left).
                const order: VoiceSubMode[] = ['transcribe', 'command', 'dictate'];
                const idx = order.indexOf(voiceSubMode);
                const delta = event.key === 'ArrowLeft' ? -1 : 1;
                const next = order[(idx + delta + order.length) % order.length];
                setVoiceSubMode(next);
                return;
            }
        }

        // Only the search field, body, and result rows participate in the
        // palette's global navigation model. Native controls in preview,
        // appearance, shortcuts, and shell output keep their own arrows,
        // Home/End, and Enter behavior.
        if (
            cheatsheetOpen ||
            shellExec ||
            appearanceOpen ||
            shortcutsOpen ||
            !isPaletteResultNavigationTarget(event.target)
        ) {
            return;
        }

        // Commands chip ←/→ drill-in (2026-06-13). Gated to the Commands
        // scope; runs BEFORE the generic arrow/Enter nav so the master-detail
        // pane gets first crack at the keyboard. Two states:
        //   • Drilled in (commandItemsFocused): ↓/↑ move the item cursor,
        //     ← exits back to the category list, Enter activates the item.
        //   • On the category list (not drilled): → or Enter drills INTO a
        //     category that has items (System Info has none → falls through,
        //     which is harmless — Enter there does nothing for the read card).
        if (paletteScope === 'commands') {
            if (commandItemsFocused) {
                if (event.key === 'ArrowDown') {
                    event.preventDefault();
                    commandItemIndex = Math.min(
                        commandItemIndex + 1,
                        Math.max(0, commandPreviewItems.length - 1),
                    );
                    return;
                }
                if (event.key === 'ArrowUp') {
                    event.preventDefault();
                    commandItemIndex = Math.max(0, commandItemIndex - 1);
                    return;
                }
                if (event.key === 'ArrowLeft') {
                    event.preventDefault();
                    commandItemsFocused = false;
                    return;
                }
                if (event.key === 'Enter') {
                    event.preventDefault();
                    const item = commandPreviewItems[commandItemIndex];
                    if (item) {
                        if (item.category === 'running-apps') {
                            openKillConfirm(item.group);
                        } else {
                            activateSystemAction(item.command);
                        }
                    }
                    return;
                }
            } else if (event.key === 'ArrowRight' || event.key === 'Enter') {
                // Drill IN from the category list — only when the category
                // actually has items (System Info is empty → fall through).
                if (commandPreviewItems.length > 0) {
                    event.preventDefault();
                    commandItemsFocused = true;
                    commandItemIndex = 0;
                    return;
                }
            }
        }

        // Emoji renders as a grid, so ↑/↓ move a whole row and ←/→ move one
        // cell. Everything else in the palette is a single column, where
        // ↑/↓ by 1 is already row movement — hence the scope check.
        if (
            paletteScope === 'emoji' &&
            (event.key === 'ArrowLeft' || event.key === 'ArrowRight')
        ) {
            event.preventDefault();
            scheduleNavStep(event.key === 'ArrowRight' ? 1 : -1);
        } else if (event.key === 'ArrowDown') {
            event.preventDefault();
            // Commands category-list nav (not drilled in): moving the
            // selected category resets the item cursor so a later drill-in
            // starts at the top of the new category's list.
            if (paletteScope === 'commands' && !commandItemsFocused) {
                commandItemIndex = 0;
            }
            scheduleNavStep(paletteScope === 'emoji' ? EMOJI_COLS : 1);
        } else if (event.key === 'ArrowUp') {
            event.preventDefault();
            if (paletteScope === 'commands' && !commandItemsFocused) {
                commandItemIndex = 0;
            }
            scheduleNavStep(paletteScope === 'emoji' ? -EMOJI_COLS : -1);
        } else if (event.key === 'Home') {
            // Jump selection to the first row. Matches /overlay's
            // behaviour: list navigation wins over cursor-in-input
            // text navigation, since palette queries are typically
            // short and reaching for the top of a long result list is
            // the common case.
            event.preventDefault();
            scheduleNavJump(0);
        } else if (event.key === 'End') {
            // Mirror — jump to last row.
            event.preventDefault();
            scheduleNavJump(-1);
        } else if (event.key === 'Enter') {
            event.preventDefault();
            // Smart Enter (search/default + clipboard modes; voice has no
            // selectables here and keeps its own flow):
            //   • Nothing selected → do nothing, palette stays open.
            //   • An OPENING row → its activate() opens/launches/dispatches and
            //     dismisses the palette (unchanged).
            //   • A COPY-STYLE row (calculator / unit conversion) → activate()
            //     copies WITHOUT closing; we refocus the input so the user can
            //     keep typing the next expression. This is the "don't close
            //     when nothing actually opened" behaviour.
            const item = selectables[selectedIndex];
            if (!item) return;
            void item.activate();
            if (!item.opens) inputEl?.focus();
        }
    }

    /* ──────────────────────────────────────────────────────────────
       Navigation scheduler — rAF-coalesced, instant-scroll.

       Root cause of the previous lag: the old handler called
       `el.scrollIntoView({ behavior: 'smooth' })` on every ArrowDown
       keypress. A held arrow key fires OS-rate key events (30–60/sec)
       but each smooth-scroll animation takes ~200 ms. New events fired
       WHILE the animation was running just queued more animations, so
       the browser would catch up after release in one big burst.

       Fix: coalesce all repeated arrows fired within a single frame
       into ONE selection update + ONE instant scroll. Held keys now
       advance at exactly 60fps with zero animation queue. Feels native
       like Raycast: each frame moves one step, releasing the key stops
       cleanly with no overshoot.
       ────────────────────────────────────────────────────────────── */

    /** Pending navigation delta (sum of steps requested this frame).
     *  Reset when the rAF callback fires. */
    let pendingNavDelta = 0;
    /** When non-null, a "jump to absolute index" request supersedes
     *  whatever delta accumulated (Home / End pre-empt held arrows). */
    let pendingNavJump: number | null = null;
    let navRafHandle: number | null = null;

    function resetPaletteSelection() {
        selectedIndex = 0;
        pendingNavDelta = 0;
        pendingNavJump = null;
        if (navRafHandle !== null) {
            cancelAnimationFrame(navRafHandle);
            navRafHandle = null;
        }
    }

    function scheduleNavStep(delta: number) {
        if (selectables.length === 0) return;
        pendingNavDelta += delta;
        if (navRafHandle !== null) return;
        navRafHandle = requestAnimationFrame(flushNavScheduler);
    }
    function scheduleNavJump(index: number) {
        if (selectables.length === 0) return;
        // Negative index = "from end" (End key uses -1).
        pendingNavJump =
            index < 0 ? selectables.length + index : Math.min(index, selectables.length - 1);
        pendingNavDelta = 0;
        if (navRafHandle !== null) return;
        navRafHandle = requestAnimationFrame(flushNavScheduler);
    }
    function flushNavScheduler() {
        navRafHandle = null;
        const len = selectables.length;
        if (len === 0) {
            pendingNavDelta = 0;
            pendingNavJump = null;
            return;
        }
        if (pendingNavJump != null) {
            selectedIndex = Math.max(0, Math.min(pendingNavJump, len - 1));
            pendingNavJump = null;
        } else if (pendingNavDelta !== 0) {
            // Add an extra `len` before modulo to handle negative
            // deltas correctly (ArrowUp from the first row → wraps).
            selectedIndex = ((selectedIndex + pendingNavDelta) % len + len) % len;
        }
        pendingNavDelta = 0;
        scrollSelectionIntoView();
    }

    /** Bring the currently-selected row into view. SYNC + INSTANT —
     *  no `await tick()` (we're already inside the rAF callback after
     *  the state update committed), no smooth-scroll animation.
     *  Walks `querySelectorAll('[data-cmd-key]')` matching
     *  `dataset.cmdKey` directly so CSS-selector-significant characters
     *  (Windows backslashes, colons) don't break it. */
    function scrollSelectionIntoView() {
        const item = selectables[selectedIndex];
        if (!item || !scrollEl) return;
        const rows = scrollEl.querySelectorAll<HTMLElement>('[data-cmd-key]');
        for (const el of rows) {
            if (el.dataset.cmdKey === item.key) {
                ensureRowVisible(el);
                return;
            }
        }
    }

    /** Minimal-scroll keep-in-view. `scrollIntoView({block:'nearest'})`
     *  would also work, but doing it ourselves lets us guarantee
     *  `behavior: 'auto'` (instant) on all browsers without engine-
     *  specific quirks, and avoids hitting the row's parent scroll
     *  containers (e.g. preview pane) accidentally. */
    function ensureRowVisible(el: HTMLElement) {
        if (!scrollEl) return;
        const containerRect = scrollEl.getBoundingClientRect();
        const rowRect = el.getBoundingClientRect();
        // Row above the viewport → scroll up so its top lines up with
        // a small padding above the container's top edge.
        if (rowRect.top < containerRect.top) {
            scrollEl.scrollTop -= containerRect.top - rowRect.top + 4;
            return;
        }
        // Row below the viewport → scroll down so its bottom sits
        // just inside the container's bottom edge.
        if (rowRect.bottom > containerRect.bottom) {
            scrollEl.scrollTop += rowRect.bottom - containerRect.bottom + 4;
        }
    }

    /* ──────────────────────────────────────────────────────────────
       Lifecycle
       ────────────────────────────────────────────────────────────── */
    /** Unlisten handle for the command-overlay-reset Tauri event. */
    let unlistenCommandReset: (() => void) | null = null;
    let unlistenClipboardUpdated: (() => void) | null = null;
    let unlistenBlur: (() => void) | null = null;

    /* The command-mode coordinator lives in the MAIN window and updates its
     * store directly there. This webview is not the coordinator, so without
     * the mirror `$commandMode.active` would never change here and the
     * continuous-mode button's label would be permanently stuck on "off".
     * (The mirror had zero callers after Cleanup Wave 1 deleted the voice
     * overlay, which was the only other window that needed it.) */
    let commandModeMirrorCleanup: (() => void) | undefined;

    onMount(async () => {
        commandModeMirrorCleanup = initCommandModeMirror();
        // Real command window only: make the document transparent AND clip
        // it to a rounded rectangle so the panel floats over the desktop
        // with clean corners (the OS window is transparent:true). Matches
        // /overlay's `:global(html,body){ background:transparent; border-
        // radius:16px; clip-path:inset(0 round 16px) }`. The transparent
        // background alone is NOT enough — WebView2 still paints a SQUARE
        // backdrop at the window corners, which leaks dark/opaque wedges
        // outside the panel's rounded corners (very visible over a light
        // desktop). `clip-path: inset(0 round 16px)` forces the entire
        // viewport — backdrop included — into the same 16px rounded shape,
        // so nothing square can show. Applied via JS GATED on
        // isCommandWindow (not a `:global` CSS rule) because /command is
        // also mounted as the embedded overlay inside the main window,
        // where clipping the document would mangle the whole app.
        //
        // Wave G (2026-05-27): the radius + clip MUST also drop to 0
        // when desktop blur is ON, otherwise the acrylic window is
        // square (DWMWCP_DONOTROUND) but the document inside stays
        // clipped to a rounded shape — giving the "rounded panel
        // inside square OS window" mismatch the user reported. Set
        // these as base styles, then the $effect below keeps them in
        // sync with the blur toggle.
        if (isCommandWindow) {
            for (const el of [document.documentElement, document.body]) {
                el.style.background = 'transparent';
                el.style.overflow = 'hidden';
                el.style.margin = '0';
                el.style.padding = '0';
            }
        }

        try {
            appVersion = await getVersion();
        } catch {
            appVersion = '0.1.0';
        }
        void refreshRecentItems();
        void refreshToolFrecency();
        // Wire voice subscriptions — these stay registered for the
        // lifetime of the palette so transcripts arrive whether the
        // user enters voice mode now or later. The handlers gate on
        // `mode === 'voice'` internally.
        setupVoiceListeners();

        // Real command-palette window (Phase 3.6.6): the window is warm
        // and REUSED between hotkey presses, so onMount runs only once.
        // The backend emits `command-overlay-reset` on every summon —
        // listen for it and reset to a clean default-mode palette
        // (clear query, drop to default mode, refresh recents, refocus).
        // Harmless when running as the embedded overlay or standalone
        // route — the event simply never fires in those contexts.
        try {
            unlistenCommandReset = await listen<string | null>('command-overlay-reset', (event) => {
                // Raycast-style restore (Wave 1.4, 2026-05-26): the palette
                // window is created once and reused, so Svelte state survives
                // across hides. We must NOT blanket-wipe on every summon —
                // doing so was the entire bug behind "I searched, closed, and
                // reopened to a blank palette". Behavior split:
                //
                //   - **General summon** (no payload): preserve `query`,
                //     `mode`, `voiceSubMode`, `searchMode`, and `selectedIndex`
                //     so the user lands back where they were. Re-fire the
                //     search if there was a pending query so visible results
                //     match the restored state. Modal-like surfaces
                //     (actions / cheatsheet / preview) reset every time
                //     because their open-state is transient by design.
                //   - **Explicit clipboard / voice hotkey** (payload =
                //     "clipboard"/"voice"): the user pressed a *targeted*
                //     hotkey, which carries the intent "open this surface
                //     fresh". Wipe and switch — restoring a stale text-search
                //     query on top of clipboard mode would be confusing.
                //
                // The palette window is created once and reused, and the
                // `storage` event doesn't cross WebView2 windows — so re-read
                // My Commands on every summon to pick up edits from the main
                // window's Settings since the last open. Always run.
                actionsOpen = false;
                cheatsheetOpen = false;
                previewOpen = false;
                reloadMyCommands();
                // Cleanup Wave 1 (2026-05-28): refresh user snippets on every
                // summon so the clipboard mode picker reflects edits the user
                // made in the Snippets tool since the last open. Cheap (the
                // snippets store is small and lives in DPAPI redb).
                void refreshSnippets();

                const target = event.payload;
                if (target === 'clipboard' || target === 'voice') {
                    // Explicit targeted summon — fresh start in target mode.
                    mode = 'default';
                    voiceSubMode = 'transcribe';
                    micWasUsed = false;
                    query = '';
                    selectedIndex = 0;
                    void runSearch('');
                    switchMode(target);
                    if (target === 'clipboard') void loadClipboardEntries();
                } else {
                    // General summon — preserve prior state. If a query was
                    // pending, re-run it so the results panel matches.
                    if (query.trim().length > 0) {
                        void runSearch(query);
                    }
                }
                // Always refresh recents + frecency so the suggestion lists
                // reflect the latest activity even when we're restoring.
                void refreshRecentItems();
                void refreshToolFrecency();

                // Focus the input. If we restored a non-empty query,
                // select-all so the next keystroke either continues refining
                // (arrow key, Enter) or overwrites the whole query in one
                // stroke (any printable key). Raycast does this — it's the
                // detail that makes "restore" feel responsive instead of
                // sticky.
                void tick().then(() => {
                    inputEl?.focus();
                    if (target !== 'clipboard' && target !== 'voice' && query.trim().length > 0) {
                        inputEl?.select();
                    }
                });
            });
        } catch {
            // listen unavailable (plain browser dev) — fine.
        }

        // Live clipboard refresh — when anything copies (or an entry is
        // pinned/deleted/labeled), the backend emits this; refresh the
        // list while clipboard mode is showing so it stays current
        // (parity with the dedicated clipboard overlay).
        try {
            unlistenClipboardUpdated = await listen('clipboard-history-updated', () => {
                if (mode === 'clipboard') void loadClipboardEntries();
            });
        } catch {
            // listen unavailable — fine.
        }

        // The command window hides on blur (backend Focused(false) handler),
        // which bypasses hidePalette — so stop media on blur too, otherwise a
        // movie/song keeps playing after the user clicks away.
        if (isCommandWindow) {
            try {
                unlistenBlur = await getCurrentWebviewWindow().listen('tauri://blur', () => {
                    stopPreviewMedia();
                });
            } catch {
                // listen unavailable (plain browser dev) — fine.
            }
        }

        // Wave C (2026-05-27): subscribe to window move events so the
        // 'Remember last' position option works. Geometry on mount is
        // applied by the existing $effect block that watches
        // commandAppearance.width / .position.
        // Wave I (2026-05-27): the move listener was removed with the
        // `remember-last` position option. Kept here as a comment to
        // make the absence intentional, not an oversight.

        await tick();
        inputEl?.focus();
    });

    /** Tear down voice subscriptions + ensure the mic is released
     *  when the page unmounts. Without this, a stale recognizer
     *  could keep capturing audio after the user navigates away. */
    onDestroy(() => {
        commandModeMirrorCleanup?.();
        commandModeMirrorCleanup = undefined;
        voiceUnlistenPartial?.();
        voiceUnlistenPartial = null;
        voiceUnlistenTranscript?.();
        voiceUnlistenTranscript = null;
        // Wave 3.3 — tear down the live-grep event listener if one is
        // still subscribed. The backend walk continues to completion
        // (spawn_blocking thread isn't tied to this component) but no
        // hits stream into the dead component's state.
        liveGrepHitUnlisten?.();
        liveGrepHitUnlisten = null;
        // Wave 4.1b-3 — same dance for shell streaming. The subprocess
        // keeps running until it exits naturally; the user can cancel
        // it explicitly via the panel button while it's visible.
        shellExecUnlisten?.();
        shellExecUnlisten = null;
        // Wave C (2026-05-27) — drop the move listener, otherwise it
        // would update remembered position from a dead component.
        unlistenWindowMoved?.();
        unlistenWindowMoved = null;
        disarmVoice('search-overlay');
        // Free Vosk models if the mic was used and we're unmounting
        // (route nav / embedded-overlay teardown). The hidePalette path
        // already handles the warm-window case; this is the safety net.
        if (micWasUsed) {
            micWasUsed = false;
            void invoke('voice_release_models').catch(() => {});
        }
        if (voiceOutcomeTimer) {
            clearTimeout(voiceOutcomeTimer);
            voiceOutcomeTimer = null;
        }
        if (quickActionCopiedTimer) {
            clearTimeout(quickActionCopiedTimer);
            quickActionCopiedTimer = null;
        }
        if (actionFeedbackTimer) {
            clearTimeout(actionFeedbackTimer);
            actionFeedbackTimer = null;
        }
        unlistenCommandReset?.();
        unlistenCommandReset = null;
        unlistenClipboardUpdated?.();
        unlistenClipboardUpdated = null;
        unlistenBlur?.();
        unlistenBlur = null;
        stopPreviewMedia();
    });

    /* ──────────────────────────────────────────────────────────────
       Voice mode (3.6.3). Hooks the shared `voiceSession` arbiter —
       same one the existing /voice-overlay and /overlay use — so the
       mic is never double-armed and the recognizer state is global.

       Two sub-modes:
         - transcribe: live partials mirror into the search input so
           results update as the user speaks. Final transcript stays
           in the input.
         - command:    final transcript routes through
           `executeVoiceCommand` from commandRegistry. The outcome
           (matched / not / cancelled) is surfaced briefly under the
           mic indicator. Does NOT transcribe into the input.
       ────────────────────────────────────────────────────────────── */
    let voicePartialText = $state<string | null>(null);
    /** Last command outcome label (shown for ~4s under the mic). */
    let voiceOutcome = $state<string | null>(null);
    let voiceOutcomeTimer: ReturnType<typeof setTimeout> | null = null;
    /** True once the mic has been armed in this palette session. Used to
     *  release the Vosk recognizer + models when the palette closes — so
     *  an idle palette doesn't keep speech models in RAM. Reset on each
     *  fresh summon (command-overlay-reset) and after a release. */
    let micWasUsed = false;

    /** Unlisten handles for the voice subscription callbacks. */
    let voiceUnlistenPartial: (() => void) | null = null;
    let voiceUnlistenTranscript: (() => void) | null = null;

    /** Arm / disarm the mic in lock-step with the user being in voice
     *  mode. We use `search-overlay` as the source tag because this
     *  palette replaces the search overlay structurally — the voice
     *  arbiter's source enum already includes it. */
    $effect(() => {
        if (mode === 'voice') {
            armVoice('search-overlay');
            micWasUsed = true;
        } else {
            disarmVoice('search-overlay');
            // Clear any lingering partial when leaving voice mode so
            // the next entry starts fresh.
            voicePartialText = null;
            voiceOutcome = null;
            if (voiceOutcomeTimer) {
                clearTimeout(voiceOutcomeTimer);
                voiceOutcomeTimer = null;
            }
        }
    });

    function showVoiceOutcome(label: string) {
        voiceOutcome = label;
        if (voiceOutcomeTimer) clearTimeout(voiceOutcomeTimer);
        voiceOutcomeTimer = setTimeout(() => {
            voiceOutcome = null;
            voiceOutcomeTimer = null;
        }, 4000);
    }

    /** Wire transcript handling — partials feed Transcribe mode's
     *  search; finals also feed Transcribe OR route to command
     *  execution depending on the sub-mode at the moment of arrival. */
    function setupVoiceListeners() {
        voiceUnlistenPartial = onVoicePartial((text) => {
            if (mode !== 'voice') return;
            voicePartialText = text;
            if (voiceSubMode === 'transcribe' && text) {
                // Mirror partial into the input so results update live.
                query = text;
                if (searchTimer) clearTimeout(searchTimer);
                searchTimer = setTimeout(() => {
                    void runSearch(text);
                }, searchDelayMs());
            }
            // Dictate mode: voicePartialText displays the live transcript
            // for user feedback, but we deliberately do NOT mirror into
            // query — the destination is the previous app, not the search
            // box. The final transcript handler does the paste.
        });

        voiceUnlistenTranscript = onVoiceTranscript(async (text) => {
            if (mode !== 'voice') return;
            voicePartialText = null;
            const trimmed = text.trim();
            if (!trimmed) return;

            if (voiceSubMode === 'transcribe') {
                // Lock the final phrase into the input + run a fresh
                // search (partial debounce already fired one but this
                // ensures the final query wins).
                query = trimmed;
                selectedIndex = 0;
                void runSearch(trimmed);
                return;
            }

            if (voiceSubMode === 'dictate') {
                // Cleanup Wave 1 (2026-05-28): dictate-to-paste-into-prev-app,
                // ported from /voice-overlay (L189-269). Pipeline:
                //   1. Normalize transcript in 'dictation' mode — fixes
                //      mishearings of editing commands ("comma", "new line",
                //      "scratch that", "undo") but leaves prose untouched.
                //   2. processDictation applies those editing commands +
                //      capitalization/spacing rules deterministically (no AI).
                //   3. Hold 800ms so the user reads the captured text before
                //      it lands in their app — same timing as the legacy
                //      voice overlay.
                //   4. Hide palette BEFORE invoking paste. paste_snippet_text
                //      has a 30ms internal delay before SetForegroundWindow +
                //      SendInput — that assumes our window is already hidden
                //      so the synthesized Ctrl+V lands on the user's previous
                //      app, not on us.
                //   5. Activity-log the dictation event so it shows in the
                //      Activity Log tool.
                const dictationText = normalizeTranscript(trimmed, { mode: 'dictation' }).normalized;
                const formatted = processDictation(dictationText, voiceModelLocale());
                if (formatted.length === 0) {
                    // The utterance net to nothing (e.g. it was only "scratch
                    // that") — close quietly.
                    showVoiceOutcome('(empty)');
                    return;
                }
                const typeMode = $settings.voiceOutputMode === 'type';
                showVoiceOutcome(
                    `${typeMode ? 'Typing' : 'Pasting'}: ${
                        formatted.length > 32 ? formatted.slice(0, 29) + '…' : formatted
                    }`,
                );
                await new Promise((r) => setTimeout(r, 800));
                await hidePalette();
                try {
                    if (typeMode) {
                        await invoke('type_out_text', { text: formatted });
                    } else {
                        await invoke('paste_snippet_text', {
                            text: formatted,
                            autoPaste: $settings.clipboardAutoPaste !== false,
                            restoreClipboard: $settings.restoreClipboardAfterPaste !== false,
                        });
                    }
                    void recordActivity({
                        toolId: 'voice-to-text',
                        summary:
                            formatted.length === 1
                                ? 'Dictated 1 character'
                                : `Dictated ${formatted.length} characters`,
                        outcome: 'success',
                    });
                } catch (error) {
                    console.warn('dictate paste failed:', error);
                    errorToast(typeMode ? "Couldn't type the dictation" : "Couldn't paste the dictation", error, {
                        hint: 'The target window may have closed or refused focus. The text is on your clipboard — try Ctrl+V manually.',
                    });
                }
                return;
            }

            // Command mode: route through the registry. The registry
            // handles safety-gate confirmation prompts, app-scoped
            // commands, and the standard match/no-match outcomes.
            try {
                const outcome = await executeVoiceCommand(
                    trimmed,
                    $installedTools,
                    { toolFrecency: toolFrecencyBoosts },
                );
                if (outcome.matched) {
                    showVoiceOutcome(`✓ ${outcome.description ?? trimmed}`);
                } else {
                    // Not a recognized command → treat the spoken phrase as a
                    // search query and show results. ("If it's not a command,
                    // it must search.") Drop to default mode so the full
                    // search experience (which also disarms the mic) takes over.
                    switchMode('default');
                    query = trimmed;
                    selectedIndex = 0;
                    void runSearch(trimmed);
                }
            } catch (error) {
                console.warn('voice command failed:', error);
                showVoiceOutcome(`Command error: ${error}`);
            }
        });
    }

    /** When the user toggles voice sub-mode while listening, clear
     *  the partial + outcome so the new mode starts clean. */
    function setVoiceSubMode(next: VoiceSubMode) {
        voiceSubMode = next;
        voicePartialText = null;
        voiceOutcome = null;
        if (voiceOutcomeTimer) {
            clearTimeout(voiceOutcomeTimer);
            voiceOutcomeTimer = null;
        }
        // Clear the input when switching FROM transcribe so the new
        // mode doesn't inherit the typed/spoken text. Dictate mode
        // ALSO clears: the input is irrelevant — the dictation goes
        // to the previous app, not the palette.
        if (next === 'command' || next === 'dictate') {
            query = '';
            selectedIndex = 0;
        }
    }

    /* ──────────────────────────────────────────────────────────────
       Clipboard mode (3.6.2). Reuses the existing clipboard backend
       (`get_clipboard_history` / `paste_clipboard_entry`) — same data
       shape as the workspace ClipboardHistory page so behavior is
       identical, only the surface changes.
       ────────────────────────────────────────────────────────────── */

    type ClipboardCategory =
        | 'text'
        | 'url'
        | 'email'
        | 'file_path'
        | 'json'
        | 'color'
        | 'hash'
        | 'code'
        | 'number'
        | 'image';

    type ClipboardEntry = {
        id: number;
        capturedAtMs: number;
        kind: 'text' | 'image';
        text: string;
        sourceApp: string | null;
        sourceAppPath: string | null;
        sensitiveKinds: string[];
        category: ClipboardCategory;
        isPinned: boolean;
        pinLabel: string | null;
        imagePath: string | null;
        thumbnailPath: string | null;
        imageWidth: number | null;
        imageHeight: number | null;
        imageSizeBytes: number | null;
        imageFormat: string | null;
    };

    let clipboardEntries = $state<ClipboardEntry[]>([]);
    let clipboardLoading = $state(false);

    async function loadClipboardEntries() {
        clipboardLoading = true;
        try {
            const entries = await invoke<ClipboardEntry[]>('get_clipboard_history');
            clipboardEntries = entries ?? [];
            // Pre-resolve source-app icons so the "From" field shows each app's
            // real icon. fetchEntryIcon dedups, so re-loads are cheap.
            for (const e of clipboardEntries) {
                if (e.sourceAppPath) void fetchEntryIcon(e.sourceAppPath, 'app');
            }
        } catch (error) {
            console.warn('clipboard load failed:', error);
            clipboardEntries = [];
        } finally {
            clipboardLoading = false;
        }
    }

    // Load on mode change to clipboard (and refresh if user has
    // re-entered clipboard mode after copying something elsewhere).
    $effect(() => {
        if (mode === 'clipboard') {
            void loadClipboardEntries();
        }
    });

    /* ─── Clipboard row actions (parity with the clipboard overlay) ──
       Pin / delete / copy-without-paste / label, on the selected entry.
       The backend commands already exist (used by /clipboard-overlay);
       these just call them from the palette. Keyboard: Alt+P pin,
       Alt+D delete, Alt+L label, Ctrl+Enter copy (vs plain Enter paste).
       Mirrors the dedicated overlay so the palette can stand in for it. */

    /** Label-editor modal state. Labeling an unpinned entry pins it on
     *  save (the backend only keeps pin_label on pinned entries). */
    let labelEditEntry = $state<ClipboardEntry | null>(null);
    let labelDraft = $state('');
    let labelInputEl: HTMLInputElement | null = $state(null);
    let labelDialogEl: HTMLElement | null = $state(null);

    async function toggleClipboardPin(entry: ClipboardEntry) {
        try {
            await invoke('pin_clipboard_entry', { id: entry.id, pinned: !entry.isPinned });
        } catch (error) {
            console.warn('pin failed:', error);
        }
    }

    async function deleteClipboardEntry(entry: ClipboardEntry) {
        try {
            await invoke('delete_clipboard_entry', { id: entry.id });
        } catch (error) {
            console.warn('delete failed:', error);
        }
    }

    /** Copy an entry back onto the OS clipboard WITHOUT pasting — used by
     *  the Ctrl+Space action panel for image entries (text entries copy
     *  via copyTextSilent). Keeps the palette open so the action panel can
     *  show feedback, like the other copy actions. */
    async function copyClipboardEntry(entry: ClipboardEntry) {
        try {
            await invoke('copy_clipboard_entry_to_clipboard', { id: entry.id });
            toast('Copied', 'success', 1500);
        } catch (error) {
            console.warn('copy failed:', error);
            errorToast("Couldn't copy to clipboard", error, {
                hint: 'Another app may be holding the clipboard. Try copying again.',
            });
        }
    }

    function openClipboardLabelEditor(entry: ClipboardEntry) {
        labelEditEntry = entry;
        labelDraft = entry.pinLabel ?? '';
        void tick().then(() => labelInputEl?.select());
    }

    async function saveLabel() {
        const entry = labelEditEntry;
        if (!entry) return;
        const label = labelDraft.trim() ? labelDraft.trim() : null;
        try {
            if (label && !entry.isPinned) {
                await invoke('pin_clipboard_entry', { id: entry.id, pinned: true });
            }
            await invoke('label_clipboard_entry', { id: entry.id, label });
        } catch (error) {
            console.warn('label failed:', error);
        }
        labelEditEntry = null;
        labelDraft = '';
        void tick().then(() => inputEl?.focus());
    }

    function cancelLabel() {
        labelEditEntry = null;
        labelDraft = '';
        void tick().then(() => inputEl?.focus());
    }

    /** Search-text summary for image entries so typing "png" / "image" /
     *  "1024" still finds them (parity with the overlay's image match). */
    function clipboardImageSummary(entry: ClipboardEntry): string {
        const dims =
            entry.imageWidth && entry.imageHeight
                ? `${entry.imageWidth} × ${entry.imageHeight}`
                : 'Image';
        const format = entry.imageFormat ? ` · ${entry.imageFormat.toUpperCase()}` : '';
        const size = entry.imageSizeBytes ? ` · ${formatBytes(entry.imageSizeBytes)}` : '';
        return `${dims}${format}${size}`;
    }

    /** Cleanup Wave 1 (2026-05-28): user snippets matching the current
     *  query — ported from the legacy /clipboard-overlay. Typing `/sig`
     *  enters slash-mode and matches against trigger+label; a query
     *  without a leading slash also matches if it happens to hit a
     *  trigger or label. Only computed in clipboard mode — saves the
     *  matcher run in default/voice modes where snippets don't surface. */
    let matchingSnippets = $derived.by<Snippet[]>(() => {
        if (mode !== 'clipboard') return [];
        const q = query.trim().toLowerCase();
        if (!q || $snippets.length === 0) return [];
        const slashMode = q.startsWith('/');
        const snippetQuery = slashMode ? q.slice(1) : q;
        return findSnippets(snippetQuery);
    });

    /** Filtered clipboard entries — query-scoped to clipboard text,
     *  pinned-first, capped at 50 for popup performance. */
    let filteredClipboard = $derived.by(() => {
        let result = clipboardEntries;
        const q = query.trim().toLowerCase();
        if (q) {
            result = result.filter((e) => {
                // Image entries have no text body — match their synthetic
                // summary ("1024 × 768 · PNG") + source app + pin label, so
                // typing "png" / "image" / a dimension still finds them.
                if (e.kind === 'image') {
                    return (
                        clipboardImageSummary(e).toLowerCase().includes(q) ||
                        (e.sourceApp ?? '').toLowerCase().includes(q) ||
                        (e.pinLabel ?? '').toLowerCase().includes(q) ||
                        'image'.includes(q)
                    );
                }
                return (
                    e.text.toLowerCase().includes(q) ||
                    (e.sourceApp ?? '').toLowerCase().includes(q) ||
                    (e.pinLabel ?? '').toLowerCase().includes(q)
                );
            });
        }
        return [...result]
            .sort((a, b) => {
                if (a.isPinned !== b.isPinned) return a.isPinned ? -1 : 1;
                return b.capturedAtMs - a.capturedAtMs;
            })
            .slice(0, 50);
    });

    /** Map clipboard category → Lucide icon. Mirrors the workspace
     *  ClipboardHistory page so visual identity carries across. */
    function clipboardCategoryIcon(category: ClipboardCategory) {
        switch (category) {
            case 'url':
                return Globe;
            case 'email':
                return Mail;
            case 'file_path':
                return Folder;
            case 'json':
                return Braces;
            case 'color':
                return Palette;
            case 'hash':
                return HashIcon;
            case 'code':
                return Code2;
            case 'number':
                return Calculator;
            case 'image':
                return ImageIcon;
            default:
                return TypeIcon;
        }
    }

    function clipboardCategoryLabel(category: ClipboardCategory): string {
        const map: Record<ClipboardCategory, string> = {
            text: 'Text',
            url: 'Link',
            email: 'Email',
            file_path: 'Path',
            json: 'JSON',
            color: 'Color',
            hash: 'Hash',
            code: 'Code',
            number: 'Number',
            image: 'Image',
        };
        return map[category] ?? 'Text';
    }

    /** Compact relative-time label for clipboard rows. Same shape as
     *  the workspace ClipboardHistory page but localized inline (no
     *  i18n at this dev stage — proper i18n keys land in 3.6.4 polish). */
    function clipboardFormatRelative(ms: number): string {
        const diff = Date.now() - ms;
        if (diff < 60_000) return 'Just now';
        if (diff < 3_600_000) {
            const m = Math.floor(diff / 60_000);
            return m === 1 ? '1 min ago' : `${m} mins ago`;
        }
        if (diff < 86_400_000) {
            const h = Math.floor(diff / 3_600_000);
            return h === 1 ? '1 hour ago' : `${h} hours ago`;
        }
        return new Date(ms).toLocaleDateString();
    }

    function clipboardPreview(text: string, max = 80): string {
        const clean = text.replace(/\s+/g, ' ').trim();
        return clean.length <= max ? clean : clean.slice(0, max) + '…';
    }

    /** Cheap color-format detection — used only by the color preview's
     *  Format metadata row. Falls back to "Color" if the string doesn't
     *  match a known format. We don't try to PARSE the color (the CSS
     *  engine does that via inline `background`); this is purely
     *  cosmetic labelling. */
    function detectColorFormat(value: string): string {
        const v = value.trim().toLowerCase();
        if (/^#[0-9a-f]{3}([0-9a-f])?$/.test(v)) return 'HEX (short)';
        if (/^#[0-9a-f]{6}([0-9a-f]{2})?$/.test(v)) return 'HEX';
        if (v.startsWith('rgb(')) return 'RGB';
        if (v.startsWith('rgba(')) return 'RGBA';
        if (v.startsWith('hsl(')) return 'HSL';
        if (v.startsWith('hsla(')) return 'HSLA';
        if (v.startsWith('hwb(')) return 'HWB';
        if (v.startsWith('lab(')) return 'LAB';
        if (v.startsWith('lch(')) return 'LCH';
        if (v.startsWith('oklab(')) return 'OKLAB';
        if (v.startsWith('oklch(')) return 'OKLCH';
        if (v.startsWith('color(')) return 'CSS color()';
        return 'Color';
    }

    /* ──────────────────────────────────────────────────────────────
       Preview pane (Phase 3.6.2.1) — macOS QuickLook-style inline
       preview. Right-side 320 px pane that shows the currently
       selected item's full content. Toggled by the Eye button in
       the top bar.

       The pane is keyed off `selectedIndex` so navigating with ↑↓
       updates the preview live — same UX as Raycast / Spotlight.
       ────────────────────────────────────────────────────────────── */

    let previewOpen = $state(false);
    /** Tracks whether the CURRENTLY-open preview was auto-opened by
     *  `setPaletteScope` (Commands / Files / Notes / Clipboard) rather than
     *  manually via Ctrl+P. Used so switching to a non-auto-open chip closes
     *  an auto-opened preview, while a manually-opened one is left alone. */
    let previewAutoOpened = $state(false);
    /** Commands chip ←/→ drill-in: whether keyboard focus is INSIDE the
     *  preview pane's item list (true) vs on the category-row list (false),
     *  and which item within the list is keyboard-focused. */
    let commandItemsFocused = $state(false);
    let commandItemIndex = $state(0);

    /** Preview geometry rule:
     *  Opening the inline preview must NOT resize the command palette window.
     *  The palette shell is already correct; preview is an in-palette mode.
     *  Only explicit fullscreen preview may resize/grow the dedicated window. */
    /** Syntax cheatsheet — in-palette help that replaces the body when
     *  open. Lists every recognized input pattern (natural language,
     *  phrase/boolean syntax, filters, math, conversions, web bangs,
     *  system commands) so the user doesn't have to memorize them.
     *  Toggled via Ctrl+/ (V2 dropped the top-bar HelpCircle button).
     *  NOTE: this is about what you can TYPE. Keyboard shortcuts live in
     *  the separate `shortcutsOpen` panel below — one job per surface. */
    let cheatsheetOpen = $state(false);
    /** Keyboard-shortcut reference (Ctrl+Alt+I, or the keyboard button in
     *  the top bar). V2 cut the footer from ~9 persistent hint chips down
     *  to 2, which read calmer but left the rest undiscoverable; this is
     *  the trade-back — one place that lists every binding, instead of a
     *  permanent cheat sheet crowding the footer. */
    let shortcutsOpen = $state(false);
    /** Live appearance editor (Ctrl+Alt+A) — edits the SAME commandAppearance
     *  store as Settings → Appearance, so tweaks preview instantly in this
     *  window without reopening. */
    let appearanceOpen = $state(false);
    let appearanceCloseEl: HTMLButtonElement | null = $state(null);
    let shortcutsCloseEl: HTMLButtonElement | null = $state(null);

    function openAppearancePanel() {
        moreScopesOpen = false;
        actionsOpen = false;
        cheatsheetOpen = false;
        shortcutsOpen = false;
        appearanceOpen = true;
        void tick().then(() => appearanceCloseEl?.focus());
    }

    function closeAppearancePanel() {
        appearanceOpen = false;
        void tick().then(() => inputEl?.focus());
    }

    function toggleAppearancePanel() {
        if (appearanceOpen) closeAppearancePanel();
        else openAppearancePanel();
    }

    function openShortcutsPanel() {
        moreScopesOpen = false;
        actionsOpen = false;
        cheatsheetOpen = false;
        appearanceOpen = false;
        shortcutsOpen = true;
        void tick().then(() => shortcutsCloseEl?.focus());
    }

    function closeShortcutsPanel() {
        shortcutsOpen = false;
        void tick().then(() => inputEl?.focus());
    }

    function toggleShortcutsPanel() {
        if (shortcutsOpen) closeShortcutsPanel();
        else openShortcutsPanel();
    }

    /** Palette Appearance Wave B (2026-05-27): theme picker writes through
     *  the app-wide `settings` store so palette + main window stay in sync
     *  on any theme switch. THEME_OPTIONS imported from settings.ts is the
     *  single source of truth for what themes exist. */
    function applyPaletteTheme(id: Theme) {
        settings.update((s) => ({ ...s, theme: id }));
    }

    /** Palette Appearance Wave E (2026-05-27): one-click style presets.
     *  Each entry is a named recipe of appearance + (optional) theme
     *  values. Clicking applies the whole bundle in one go via
     *  applyAppearancePatch(). 'Custom' isn't a preset to apply — it's
     *  the label we show when the user's current values don't match
     *  any preset. */
    interface StylePreset {
        id: string;
        label: string;
        description: string;
        theme?: Theme;
        patch: Partial<{
            opacity: number;
            accent: string | null;
            desktopBlur: boolean;
            density: 'compact' | 'cozy' | 'comfortable';
            animationLevel: 'reduced' | 'default' | 'lively';
            accentGlow: boolean;
            width: PaletteWidth;
            position: PalettePosition;
        }>;
    }

    const STYLE_PRESETS: StylePreset[] = [
        {
            id: 'default',
            label: 'Default',
            description: 'KIL flagship — dark, emerald, cozy density, rounded corners.',
            theme: 'dark',
            patch: {
                opacity: 0.93,
                accent: null,
                desktopBlur: false,
                density: 'cozy',
                animationLevel: 'default',
                accentGlow: true,
                width: 'standard',
                position: 'center',
            },
        },
        {
            id: 'compact',
            label: 'Compact',
            description: 'Dense rows, narrower panel — more results visible at a glance.',
            patch: {
                density: 'compact',
                width: 'standard',
                accentGlow: false,
                animationLevel: 'reduced',
            },
        },
        {
            id: 'minimal',
            label: 'Minimal',
            description: 'No blur, no glow, calm motion — pure utilitarian.',
            patch: {
                desktopBlur: false,
                accentGlow: false,
                animationLevel: 'reduced',
                opacity: 1,
            },
        },
        {
            id: 'frosted',
            label: 'Frosted',
            description: 'Acrylic blur ON (square corners), lively animations.',
            patch: {
                desktopBlur: true,
                animationLevel: 'lively',
                accentGlow: true,
                opacity: 0.86,
            },
        },
    ];

    function applyStylePreset(preset: StylePreset) {
        if (preset.theme) {
            settings.update((s) => ({ ...s, theme: preset.theme as Theme }));
        }
        applyAppearancePatch(preset.patch);
    }

    /** Wave C (2026-05-27): check if a section should be hidden by the
     *  user's Hide-sections settings. Combined with `inScope()` to guard
     *  each section's render. */
    function isHidden(id: PaletteSectionId): boolean {
        return $commandAppearance.hiddenSections.includes(id);
    }

    /** Wave C (2026-05-27): re-position + re-size the palette window
     *  according to the user's appearance choices. Only runs in the
     *  dedicated command window; on the main-window embedded preview
     *  this is a no-op (the host window owns its geometry).
     *
     *  Centering uses Tauri's built-in `center()`. Top-third positions
     *  the window horizontally centered, vertically at 1/3 of the
     *  monitor height. Remember-last reads the stored rememberedX/Y
     *  from the appearance store (saved by the `tauri://moved` listener
     *  set up in onMount). */
    /** Wave G/H (2026-05-27): keep track of whether we've shown the
     *  user "needs restart" toast for width changes that fail silently
     *  because the backend window was built with `.resizable(false)`
     *  before Wave F flipped it. Once per session. */
    let widthRestartToastShown = $state(false);

    async function applyPaletteGeometry(opts: { announce?: boolean } = {}) {
        if (!isCommandWindow) return;
        const win = getCurrentWebviewWindow();
        const a = $commandAppearance;
        const widthOption = WIDTH_OPTIONS.find((w) => w.id === a.width);
        const widthPx = widthOption?.widthPx ?? 700;
        // Wave G/H: log on success/failure so the user can diagnose
        // from devtools when something looks wrong + show a toast
        // when explicitly requested so the user knows the click landed.
        // Monitor geometry up front — drives BOTH the adaptive height and the
        // top/bottom positions.
        let monWidthLogical = 1920;
        let monHeightLogical = 1080;
        let monX = 0;
        let monY = 0;
        try {
            const monitor = await currentMonitor();
            const sf = monitor?.scaleFactor || 1;
            monWidthLogical = (monitor?.size.width ?? 1920) / sf;
            monHeightLogical = (monitor?.size.height ?? 1080) / sf;
            monX = (monitor?.position?.x ?? 0) / sf;
            monY = (monitor?.position?.y ?? 0) / sf;
        } catch (error) {
            console.warn('[palette] currentMonitor failed:', error);
        }

        // Adaptive height: keep ≥280 logical px of vertical breathing room so
        // the Top and Center positions stay visibly DISTINCT even on short
        // laptop screens — a fixed 560 filled almost the whole height there,
        // making Top and Center look identical. Capped at 560, floored at 380.
        const targetHeight = Math.round(Math.max(380, Math.min(560, monHeightLogical - 280)));

        let sizeOk = false;
        try {
            const scale = await win.scaleFactor();
            await win.setSize(new LogicalSize(widthPx, targetHeight));
            // Verify the resize took effect — Tauri silently ignores setSize on
            // a window built non-resizable (pre-Wave-F backend).
            const verifySize = await win.outerSize();
            const verifyWidthLogical = verifySize.width / scale;
            sizeOk = Math.abs(verifyWidthLogical - widthPx) < 2;
            if (import.meta.env.DEV) {
                console.log(
                    `[palette] setSize ${widthPx}×${targetHeight} logical — ${sizeOk ? 'OK' : 'IGNORED'}`,
                );
            }
            if (!sizeOk && opts.announce && !widthRestartToastShown) {
                widthRestartToastShown = true;
                toast(
                    'Width change needs a KeepItLocal restart to take effect (one-time).',
                    'info',
                    5500,
                );
            }
        } catch (error) {
            console.warn('[palette] setSize failed:', error);
        }

        try {
            if (a.position === 'center') {
                await win.center();
                if (opts.announce) toast('Position: Center', 'success', 1500);
            } else if (a.position === 'top') {
                // Window top 40 px from screen top — Spotlight feel.
                const x = monX + (monWidthLogical - widthPx) / 2;
                const y = monY + 40;
                await win.setPosition(new LogicalPosition(x, y));
                if (opts.announce) toast('Position: Top', 'success', 1500);
            } else if (a.position === 'bottom') {
                // Window bottom 40 px from screen bottom — uses the height we
                // just set so the window sits flush above the taskbar.
                const x = monX + (monWidthLogical - widthPx) / 2;
                const y = monY + monHeightLogical - targetHeight - 40;
                await win.setPosition(new LogicalPosition(x, y));
                if (opts.announce) toast('Position: Bottom', 'success', 1500);
            }
        } catch (error) {
            console.warn('[palette] setPosition failed:', error);
        }
    }

    // Wave I (2026-05-27): removed the tauri://moved listener that
    // previously powered the `remember-last` position option. The
    // position list is now Top / Center / Bottom only — there's no
    // user-dragged position to remember, so the listener and the
    // rememberedX/Y fields aren't read anymore. (The store fields
    // remain on disk for legacy installs; they're ignored.)
    let unlistenWindowMoved: (() => void) | null = null;

    // Re-apply geometry on width/position change (Wave C, fixed in
    // Wave G). The previous version used `const _w = …; void _w;` to
    // establish reactivity; Svelte 5 might optimize that away. Use a
    // template-string read instead — guarantees the proxy properties
    // are read inside the effect closure so changes do re-fire.
    // Wave I (2026-05-27): the previous effect re-fired on EVERY
    // commandAppearance change (animationLevel, accent, theme, etc.)
    // because the Svelte store auto-subscription via `$store` fires on
    // any update to the store, not just the four geometry fields. That
    // meant clicking "Lively" animation also fired the geometry toast.
    // Fix: memoize against a join-key and bail out when it hasn't
    // changed. The effect still runs on every store update, but
    // applyPaletteGeometry only runs (and toasts) on real geometry
    // changes.
    let lastGeomKey = $state('');
    $effect(() => {
        const key = `${$commandAppearance.width}|${$commandAppearance.position}`;
        if (key === lastGeomKey) return;
        const announce = lastGeomKey !== ''; // skip toast on the first mount
        lastGeomKey = key;
        void applyPaletteGeometry({ announce });
    });

    // Wave H (2026-05-27): keep document root + body border-radius +
    // clip-path in sync with desktopBlur. The onMount block above sets
    // base body styles but no longer hard-codes the radius — this
    // effect owns it.
    //
    // Blur OFF: 16 px radius + matching clip-path so the rounded
    // panel corners aren't squared off by the document backdrop.
    // Blur ON:  zero radius + no clip — the OS window is square
    // (DWMWCP_DONOTROUND), the panel is square (border-radius: 0
    // from the .has-desktop-blur rule), and the document must also
    // be square or it clips the panel back to a rounded shape.
    // Without this fix the user saw "rounded panel inside square OS
    // window" because the document clip-path overrode the panel CSS.
    $effect(() => {
        if (!isCommandWindow || typeof document === 'undefined') return;
        const blur = $commandAppearance.desktopBlur;
        const radius = blur ? '0' : '16px';
        const clip = blur ? 'none' : 'inset(0 round 16px)';
        for (const el of [document.documentElement, document.body]) {
            el.style.borderRadius = radius;
            el.style.clipPath = clip;
        }
    });

    /** Resolve the currently-selected item back to its original data
     *  source so the preview pane can render it. We tag selectables
     *  with `kind:id` keys (see the `selectables` derived earlier),
     *  so this just reverses that mapping. */
    type PreviewPayload =
        | { kind: 'clipboard'; entry: ClipboardEntry }
        | { kind: 'file'; result: FileSearchResultItem }
        | { kind: 'app'; result: LaunchTargetItem }
        | { kind: 'tool'; name: string; description: string }
        | { kind: 'core'; pillar: CorePillar }
        | { kind: 'summary'; label: string; kindLabel: string }
        | {
              kind: 'command-category';
              category: 'system-info' | 'system-actions' | 'control-panel' | 'running-apps';
          }
        | null;

    function previewKindLabel(key: string): string {
        if (key.startsWith('window:')) return 'Open window';
        if (key.startsWith('emoji:')) return 'Emoji';
        if (key.startsWith('browser:')) return 'Browser result';
        if (key.startsWith('settings:')) return 'Settings';
        if (key.startsWith('mycmd:')) return 'My Command';
        if (key.startsWith('snip:')) return 'Snippet';
        if (key.startsWith('reminder-create')) return 'Reminder';
        if (key.startsWith('note-') || key.startsWith('quick-note-open')) return 'Note';
        if (key.startsWith('tf:')) return 'Time & Focus';
        if (key.startsWith('kill:')) return 'Running app';
        if (key === 'quick-action') return 'Quick action';
        return 'Result';
    }

    let previewItem = $derived.by<PreviewPayload>(() => {
        if (!previewOpen) return null;
        const sel = selectables[selectedIndex];
        if (!sel) return null;
        const key = sel.key;

        // Commands chip (2026-06-13): the 4 category rows drive a
        // master-detail preview. The suffix after `cmd-cat:` is the
        // category id; the preview pane renders that category's items.
        if (key.startsWith('cmd-cat:')) {
            const category = key.slice('cmd-cat:'.length) as
                | 'system-info'
                | 'system-actions'
                | 'control-panel'
                | 'running-apps';
            return { kind: 'command-category', category };
        }
        if (key.startsWith('clip:')) {
            const id = parseInt(key.slice(5), 10);
            const entry = clipboardEntries.find((e) => e.id === id);
            return entry ? { kind: 'clipboard', entry } : null;
        }
        if (key.startsWith('file:')) {
            const path = key.slice(5);
            const result = fileResults.find((r) => r.path === path);
            return result ? { kind: 'file', result } : null;
        }
        if (key.startsWith('content:')) {
            const path = key.slice(8);
            const result = contentResults.find((r) => r.path === path);
            // Cast to FileSearchResultItem shape — the preview pane
            // already handles the common fields (name, ext, size,
            // modifiedMs); the snippet shows in the row rather than
            // the preview pane, so a structural conversion is enough.
            if (!result) return null;
            return {
                kind: 'file',
                result: {
                    path: result.path,
                    fileName: result.fileName,
                    entryType: 'file',
                    extension: result.extension,
                    size: result.size,
                    modifiedMs: result.modifiedMs,
                    score: result.score,
                    matchedKeywords: result.matchedKeywords,
                    matchReason: '',
                },
            };
        }
        if (key.startsWith('live-grep:')) {
            // Key shape: live-grep:<path>:<lineNumber>. Recover the path and
            // preview the file like any other file row (the snippet shows in
            // the row; the pane lazy-loads the file content).
            const rest = key.slice('live-grep:'.length);
            const m = rest.match(/^(.*):(\d+)$/);
            const path = m ? m[1] : rest;
            const fileName = path.split(/[\\/]/).pop() ?? path;
            const dot = fileName.lastIndexOf('.');
            return {
                kind: 'file',
                result: {
                    path,
                    fileName,
                    entryType: 'file',
                    extension: dot > 0 ? fileName.slice(dot) : '',
                    size: 0,
                    modifiedMs: 0,
                    score: 0,
                    matchedKeywords: [],
                    matchReason: '',
                },
            };
        }
        if (key.startsWith('launch:')) {
            const path = key.slice(7);
            // Look up against deduped — the selectable key matches a
            // visible row, so the dedupe set is the right scope.
            const result = dedupedLaunchResults.find((a) => a.path === path);
            return result ? { kind: 'app', result } : null;
        }
        if (key.startsWith('keepitlocal:')) {
            const id = key.slice(12);
            const match = keepitlocalMatches.find((m) => m.id === id);
            return match
                ? { kind: 'tool', name: match.name, description: match.description }
                : null;
        }
        if (key.startsWith('suggested:')) {
            const id = key.slice(10);
            const tool = suggestedTools.find((t) => t.id === id);
            return tool
                ? { kind: 'tool', name: tool.name, description: tool.description }
                : null;
        }
        if (key.startsWith('recent-tool:')) {
            const toolId = key.slice(12);
            const tool = $installedTools.find((t) => t.id === toolId);
            return tool
                ? { kind: 'tool', name: tool.name, description: tool.description }
                : null;
        }
        if (key.startsWith('core:')) {
            const id = key.slice(5);
            const pillar = corePillars.find((p) => p.id === id);
            return pillar ? { kind: 'core', pillar } : null;
        }
        // Wave G (2026-05-27): preview cases for the chip empty-state
        // selectable keys. Without these, pressing Ctrl+P with the
        // Notes / Tools / Files / Apps chip active would always show
        // "Select an item to preview" — the keys exist but previewItem
        // returned null.
        if (key.startsWith('tool-all:')) {
            const id = key.slice(9);
            const tool = $installedTools.find((t) => t.id === id);
            return tool
                ? { kind: 'tool', name: tool.name, description: tool.description }
                : null;
        }
        // note-all: (empty-state list) and note-hit: (query matches, incl.
        // deep body hits) both carry `<prefix>:<path>` with a 9-char prefix
        // and preview identically — a query result must be previewable too,
        // not just the browse list.
        if (key.startsWith('note-all:') || key.startsWith('note-hit:')) {
            const path = key.slice(9);
            const note = $notes.find((n) => n.path === path);
            if (!note) return null;
            // .ki files are Markdown — the preview pane treats them as
            // text. The extension stays `.ki` so the kind-router opens
            // them with the right viewer.
            return {
                kind: 'file',
                result: {
                    path: note.path,
                    fileName: (note.title || 'Untitled') + '.ki',
                    entryType: 'file',
                    extension: '.ki',
                    size: 0,
                    modifiedMs: (note as { modifiedMs?: number }).modifiedMs ?? 0,
                    score: 0,
                    matchedKeywords: [],
                    matchReason: '',
                },
            };
        }
        if (key === 'note-new') {
            return {
                kind: 'tool',
                name: 'Create new note',
                description: 'Press Enter to open a floating sticky note.',
            };
        }
        if (key.startsWith('recent-file-all:') || key.startsWith('recent-file:')) {
            // The same recent files appear in two places: the browse-all
            // view (`recent-file-all:`) and the default suggestions list
            // (`recent-file:`). Both resolve to the identical file preview.
            const path = key.startsWith('recent-file-all:') ? key.slice(16) : key.slice(12);
            const item = recentItems?.files.find((r) => r.path === path);
            if (!item) return null;
            // Wave H (2026-05-27): derive `extension` from the file
            // path so the preview pane's media-player detection
            // (isAudioFile / isVideoFile / isImageFile) actually
            // matches. Without this, recent .mp3 / .mp4 / .png
            // showed a generic "Select an item" instead of the
            // audio/video player or image thumb. The dot is kept
            // (`.mp3`) because isAudioFile / isVideoFile already
            // strip it.
            const dotIdx = item.path.lastIndexOf('.');
            const ext =
                dotIdx > 0 && dotIdx > item.path.lastIndexOf('\\') &&
                dotIdx > item.path.lastIndexOf('/')
                    ? item.path.slice(dotIdx)
                    : '';
            // Wave I (2026-05-27): use real on-disk metadata supplied
            // by the backend (`size_bytes` / `modified_ms`) instead of
            // hardcoded zeros. The previous implementation showed every
            // recent file as "0 B · —" in the preview pane because
            // RecentItem didn't carry stat info; the backend now
            // populates both fields after the per-kind truncate.
            return {
                kind: 'file',
                result: {
                    path: item.path,
                    fileName: item.displayName,
                    entryType: 'file',
                    extension: ext,
                    size: item.sizeBytes ?? 0,
                    modifiedMs: item.modifiedMs ?? 0,
                    score: 0,
                    matchedKeywords: [],
                    matchReason: '',
                },
            };
        }
        if (key.startsWith('recent-folder:')) {
            const path = key.slice(14);
            const item = recentItems?.folders.find((r) => r.path === path);
            if (!item) return null;
            // Recent folders preview like folder search hits: the `file`
            // branch renders a folder-content listing when entryType is
            // 'folder' (size / extension omitted for folders).
            return {
                kind: 'file',
                result: {
                    path: item.path,
                    fileName: item.displayName,
                    entryType: 'folder',
                    extension: '',
                    size: 0,
                    modifiedMs: item.modifiedMs ?? 0,
                    score: 0,
                    matchedKeywords: [],
                    matchReason: '',
                },
            };
        }
        if (key.startsWith('app-all:')) {
            const path = key.slice(8);
            const app = browseAllApps.find((a) => a.path === path);
            return app ? { kind: 'app', result: app } : null;
        }
        if (key.startsWith('recent-app:') || key.startsWith('recent-app-all:')) {
            const path = key.startsWith('recent-app-all:')
                ? key.slice(15)
                : key.slice(11);
            const item = recentApps.find((r) => r.path === path);
            if (!item) return null;
            return {
                kind: 'app',
                result: {
                    id: item.path,
                    name: item.displayName,
                    path: item.path,
                    kind: item.kind === 'tool' ? 'tool' : 'app',
                    source: 'recent',
                    score: 0,
                },
            };
        }
        return {
            kind: 'summary',
            label: sel.label,
            kindLabel: previewKindLabel(key),
        };
    });

    /** Commands chip ←/→ drill-in (2026-06-13): the flat list of items shown
     *  in the preview pane for the currently-selected category. Mirrors the
     *  exact filtering the `commandCategoryPane` snippet renders, so the
     *  keyboard cursor lands on the same rows the user sees. System Info is
     *  read-only (no drillable items → empty list). */
    type CommandPreviewItem =
        | { category: 'running-apps'; group: ProcessGroup }
        | {
              category: 'system-actions' | 'control-panel';
              command: SystemCommandItem;
          };
    let commandPreviewItems = $derived.by<CommandPreviewItem[]>(() => {
        const item = previewItem;
        if (!item || item.kind !== 'command-category') return [];
        if (item.category === 'system-actions') {
            return systemCommands
                .filter(
                    (c) =>
                        c.group === 'action' &&
                        commandQueryMatches(`${c.name} ${c.description}`),
                )
                .map((command) => ({ category: 'system-actions', command }));
        }
        if (item.category === 'control-panel') {
            return systemCommands
                .filter(
                    (c) =>
                        c.group === 'control-panel' &&
                        commandQueryMatches(`${c.name} ${c.description}`),
                )
                .map((command) => ({ category: 'control-panel', command }));
        }
        if (item.category === 'running-apps') {
            return processGroups
                .filter((g) => commandQueryMatches(g.name))
                .map((group) => ({ category: 'running-apps', group }));
        }
        // 'system-info' → read-only, nothing to drill into.
        return [];
    });
    // Never let the drill-in cursor get stuck on an empty / System-Info
    // category: when there are no items, snap focus back to the category list.
    $effect(() => {
        if (commandPreviewItems.length === 0) commandItemsFocused = false;
    });
    // Clamp the item cursor to the list as it shrinks (e.g. the query
    // narrows the filtered items) so the highlight never points past the end.
    $effect(() => {
        const max = Math.max(0, commandPreviewItems.length - 1);
        if (commandItemIndex > max) commandItemIndex = max;
    });
    // Keep the keyboard-focused drill-in item scrolled into view. The item
    // cursor (`commandItemIndex`) isn't part of `selectables`, so the generic
    // scroll-into-view doesn't cover it — without this you can arrow past the
    // visible rows and the list never follows (you "pick" items you can't see).
    $effect(() => {
        if (!commandItemsFocused) return;
        const idx = commandItemIndex;
        void tick().then(() => {
            document
                .querySelector(`.cmd-cat-rows [data-cmd-item="${idx}"]`)
                ?.scrollIntoView({ block: 'nearest' });
        });
    });

    /** Stop any in-palette audio/video playback. A hidden Tauri window keeps a
     *  <video>/<audio> element playing in the background, so we pause + rewind
     *  whenever the palette is dismissed (Esc/activation) OR hidden on blur. */
    function stopPreviewMedia() {
        if (typeof document === 'undefined') return;
        for (const el of Array.from(
            document.querySelectorAll<HTMLMediaElement>('audio, video'),
        )) {
            try {
                el.pause();
                el.currentTime = 0;
            } catch {
                // ignore — element may be mid-teardown
            }
        }
    }

    function togglePreview() {
        previewOpen = !previewOpen;
        // A manual Ctrl+P toggle de-classifies this preview as auto-opened:
        // a preview the user opened by hand should survive a scope switch.
        previewAutoOpened = false;
    }

    /** Heuristic: is this file path an image we can show as a thumb? */
    function isImageFile(extension: string): boolean {
        if (!extension) return false;
        const ext = extension.replace(/^\./, '').toLowerCase();
        return ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'avif', 'svg'].includes(ext);
    }

    /** Audio formats WebView2 can play via the asset protocol. */
    function isAudioFile(extension: string): boolean {
        if (!extension) return false;
        const ext = extension.replace(/^\./, '').toLowerCase();
        return ['mp3', 'wav', 'ogg', 'oga', 'm4a', 'aac', 'flac', 'opus', 'weba'].includes(ext);
    }

    /** Any video file — so it's recognized as a video (its own preview branch)
     *  rather than falling through to the text reader and dumping binary. */
    function isVideoFile(extension: string): boolean {
        if (!extension) return false;
        const ext = extension.replace(/^\./, '').toLowerCase();
        return (
            ['mp4', 'm4v', 'webm', 'ogv', 'mov', 'mkv'].includes(ext) ||
            ['avi', 'wmv', 'flv', 'mpeg', 'mpg', 'm2ts', 'ts', 'vob', 'asf', '3gp', '3g2'].includes(ext)
        );
    }

    /** PDF — rendered as actual pages via WebView2's built-in PDF viewer
     *  (an <iframe> over the asset: URL) instead of a text dump. CSP allows
     *  the asset origin in frame-src/object-src (see tauri.conf.json). */
    function isPdfFile(extension: string): boolean {
        if (!extension) return false;
        return extension.replace(/^\./, '').toLowerCase() === 'pdf';
    }

    /** Office / e-book formats WebView2 can't render natively, but whose
     *  text the content-extractor CAN pull. These show the extracted text
     *  in the reader with an honest "Text preview" label so the user knows
     *  formatting/layout isn't reproduced (press Enter for the real doc). */
    function isExtractOnlyDoc(extension: string | null | undefined): boolean {
        if (!extension) return false;
        const ext = extension.replace(/^\./, '').toLowerCase();
        return [
            'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx',
            'odt', 'ods', 'odp', 'rtf', 'epub',
        ].includes(ext);
    }


    /** Rich preview category helpers. These do not execute remote content or
     *  add network dependencies; they only change the local presentation of
     *  text already returned by `read_file_preview`. */
    function isMarkdownFile(extension: string | null | undefined): boolean {
        if (!extension) return false;
        return ['md', 'markdown', 'mdown', 'mkd'].includes(extension.replace(/^\./, '').toLowerCase());
    }

    function isWordLikeDoc(extension: string | null | undefined): boolean {
        if (!extension) return false;
        return ['doc', 'docx', 'odt', 'rtf'].includes(extension.replace(/^\./, '').toLowerCase());
    }

    function isSpreadsheetLikeDoc(extension: string | null | undefined): boolean {
        if (!extension) return false;
        return ['xls', 'xlsx', 'ods', 'csv', 'tsv'].includes(extension.replace(/^\./, '').toLowerCase());
    }

    function isPresentationLikeDoc(extension: string | null | undefined): boolean {
        if (!extension) return false;
        return ['ppt', 'pptx', 'odp'].includes(extension.replace(/^\./, '').toLowerCase());
    }

    function previewLanguageLabel(extension: string | null | undefined): string {
        if (!extension) return 'Text';
        const e = extension.replace(/^\./, '').toLowerCase();
        const map: Record<string, string> = {
            rs: 'Rust', ts: 'TypeScript', tsx: 'TSX', js: 'JavaScript', jsx: 'JSX',
            svelte: 'Svelte', vue: 'Vue', py: 'Python', go: 'Go', java: 'Java',
            kt: 'Kotlin', c: 'C', cpp: 'C++', h: 'C/C++ Header', hpp: 'C++ Header',
            cs: 'C#', rb: 'Ruby', php: 'PHP', swift: 'Swift', dart: 'Dart', lua: 'Lua',
            r: 'R', json: 'JSON', jsonc: 'JSONC', toml: 'TOML', yaml: 'YAML', yml: 'YAML',
            xml: 'XML', html: 'HTML', htm: 'HTML', css: 'CSS', scss: 'SCSS', sass: 'Sass',
            less: 'Less', sql: 'SQL', sh: 'Shell', bash: 'Bash', zsh: 'Zsh', fish: 'Fish',
            ps1: 'PowerShell', bat: 'Batch', cmd: 'Batch', env: 'ENV', ini: 'INI', conf: 'Config',
            cfg: 'Config', log: 'Log', md: 'Markdown', markdown: 'Markdown', txt: 'Text',
            doc: 'Word', docx: 'Word', odt: 'OpenDocument Text', rtf: 'Rich Text',
            xls: 'Excel', xlsx: 'Excel', ods: 'OpenDocument Sheet', csv: 'CSV', tsv: 'TSV',
            ppt: 'PowerPoint', pptx: 'PowerPoint', odp: 'OpenDocument Presentation', epub: 'EPUB',
        };
        return map[e] ?? e.toUpperCase();
    }

    function documentParagraphs(text: string): string[] {
        return text
            .replace(/\r\n/g, '\n')
            .split(/\n{2,}/)
            .map((p) => p.replace(/\n/g, ' ').replace(/\s+/g, ' ').trim())
            .filter(Boolean)
            .slice(0, 80);
    }

    function firstPreviewHitIndex(values: string[], terms: string[] = previewSearchTerms): number {
        if (!values.length || !terms.length) return -1;
        return values.findIndex((value) => previewTextHasTerm(value, terms));
    }

    /** Text content preview for the selected file (search mode). Lazily loaded
     *  via `read_file_preview`, which reuses the content-index extractor — so
     *  this shows real text for plain-text/code/Markdown/.ki AND rich formats
     *  (PDF, DOCX, XLSX…). Image/audio/video files render their own player
     *  instead, so they're skipped here. `lastPreviewPath` is a plain (non-
     *  reactive) guard so the effect doesn't re-trigger itself. */
    let filePreviewText = $state<string | null>(null);
    let filePreviewLoading = $state(false);
    /** Immediate children of the selected folder, for the folder-content
     *  preview. Lazily loaded via `list_folder_children` (backend caps at
     *  250 entries). Cleared whenever the selection isn't a folder. */
    type FolderChild = { name: string; isDir: boolean; size: number; modifiedMs: number };
    let folderChildren = $state<FolderChild[]>([]);
    /** Real structured Markdown for a `.docx`, from `read_docx_markdown`.
     *  Non-null → the Word preview renders headings/lists/tables via the
     *  same Markdown pipeline as `.md`; null → fall back to flat text
     *  (the only path for `.doc`/`.rtf`/`.odt`). */
    let docxMarkdown = $state<string | null>(null);
    /** 2026-05-27 polish: line-wrap toggle for the file reader. ON by
     *  default (most readable for prose); the user flips it OFF to see
     *  long code/log lines unwrapped + horizontal-scroll. */
    let previewWrap = $state(true);
    function togglePreviewWrap() {
        previewWrap = !previewWrap;
    }
    async function copyPreviewText() {
        if (!filePreviewText) return;
        try {
            await navigator.clipboard.writeText(filePreviewText);
            toast('File content copied', 'success', 1500);
        } catch (error) {
            errorToast("Couldn't copy to clipboard", error, {
                hint: 'Another app may be holding the clipboard. Try copying again.',
            });
        }
    }
    /** The file-reader text split into lines for the line-numbered /
     *  jump-to-match reader. Trailing blank lines trimmed so the reader
     *  doesn't end in dead space. */
    let readerLines = $derived.by<string[]>(() => {
        if (!filePreviewText) return [];
        return filePreviewText.replace(/\n+$/, '').split('\n');
    });

    /** Search terms used inside preview rendering. This is intentionally
     *  local and deterministic: no network calls, no external highlighter. */
    let previewSearchTerms = $derived.by<string[]>(() => {
        return Array.from(new Set(
            query
                .trim()
                .split(/[^\p{L}\p{N}_-]+/u)
                .map((term) => term.trim())
                .filter((term) => term.length >= 2 && term.length <= 48)
        )).slice(0, 8);
    });

    function previewTextHasTerm(value: string, terms: string[] = previewSearchTerms): boolean {
        if (!value || !terms.length) return false;
        const lower = value.toLowerCase();
        return terms.some((term) => lower.includes(term.toLowerCase()));
    }

    function highlightPreviewTerms(value: string, terms: string[] = previewSearchTerms): string {
        const escaped = escapeHtml(value || '');
        if (!terms.length) return escaped;
        const rx = new RegExp(`(${terms.map(escapeRegex).join('|')})`, 'giu');
        return escaped.replace(rx, '<mark class="cmd-preview-hitmark">$1</mark>');
    }

    /** Line number of the search match for the current preview.
     *  Priority:
     *  1) exact live-grep line from the selected result key;
     *  2) first loaded preview line containing the current query term.
     *
     *  This makes regular Tantivy/content-search rows jump too, not only
     *  live-grep rows. */
    let previewMatchLine = $derived.by<number | null>(() => {
        if (!previewOpen) return null;
        const key = selectables[selectedIndex]?.key;
        if (key?.startsWith('live-grep:')) {
            const m = key.slice('live-grep:'.length).match(/:(\d+)$/);
            if (m) {
                const n = parseInt(m[1], 10);
                if (Number.isFinite(n) && n > 0) return n;
            }
        }
        if (!readerLines.length || !previewSearchTerms.length) return null;
        const idx = readerLines.findIndex((line) => previewTextHasTerm(line, previewSearchTerms));
        return idx >= 0 ? idx + 1 : null;
    });
    /** Past this many lines we fall back to the plain <pre> reader so a
     *  64 KB preview of a minified/huge file doesn't spray thousands of
     *  DOM nodes. Most previews are far smaller. */
    const READER_LINE_CAP = 3000;

    /** Fullscreen preview overlay (covers the palette window for any
     *  previewable file — image/video/pdf/text/note — without closing the
     *  palette). Toggled by the header button / image click; Esc exits it
     *  (handled in onKeydown before the palette-close path). */
    let previewFullscreen = $state(false);
    let fullscreenDialogEl: HTMLElement | null = $state(null);
    let fullscreenCloseEl: HTMLButtonElement | null = $state(null);

    function openPreviewFullscreen() {
        previewFullscreen = true;
        void tick().then(() => fullscreenCloseEl?.focus());
    }

    function closePreviewFullscreen() {
        previewFullscreen = false;
        void tick().then(() => inputEl?.focus());
    }

    function togglePreviewFullscreen() {
        if (previewFullscreen) closePreviewFullscreen();
        else openPreviewFullscreen();
    }

    function trapModalFocus(event: KeyboardEvent) {
        if (event.key !== 'Tab') return false;
        const dialog = pendingConfirm
            ? confirmDialogEl
            : labelEditEntry
              ? labelDialogEl
              : previewFullscreen
                ? fullscreenDialogEl
                : null;
        if (!dialog) return false;

        const focusable = Array.from(
            dialog.querySelectorAll<HTMLElement>(
                'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), audio[controls], video[controls], iframe, [tabindex]:not([tabindex="-1"])',
            ),
        ).filter((element) => element.getClientRects().length > 0);
        if (focusable.length === 0) {
            event.preventDefault();
            dialog.focus();
            return true;
        }

        const currentIndex = focusable.indexOf(document.activeElement as HTMLElement);
        const nextIndex = event.shiftKey ? focusable.length - 1 : 0;
        if (
            currentIndex < 0 ||
            (!event.shiftKey && currentIndex === focusable.length - 1) ||
            (event.shiftKey && currentIndex === 0)
        ) {
            event.preventDefault();
            focusable[nextIndex]?.focus();
            return true;
        }
        return false;
    }

    function isPaletteResultNavigationTarget(target: EventTarget | null) {
        const active = target instanceof HTMLElement ? target : document.activeElement;
        return (
            active === inputEl ||
            active === document.body ||
            (active instanceof HTMLElement && active.closest('[data-cmd-key]') !== null)
        );
    }

    /** Optimistic video playback: we attempt <video> for EVERY video format
     *  (so AVI/MKV/MOV/etc. play whenever WebView2 can decode them). If the
     *  codec/container can't be decoded, the element fires `error` and we
     *  swap to a graceful "open in your player" card. Reset per file. */
    let videoError = $state(false);

    /** Per-file reset: re-attempt video on each new file, and drop out of
     *  fullscreen when there's nothing previewable selected. */
    let lastStagePath = '';
    $effect(() => {
        const item = previewItem;
        const path = item && item.kind === 'file' ? item.result.path : '';
        if (path !== lastStagePath) {
            lastStagePath = path;
            videoError = false;
        }
        // Only drop out of fullscreen when there's genuinely nothing to show
        // fullscreen. A clipboard IMAGE is fullscreenable too, so don't force
        // it closed on a background clipboard refresh (which re-derives
        // previewItem and would otherwise collapse the zoom mid-view).
        const fullscreenable =
            !!path ||
            (item?.kind === 'clipboard' &&
                item.entry.kind === 'image' &&
                !!(item.entry.imagePath || item.entry.thumbnailPath));
        if (!fullscreenable) previewFullscreen = false;
    });

    /** Fullscreen needs real estate: the dedicated command window defaults to
     *  a compact 560 px tall (COMMAND_WINDOW_HEIGHT). When the user enters
     *  fullscreen we grow the window to ~90% of the monitor so the preview is
     *  genuinely spacious, then restore the prior size on exit. setSize is the
     *  same proven API applyPaletteGeometry uses; everything is guarded so any
     *  failure just leaves the overlay at the current window size. */
    let preFullscreenSize: { w: number; h: number } | null = null;
    $effect(() => {
        const fs = previewFullscreen;
        if (!isCommandWindow) return;
        const win = getCurrentWebviewWindow();
        if (fs) {
            void (async () => {
                try {
                    const scale = (await win.scaleFactor()) || 1;
                    const outer = await win.outerSize();
                    preFullscreenSize = { w: outer.width / scale, h: outer.height / scale };
                    const mon = await currentMonitor();
                    const ms = mon?.scaleFactor || scale;
                    const availW = mon ? mon.size.width / ms : 1280;
                    const availH = mon ? mon.size.height / ms : 800;
                    const targetW = Math.min(1280, Math.max(900, Math.round(availW * 0.9)));
                    const targetH = Math.min(920, Math.max(560, Math.round(availH * 0.86)));
                    await win.setSize(new LogicalSize(targetW, targetH));
                    await win.center();
                } catch {
                    /* keep current size — overlay still fills the window */
                }
            })();
        } else if (preFullscreenSize) {
            const restore = preFullscreenSize;
            preFullscreenSize = null;
            void (async () => {
                try {
                    await win.setSize(new LogicalSize(restore.w, restore.h));
                    await applyPaletteGeometry();
                } catch {
                    /* ignore — non-fatal */
                }
            })();
        }
    });

    /** Svelte action: scroll a reader line to the vertical center of the
     *  reader element (not the whole pane/window) when it's the active
     *  match. Manual scrollTop math keeps the scroll contained. */
    function nearestScrollablePreviewContainer(node: HTMLElement): HTMLElement | null {
        let current: HTMLElement | null = node.parentElement;
        while (current) {
            const style = getComputedStyle(current);
            const canScrollY = /(auto|scroll|overlay)/.test(style.overflowY);
            if (canScrollY && current.scrollHeight > current.clientHeight + 2) {
                return current;
            }
            if (current.classList.contains('cmd-stage-host')) break;
            current = current.parentElement;
        }
        return node.closest('.cmd-preview-reader, .cmd-code-preview, .cmd-word-preview, .cmd-sheet-grid-wrap, .cmd-markdown-preview') as HTMLElement | null;
    }

    function centerNodeInScrollContainer(node: HTMLElement, reader: HTMLElement) {
        const readerRect = reader.getBoundingClientRect();
        const nodeRect = node.getBoundingClientRect();
        const nodeCenterInsideReader =
            nodeRect.top - readerRect.top + reader.scrollTop + nodeRect.height / 2;
        const target = nodeCenterInsideReader - reader.clientHeight / 2;
        reader.scrollTo({ top: Math.max(0, target), behavior: 'auto' });
    }

    function scrollHit(node: HTMLElement, active: boolean) {
        const run = () => {
            const align = () => {
                const reader = nearestScrollablePreviewContainer(node);
                if (reader) {
                    centerNodeInScrollContainer(node, reader);
                } else {
                    try {
                        node.scrollIntoView({ block: 'center' });
                    } catch {
                        /* noop */
                    }
                }
            };

            /* The inline preview pane has a tighter layout than fullscreen and
               its measured height can settle one frame later after the toolbar
               and preview body finish layout. Align twice, then once shortly
               after, so the final position is based on the real scroll box. */
            requestAnimationFrame(() => requestAnimationFrame(align));
            window.setTimeout(align, 90);
        };
        if (active) run();
        return {
            update(next: boolean) {
                if (next) run();
            },
        };
    }

    /** Wave J (2026-05-27): `.ki` is the KeepItLocal note format.
     *  Detected here so the preview pane can switch from the generic
     *  `<pre>{text}</pre>` reader to a polished markdown-rendered note
     *  view (frontmatter hidden, body styled). The dot-prefixed form
     *  matches what backend search returns; the unprefixed form is a
     *  defensive belt for any caller that strips it. */
    function isNoteFile(ext: string | null | undefined): boolean {
        if (!ext) return false;
        return ext.replace(/^\./, '').toLowerCase() === 'ki';
    }

    /** Parsed note for the current preview, or null when the active
     *  preview isn't a `.ki` file (or the body hasn't loaded yet). The
     *  parse is cheap and pure so we recompute it from the raw text on
     *  every change — no cache needed. */
    let notePreview = $derived.by(() => {
        const item = previewItem;
        if (!item || item.kind !== 'file') return null;
        if (!isNoteFile(item.result.extension)) return null;
        if (!filePreviewText) return null;
        return parseNoteMarkdown(filePreviewText, item.result.fileName);
    });

    /** Note path forwarded to the renderer so it can resolve relative
     *  `<img src>` paths and convert local absolute paths through
     *  `convertFileSrc`. Wave K (2026-05-28). */
    let notePreviewPath = $derived.by<string | undefined>(() => {
        const item = previewItem;
        if (!item || item.kind !== 'file') return undefined;
        if (!isNoteFile(item.result.extension)) return undefined;
        return item.result.path;
    });

    /** Sanitized HTML for the note body. Empty string until the parse
     *  produces a body or while the file is loading. Safe for `{@html}`
     *  — see `src/lib/notes/preview.ts` for the sanitizer's scope. */
    let noteBodyHtml = $derived(
        notePreview ? renderNoteHtml(notePreview.bodyMarkdown, notePreviewPath) : '',
    );


    /** Generic Markdown file preview — render the markdown as sanitized HTML
     *  instead of showing literal markdown syntax. This reuses the existing
     *  local renderer used by KeepItLocal notes; no external assets/libs are
     *  fetched by the palette code. */
    let markdownPreviewHtml = $derived.by<string>(() => {
        const item = previewItem;
        if (!item || item.kind !== 'file') return '';
        if (!isMarkdownFile(item.result.extension)) return '';
        if (!filePreviewText) return '';
        return renderNoteHtml(filePreviewText, item.result.path);
    });


    function escapePreviewHtml(value: string): string {
        return value
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    }

    function codeKeywordsFor(ext: string | null | undefined): string[] {
        const e = (ext || '').replace(/^\./, '').toLowerCase();
        const common = ['if', 'else', 'for', 'while', 'do', 'switch', 'case', 'break', 'continue', 'return', 'try', 'catch', 'finally', 'throw', 'throws', 'new', 'class', 'interface', 'enum', 'extends', 'implements', 'public', 'private', 'protected', 'static', 'final', 'const', 'let', 'var', 'async', 'await', 'import', 'export', 'from', 'default', 'true', 'false', 'null', 'undefined'];
        const maps: Record<string, string[]> = {
            java: [...common, 'package', 'void', 'int', 'long', 'double', 'float', 'boolean', 'char', 'byte', 'short', 'String', 'record', 'sealed'],
            cs: [...common, 'namespace', 'using', 'readonly', 'partial', 'virtual', 'override', 'abstract', 'sealed', 'string', 'int', 'long', 'decimal', 'bool', 'object', 'Task', 'IEnumerable', 'var'],
            sql: ['select', 'from', 'where', 'join', 'inner', 'left', 'right', 'full', 'outer', 'on', 'group', 'by', 'order', 'having', 'insert', 'into', 'update', 'delete', 'create', 'alter', 'drop', 'table', 'view', 'index', 'procedure', 'function', 'values', 'set', 'and', 'or', 'not', 'null', 'is', 'in', 'exists', 'between', 'like', 'case', 'when', 'then', 'else', 'end', 'distinct', 'top', 'limit', 'offset', 'union', 'all', 'as', 'with', 'recursive', 'returning', 'conflict', 'excluded', 'do', 'nothing', 'true', 'false', 'jsonb', 'array', 'text', 'now'],
            ts: [...common, 'type', 'interface', 'implements', 'readonly', 'keyof', 'typeof', 'any', 'unknown', 'never', 'string', 'number', 'boolean'],
            tsx: [...common, 'type', 'interface', 'implements', 'readonly', 'keyof', 'typeof', 'any', 'unknown', 'never', 'string', 'number', 'boolean'],
            js: common,
            jsx: common,
            py: ['def', 'class', 'if', 'elif', 'else', 'for', 'while', 'try', 'except', 'finally', 'with', 'as', 'return', 'yield', 'import', 'from', 'pass', 'break', 'continue', 'raise', 'True', 'False', 'None', 'async', 'await', 'lambda', 'global', 'nonlocal', 'in', 'is', 'and', 'or', 'not'],
            rs: ['fn', 'let', 'mut', 'pub', 'impl', 'trait', 'struct', 'enum', 'match', 'if', 'else', 'while', 'for', 'loop', 'return', 'use', 'mod', 'crate', 'self', 'Self', 'async', 'await', 'move', 'where', 'const', 'static', 'ref', 'true', 'false', 'None', 'Some', 'Ok', 'Err'],
            go: ['package', 'import', 'func', 'type', 'struct', 'interface', 'var', 'const', 'if', 'else', 'for', 'range', 'switch', 'case', 'default', 'return', 'defer', 'go', 'select', 'chan', 'map', 'true', 'false', 'nil'],
        };
        return maps[e] || common;
    }

    function codePreviewSearchTerms(search: string): string[] {
        return Array.from(new Set(
            search
                .trim()
                .split(/[^\p{L}\p{N}_-]+/u)
                .map((term) => term.trim())
                .filter((term) => term.length >= 2 && term.length <= 48)
        )).slice(0, 8);
    }

    function codeLineHasSearchTerm(line: string, terms: string[]): boolean {
        if (!terms.length) return false;
        const lower = line.toLowerCase();
        return terms.some((term) => lower.includes(term.toLowerCase()));
    }

    function highlightCodePlainText(raw: string, ext: string | null | undefined, terms: string[]): string {
        if (!raw) return '';
        const e = (ext || '').replace(/^\./, '').toLowerCase();
        const keywords = codeKeywordsFor(e).map(escapeRegex);
        const queryTerms = terms.map(escapeRegex);
        const parts: string[] = [];
        if (queryTerms.length) parts.push(`(?<hit>${queryTerms.join('|')})`);
        if (keywords.length) parts.push(`(?<kw>\\b(?:${keywords.join('|')})\\b)`);
        parts.push('(?<num>\\b(?:0x[0-9a-fA-F]+|\\d+(?:\\.\\d+)?)\\b)');
        const flags = e === 'sql' || queryTerms.length ? 'giu' : 'gu';
        const rx = new RegExp(parts.join('|'), flags);

        let out = '';
        let cursor = 0;
        let match: RegExpExecArray | null;
        while ((match = rx.exec(raw))) {
            out += escapePreviewHtml(raw.slice(cursor, match.index));
            const value = escapePreviewHtml(match[0]);
            const groups = match.groups || {};
            if (groups.hit) out += `<span class="tok-hit">${value}</span>`;
            else if (groups.kw) out += `<span class="tok-kw">${value}</span>`;
            else if (groups.num) out += `<span class="tok-num">${value}</span>`;
            cursor = match.index + match[0].length;
        }
        out += escapePreviewHtml(raw.slice(cursor));
        return out;
    }

    function highlightCodeLine(rawLine: string, ext: string | null | undefined, terms: string[]): string {
        const e = (ext || '').replace(/^\./, '').toLowerCase();
        const commentMarker = e === 'sql'
            ? rawLine.indexOf('--')
            : ['py', 'sh', 'bash', 'zsh', 'fish', 'ps1', 'env', 'conf', 'cfg', 'ini'].includes(e)
                ? rawLine.indexOf('#')
                : rawLine.indexOf('//');
        const codePart = commentMarker >= 0 ? rawLine.slice(0, commentMarker) : rawLine;
        const commentPart = commentMarker >= 0 ? rawLine.slice(commentMarker) : '';

        const pieces: string[] = [];
        let cursor = 0;
        const stringRx = /("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|`(?:\\.|[^`\\])*`)/g;
        let match: RegExpExecArray | null;
        while ((match = stringRx.exec(codePart))) {
            pieces.push(highlightCodePlainText(codePart.slice(cursor, match.index), e, terms));
            const stringText = escapePreviewHtml(match[0]);
            pieces.push(`<span class="tok-str">${stringText}</span>`);
            cursor = match.index + match[0].length;
        }
        pieces.push(highlightCodePlainText(codePart.slice(cursor), e, terms));
        if (commentPart) pieces.push(`<span class="tok-com">${escapePreviewHtml(commentPart)}</span>`);
        const html = pieces.join('');
        return html || '&nbsp;';
    }

    function buildCodePreviewHtml(text: string, ext: string | null | undefined, matchLine: number | null, search: string): string {
        const terms = codePreviewSearchTerms(search);
        return text.replace(/\n+$/, '').split('\n').slice(0, READER_LINE_CAP).map((line, i) => {
            const n = i + 1;
            const isExactHit = matchLine === n;
            const hasSearchHit = !isExactHit && codeLineHasSearchTerm(line, terms);
            const classes = `cmd-code-line${isExactHit ? ' is-hit' : ''}${hasSearchHit ? ' has-hit' : ''}`;
            return `<div class="${classes}" data-line="${n}" role="row"><span class="cmd-code-ln" role="cell">${n}</span><span class="cmd-code-src" role="cell">${highlightCodeLine(line, ext, terms)}</span></div>`;
        }).join('');
    }

    let codePreviewEl = $state<HTMLDivElement | null>(null);

    let codePreviewHtml = $derived.by<string>(() => {
        const item = previewItem;
        if (!item || item.kind !== 'file' || !filePreviewText) return '';
        return buildCodePreviewHtml(filePreviewText, item.result.extension, previewMatchLine, query);
    });

    $effect(() => {
        const html = codePreviewHtml;
        const matchLine = previewMatchLine;
        const node = codePreviewEl;
        if (!html || !matchLine || !node || !previewOpen) return;
        void tick().then(() => {
            const hit = node.querySelector<HTMLElement>('.cmd-code-line.is-hit');
            if (!hit) return;
            const target = hit.offsetTop - node.clientHeight / 2 + hit.offsetHeight / 2;
            node.scrollTop = Math.max(0, target);
        });
    });

    function parseDelimitedText(text: string, delimiter: ',' | '\t'): string[][] {
        const rows: string[][] = [];
        let row: string[] = [];
        let cell = '';
        let quoted = false;
        for (let i = 0; i < text.length && rows.length < 120; i++) {
            const ch = text[i];
            const next = text[i + 1];
            if (ch === '"') {
                if (quoted && next === '"') { cell += '"'; i++; }
                else quoted = !quoted;
            } else if (ch === delimiter && !quoted) {
                row.push(cell.trim()); cell = '';
            } else if ((ch === '\n' || ch === '\r') && !quoted) {
                if (ch === '\r' && next === '\n') i++;
                row.push(cell.trim());
                rows.push(row.slice(0, 40));
                row = []; cell = '';
            } else {
                cell += ch;
            }
        }
        if (cell || row.length) {
            row.push(cell.trim());
            rows.push(row.slice(0, 40));
        }
        return rows.filter((r) => r.some((c) => c.length > 0));
    }

    function spreadsheetPreviewRows(text: string, ext: string | null | undefined): string[][] {
        const e = (ext || '').replace(/^\./, '').toLowerCase();
        if (e === 'tsv') return parseDelimitedText(text, '\t');
        if (e === 'csv') return parseDelimitedText(text, ',');

        const tabRows = parseDelimitedText(text, '\t');
        if (tabRows.some((r) => r.length > 1)) return tabRows;

        return text
            .replace(/\r\n/g, '\n')
            .split('\n')
            .slice(0, 120)
            .map((line) => line.split(/\s{2,}/).map((c) => c.trim()).filter(Boolean).slice(0, 40))
            .filter((r) => r.length > 0);
    }

    let sheetPreviewRows = $derived.by<string[][]>(() => {
        const item = previewItem;
        if (!item || item.kind !== 'file' || !filePreviewText) return [];
        if (!isSpreadsheetLikeDoc(item.result.extension)) return [];
        return spreadsheetPreviewRows(filePreviewText, item.result.extension);
    });

    /** Lightweight extension check — drives the reader's monospace
     *  rendering. Kept inline (no new dependency) so adding a new
     *  language is a one-line edit. */
    function isCodeLikeExtension(ext: string | null | undefined): boolean {
        if (!ext) return false;
        const e = ext.toLowerCase().replace(/^\./, '');
        return [
            'rs', 'ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs',
            'svelte', 'vue', 'astro',
            'py', 'go', 'java', 'kt', 'c', 'cpp', 'h', 'hpp',
            'cs', 'rb', 'php', 'swift', 'dart', 'lua', 'r',
            'json', 'jsonc', 'toml', 'yaml', 'yml', 'xml',
            'html', 'css', 'scss', 'sass', 'less',
            'sql', 'sh', 'bash', 'zsh', 'fish', 'ps1', 'bat', 'cmd',
            'env', 'ini', 'conf', 'cfg',
            'log', 'gitignore', 'dockerfile', 'makefile',
        ].includes(e);
    }
    let lastPreviewPath = '';

    $effect(() => {
        const item = previewItem;
        let targetPath = '';
        if (item && item.kind === 'file') {
            const f = item.result;
            if (
                f.entryType !== 'folder' &&
                !isImageFile(f.extension) &&
                !isAudioFile(f.extension) &&
                !isVideoFile(f.extension) &&
                !isPdfFile(f.extension)
            ) {
                targetPath = f.path;
            }
        }
        if (targetPath === lastPreviewPath) return;
        lastPreviewPath = targetPath;
        // The structured-Word view is keyed off the same path; reset it on
        // every selection change and re-fetch below only for Word-like docs.
        docxMarkdown = null;
        if (!targetPath) {
            filePreviewText = null;
            filePreviewLoading = false;
            return;
        }
        filePreviewText = null;
        filePreviewLoading = true;
        // Resolve the file's shell icon so the no-preview fallback (binary /
        // unsupported types) can show it instead of a blank pane.
        void fetchEntryIcon(targetPath, 'file');
        void invoke<string | null>('read_file_preview', { path: targetPath, maxBytes: 65536 })
            .then((text) => {
                if (lastPreviewPath !== targetPath) return; // selection moved on
                filePreviewText = text && text.trim() ? text : null;
            })
            .catch(() => {
                if (lastPreviewPath === targetPath) filePreviewText = null;
            })
            .finally(() => {
                if (lastPreviewPath === targetPath) filePreviewLoading = false;
            });

        // Elegant Word preview: for .docx the backend returns real Markdown
        // (headings/lists/tables); .doc/.rtf/.odt return null → flat-text
        // fallback. Best-effort — failure just leaves docxMarkdown null.
        if (item && item.kind === 'file' && isWordLikeDoc(item.result.extension)) {
            void invoke<string | null>('read_docx_markdown', { path: targetPath })
                .then((md) => {
                    if (lastPreviewPath !== targetPath) return;
                    docxMarkdown = md && md.trim() ? md : null;
                })
                .catch(() => {
                    if (lastPreviewPath === targetPath) docxMarkdown = null;
                });
        }
    });

    /** Lazy-load a folder's immediate children for the folder-content
     *  preview. Mirrors the file-preview effect but targets folders, which
     *  the effect above deliberately excludes. Best-effort: on failure the
     *  listing is simply empty. */
    let lastFolderPath = '';
    $effect(() => {
        const item = previewItem;
        let folderPath = '';
        if (item && item.kind === 'file' && item.result.entryType === 'folder') {
            folderPath = item.result.path;
        }
        if (folderPath === lastFolderPath) return;
        lastFolderPath = folderPath;
        if (!folderPath) {
            folderChildren = [];
            return;
        }
        folderChildren = [];
        void invoke<FolderChild[]>('list_folder_children', { path: folderPath })
            .then((children) => {
                if (lastFolderPath !== folderPath) return; // selection moved on
                folderChildren = children ?? [];
            })
            .catch(() => {
                if (lastFolderPath === folderPath) folderChildren = [];
            });
    });

    /** Bytes → "12 KB" / "3.4 MB". Local helper used by previews. */
    function formatPreviewBytes(bytes: number): string {
        if (bytes < 1024) return `${bytes} B`;
        if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
        return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    }

    /** Paste an entry via the backend. Same hide-then-paste pattern
     *  the existing clipboard-overlay uses — the backend's 30 ms
     *  delay catches a fully-released focus before SetForegroundWindow
     *  + SendInput run, so Ctrl+V lands in the previous app. */
    async function pasteClipboardEntry(entry: ClipboardEntry) {
        try {
            await hidePalette();
            await invoke('paste_clipboard_entry', {
                id: entry.id,
                autoPaste: $settings.clipboardAutoPaste !== false,
            });
        } catch (error) {
            console.warn('clipboard paste failed:', error);
            errorToast("Couldn't paste that clipboard entry", error, {
                hint: 'The target window may have closed or refused focus. The text is still on your clipboard — try Ctrl+V manually.',
            });
        }
    }

    /** Cleanup Wave 1.1 (2026-05-28): expand a snippet template (resolving
     *  variables like {{clipboard}}) and paste the result into the
     *  previously focused app.
     *
     *  HISTORY: an earlier version hid the palette BEFORE expanding the
     *  template; if the expansion threw, the error toast was invisible
     *  (the palette's ToastContainer is inside the hidden window). It
     *  also pasted an empty string into the previously focused app when
     *  the snippet template was empty — silent regression of "/sig
     *  doesn't paste". Order is now: 1) expand template, 2) guard for
     *  empty body, 3) hide palette, 4) invoke paste. So expansion errors
     *  surface in the still-visible palette and the user is never told
     *  the paste "succeeded" with nothing on the clipboard. */
    async function pasteSnippet(snippet: Snippet) {
        let expanded: string;
        try {
            // Resolve {{clipboard}} against the LIVE clipboard, read before
            // paste_snippet_text overwrites it. This used to pass null, so
            // {{clipboard}} silently expanded to nothing on every paste —
            // while snippet_expand.rs's comment claimed "it still works via
            // the overlay". It didn't; this is the path that makes that true.
            // Failure is non-fatal: an empty {{clipboard}} beats no paste.
            const clip = await invoke<string | null>('get_clipboard_text').catch(() => null);
            expanded = await previewSnippet(snippet.template, clip);
        } catch (error) {
            console.warn('snippet expansion failed:', snippet.trigger, error);
            errorToast(`Couldn't expand snippet /${snippet.trigger}`, error, {
                hint: 'The template may reference an unknown variable or failed to load. Open the Snippets tool to check.',
            });
            return;
        }
        if (!expanded.trim()) {
            // Empty template or expansion stripped everything — paste would
            // silently overwrite the clipboard with nothing. Tell the user.
            toast(
                `Snippet "/${snippet.trigger}" has no body — open the Snippets tool to add one.`,
                'info',
                4500,
            );
            return;
        }
        // Mirrors `pasteClipboardEntry`'s hide-first pattern from here on:
        // the backend's paste_snippet_text uses a 30ms internal delay
        // before SetForegroundWindow + SendInput, so the palette window
        // MUST be hidden by the time that timer elapses or Ctrl+V lands
        // on us instead of the user's app.
        try {
            await hidePalette();
            await invoke('paste_snippet_text', {
                text: expanded,
                autoPaste: $settings.clipboardAutoPaste !== false,
            });
            void recordSnippetUse(snippet.id);
        } catch (error) {
            // Palette is hidden by now — toast wouldn't reach the user.
            // Console-only; the backend writes to clipboard before the
            // injection attempt anyway, so the user can still Ctrl+V
            // manually into their app.
            console.warn('snippet paste injection failed:', snippet.trigger, error);
        }
    }
</script>

<svelte:window onkeydown={onKeydown} onpointerdown={onWindowPointerDown} />

<div
    class="cmd-root"
    class:is-overlay={asOverlay}
    class:is-window={isCommandWindow}
    class:has-desktop-blur={isCommandWindow && $commandAppearance.desktopBlur}
    class:has-accent-glow={$commandAppearance.accentGlow}
    style={cmdRootStyle}
    data-mode={mode}
    onclick={(e) => {
        // Scrim click (outside the panel) closes the overlay. Only in
        // overlay mode, and only when the click landed on the root
        // itself — not bubbled up from inside the panel.
        if (asOverlay && e.target === e.currentTarget) {
            onClose?.();
        }
    }}
    onkeydown={() => {}}
    role="presentation"
>
    <div class="cmd-panel">
        <!-- ─── Commands scope body (2026-06-13 redesign) ──────────────
             The Commands chip now shows EXACTLY 4 category rows. Selecting
             a row drives the master-detail preview pane (where the actual
             items live + are clicked) — it never dismisses the palette.
             Counts come straight from the loaded sources. -->
        <!-- ─── Emoji scope body (2026-07-29) ──────────────────────────
             Rendered by BOTH the empty-query state and the query-active
             body, so `emojiMatches` (and therefore `selectables`) drives
             one list and arrow-nav can never disagree with the screen.
             `emojiMatches` already returns recents-first when the query
             is empty, so no separate "recents" section is needed. -->
        {#snippet emojiBody()}
            <!-- Grid, not a list: emoji are tiny and visual, so a column of
                 1,500 one-per-row buttons shows ~8 at a time and reads like a
                 spreadsheet. The grid shows ~60. Name of the selected glyph
                 moves to the section header — repeating it per cell is what
                 forced the list shape in the first place. -->
            <div class="cmd-section">
                <div class="cmd-section-label">
                    <Smile class="cmd-section-label-ico" />
                    <span>{query.trim() ? 'EMOJI' : 'RECENT & ALL EMOJI'}</span>
                    <span class="cmd-section-count">{emojiMatches.length}</span>
                    {#if selectedEmojiName}
                        <span class="cmd-emoji-hint">{selectedEmojiName}</span>
                    {/if}
                </div>
                <div class="cmd-emoji-grid">
                    {#each emojiMatches as e (e.c)}
                        <button
                            type="button"
                            class="cmd-emoji-cell"
                            class:is-selected={selectables[selectedIndex]?.key ===
                                `emoji:${e.c}`}
                            data-cmd-key={`emoji:${e.c}`}
                            title={e.n}
                            aria-label={e.n}
                            onclick={() => void copyEmojiRow(e)}
                        >
                            <span aria-hidden="true">{e.c}</span>
                        </button>
                    {/each}
                </div>
            </div>
        {/snippet}
        {#snippet windowsRows()}
            <div class="cmd-section">
                <div class="cmd-section-label">
                    <AppWindow class="cmd-section-label-ico" />
                    <span>OPEN WINDOWS</span>
                    <span class="cmd-section-count">{windowMatches.length}</span>
                </div>
                {#each windowMatches as win (win.hwnd)}
                    <button
                        type="button"
                        class="cmd-row"
                        class:is-selected={selectables[selectedIndex]?.key === `window:${win.hwnd}`}
                        data-cmd-key={`window:${win.hwnd}`}
                        onclick={() => void focusWindowRow(win)}
                    >
                        <span class="cmd-row-icon" aria-hidden="true">
                            {#if win.exePath && entryIcons[win.exePath]}
                                <img
                                    src={entryIcons[win.exePath]}
                                    alt=""
                                    loading="lazy"
                                    class="cmd-row-icon-img"
                                />
                            {:else}
                                <AppWindow class="cmd-row-icon-svg" />
                            {/if}
                        </span>
                        <span class="cmd-row-text">
                            <span class="cmd-row-title">{win.title || win.app}</span>
                            <span class="cmd-row-sub">{win.app}</span>
                        </span>
                        {#if win.isForeground}
                            <span class="cmd-row-running" title="Currently in front">In front</span>
                        {/if}
                    </button>
                {/each}
            </div>
        {/snippet}
        {#snippet windowsBody()}
            {#if openWindowsLoading && openWindows.length === 0}
                <div class="cmd-empty is-loading">
                    <AppWindow class="cmd-empty-ico" />
                    <p class="cmd-empty-text">Loading open windows…</p>
                </div>
            {:else if windowMatches.length}
                {@render windowsRows()}
            {:else if openWindowsLoadError}
                <div class="cmd-empty">
                    <AppWindow class="cmd-empty-ico" />
                    <p class="cmd-empty-text">Couldn't read open windows.</p>
                    <p class="cmd-empty-sub">Try opening the Windows scope again.</p>
                </div>
            {:else}
                <div class="cmd-empty">
                    <AppWindow class="cmd-empty-ico" />
                    <p class="cmd-empty-text">No open windows found.</p>
                    <p class="cmd-empty-sub">Open an app, then return here to switch to it.</p>
                </div>
            {/if}
        {/snippet}
        {#snippet commandsBody()}
            <div class="cmd-section">
                <div class="cmd-section-label">
                    <TerminalSquare class="cmd-section-label-ico" />
                    <span>COMMANDS</span>
                </div>
                {#each COMMAND_CATEGORIES as cat (cat.id)}
                    {@const catKey = `cmd-cat:${cat.id}`}
                    {@const count =
                        cat.id === 'system-actions'
                            ? systemCommands.filter((c) => c.group === 'action').length
                            : cat.id === 'control-panel'
                              ? systemCommands.filter((c) => c.group === 'control-panel')
                                    .length
                              : cat.id === 'running-apps'
                                ? processGroups.length
                                : null}
                    <button
                        type="button"
                        class="cmd-row"
                        class:is-selected={selectables[selectedIndex]?.key === catKey}
                        data-cmd-key={catKey}
                        onclick={() => selectCommandCategory(catKey)}
                    >
                        <span class="cmd-row-icon" aria-hidden="true">
                            {#if cat.id === 'system-info'}
                                <Cpu class="cmd-row-icon-svg" />
                            {:else if cat.id === 'system-actions'}
                                <Power class="cmd-row-icon-svg" />
                            {:else if cat.id === 'control-panel'}
                                <Settings class="cmd-row-icon-svg" />
                            {:else}
                                <AppWindow class="cmd-row-icon-svg" />
                            {/if}
                        </span>
                        <span class="cmd-row-text">
                            <span class="cmd-row-title">{cat.label}</span>
                        </span>
                        {#if count !== null}
                            <span class="cmd-row-count">{count}</span>
                        {/if}
                        <ChevronRight class="cmd-row-chevron" />
                    </button>
                {/each}
            </div>
        {/snippet}

        <!-- ─── Command-category preview pane (2026-06-13 redesign) ─────
             Master-detail body for the selected Commands category. Lives
             in the preview pane (and is reused by the fullscreen surface).
             Rows are mouse-clickable; destructive actions still route
             through the shared confirm modal. All item lists are filtered
             by the typed query via `commandQueryMatches`. -->
        {#snippet commandCategoryPane(
            category: 'system-info' | 'system-actions' | 'control-panel' | 'running-apps',
        )}
            {#if category === 'system-info'}
                <header class="cmd-preview-head">
                    <span class="cmd-preview-kind">System info</span>
                </header>
                {#if systemInfo}
                    <div class="cmd-sysinfo-grid cmd-preview-sysinfo">
                        <div class="cmd-sysinfo-cell">
                            <span class="cmd-sysinfo-label">OS</span>
                            <span class="cmd-sysinfo-value">
                                {systemInfo.osName}
                                {systemInfo.osVersion}
                            </span>
                        </div>
                        <div class="cmd-sysinfo-cell">
                            <span class="cmd-sysinfo-label">CPU</span>
                            <span class="cmd-sysinfo-value">
                                {systemInfo.cpuModel} · {systemInfo.logicalCores} cores
                            </span>
                        </div>
                        <div class="cmd-sysinfo-cell">
                            <span class="cmd-sysinfo-label">Memory</span>
                            <span class="cmd-sysinfo-value">
                                {formatBytes(systemInfo.ramUsedBytes)} / {formatBytes(
                                    systemInfo.ramTotalBytes,
                                )}
                            </span>
                        </div>
                        {#if systemInfo.batteryPercent !== null}
                            <div class="cmd-sysinfo-cell">
                                <span class="cmd-sysinfo-label">Battery</span>
                                <span class="cmd-sysinfo-value">
                                    {systemInfo.batteryPercent}%
                                </span>
                            </div>
                        {/if}
                        <div class="cmd-sysinfo-cell">
                            <span class="cmd-sysinfo-label">Uptime</span>
                            <span class="cmd-sysinfo-value">
                                {formatUptime(systemInfo.uptimeSecs)}
                            </span>
                        </div>
                        {#each systemInfo.disks as disk (disk.mount)}
                            <div class="cmd-sysinfo-cell">
                                <span class="cmd-sysinfo-label">{disk.mount}</span>
                                <span class="cmd-sysinfo-value">
                                    {formatBytes(disk.freeBytes)} free of {formatBytes(
                                        disk.totalBytes,
                                    )}
                                </span>
                            </div>
                        {/each}
                    </div>
                {:else}
                    <p class="cmd-preview-desc">Loading…</p>
                {/if}
            {:else if category === 'system-actions'}
                {@const actions = systemCommands.filter(
                    (c) =>
                        c.group === 'action' &&
                        commandQueryMatches(`${c.name} ${c.description}`),
                )}
                <header class="cmd-preview-head">
                    <span class="cmd-preview-kind">System actions</span>
                </header>
                {#if actions.length}
                    <div class="cmd-cat-rows">
                        {#each actions as item, i (item.id)}
                            <button
                                type="button"
                                class="cmd-row cmd-row-system"
                                class:is-destructive={item.requiresConfirmation}
                                class:is-selected={commandItemsFocused &&
                                    i === commandItemIndex}
                                data-cmd-item={i}
                                onclick={() => activateSystemAction(item)}
                            >
                                <span class="cmd-row-icon" aria-hidden="true">
                                    {#if item.id === 'shutdown' || item.id === 'restart' || item.id === 'signout'}
                                        <Power class="cmd-row-icon-svg" />
                                    {:else if item.id === 'cmd' || item.id === 'terminal' || item.id === 'powershell'}
                                        <Terminal class="cmd-row-icon-svg" />
                                    {:else if item.id === 'settings' || item.id === 'control'}
                                        <Settings class="cmd-row-icon-svg" />
                                    {:else if item.id === 'lock'}
                                        <Lock class="cmd-row-icon-svg" />
                                    {:else}
                                        <AppWindow class="cmd-row-icon-svg" />
                                    {/if}
                                </span>
                                <span class="cmd-row-text">
                                    <span class="cmd-row-title">{item.name}</span>
                                    <span class="cmd-row-sub">
                                        {item.description}{item.requiresConfirmation
                                            ? ' · Will confirm first'
                                            : ''}
                                    </span>
                                </span>
                            </button>
                        {/each}
                    </div>
                {:else}
                    <p class="cmd-preview-desc">No matching actions.</p>
                {/if}
            {:else if category === 'control-panel'}
                {@const applets = systemCommands.filter(
                    (c) =>
                        c.group === 'control-panel' &&
                        commandQueryMatches(`${c.name} ${c.description}`),
                )}
                <header class="cmd-preview-head">
                    <span class="cmd-preview-kind">Control Panel</span>
                </header>
                {#if applets.length}
                    <div class="cmd-cat-rows">
                        {#each applets as item, i (item.id)}
                            <button
                                type="button"
                                class="cmd-row cmd-row-system"
                                class:is-destructive={item.requiresConfirmation}
                                class:is-selected={commandItemsFocused &&
                                    i === commandItemIndex}
                                data-cmd-item={i}
                                onclick={() => activateSystemAction(item)}
                            >
                                <span class="cmd-row-icon" aria-hidden="true">
                                    {#if item.id === 'settings' || item.id === 'control'}
                                        <Settings class="cmd-row-icon-svg" />
                                    {:else}
                                        <Wrench class="cmd-row-icon-svg" />
                                    {/if}
                                </span>
                                <span class="cmd-row-text">
                                    <span class="cmd-row-title">{item.name}</span>
                                    <span class="cmd-row-sub">
                                        {item.description}{item.requiresConfirmation
                                            ? ' · Will confirm first'
                                            : ''}
                                    </span>
                                </span>
                            </button>
                        {/each}
                    </div>
                {:else}
                    <p class="cmd-preview-desc">No matching applets.</p>
                {/if}
            {:else}
                {@const apps = processGroups.filter((g) =>
                    commandQueryMatches(g.name),
                )}
                <header class="cmd-preview-head">
                    <span class="cmd-preview-kind">Running apps</span>
                </header>
                {#if apps.length}
                    <div class="cmd-cat-rows">
                        {#each apps as group, i (group.exePath ?? group.name)}
                            <button
                                type="button"
                                class="cmd-row"
                                class:is-selected={commandItemsFocused &&
                                    i === commandItemIndex}
                                data-cmd-item={i}
                                onclick={() => openKillConfirm(group)}
                            >
                                <span class="cmd-row-icon" aria-hidden="true">
                                    {#if group.exePath && entryIcons[group.exePath]}
                                        <img
                                            src={entryIcons[group.exePath]}
                                            alt=""
                                            loading="lazy"
                                            class="cmd-row-icon-img"
                                        />
                                    {:else}
                                        <AppWindow class="cmd-row-icon-svg" />
                                    {/if}
                                </span>
                                <span class="cmd-row-text">
                                    <span class="cmd-row-title">{group.name}</span>
                                    <span class="cmd-row-sub">
                                        {group.windowCount} window{group.windowCount === 1
                                            ? ''
                                            : 's'} · {formatBytes(group.memoryBytes)}
                                    </span>
                                </span>
                            </button>
                        {/each}
                    </div>
                {:else}
                    <p class="cmd-preview-desc">No running apps match.</p>
                {/if}
            {/if}
        {/snippet}

        <!-- ─── Shared file-preview content (2026-05-31 preview overhaul) ──
             One snippet renders the previewable content for a file so the
             INLINE preview pane and the FULLSCREEN overlay stay identical.
             Sizing is driven by the ancestor `.cmd-stage-host` (flex column,
             fills available height) — same markup, two contexts. Video is
             optimistic (attempt playback for every format incl. AVI; fall
             back to a card on decode error). The image is click-to-fullscreen. -->
        {#snippet fileStage(f: FileSearchResultItem, name: string | undefined)}
            {#if f.entryType === 'folder'}
                <!-- Folder-content listing (2026-06-13). Immediate children
                     from `list_folder_children` (dirs-first, capped at 250).
                     Dirs use the Folder glyph, files FileText + size. -->
                <div class="cmd-folder-list">
                    <div class="cmd-folder-list-head">
                        {#if folderChildren.length === 0}
                            Empty folder
                        {:else}
                            {folderChildren.length.toLocaleString('en-US')} item{folderChildren.length === 1 ? '' : 's'}
                        {/if}
                    </div>
                    {#each folderChildren as child}
                        <div class="cmd-folder-row">
                            <span class="cmd-folder-row-ico" aria-hidden="true">
                                {#if child.isDir}
                                    <Folder class="cmd-folder-row-ico-svg" />
                                {:else}
                                    <FileText class="cmd-folder-row-ico-svg" />
                                {/if}
                            </span>
                            <span class="cmd-folder-row-name">{child.name}</span>
                            {#if !child.isDir}
                                <span class="cmd-folder-row-size">{formatBytes(child.size)}</span>
                            {/if}
                        </div>
                    {/each}
                </div>
            {:else if f.entryType !== 'folder' && isImageFile(f.extension)}
                <button
                    type="button"
                    class="cmd-preview-image-wrap"
                    onclick={togglePreviewFullscreen}
                    title={previewFullscreen ? 'Exit fullscreen (Esc)' : 'Fullscreen (Esc to exit)'}
                >
                    <img
                        src={convertFileSrc(f.path, 'kilmedia')}
                        alt={name}
                        class="cmd-preview-image"
                        draggable="false"
                    />
                </button>
            {:else if f.entryType !== 'folder' && isAudioFile(f.extension)}
                <div class="cmd-preview-media-wrap">
                    <!-- svelte-ignore a11y_media_has_caption -->
                    <audio
                        class="cmd-preview-audio"
                        src={convertFileSrc(f.path, 'kilmedia')}
                        controls
                        controlsList="nodownload noplaybackrate"
                        preload="metadata"
                        oncontextmenu={(e) => e.preventDefault()}
                    ></audio>
                </div>
            {:else if f.entryType !== 'folder' && isVideoFile(f.extension)}
                {#if !videoError}
                    <div class="cmd-preview-media-wrap cmd-preview-video-wrap">
                        <!-- svelte-ignore a11y_media_has_caption -->
                        <!-- Optimistic: play any format WebView2 can decode
                             (incl. some AVI/MKV/MOV). `onerror` → graceful
                             card for codecs it can't. -->
                        <video
                            class="cmd-preview-video"
                            src={convertFileSrc(f.path, 'kilmedia')}
                            controls
                            controlsList="nodownload noplaybackrate noremoteplayback"
                            preload="metadata"
                            disablePictureInPicture
                            oncontextmenu={(e) => e.preventDefault()}
                            onerror={() => (videoError = true)}
                        ></video>
                    </div>
                {:else}
                    <div class="cmd-preview-reader cmd-preview-reader-dim">
                        <span>{f.extension.replace('.', '').toUpperCase()} video — this codec can't be decoded in-app. Press Enter to open it in your default player.</span>
                    </div>
                {/if}
            {:else if f.entryType !== 'folder' && isPdfFile(f.extension)}
                {#if f.size > 100 * 1024 * 1024}
                    <!-- V2: large PDFs (scanned docs can be 100s of MB) stall the
                         inline viewer — skip the render and offer to open. -->
                    <div class="cmd-preview-reader cmd-preview-reader-dim">
                        <span>Large PDF ({formatBytes(f.size)}) — press Enter to open it in your default viewer.</span>
                    </div>
                {:else}
                    <!-- Real PDF render via WebView2's built-in viewer. asset:
                         origin allowed in frame-src (tauri.conf.json CSP). -->
                    <div class="cmd-preview-pdf-wrap">
                        <iframe
                            class="cmd-preview-pdf"
                            title={name}
                            src={convertFileSrc(f.path, 'kilmedia') + '#toolbar=0&navpanes=0&view=FitH'}
                        ></iframe>
                    </div>
                {/if}
            {:else if filePreviewLoading}
                <div class="cmd-preview-reader cmd-preview-reader-dim">
                    <span class="cmd-preview-reader-spinner" aria-hidden="true"></span>
                    <span>Loading preview…</span>
                </div>
            {:else if notePreview}
                <!-- Polished `.ki` note view (Wave J). `{@html}` is the
                     sanitizer's output — see renderNoteHtml's contract. -->
                <div class="cmd-note-preview">
                    <div class="cmd-note-meta-row">
                        {#if notePreview.pinned}
                            <span class="cmd-note-pinned" title="Pinned">
                                <Pin class="cmd-note-pinned-ico" /> Pinned
                            </span>
                        {/if}
                        {#if notePreview.updatedAt}
                            <span class="cmd-note-meta-chip">
                                Updated {formatDateTime(notePreview.updatedAt)}
                            </span>
                        {:else if notePreview.createdAt}
                            <span class="cmd-note-meta-chip">
                                Created {formatDateTime(notePreview.createdAt)}
                            </span>
                        {/if}
                        {#if notePreview.tags.length > 0}
                            <span class="cmd-note-tags">
                                {#each notePreview.tags as tag}
                                    <span class="cmd-note-tag">#{tag}</span>
                                {/each}
                            </span>
                        {/if}
                    </div>
                    <h4 class="cmd-note-title">{notePreview.title}</h4>
                    {#if noteBodyHtml}
                        <div class="cmd-note-body">
                            {@html noteBodyHtml}
                        </div>
                    {:else if notePreview.bodyMarkdown.trim()}
                        <pre class="cmd-note-body-fallback">{notePreview.bodyMarkdown}</pre>
                    {/if}
                </div>
            {:else if filePreviewText}
                {@const isCode = isCodeLikeExtension(f.extension) && !isMarkdownFile(f.extension)}
                {@const isMarkdownPreview = isMarkdownFile(f.extension)}
                {@const isExtractedDoc = isExtractOnlyDoc(f.extension)}
                {@const isWordDoc = isWordLikeDoc(f.extension)}
                {@const isSheetDoc = isSpreadsheetLikeDoc(f.extension)}
                {@const isDeckDoc = isPresentationLikeDoc(f.extension)}
                {@const isPlainTextPreview = !isCode && !isExtractedDoc && !isMarkdownPreview}
                {@const wordParagraphs = isWordDoc ? documentParagraphs(filePreviewText) : []}
                {@const firstWordHitIndex = isWordDoc ? firstPreviewHitIndex(wordParagraphs) : -1}
                {@const useLineReader =
                    readerLines.length > 0 &&
                    readerLines.length <= READER_LINE_CAP &&
                    (isCode || previewMatchLine !== null)}
                {#if isExtractedDoc && !isWordDoc && !isSheetDoc && !isDeckDoc}
                    <div class="cmd-preview-reader-note">
                        Text preview · {previewLanguageLabel(f.extension)} — extracted content only. Press Enter to open the original file.
                    </div>
                {/if}
                <div class="cmd-preview-reader-toolbar">
                    <div class="cmd-preview-reader-stats" aria-label="Preview statistics">
                        <span>{previewLanguageLabel(f.extension)}</span>
                        {#if isWordDoc}
                            <span>{wordParagraphs.length.toLocaleString('en-US')} paragraphs</span>
                        {:else}
                            <span>{readerLines.length.toLocaleString('en-US')} lines</span>
                        {/if}
                        <span>{filePreviewText.length.toLocaleString('en-US')} chars</span>
                    </div>
                    <div class="cmd-preview-reader-controls">
                        {#if !isMarkdownPreview && !isWordDoc && !isSheetDoc}
                            <button
                                type="button"
                                class="cmd-preview-reader-btn"
                                onclick={togglePreviewWrap}
                                title={previewWrap ? 'Disable line wrap' : 'Enable line wrap'}
                            >
                                {previewWrap ? 'Wrap: ON' : 'Wrap: OFF'}
                            </button>
                        {/if}
                        <button
                            type="button"
                            class="cmd-preview-reader-btn"
                            onclick={() => void copyPreviewText()}
                            title="Copy file content to clipboard"
                        >
                            Copy
                        </button>
                    </div>
                </div>
                {#if isMarkdownPreview}
                    <article class="cmd-markdown-preview cmd-document-page">
                        {@html markdownPreviewHtml}
                    </article>
                {:else if isCode}
                    <div class="cmd-code-preview" class:cmd-code-nowrap={!previewWrap} bind:this={codePreviewEl} role="table" aria-label="Code preview">{@html codePreviewHtml}</div>
                {:else if isWordDoc && docxMarkdown}
                    <!-- .docx → real structured Markdown, rendered EXACTLY like
                         the .md branch (same renderer + same .cmd-markdown-preview
                         / .cmd-document-page styling) so it reads like a clean
                         document instead of a gray paper card. Inline images
                         aren't extracted (preview only) → image refs may be
                         blank, which is acceptable here. -->
                    <article class="cmd-markdown-preview cmd-document-page">
                        {@html renderNoteHtml(docxMarkdown, f.path)}
                    </article>
                {:else if isWordDoc}
                    <!-- .doc / .rtf / .odt (no structured Markdown converter) keep
                         the flat paragraph fallback in the paper-card surface. -->
                    <article class="cmd-word-preview">
                        <div class="cmd-word-page">
                            <div class="cmd-word-page-topline"></div>
                            {#each wordParagraphs as paragraph, i}
                                {@const paragraphHit = previewTextHasTerm(paragraph)}
                                {@const activeParagraphHit = i === firstWordHitIndex}
                                {#if i === 0 && paragraph.length < 120}
                                    <h1 class:is-doc-hit={paragraphHit} use:scrollHit={activeParagraphHit}>{@html highlightPreviewTerms(paragraph)}</h1>
                                {:else}
                                    <p class:is-doc-hit={paragraphHit} use:scrollHit={activeParagraphHit}>{@html highlightPreviewTerms(paragraph)}</p>
                                {/if}
                            {/each}
                        </div>
                    </article>
                {:else if isSheetDoc}
                    <div class="cmd-sheet-preview">
                        <div class="cmd-sheet-grid-wrap">
                            <table class="cmd-sheet-grid">
                                <tbody>
                                    {#each sheetPreviewRows as row, rowIndex}
                                        {@const rowHit = row.some((cell) => previewTextHasTerm(cell))}
                                        <tr class:is-sheet-hit={rowHit} use:scrollHit={rowHit}>
                                            <th class="cmd-sheet-rowhead">{rowIndex + 1}</th>
                                            {#each row as cell}
                                                <td>{@html highlightPreviewTerms(cell)}</td>
                                            {/each}
                                        </tr>
                                    {/each}
                                </tbody>
                            </table>
                        </div>
                    </div>
                {:else if isDeckDoc}
                    <pre class="cmd-preview-reader cmd-deck-preview" class:cmd-preview-reader-nowrap={!previewWrap}>{filePreviewText}</pre>
                {:else if useLineReader}
                    <div
                        class="cmd-preview-reader cmd-preview-reader-lines cmd-preview-reader-gutter"
                        class:cmd-preview-reader-mono={isCode}
                        class:cmd-preview-reader-paper={isPlainTextPreview}
                        class:cmd-preview-reader-nowrap={!previewWrap}
                    >
                        {#each readerLines as line, i}
                            <div
                                class="cmd-rln"
                                class:cmd-rln-hit={previewMatchLine === i + 1}
                                use:scrollHit={previewMatchLine === i + 1}
                            ><span class="cmd-rln-no">{i + 1}</span><span class="cmd-rln-tx">{@html highlightPreviewTerms(line || ' ')}</span></div>
                        {/each}
                    </div>
                {:else}
                    <pre
                        class="cmd-preview-reader"
                        class:cmd-preview-reader-mono={isCode}
                        class:cmd-preview-reader-paper={isPlainTextPreview}
                        class:cmd-preview-reader-nowrap={!previewWrap}
                    >{filePreviewText}</pre>
                {/if}
            {:else}
                <!-- V2: a file type we can't preview inline (binary / unsupported)
                     — show its shell icon so the pane isn't blank. The icon is
                     resolved by fetchEntryIcon in the preview effect. -->
                <div class="cmd-preview-noprev">
                    {#if entryIcons[f.path]}
                        <img
                            class="cmd-preview-noprev-icon"
                            src={entryIcons[f.path]}
                            alt={f.extension || 'file'}
                            draggable="false"
                        />
                    {:else}
                        <FileText class="cmd-preview-noprev-glyph" />
                    {/if}
                    <span class="cmd-preview-noprev-label">
                        {f.extension ? `${f.extension.replace('.', '').toUpperCase()} file` : 'File'} · press Enter to open
                    </span>
                </div>
            {/if}
        {/snippet}

        <!-- The preview and Ctrl+Space panel intentionally share this action
             source. One clear default verb stays visible; the existing panel
             retains every secondary action. -->
        {#snippet previewActionBar()}
            {#if primarySelectedAction}
                {@const PrimaryActionIcon = primarySelectedAction.icon}
                <div class="cmd-preview-actions">
                    <button
                        type="button"
                        class="cmd-preview-btn cmd-preview-btn-primary"
                        onclick={() => void primarySelectedAction.activate()}
                    >
                        <PrimaryActionIcon class="cmd-preview-btn-ico" />
                        {primarySelectedAction.label}
                    </button>
                    {#if availableActions.length > 1}
                        <button
                            type="button"
                            class="cmd-preview-btn"
                            onclick={openActionsPanel}
                        >
                            <ChevronRight class="cmd-preview-btn-ico" />
                            Actions
                        </button>
                    {/if}
                </div>
            {/if}
        {/snippet}

        <!-- ─── Top bar ─────────────────────────────────────────────
             Mode-aware chrome:
               - default: leading search icon
               - clipboard / voice: back arrow + mode pill
               - voice: live transcript shows in the input
               - mic button always present on the right (passive in
                 default mode, active when voice is engaged)
        -->
        <header class="cmd-top">
            {#if mode === 'default'}
                <Search class="cmd-top-leading-ico" />
            {:else}
                <button
                    type="button"
                    class="cmd-back"
                    onclick={goBackToDefault}
                    title="Back to search"
                    aria-label="Back to search"
                >
                    <ArrowLeft class="cmd-back-ico" />
                </button>
                <span class="cmd-mode-pill" data-mode={mode}>
                    {mode === 'clipboard' ? 'Clipboard' : 'Voice'}
                </span>
            {/if}

            <input
                bind:this={inputEl}
                value={query}
                oninput={onQueryInput}
                placeholder={mode === 'default'
                    ? "Search apps, files, math, conversions… (try '50 mi to km')"
                    : mode === 'clipboard'
                      ? 'Search clipboard…'
                      : voiceSubMode === 'transcribe'
                        ? 'Listening… speak your search'
                        : voiceSubMode === 'dictate'
                          ? 'Listening… speak the text to paste'
                          : 'Say a command…'}
                class="cmd-input"
                autocomplete="off"
                spellcheck="false"
                aria-label="Search KeepItLocal"
            />

            <span
                id="cmd-selection-status"
                class="cmd-sr-only"
                role="status"
                aria-live="polite"
                aria-atomic="true"
            >{selectionAnnouncement}</span>

            {#if query && mode === 'default'}
                <button
                    type="button"
                    class="cmd-input-clear"
                    onclick={() => {
                        query = '';
                        inputEl?.focus();
                    }}
                    title="Clear"
                    aria-label="Clear"
                >
                    <X class="cmd-input-clear-ico" />
                </button>
            {/if}

            <!-- Preview toggle — Eye icon, accent-tinted when on. Opens
                 a right-side preview pane (macOS QuickLook-style). In clipboard
                 mode it shows the selected entry's full content; in default/
                 search mode it shows a rich file preview — scrollable text /
                 PDF / Office content, image thumbs, and an in-palette audio/
                 video player. Voice has its own visualization, so the button
                 is hidden there. Keyboard shortcut: Ctrl+P (Cmd+P on macOS). -->
            {#if mode === 'clipboard' || mode === 'default'}
                <button
                    type="button"
                    class="cmd-icon-btn"
                    class:is-active={previewOpen}
                    onclick={togglePreview}
                    title={previewOpen ? 'Hide preview (Ctrl+P)' : 'Show preview (Ctrl+P)'}
                    aria-label="Toggle preview"
                    aria-pressed={previewOpen}
                >
                    {#if previewOpen}
                        <EyeOff class="cmd-icon-btn-ico" />
                    {:else}
                        <Eye class="cmd-icon-btn-ico" />
                    {/if}
                </button>
            {/if}

            <!-- Keyboard shortcuts (Ctrl+Alt+I). The footer only advertises
                 two hints by design, so this is the discoverable way in to
                 the full set. Syntax (what you can TYPE) stays on Ctrl+/ —
                 deliberately a different surface from bindings. -->
            <button
                type="button"
                class="cmd-icon-btn"
                class:is-active={shortcutsOpen}
                onclick={toggleShortcutsPanel}
                title={shortcutsOpen
                    ? 'Hide keyboard shortcuts (Ctrl+Alt+I)'
                    : 'Keyboard shortcuts (Ctrl+Alt+I)'}
                aria-label="Keyboard shortcuts"
                aria-pressed={shortcutsOpen}
            >
                <Keyboard class="cmd-icon-btn-ico" />
            </button>

            <!-- "Open results in main window" moved to the Ctrl+Space
                 action panel (see availableActions). -->
        </header>

        <!-- ─── Scope navigation ───────────────────────────────────
             Everyday destinations stay visible. Specialized scopes
             remain one click away under More, rather than forcing every
             capability to compete for top-level attention. -->
        {#if !cheatsheetOpen && $commandAppearance.showChips}
            <div class="cmd-chips" role="group" aria-label="Palette scope">
                <div class="cmd-scope-tabs" role="group" aria-label="Primary palette scope">
                    {#each primaryScopeChips as chip (chip.id)}
                        <button
                            type="button"
                            class="cmd-chip"
                            class:is-on={paletteScope === chip.id}
                            aria-pressed={paletteScope === chip.id}
                            onclick={() => choosePaletteScope(chip.id)}
                        >
                            {chip.label}
                        </button>
                    {/each}
                </div>
                <div
                    class="cmd-more-scopes"
                    bind:this={moreScopesEl}
                    onfocusout={onMoreScopesFocusOut}
                >
                    <button
                        type="button"
                        class="cmd-chip cmd-chip-more"
                        class:is-on={activeMoreScope !== null}
                        class:is-open={moreScopesOpen}
                        aria-haspopup="menu"
                        aria-expanded={moreScopesOpen}
                        aria-controls="cmd-more-scope-menu"
                        aria-label={activeMoreScope
                            ? `More scopes, ${activeMoreScope.label} selected`
                            : 'More scopes'}
                        title={activeMoreScope
                            ? `More scopes — ${activeMoreScope.label} selected`
                            : 'More scopes'}
                        onclick={toggleMoreScopes}
                    >
                        <span>{activeMoreScope ? `More · ${activeMoreScope.label}` : 'More'}</span>
                        <ChevronDown class="cmd-chip-more-ico" aria-hidden="true" />
                    </button>
                    {#if moreScopesOpen}
                        <div id="cmd-more-scope-menu" class="cmd-more-scope-menu" role="menu" aria-label="More palette scopes">
                            {#each moreScopeChips as chip, index (chip.id)}
                                <button
                                    type="button"
                                    class="cmd-more-scope-option"
                                    class:is-on={paletteScope === chip.id}
                                    data-more-scope-index={index}
                                    role="menuitemradio"
                                    tabindex={index === moreScopeIndex ? 0 : -1}
                                    aria-checked={paletteScope === chip.id}
                                    onclick={() => choosePaletteScope(chip.id)}
                                >
                                    {chip.label}
                                </button>
                            {/each}
                        </div>
                    {/if}
                </div>
            </div>
        {/if}

        <!-- 2026-05-27 polish (Model C): "Showing only X matching 'Y' ·
             Esc to broaden" hint that fires whenever the user types a
             query while a non-All chip is active. Keeps the
             narrowed-search affordance for power users while making
             the restriction visible to anyone who'd otherwise be
             confused by "where are my results?" -->
        {#if !cheatsheetOpen && paletteScope !== 'all' && query.trim() && mode === 'default'}
            {@const activeChip = SCOPE_CHIPS.find((c) => c.id === paletteScope)}
            <div class="cmd-scope-hint" role="status" aria-live="polite">
                <span class="cmd-scope-hint-text">
                    Showing only <strong>{activeChip?.label ?? paletteScope}</strong>
                    matching <span class="cmd-scope-hint-q">"{query.trim()}"</span>
                </span>
                <button
                    type="button"
                    class="cmd-scope-hint-broaden"
                    onclick={() => setPaletteScope('all')}
                    title="Search across every category"
                >
                    <kbd>Esc</kbd> to broaden
                </button>
            </div>
        {/if}

        <!-- ─── Meta strip — Files / Inside toggle + search timing ──
             Only mounted in default mode while a query is active.
             Stays out of the way during empty-state intro, clipboard
             mode, and voice mode (those modes don't have content vs
             filename axis). -->
        {#if mode === 'default' && query.trim()}
            <div class="cmd-meta">
                <!-- 2026-05-27 polish: Files / Inside picker only makes
                     sense for scopes that have a file-vs-content axis.
                     Tools / Apps / Clipboard / Voice all do their own
                     ad-hoc search and don't read this toggle, so
                     showing it under those chips is just visual noise.
                     The right-side timing chip stays visible across
                     all scopes — it's useful for any search. -->
                {#if paletteScope === 'all' || paletteScope === 'files'}
                    <span class="cmd-meta-modes" role="group" aria-label="Search scope">
                        <button
                            type="button"
                            class="cmd-meta-mode"
                            class:is-on={searchMode === 'files'}
                            aria-pressed={searchMode === 'files'}
                            onclick={() => setSearchMode('files')}
                        >
                            Files
                        </button>
                        <button
                            type="button"
                            class="cmd-meta-mode"
                            class:is-on={searchMode === 'content'}
                            aria-pressed={searchMode === 'content'}
                            onclick={() => setSearchMode('content')}
                        >
                            Inside
                        </button>
                    </span>
                {:else}
                    <!-- Placeholder span keeps justify-content:
                         space-between from collapsing the right side
                         (timing chip) flush left when the picker is
                         hidden. -->
                    <span></span>
                {/if}
                <span class="cmd-meta-right">
                    {#if (searching || liveGrepBusy) && (fileResults.length > 0 || contentResults.length > 0 || liveGrepHits.length > 0 || dedupedLaunchResults.length > 0)}
                        <!-- Soft "still working" cue when a new search is
                             in flight while previous results are visible.
                             Wave 3.3.3 (2026-05-27): also fires when the
                             ripgrep fallback is mid-walk — from the user's
                             POV "the app is searching", regardless of which
                             engine is doing the work. Doesn't replace the
                             timing chip — that stays until new numbers land,
                             communicating "the count you're looking at is
                             about to update". -->
                        <span class="cmd-meta-updating" aria-live="polite">
                            <span class="cmd-meta-spinner" aria-hidden="true"></span>
                            Updating…
                        </span>
                    {/if}
                    {#if queryMs !== null}
                        <span class="cmd-meta-time" aria-live="polite">
                            Found in {queryMs} ms
                        </span>
                    {/if}
                </span>
            </div>
        {/if}

        <!-- ─── Body + optional preview pane ─────────────────────
             When preview is on, the body splits into the list on
             the left and a 320 px preview pane on the right. Each
             half scrolls independently.

             The body content is wrapped in `{#key mode}` so a
             mode switch (default ↔ clipboard ↔ voice) triggers a
             clean unmount + remount → the entrance fade-in CSS
             plays once per mode change, giving the transition a
             macOS-like settle without flickering during in-mode
             updates (search re-runs, etc.). -->
        <div class="cmd-body-wrap" class:has-preview={previewOpen}>
        <!-- contextmenu is a mouse-only enhancement (right-click → action
             panel); the accessible path is the row buttons + Ctrl+Space,
             so the static-element interaction lint doesn't apply here. -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
            class="cmd-body"
            bind:this={scrollEl}
            onscroll={onBodyScroll}
            oncontextmenu={onRowContextMenu}
            onfocusin={onBodyFocusIn}
        >
        {#key mode}
        <div class="cmd-body-content">
            <!-- Wave 4.1b-3 (2026-05-27): shell command streaming
                 panel. Pre-empts the rest of the body so the output of
                 the just-launched command is the only thing on screen.
                 Mounted above the mode switch so it shows regardless of
                 default / clipboard / voice mode. Dismiss → null, then
                 the body returns to its normal content. -->
            {#if shellExec}
                <div
                    class="cmd-shell"
                    class:is-running={shellExec.running}
                    class:is-error={!shellExec.running &&
                        !shellExec.cancelled &&
                        (shellExec.exitCode !== 0 || shellExec.hadStderr)}
                    class:is-cancelled={shellExec.cancelled}
                >
                    <header class="cmd-shell-head">
                        <span class="cmd-shell-head-status" aria-hidden="true">
                            {#if shellExec.running}
                                <span class="cmd-shell-spinner"></span>
                            {:else if shellExec.cancelled}
                                <X class="cmd-shell-icon" />
                            {:else if shellExec.exitCode === 0 && !shellExec.hadStderr}
                                <Sparkles class="cmd-shell-icon" />
                            {:else}
                                <AlertTriangle class="cmd-shell-icon" />
                            {/if}
                        </span>
                        <span class="cmd-shell-head-text">
                            <span class="cmd-shell-title">
                                {shellExec.label}
                                <span class="cmd-shell-kind">{shellExec.shellKind}</span>
                            </span>
                            <span class="cmd-shell-sub">
                                {#if shellExec.running}
                                    Running… {shellExec.lines.length} line{shellExec.lines.length === 1 ? '' : 's'}
                                {:else if shellExec.cancelled}
                                    Cancelled · {shellExec.lines.length} line{shellExec.lines.length === 1 ? '' : 's'}{shellExec.durationMs != null ? ` · ${shellExec.durationMs} ms` : ''}
                                {:else}
                                    Exit {shellExec.exitCode ?? '?'} · {shellExec.lines.length} line{shellExec.lines.length === 1 ? '' : 's'}{shellExec.durationMs != null ? ` · ${shellExec.durationMs} ms` : ''}
                                {/if}
                            </span>
                        </span>
                        <div class="cmd-shell-actions">
                            {#if shellExec.running}
                                <button
                                    type="button"
                                    class="cmd-shell-btn cmd-shell-btn-cancel"
                                    onclick={() => void cancelShellExec()}
                                    title="Stop the command (kills the subprocess)"
                                >
                                    Cancel
                                </button>
                            {/if}
                            <button
                                type="button"
                                class="cmd-shell-btn"
                                onclick={dismissShellExec}
                                title={shellExec.running ? 'Hide panel (command keeps running)' : 'Close panel'}
                            >
                                {shellExec.running ? 'Hide' : 'Close'}
                            </button>
                        </div>
                    </header>
                    {#if shellExec.lines.length > 0}
                        <pre class="cmd-shell-out">{#each shellExec.lines as ln, i (i)}<span class="cmd-shell-line" class:is-err={ln.stream === 'stderr'}>{ln.line || ' '}</span>
{/each}</pre>
                    {:else if shellExec.running}
                        <p class="cmd-shell-empty">Waiting for output…</p>
                    {:else}
                        <p class="cmd-shell-empty">No output.</p>
                    {/if}
                </div>
            {/if}

            {#if mode === 'default' && cheatsheetOpen}
                <!-- Syntax cheatsheet replaces the regular body when
                     open. Toggled via the HelpCircle button or Ctrl+/.
                     Acts as an in-palette reference so users discover
                     supported patterns without leaving the surface. -->
                <div class="cmd-cheatsheet">
                    <header class="cmd-cheatsheet-head">
                        <h2 class="cmd-cheatsheet-title">Search syntax</h2>
                        <p class="cmd-cheatsheet-sub">
                            Everything you can type into the palette. Press
                            <kbd>Ctrl+/</kbd> again to close.
                        </p>
                    </header>

                    <section class="cmd-cheatsheet-group">
                        <h3 class="cmd-cheatsheet-gtitle">Natural language</h3>
                        <div class="cmd-cheatsheet-row">
                            <code>recent invoices pdf</code><span>Words match across filename, path, and extension.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>annual report</code><span>Multiple words filter to results containing each.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>screenshot from yesterday</code><span>Free-form queries — stopwords are auto-stripped.</span>
                        </div>
                    </section>

                    <section class="cmd-cheatsheet-group">
                        <h3 class="cmd-cheatsheet-gtitle">Phrases &amp; boolean</h3>
                        <div class="cmd-cheatsheet-row">
                            <code>"q3 budget"</code><span>Exact phrase match (use double quotes).</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>tax AND 2025</code><span>Both terms must appear.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>invoice OR receipt</code><span>Either term matches.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>budget -draft</code><span>Exclude results containing "draft".</span>
                        </div>
                    </section>

                    <section class="cmd-cheatsheet-group">
                        <h3 class="cmd-cheatsheet-gtitle">Filters</h3>
                        <div class="cmd-cheatsheet-row">
                            <code>ext:pdf invoice</code><span>Restrict to a file extension.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>path:downloads tax</code><span>Restrict to files under a path fragment.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>size:&gt;10mb video</code><span>Size comparison: &gt;, &lt;, =. Units: b, kb, mb, gb.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>modified:&gt;2025-01-01</code><span>Date comparison on modified time.</span>
                        </div>
                    </section>

                    <section class="cmd-cheatsheet-group">
                        <h3 class="cmd-cheatsheet-gtitle">Math, conversions &amp; calc</h3>
                        <div class="cmd-cheatsheet-row">
                            <code>23 * 47</code><span>Plain arithmetic — copies result on Enter.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>sin(45deg)</code><span>Trig, log, sqrt, factorials, constants (pi, e, tau, phi).</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>50 mi to km</code><span>Unit conversions (length, weight, time, temperature, etc.).</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>20% of 80</code><span>Percentage math, "X is what % of Y", "increase/decrease by".</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>0xff to dec</code><span>Base conversion: hex / dec / bin / oct.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>3 days from now</code><span>Date math, day-of-week, age, workdays.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>#ff5500 to rgb</code><span>Color format conversion + WCAG contrast.</span>
                        </div>
                    </section>

                    <section class="cmd-cheatsheet-group">
                        <h3 class="cmd-cheatsheet-gtitle">Utilities</h3>
                        <div class="cmd-cheatsheet-row">
                            <code>0xff &amp; 0x0f</code><span>Bitwise ops (&amp;, |, ^, ~, &lt;&lt;, &gt;&gt;).</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>uuid</code><span>Generate UUID v4. Also: <code>random 1-100</code>, <code>random color</code>.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>tip 15% on $50</code><span>Tip + total. Also: <code>30% off $100</code>.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>base64 "hello"</code><span>String ops: base64, urlencode, length, words, reverse.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>sum 1,2,3,4</code><span>Quick stats: sum, avg, min, max, count, product.</span>
                        </div>
                    </section>

                    <section class="cmd-cheatsheet-group">
                        <h3 class="cmd-cheatsheet-gtitle">Web shortcuts</h3>
                        <p class="cmd-cheatsheet-note">
                            Enable in Settings → Web search to use these.
                        </p>
                        <div class="cmd-cheatsheet-row">
                            <code>g react hooks</code><span>Google search.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>?how does X work</code><span>DuckDuckGo search.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>gh sveltejs/svelte</code><span>GitHub repo lookup.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>yt synthwave</code><span>YouTube search.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>https://example.com</code><span>Bare URL — opens in default browser.</span>
                        </div>
                    </section>

                    <section class="cmd-cheatsheet-group">
                        <h3 class="cmd-cheatsheet-gtitle">System commands</h3>
                        <div class="cmd-cheatsheet-row">
                            <code>lock</code><span>Lock the workstation.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>sleep</code><span>Put the system to sleep.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>shutdown</code><span>Power off (will confirm first).</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>restart</code><span>Reboot (will confirm first).</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>terminal</code><span>Open Terminal / cmd / PowerShell.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>settings</code><span>Open Windows Settings.</span>
                        </div>
                    </section>

                    <section class="cmd-cheatsheet-group">
                        <h3 class="cmd-cheatsheet-gtitle">Keyboard</h3>
                        <div class="cmd-cheatsheet-row">
                            <code>↑ ↓ ↵</code><span>Navigate and execute.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>Esc</code><span>Clear query, leave a sub-mode, or close the palette.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>Ctrl+Space</code><span>Actions for the selected item.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>Ctrl+Alt+← / →</code><span>Toggle Files / Inside search scope.</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>Ctrl+P</code><span>Toggle preview pane (clipboard mode).</span>
                        </div>
                        <div class="cmd-cheatsheet-row">
                            <code>Ctrl+/</code><span>This help.</span>
                        </div>
                    </section>
                </div>
            {:else if mode === 'default'}
                {#if !query.trim() && paletteScope === 'tools'}
                    <!-- Wave F (2026-05-27): Tools chip empty state.
                         Show every installed KIL tool so the user can
                         scan + click without typing. -->
                    <div class="cmd-section">
                        <div class="cmd-section-label">
                            <Wrench class="cmd-section-label-ico" />
                            <span>ALL TOOLS</span>
                            <span class="cmd-section-count">
                                {$installedTools.length}
                            </span>
                        </div>
                        {#each $installedTools as tool (tool.id)}
                            {@const Icon = tool.icon ?? Wrench}
                            <button
                                type="button"
                                class="cmd-row"
                                class:is-selected={selectables[selectedIndex]?.key ===
                                    `tool-all:${tool.id}`}
                                data-cmd-key={`tool-all:${tool.id}`}
                                onclick={() => void openMainAtTool(tool.id)}
                            >
                                <span class="cmd-row-icon" aria-hidden="true">
                                    <Icon class="cmd-row-icon-svg" />
                                </span>
                                <span class="cmd-row-text">
                                    <span class="cmd-row-title">{tool.name}</span>
                                    {#if tool.description}
                                        <span class="cmd-row-sub">{tool.description}</span>
                                    {/if}
                                </span>
                            </button>
                        {/each}
                    </div>
                {:else if !query.trim() && paletteScope === 'files'}
                    <!-- Wave F: Files chip empty state — show recent files
                         (the user wants to find their recent work without
                         remembering the filename to type). -->
                    {#if recentItems && recentItems.files.length > 0}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <Clock class="cmd-section-label-ico" />
                                <span>RECENT FILES</span>
                            </div>
                            {#each recentItems.files as item (item.path)}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `recent-file-all:${item.path}`}
                                    data-cmd-key={`recent-file-all:${item.path}`}
                                    onclick={() => void openFile(item.path)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        {#if entryIcons[item.path]}
                                            <img
                                                src={entryIcons[item.path]}
                                                alt=""
                                                class="cmd-row-icon-img"
                                            />
                                        {:else}
                                            <FileText class="cmd-row-icon-svg" />
                                        {/if}
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{item.displayName}</span>
                                        <span class="cmd-row-sub">{item.path}</span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {:else}
                        <div class="cmd-empty">
                            <FileText class="cmd-empty-ico" />
                            <p class="cmd-empty-text">No recent files yet.</p>
                            <p class="cmd-empty-sub">Type a filename to search.</p>
                        </div>
                    {/if}
                {:else if !query.trim() && paletteScope === 'notes'}
                    <!-- Wave F: Notes chip empty state — Create-new CTA
                         followed by every existing .ki note. Click to
                         open in the Notes pillar. -->
                    <div class="cmd-section">
                        <div class="cmd-section-label">
                            <NotebookPen class="cmd-section-label-ico" />
                            <span>NOTES</span>
                            {#if $notes.length > 0}
                                <span class="cmd-section-count">{$notes.length}</span>
                            {/if}
                        </div>
                        <button
                            type="button"
                            class="cmd-row"
                            class:is-selected={selectables[selectedIndex]?.key === 'note-new'}
                            data-cmd-key="note-new"
                            onclick={() => void openQuickNote()}
                        >
                            <span class="cmd-row-icon" aria-hidden="true">
                                <NotebookPen class="cmd-row-icon-svg" />
                            </span>
                            <span class="cmd-row-text">
                                <span class="cmd-row-title">Create new note</span>
                                <span class="cmd-row-sub">Opens a floating sticky note</span>
                            </span>
                        </button>
                        {#each $notes as note (note.path)}
                            <button
                                type="button"
                                class="cmd-row"
                                class:is-selected={selectables[selectedIndex]?.key ===
                                    `note-all:${note.path}`}
                                data-cmd-key={`note-all:${note.path}`}
                                onclick={() => void openFile(note.path)}
                            >
                                <span class="cmd-row-icon" aria-hidden="true">
                                    <FileText class="cmd-row-icon-svg" />
                                </span>
                                <span class="cmd-row-text">
                                    <span class="cmd-row-title">{note.title || 'Untitled'}</span>
                                    {#if note.path}
                                        <span class="cmd-row-sub">{note.path}</span>
                                    {/if}
                                </span>
                            </button>
                        {/each}
                    </div>
                {:else if !query.trim() && paletteScope === 'apps'}
                    <!-- Wave G (2026-05-27): Apps chip empty state shows
                         EVERY installed app (cache-resident; loaded once
                         on first click via search_launch_targets +
                         browseAll). Recents lead — frecency-sorted from
                         the backend — so daily-used apps land at the
                         top. The list also re-uses the existing
                         `entryIcons` so real Windows shell icons render
                         for each app, not generic placeholders. -->
                    {#if browseAllAppsLoading && browseAllApps.length === 0}
                        <div class="cmd-loadmore">
                            <span class="cmd-loadmore-spinner" aria-hidden="true"></span>
                            <span>Loading installed apps…</span>
                        </div>
                    {:else if browseAllApps.length > 0}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <AppWindow class="cmd-section-label-ico" />
                                <span>ALL APPS</span>
                                <span class="cmd-section-count">
                                    {browseAllApps.length}
                                </span>
                            </div>
                            {#each browseAllApps as app (app.path)}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `app-all:${app.path}`}
                                    data-cmd-key={`app-all:${app.path}`}
                                    onclick={() => void launchApp(app.path)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        {#if entryIcons[app.path]}
                                            <img
                                                src={entryIcons[app.path]}
                                                alt=""
                                                class="cmd-row-icon-img"                                            />
                                        {:else}
                                            <AppWindow class="cmd-row-icon-svg" />
                                        {/if}
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{app.name}</span>
                                        <span class="cmd-row-sub">{app.path}</span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {:else}
                        <div class="cmd-empty">
                            <AppWindow class="cmd-empty-ico" />
                            <p class="cmd-empty-text">No apps cached yet</p>
                            <p class="cmd-empty-sub">
                                Run a launcher search first to populate the cache.
                            </p>
                        </div>
                    {/if}
                {:else if !query.trim() && paletteScope === 'emoji'}
                    <!-- Emoji chip empty state: most-recently-picked first,
                         then the bundled table in Unicode order. -->
                    {@render emojiBody()}
                {:else if !query.trim() && paletteScope === 'commands'}
                    <!-- Commands chip empty state: system-info card, system
                         actions, and running apps. Same snippet the query-
                         active body renders, so selectables stay in sync. -->
                    {@render commandsBody()}
                {:else if !query.trim() && paletteScope === 'windows'}
                    {@render windowsBody()}
                {:else if !query.trim() && paletteScope === 'browser'}
                    <div class="cmd-empty">
                        <Globe class="cmd-empty-ico" />
                        <p class="cmd-empty-text">Search your browser data</p>
                        <p class="cmd-empty-sub">
                            Type at least two characters to search {$settings.browserHistoryEnabled
                                ? 'bookmarks and history'
                                : 'bookmarks'}.
                        </p>
                    </div>
                {:else if !query.trim()}
                    <!-- Empty query, scope=all: KEEPITLOCAL CORE + Suggested
                         Tools + recent items (the original empty state). -->
                    <div class="cmd-section">
                        <div class="cmd-section-label">CORE</div>
                        {#each corePillars as pillar (pillar.id)}
                            {@const Icon = pillar.icon}
                            <button
                                type="button"
                                class="cmd-row"
                                class:is-selected={selectables[selectedIndex]?.key ===
                                    `core:${pillar.id}`}
                                data-cmd-key={`core:${pillar.id}`}
                                onclick={() => void pillar.activate()}
                            >
                                <span class="cmd-row-icon" aria-hidden="true">
                                    <Icon class="cmd-row-icon-svg" />
                                </span>
                                <span class="cmd-row-text">
                                    <span class="cmd-row-title">{pillar.name}</span>
                                    <span class="cmd-row-sub">{pillar.description}</span>
                                </span>
                            </button>
                        {/each}
                    </div>

                    {#if !isHidden('suggested-tools') && suggestedTools.length > 0}
                        <div class="cmd-section">
                            <div class="cmd-section-label">SUGGESTED APPS</div>
                            {#each suggestedTools as tool (tool.id)}
                                {@const Icon = tool.icon ?? Wrench}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `suggested:${tool.id}`}
                                    data-cmd-key={`suggested:${tool.id}`}
                                    onclick={() => void openMainAtTool(tool.id)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        <Icon class="cmd-row-icon-svg" />
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{tool.name}</span>
                                        <span class="cmd-row-sub">{tool.category} Pack</span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {/if}

                    {#if !isHidden('recent-items') && recentApps.length > 0}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <Clock class="cmd-section-label-ico" />
                                <span>RECENT APPS</span>
                            </div>
                            {#each recentApps as item (item.kind + ':' + item.path)}
                                {@const isTool = item.kind === 'tool'}
                                {@const resolved = isTool ? toolForId(item.path) : null}
                                {@const ToolIcon = resolved?.icon ?? Wrench}
                                {@const cmdKey = isTool
                                    ? `recent-tool:${item.path}`
                                    : `recent-app:${item.path}`}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key === cmdKey}
                                    data-cmd-key={cmdKey}
                                    onclick={() =>
                                        isTool
                                            ? void openMainAtTool(item.path)
                                            : void launchApp(item.path)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        {#if isTool}
                                            <ToolIcon class="cmd-row-icon-svg" />
                                        {:else if entryIcons[item.path]}
                                            <img
                                                src={entryIcons[item.path]}
                                                alt=""
                                                loading="lazy"
                                                class="cmd-row-icon-img"
                                            />
                                        {:else}
                                            <AppWindow class="cmd-row-icon-svg" />
                                        {/if}
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{item.displayName}</span>
                                        <span class="cmd-row-sub">
                                            opened {item.launchCount} time{item.launchCount ===
                                            1
                                                ? ''
                                                : 's'}
                                        </span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {/if}

                    {#if !isHidden('recent-items') && recentItems && recentItems.files.length > 0}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <Clock class="cmd-section-label-ico" />
                                <span>RECENT FILES</span>
                            </div>
                            {#each recentItems.files.slice(0, 3) as item (item.path)}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `recent-file:${item.path}`}
                                    data-cmd-key={`recent-file:${item.path}`}
                                    onclick={() => void openFile(item.path)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        {#if entryIcons[item.path]}
                                            <img
                                                src={entryIcons[item.path]}
                                                alt=""
                                                loading="lazy"
                                                class="cmd-row-icon-img"
                                            />
                                        {:else}
                                            <FileText class="cmd-row-icon-svg" />
                                        {/if}
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{item.displayName}</span>
                                        <span class="cmd-row-sub">{item.path}</span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {/if}

                    {#if !isHidden('recent-folders') && recentItems && recentItems.folders.length > 0}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <Clock class="cmd-section-label-ico" />
                                <span>RECENT FOLDERS</span>
                            </div>
                            {#each recentItems.folders.slice(0, 3) as item (item.path)}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `recent-folder:${item.path}`}
                                    data-cmd-key={`recent-folder:${item.path}`}
                                    onclick={() => void openFile(item.path)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        {#if entryIcons[item.path]}
                                            <img
                                                src={entryIcons[item.path]}
                                                alt=""
                                                loading="lazy"
                                                class="cmd-row-icon-img"
                                            />
                                        {:else}
                                            <Folder class="cmd-row-icon-svg" />
                                        {/if}
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{item.displayName}</span>
                                        <span class="cmd-row-sub">{item.path}</span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {/if}
                {:else}
                    <!-- Query-active state: reminder quick-add + My Commands
                         first (high-intent), then Time & Focus actions,
                         quick-action, apps, tools, files. -->
                    {#if inScope('tools') && !isHidden('reminders') && reminderMatch}
                        {@const rm = reminderMatch}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <Bell class="cmd-section-label-ico" />
                                <span>REMINDER</span>
                            </div>
                            <button
                                type="button"
                                class="cmd-row"
                                class:is-selected={selectables[selectedIndex]?.key ===
                                    'reminder-create'}
                                data-cmd-key="reminder-create"
                                onclick={() => void createReminderFromPalette(rm)}
                            >
                                <span class="cmd-row-icon" aria-hidden="true">
                                    <Bell class="cmd-row-icon-svg" />
                                </span>
                                <span class="cmd-row-text">
                                    <span class="cmd-row-title">Remind me: {rm.text}</span>
                                    <span class="cmd-row-sub">{formatReminderDue(rm.dueMs)}</span>
                                </span>
                            </button>
                        </div>
                    {/if}
                    {#if inScope('notes') && noteMatch}
                        {@const nt = noteMatch}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <NotebookPen class="cmd-section-label-ico" />
                                <span>NOTE</span>
                            </div>
                            <button
                                type="button"
                                class="cmd-row"
                                class:is-selected={selectables[selectedIndex]?.key === 'note-create'}
                                data-cmd-key="note-create"
                                onclick={() => void createNoteFromPalette(nt)}
                            >
                                <span class="cmd-row-icon" aria-hidden="true">
                                    <NotebookPen class="cmd-row-icon-svg" />
                                </span>
                                <span class="cmd-row-text">
                                    <span class="cmd-row-title">New note: {nt}</span>
                                    <span class="cmd-row-sub">Saved to your local notebook</span>
                                </span>
                            </button>
                        </div>
                    {/if}
                    {#if inScope('notes') && !isHidden('quick-notes') && quickNoteMatch}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <NotebookPen class="cmd-section-label-ico" />
                                <span>NOTE</span>
                            </div>
                            <button
                                type="button"
                                class="cmd-row"
                                class:is-selected={selectables[selectedIndex]?.key ===
                                    'quick-note-open'}
                                data-cmd-key="quick-note-open"
                                onclick={() => void openQuickNote()}
                            >
                                <span class="cmd-row-icon" aria-hidden="true">
                                    <NotebookPen class="cmd-row-icon-svg" />
                                </span>
                                <span class="cmd-row-text">
                                    <span class="cmd-row-title">Create a note</span>
                                    <span class="cmd-row-sub">Opens a quick sticky note</span>
                                </span>
                            </button>
                        </div>
                    {/if}
                    {#if paletteScope === 'notes' && query.trim() && notesForQuery.length}
                        <!-- Notes matching the query — title/preview/tags plus
                             deep body matches (search_note_bodies). This is
                             what makes a word buried in a note findable from
                             the Notes chip, not just from Files/Inside. -->
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <NotebookPen class="cmd-section-label-ico" />
                                <span>NOTES</span>
                                <span class="cmd-section-count">{notesForQuery.length}</span>
                            </div>
                            {#each notesForQuery as note (note.path)}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `note-hit:${note.path}`}
                                    data-cmd-key={`note-hit:${note.path}`}
                                    onclick={() => void openFile(note.path)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        <FileText class="cmd-row-icon-svg" />
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{note.title || 'Untitled'}</span>
                                        <span class="cmd-row-sub">{note.path}</span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {/if}
                    {#if paletteScope === 'browser' && query.trim()}
                        <!-- Bookmarks + history. Rendered here to mirror the
                             selectables push order above — if the two ever
                             disagree, arrow-nav highlights the wrong row. -->
                        {#if query.trim().length < 2}
                            <div class="cmd-empty">
                                <Globe class="cmd-empty-ico" />
                                <p class="cmd-empty-text">Keep typing to search the web</p>
                                <p class="cmd-empty-sub">
                                    Enter at least two characters to search {$settings.browserHistoryEnabled
                                        ? 'bookmarks and history'
                                        : 'bookmarks'}.
                                </p>
                            </div>
                        {:else if browserSearching}
                            <div class="cmd-empty is-loading">
                                <Globe class="cmd-empty-ico" />
                                <p class="cmd-empty-text">Searching browser data…</p>
                            </div>
                        {:else if browserHits.length}
                            <div class="cmd-section">
                                <div class="cmd-section-label">
                                    <Globe class="cmd-section-label-ico" />
                                    <span>WEB</span>
                                    <span class="cmd-section-count">{browserHits.length}</span>
                                </div>
                                {#each browserHits as hit (hit.url)}
                                    <button
                                        type="button"
                                        class="cmd-row"
                                        class:is-selected={selectables[selectedIndex]?.key ===
                                            `browser:${hit.url}`}
                                        data-cmd-key={`browser:${hit.url}`}
                                        onclick={() => void openBrowserHit(hit)}
                                    >
                                        <span class="cmd-row-icon" aria-hidden="true">
                                            {#if hit.kind === 'bookmark'}
                                                <Star class="cmd-row-icon-svg" />
                                            {:else}
                                                <Clock class="cmd-row-icon-svg" />
                                            {/if}
                                        </span>
                                        <span class="cmd-row-text">
                                            <span class="cmd-row-title">{hit.title}</span>
                                            <span class="cmd-row-sub">{hit.displayUrl}</span>
                                        </span>
                                        <span class="cmd-row-meta">
                                            {hit.browser}
                                        </span>
                                    </button>
                                {/each}
                            </div>
                        {:else}
                            <div class="cmd-empty">
                                <Globe class="cmd-empty-ico" />
                                <p class="cmd-empty-text">
                                    No {$settings.browserHistoryEnabled
                                        ? 'bookmarks or history'
                                        : 'bookmarks'} match "<strong>{query.trim()}</strong>"
                                </p>
                                <p class="cmd-empty-sub">
                                    {$settings.browserHistoryEnabled
                                        ? 'Searched bookmarks and history across every installed browser.'
                                        : 'Searching bookmarks only — enable history in Settings to include it.'}
                                </p>
                            </div>
                        {/if}
                    {/if}
                    {#if inScope('tools') && myCommandMatches.length}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <Zap class="cmd-section-label-ico" />
                                <span>MY COMMANDS</span>
                            </div>
                            {#each myCommandMatches as m (m.cmd.id)}
                                {@const RowIcon =
                                    m.cmd.type === 'app'
                                        ? AppWindow
                                        : m.cmd.type === 'file'
                                          ? FileText
                                          : m.cmd.type === 'folder'
                                            ? Folder
                                            : Globe}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `mycmd:${m.cmd.id}`}
                                    data-cmd-key={`mycmd:${m.cmd.id}`}
                                    onclick={() => void runMyCommand(m.cmd, m.arg)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        <RowIcon class="cmd-row-icon-svg" />
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{m.cmd.label}</span>
                                        <span class="cmd-row-sub">
                                            {#if isBang(m.cmd) && m.arg}
                                                Search "{m.arg}"
                                            {:else if isBang(m.cmd)}
                                                Type a query after "{m.cmd.keyword}"
                                            {:else}
                                                {m.cmd.target}
                                            {/if}
                                        </span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {/if}
                    {#if inScope('tools') && !isHidden('time-focus') && timeFocusMatches.length}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <Timer class="cmd-section-label-ico" />
                                <span>TIME &amp; FOCUS</span>
                            </div>
                            {#each timeFocusMatches as a (a.id)}
                                {@const RowIcon = a.icon}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key === `tf:${a.id}`}
                                    data-cmd-key={`tf:${a.id}`}
                                    onclick={() => void runTimeFocusAction(a)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        <RowIcon class="cmd-row-icon-svg" />
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{a.label}</span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {/if}
                    {#if inScope('tools') && quickAction}
                        {#if quickAction.type === 'calculator' || quickAction.type === 'unitConversion'}
                            <div class="cmd-section">
                                <div class="cmd-section-label">
                                    <Calculator class="cmd-section-label-ico" />
                                    <span>
                                        {quickAction.type === 'calculator'
                                            ? 'CALCULATION'
                                            : 'CONVERSION'}
                                    </span>
                                    {#if quickActionCopied}
                                        <span class="cmd-section-copied">Copied!</span>
                                    {/if}
                                </div>
                                <!-- Split-card layout mirrors /overlay: 22 px values
                                     centered on each side, "→" arrow between, a
                                     small chip ("Expression" / "Result") under each
                                     value. The whole card is a single keyboard /
                                     click target. -->
                                <button
                                    type="button"
                                    class="cmd-quick-card"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        'quick-action'}
                                    data-cmd-key="quick-action"
                                    onclick={() => void activateQuickAction(quickAction!)}
                                >
                                    <span class="cmd-quick-side">
                                        <span class="cmd-quick-value">
                                            {quickAction.type === 'calculator'
                                                ? quickAction.expression
                                                : quickAction.original}
                                        </span>
                                        <span class="cmd-quick-chip">
                                            {quickAction.type === 'calculator'
                                                ? 'Expression'
                                                : 'Input'}
                                        </span>
                                    </span>
                                    <span class="cmd-quick-arrow" aria-hidden="true">→</span>
                                    <span class="cmd-quick-side">
                                        <span class="cmd-quick-value cmd-quick-value-result">
                                            {quickAction.result}
                                        </span>
                                        <span class="cmd-quick-chip">Result</span>
                                    </span>
                                </button>
                            </div>
                        {:else if quickAction.type === 'openUrl' || quickAction.type === 'webSearch'}
                            <!-- Bang shortcuts / web search — render as a
                                 single Globe-iconed row (not a split card).
                                 The split-card visual reads as a "compute
                                 result on the right" which is wrong for a
                                 URL that just opens. A regular row with a
                                 destination URL subtitle matches /overlay. -->
                            <div class="cmd-section">
                                <div class="cmd-section-label">
                                    <Globe class="cmd-section-label-ico" />
                                    <span>
                                        {quickAction.type === 'openUrl'
                                            ? 'OPEN URL'
                                            : 'WEB SEARCH'}
                                    </span>
                                </div>
                                <button
                                    type="button"
                                    class="cmd-row cmd-row-bang"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        'quick-action'}
                                    data-cmd-key="quick-action"
                                    onclick={() => void activateQuickAction(quickAction!)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        <Globe class="cmd-row-icon-svg" />
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">
                                            {quickAction.type === 'openUrl'
                                                ? quickAction.display
                                                : `${quickAction.provider}: ${quickAction.query}`}
                                        </span>
                                        <span class="cmd-row-sub">
                                            {quickAction.type === 'openUrl'
                                                ? `Open in browser · ${quickAction.url}`
                                                : `Search ${quickAction.provider} for "${quickAction.query}"`}
                                        </span>
                                    </span>
                                </button>
                            </div>
                        {:else}
                            <!-- System command (lock / sleep / shutdown /
                                 restart / signout / open terminal / open
                                 settings). Icon picked from a small map by
                                 command id. Destructive commands are gated
                                 on a confirm dialog in activateQuickAction
                                 (`requiresConfirmation` from backend) —
                                 the row itself just shows a "Will confirm"
                                 hint in the subtitle so the user knows
                                 what's coming. -->
                            <div class="cmd-section">
                                <div class="cmd-section-label">
                                    <Power class="cmd-section-label-ico" />
                                    <span>SYSTEM COMMAND</span>
                                </div>
                                <button
                                    type="button"
                                    class="cmd-row cmd-row-system"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        'quick-action'}
                                    class:is-destructive={quickAction.requiresConfirmation}
                                    data-cmd-key="quick-action"
                                    onclick={() => void activateQuickAction(quickAction!)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        {#if quickAction.id === 'shutdown' || quickAction.id === 'restart' || quickAction.id === 'signout'}
                                            <Power class="cmd-row-icon-svg" />
                                        {:else if quickAction.id === 'cmd' || quickAction.id === 'terminal' || quickAction.id === 'powershell'}
                                            <Terminal class="cmd-row-icon-svg" />
                                        {:else if quickAction.id === 'settings' || quickAction.id === 'control'}
                                            <Settings class="cmd-row-icon-svg" />
                                        {:else if quickAction.id === 'lock'}
                                            <Lock class="cmd-row-icon-svg" />
                                        {:else}
                                            <AppWindow class="cmd-row-icon-svg" />
                                        {/if}
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{quickAction.name}</span>
                                        <span class="cmd-row-sub">
                                            {quickAction.description}{quickAction.requiresConfirmation
                                                ? ' · Will confirm first'
                                                : ''}
                                        </span>
                                    </span>
                                </button>
                            </div>
                        {/if}
                    {/if}

                    <!-- Commands scope (2026-06-13 redesign): the 4 category
                         rows; the typed query filters the ITEMS inside the
                         preview pane, the category list itself is constant. -->
                    {#if paletteScope === 'commands'}
                        {@render commandsBody()}
                    {/if}

                    <!-- 'all' scope kill type-to-find: "kill chrome" surfaces
                         the running-app rows inline so Enter closes them. The
                         system-info card and system actions are click-only via
                         the Commands chip now, so they don't appear here. -->
                    {#if paletteScope === 'all' && killRows.length}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <Activity class="cmd-section-label-ico" />
                                <span>RUNNING APPS</span>
                                <span class="cmd-section-count">{killRows.length}</span>
                            </div>
                            {#each killRows as group (group.exePath ?? group.name)}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `kill:${group.exePath ?? group.name}`}
                                    data-cmd-key={`kill:${group.exePath ?? group.name}`}
                                    onclick={() => openKillConfirm(group)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        {#if group.exePath && entryIcons[group.exePath]}
                                            <img
                                                src={entryIcons[group.exePath]}
                                                alt=""
                                                loading="lazy"
                                                class="cmd-row-icon-img"
                                            />
                                        {:else}
                                            <AppWindow class="cmd-row-icon-svg" />
                                        {/if}
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{group.name}</span>
                                        <span class="cmd-row-sub">
                                            {group.windowCount} window{group.windowCount === 1
                                                ? ''
                                                : 's'} · {formatBytes(group.memoryBytes)}
                                        </span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {/if}

                    {#if inScope('windows') && windowMatches.length}
                        {@render windowsRows()}
                    {/if}

                    {#if inScope('emoji') && emojiMatches.length}
                        {@render emojiBody()}
                    {/if}

                    {#if inScope('apps') && dedupedLaunchResults.length}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <AppWindow class="cmd-section-label-ico" />
                                <span>INSTALLED APPS</span>
                            </div>
                            {#each dedupedLaunchResults as app (app.id)}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `launch:${app.path}`}
                                    data-cmd-key={`launch:${app.path}`}
                                    onclick={() => void launchApp(app.path)}
                                >
                                    <span
                                        class="cmd-row-icon"
                                        class:is-running={runningAppPaths.has(app.path)}
                                        aria-hidden="true"
                                    >
                                        {#if entryIcons[app.path]}
                                            <img
                                                src={entryIcons[app.path]}
                                                alt=""
                                                loading="lazy"
                                                class="cmd-row-icon-img"                                            />
                                        {:else}
                                            <AppWindow class="cmd-row-icon-svg" />
                                        {/if}
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{app.name}</span>
                                        <span class="cmd-row-sub">{app.path}</span>
                                    </span>
                                    {#if runningAppPaths.has(app.path)}
                                        <span class="cmd-row-running" title="Running">Running</span>
                                    {/if}
                                </button>
                            {/each}
                        </div>
                    {/if}

                    {#if inScope('tools') && keepitlocalMatches.length}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <Wrench class="cmd-section-label-ico" />
                                <span>KEEPITLOCAL TOOLS</span>
                            </div>
                            {#each keepitlocalMatches as tool (tool.id)}
                                {@const resolved = toolForId(tool.id)}
                                {@const Icon = resolved?.icon ?? Wrench}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `keepitlocal:${tool.id}`}
                                    data-cmd-key={`keepitlocal:${tool.id}`}
                                    onclick={() => void openMainAtTool(tool.id)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        <Icon class="cmd-row-icon-svg" />
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{tool.name}</span>
                                        <span class="cmd-row-sub">{tool.description}</span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {/if}

                    {#if inScope('tools') && !isHidden('settings') && settingsMatches.length}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <Settings class="cmd-section-label-ico" />
                                <span>SETTINGS</span>
                            </div>
                            {#each settingsMatches as s (s.id)}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `settings:${s.id}`}
                                    data-cmd-key={`settings:${s.id}`}
                                    onclick={() => void openMainAtSettings(s.id)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        <Settings class="cmd-row-icon-svg" />
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">{s.label}</span>
                                        <span class="cmd-row-sub">Open Settings → {s.label}</span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {/if}

                    {#if inScope('files') && dedupedFileResults.length}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <FileText class="cmd-section-label-ico" />
                                <span>FILES & FOLDERS</span>
                                <span class="cmd-section-count">
                                    {dedupedFileResults.length} of {fileTotalHits}
                                </span>
                            </div>
                            {#each dedupedFileResults as file (file.path)}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `file:${file.path}`}
                                    data-cmd-key={`file:${file.path}`}
                                    onclick={() => void openFile(file.path)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        {#if entryIcons[file.path]}
                                            <img
                                                src={entryIcons[file.path]}
                                                alt=""
                                                loading="lazy"
                                                class="cmd-row-icon-img"
                                            />
                                        {:else if file.entryType === 'folder'}
                                            <Folder class="cmd-row-icon-svg" />
                                        {:else}
                                            <FileText class="cmd-row-icon-svg" />
                                        {/if}
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">
                                            <!-- V2: highlight the matched
                                                 substring inside the name itself
                                                 instead of a separate "Matched:"
                                                 chip. Escaped + <mark>-wrapped by
                                                 highlightSnippet. -->
                                            {@html highlightSnippet(
                                                file.fileName ||
                                                    file.path.split(/[\\/]/).pop() ||
                                                    '',
                                                file.matchedKeywords,
                                            )}
                                            {#if file.sensitiveKinds && file.sensitiveKinds.length > 0}
                                                <!-- Sensitive-content badge —
                                                     surfaces when the backend
                                                     flagged the file as carrying
                                                     potentially-sensitive content
                                                     (private keys, .env, etc.).
                                                     Inline next to the title so
                                                     it can't be missed before
                                                     opening. -->
                                                <span
                                                    class="cmd-row-sensitive"
                                                    title="Contains sensitive content: {file.sensitiveKinds.join(
                                                        ', ',
                                                    )}"
                                                >
                                                    Sensitive
                                                </span>
                                            {/if}
                                        </span>
                                        <span class="cmd-row-sub">{file.path}</span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {/if}

                    <!-- INSIDE FILES — unified content results section
                         (Wave 3.3.3, 2026-05-27). Tantivy index hits and
                         ripgrep fallback hits render as visually
                         IDENTICAL rows in one section. From the user's
                         perspective there's just one "Inside files"
                         search; the engine choice happens invisibly
                         (index path first, ripgrep auto-fires only if
                         the index returned nothing — either because
                         the query has zero index matches OR because
                         the index isn't built yet). -->
                    <!-- Wave 7.6 bug fix (2026-05-28): also require
                         `searchMode === 'content'` so that switching
                         back to file mode immediately hides any
                         lingering content / live-grep hits. Without
                         this gate, residual hits could render under a
                         file-mode query because `inScope('files')` is
                         true in both file AND content modes. -->
                    {#if searchMode === 'content' && inScope('files') && (dedupedContentResults.length || liveGrepHits.length || liveGrepBusy)}
                        <div class="cmd-section">
                            <div class="cmd-section-label">
                                <FileText class="cmd-section-label-ico" />
                                <span>INSIDE FILES</span>
                                {#if insideHitCount > 0}
                                    <span
                                        class="cmd-section-count"
                                        title={liveGrepSummary?.truncated
                                            ? 'Results capped for speed — refine the query to narrow them down.'
                                            : undefined}
                                    >
                                        {insideHitCount}{liveGrepSummary?.truncated ? '+' : ''} hit{insideHitCount === 1 ? '' : 's'}
                                    </span>
                                {/if}
                            </div>
                            {#each dedupedContentResults as file (file.path)}
                                <button
                                    type="button"
                                    class="cmd-row cmd-row-content"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `content:${file.path}`}
                                    data-cmd-key={`content:${file.path}`}
                                    onclick={() => void openFile(file.path)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        {#if entryIcons[file.path]}
                                            <img
                                                src={entryIcons[file.path]}
                                                alt=""
                                                loading="lazy"
                                                class="cmd-row-icon-img"
                                            />
                                        {:else}
                                            <FileText class="cmd-row-icon-svg" />
                                        {/if}
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">
                                            {file.fileName || file.path.split(/[\\/]/).pop() || ''}
                                        </span>
                                        {#if file.snippet}
                                            <span class="cmd-row-snippet">
                                                {@html highlightSnippet(
                                                    file.snippet,
                                                    file.matchedKeywords,
                                                )}
                                            </span>
                                        {/if}
                                        <span class="cmd-row-sub">{file.path}</span>
                                    </span>
                    {#if file.matchCount > 1}
                                        <!-- Match-count pill — only when > 1
                                             since "1 hit" is implied. Accent
                                             so the eye lands on files with
                                             dense matches (likely the
                                             relevant one). -->
                                        <span class="cmd-row-pill">
                                            +{file.matchCount}
                                        </span>
                                    {/if}
                                </button>
                            {/each}
                            {#each liveGrepHits.slice(0, 200) as hit (hit.path + ':' + hit.lineNumber)}
                                {@const lgKey = `live-grep:${hit.path}:${hit.lineNumber}`}
                                {@const snippet = liveGrepHitSnippet(hit.line, hit.matchStart, hit.matchEnd)}
                                <button
                                    type="button"
                                    class="cmd-row cmd-row-content"
                                    class:is-selected={selectables[selectedIndex]?.key === lgKey}
                                    data-cmd-key={lgKey}
                                    onclick={() => void openLiveGrepHit(hit)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        <FileText class="cmd-row-icon-svg" />
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">
                                            {hit.path.split(/[\\/]/).pop()}:{hit.lineNumber}
                                        </span>
                                        <span class="cmd-row-snippet">
                                            {#if snippet.hasMatch}{snippet.pre}<mark class="cmd-livegrep-mark">{snippet.match}</mark>{snippet.post}{:else}{snippet.pre}{/if}
                                        </span>
                                        <span class="cmd-row-sub">{hit.path}</span>
                                    </span>
                                </button>
                            {/each}
                        </div>
                    {/if}

                    <!-- Pagination sentinel — Wave I (2026-05-27).
                         The explicit "Load more" button was removed in
                         favour of Raycast-style infinite scroll: when
                         the user scrolls within 240 px of the body
                         bottom, `onBodyScroll` fires `loadMoreFileResults`
                         automatically. This sentinel renders a quiet
                         "Loading more…" spinner while a fetch is in
                         flight and a soft "X of Y" hint otherwise, so
                         the user knows MORE exists even before they
                         scroll into the trigger zone. Arrow-nav past
                         the visible list also triggers a scroll (via
                         ensureRowVisible) which then trips the auto-
                         load — no button needed. -->
                    {#if (fileResults.length > 0 && fileResults.length < fileTotalHits) || (contentResults.length > 0 && contentResults.length < contentTotalHits) || isLoadingMoreFiles}
                        <div class="cmd-loadmore" aria-live="polite">
                            {#if isLoadingMoreFiles}
                                <span class="cmd-loadmore-spinner" aria-hidden="true"></span>
                                <span>Loading more results…</span>
                            {:else}
                                <span class="cmd-loadmore-hint">
                                    Scroll for more
                                    {#if searchMode === 'content'}
                                        ({contentResults.length} of {contentTotalHits})
                                    {:else}
                                        ({fileResults.length} of {fileTotalHits})
                                    {/if}
                                </span>
                            {/if}
                        </div>
                    {/if}

                    {#if paletteScope !== 'browser' && !quickAction && !windowMatches.length && !emojiMatches.length && !dedupedLaunchResults.length && !keepitlocalMatches.length && !myCommandMatches.length && !reminderMatch && !noteMatch && !quickNoteMatch && !notesForQuery.length && !timeFocusMatches.length && !settingsMatches.length && !dedupedFileResults.length && !dedupedContentResults.length && !liveGrepHits.length && !searching && !liveGrepBusy}
                        <div class="cmd-empty">
                            <Sparkles class="cmd-empty-ico" />
                            <p class="cmd-empty-text">
                                {paletteScope === 'notes'
                                    ? `No notes match "${query.trim()}"`
                                    : `No matches for "${query.trim()}"`}
                            </p>
                            <p class="cmd-empty-sub">
                                {paletteScope === 'notes'
                                    ? 'Notes are searched by name, tags, and contents.'
                                    : searchMode === 'content'
                                      ? 'Try the Files mode for filename matches.'
                                      : 'Try Inside mode to search within file contents.'}
                            </p>
                        </div>
                    {/if}
                {/if}
            {:else if mode === 'clipboard'}
                <div class="cmd-section">
                    {#if clipboardLoading && clipboardEntries.length === 0}
                        <!-- is-loading: pulses the icon so "still working" reads
                             differently from a final "nothing here" empty state. -->
                        <div class="cmd-empty is-loading">
                            <Clipboard class="cmd-empty-ico" />
                            <p class="cmd-empty-text">Loading clipboard…</p>
                        </div>
                    {:else if filteredClipboard.length === 0 && matchingSnippets.length === 0}
                        <div class="cmd-empty">
                            <Clipboard class="cmd-empty-ico" />
                            <p class="cmd-empty-text">
                                {query.trim()
                                    ? `No matches for "${query.trim()}"`
                                    : 'No clipboard history yet'}
                            </p>
                            <p class="cmd-empty-sub">
                                {query.trim()
                                    ? 'Try a shorter search or Esc to go back.'
                                    : 'Copy something to start filling your local history.'}
                            </p>
                        </div>
                    {:else}
                        <!-- Cleanup Wave 1 (2026-05-28): snippet matches above
                             the clipboard history. Typing `/sig` enters slash-
                             mode and ranks snippets first so Enter expands the
                             template + pastes into the previously focused app
                             (parity with the legacy /clipboard-overlay). -->
                        {#if matchingSnippets.length > 0}
                            <div class="cmd-section-label">Snippets</div>
                            {#each matchingSnippets as snippet (snippet.id)}
                                <button
                                    type="button"
                                    class="cmd-row"
                                    class:is-selected={selectables[selectedIndex]?.key ===
                                        `snip:${snippet.id}`}
                                    data-cmd-key={`snip:${snippet.id}`}
                                    onmousedown={(e) => {
                                        // Same focus-preservation contract as
                                        // clipboard rows — see the clipboard
                                        // row's mousedown comment below.
                                        e.preventDefault();
                                    }}
                                    onclick={() => void pasteSnippet(snippet)}
                                >
                                    <span class="cmd-row-icon" aria-hidden="true">
                                        <Tag class="cmd-row-icon-svg" />
                                    </span>
                                    <span class="cmd-row-text">
                                        <span class="cmd-row-title">
                                            /{snippet.trigger}
                                            {#if snippet.label}
                                                <span class="cmd-row-pinned">{snippet.label}</span>
                                            {/if}
                                        </span>
                                        <span class="cmd-row-sub">
                                            {clipboardPreview(snippet.template, 80)}
                                        </span>
                                    </span>
                                </button>
                            {/each}
                        {/if}
                        {#each filteredClipboard as entry, i (entry.id)}
                            {@const prevEntry = i > 0 ? filteredClipboard[i - 1] : undefined}
                            {@const CatIcon = clipboardCategoryIcon(entry.category)}
                            {@const isColor = entry.category === 'color'}
                            <!-- Phase 6.5-4 (2026-05-27): tier-aware blur.
                                 Only HIGH-tier kinds (AWS, GitHub, Stripe,
                                 PEM, JWT, credit card, …) trigger the blur
                                 in the preview row — LOW-tier shapes
                                 (email/phone/IP) don't deserve a shoulder-
                                 surf shield in the clipboard list. The
                                 backend still tags the entry as
                                 sensitive_kinds for retention purposes;
                                 this guards only the UI blur. -->
                            {@const isSensitive = hasHighTierKind(entry.sensitiveKinds)}
                            <!-- Group headers: pinned-first sort means we
                                 emit "Pinned" before the first pinned row and
                                 "History" at the pinned→unpinned boundary
                                 (or "Recent copies" when there are no pins). -->
                            {#if entry.isPinned && !prevEntry}
                                <div class="cmd-section-label">Pinned</div>
                            {:else if !entry.isPinned && (!prevEntry || prevEntry.isPinned)}
                                <div class="cmd-section-label">
                                    {prevEntry?.isPinned ? 'History' : 'Recent copies'}
                                </div>
                            {/if}
                            <!-- Cleanup Wave 1.1 (2026-05-28): each row is
                                 now wrapped in a positioning container so
                                 the inline action buttons (wand / copy /
                                 pin / delete) can sit on top of the
                                 right edge without nesting buttons inside
                                 the row button (which is invalid HTML). -->
                            <div
                                class="cmd-row-wrap"
                                class:is-pinned={entry.isPinned}
                                class:is-selected-wrap={selectables[selectedIndex]?.key ===
                                    `clip:${entry.id}`}
                            >
                            <button
                                type="button"
                                class="cmd-row cmd-row-with-actions"
                                class:is-selected={selectables[selectedIndex]?.key ===
                                    `clip:${entry.id}`}
                                class:is-sensitive={isSensitive}
                                data-cmd-key={`clip:${entry.id}`}
                                onmousedown={(e) => {
                                    // Prevent the button from claiming focus on
                                    // mousedown — the previously-focused app
                                    // (where the user wants the paste to land)
                                    // must still own focus when the backend's
                                    // SetForegroundWindow + SendInput fires.
                                    // Click still dispatches normally. Without
                                    // this, mouse paste silently fails because
                                    // SendInput targets the (focused) palette.
                                    e.preventDefault();
                                }}
                                onclick={() => void pasteClipboardEntry(entry)}
                            >
                                <span
                                    class="cmd-row-icon"
                                    class:is-color-swatch={isColor}
                                    style={isColor ? `background: ${entry.text.trim()};` : ''}
                                    aria-hidden="true"
                                >
                                    {#if entry.kind === 'image' && (entry.thumbnailPath || entry.imagePath)}
                                        <img
                                            src={convertFileSrc(
                                                entry.thumbnailPath ?? entry.imagePath ?? '',
                                            )}
                                            alt=""
                                            class="cmd-row-thumb"
                                            draggable="false"
                                        />
                                    {:else if isColor}
                                        <!-- The tile background IS the color. No
                                             inner icon — the swatch IS the icon. -->
                                    {:else}
                                        <CatIcon class="cmd-row-icon-svg" />
                                    {/if}
                                </span>
                                <span class="cmd-row-text">
                                    <span class="cmd-row-title">
                                        <!-- Sensitive content is wrapped in a
                                             dedicated span so we can blur ONLY
                                             the actual value, leaving the
                                             badge and meta line clearly
                                             readable. Image entries can't be
                                             blurred this way (they're handled
                                             above as a thumb); they don't
                                             carry sensitive content in
                                             practice. -->
                                        {#if entry.kind === 'image'}
                                            {entry.imageFormat?.toUpperCase() ?? 'Image'} clipboard
                                        {:else}
                                            <span class="cmd-row-secret">
                                                {clipboardPreview(entry.text)}
                                            </span>
                                        {/if}
                                        {#if isSensitive}
                                            <!-- Phase 6.5-5 (2026-05-27):
                                                 surface the specific kind
                                                 instead of generic "Sensitive".
                                                 First non-universal kind wins
                                                 (sensitive_kinds[0] is the
                                                 "sensitive" tag); fall back to
                                                 generic if for some reason
                                                 only that tag is present. -->
                                            {@const primaryKind = entry.sensitiveKinds.find(
                                                (k) => k !== 'sensitive',
                                            )}
                                            <span
                                                class="cmd-row-sensitive"
                                                title="Contains: {entry.sensitiveKinds
                                                    .filter((k) => k !== 'sensitive')
                                                    .map(sensitiveKindLabel)
                                                    .join(', ')} — open preview to see full value"
                                            >
                                                {primaryKind ? sensitiveKindLabel(primaryKind) : 'Sensitive'}
                                            </span>
                                        {/if}
                                    </span>
                                    <span class="cmd-row-sub">
                                        {clipboardFormatRelative(entry.capturedAtMs)} ·
                                        {clipboardCategoryLabel(entry.category)}
                                        {#if entry.isPinned}
                                            · <span class="cmd-row-pinned">Pinned</span>
                                        {/if}
                                    </span>
                                </span>
                            </button>
                            <!-- Cleanup Wave 1.1 (2026-05-28): per-row
                                 inline actions — Wand2 transform menu (text
                                 entries only), Copy back to clipboard,
                                 Pin/Unpin, Delete. Mirrors the workspace
                                 ClipboardHistory page so the palette and
                                 the full tool feel identical. Each action
                                 button uses onmousedown.preventDefault() +
                                 stopPropagation() to keep focus on the
                                 user's previous app AND prevent the row's
                                 paste-on-click from firing. -->
                            <div class="cmd-row-actions">
                                {#if entry.kind === 'text'}
                                    <ClipboardActionMenu text={entry.text} category={entry.category} />
                                {/if}
                                <button
                                    type="button"
                                    class="cmd-row-action-btn"
                                    title="Copy to clipboard"
                                    aria-label="Copy to clipboard"
                                    onmousedown={(e) => e.preventDefault()}
                                    onclick={(e) => {
                                        e.stopPropagation();
                                        void copyClipboardEntry(entry);
                                    }}
                                >
                                    <Copy class="cmd-row-action-ico" />
                                </button>
                                <button
                                    type="button"
                                    class="cmd-row-action-btn"
                                    title={entry.isPinned ? 'Unpin' : 'Pin'}
                                    aria-label={entry.isPinned ? 'Unpin' : 'Pin'}
                                    onmousedown={(e) => e.preventDefault()}
                                    onclick={(e) => {
                                        e.stopPropagation();
                                        void toggleClipboardPin(entry);
                                    }}
                                >
                                    {#if entry.isPinned}
                                        <PinOff class="cmd-row-action-ico" />
                                    {:else}
                                        <Pin class="cmd-row-action-ico" />
                                    {/if}
                                </button>
                                <button
                                    type="button"
                                    class="cmd-row-action-btn is-danger"
                                    title="Delete"
                                    aria-label="Delete"
                                    onmousedown={(e) => e.preventDefault()}
                                    onclick={(e) => {
                                        e.stopPropagation();
                                        void deleteClipboardEntry(entry);
                                    }}
                                >
                                    <Trash2 class="cmd-row-action-ico" />
                                </button>
                            </div>
                            </div>
                        {/each}
                    {/if}
                </div>
            {:else if mode === 'voice'}
                <div class="cmd-section cmd-voice-section">
                    <!-- Sub-mode toggle. Transcribe = speech → input;
                         Command = speech → execute command via registry;
                         Dictate = speech → paste into prev app (Cleanup
                         Wave 1, 2026-05-28). -->
                    <div class="cmd-voice-sub">
                        <button
                            type="button"
                            class="cmd-voice-sub-btn"
                            class:is-on={voiceSubMode === 'transcribe'}
                            onclick={() => setVoiceSubMode('transcribe')}
                        >
                            <Mic class="cmd-voice-sub-ico" />
                            Transcribe
                        </button>
                        <button
                            type="button"
                            class="cmd-voice-sub-btn"
                            class:is-on={voiceSubMode === 'command'}
                            onclick={() => setVoiceSubMode('command')}
                        >
                            <Zap class="cmd-voice-sub-ico" />
                            Command
                        </button>
                        <button
                            type="button"
                            class="cmd-voice-sub-btn"
                            class:is-on={voiceSubMode === 'dictate'}
                            onclick={() => setVoiceSubMode('dictate')}
                            title="Speak a sentence and have it pasted into the app you were just using"
                        >
                            <Tag class="cmd-voice-sub-ico" />
                            Dictate
                        </button>
                    </div>

                    <!-- Listening visual — uses the voiceSession store
                         state so it stays accurate across mic arbitration. -->
                    <div class="cmd-voice-state">
                        {#if $voiceSession.listening}
                            <span class="cmd-voice-pulse" aria-hidden="true"></span>
                            <span class="cmd-voice-state-label">Listening…</span>
                        {:else if $voiceSession.active}
                            <span class="cmd-voice-dot" aria-hidden="true"></span>
                            <span class="cmd-voice-state-label cmd-voice-state-dim">
                                Voice armed — speak any time
                            </span>
                        {:else}
                            <span class="cmd-voice-dot cmd-voice-dot-off" aria-hidden="true"
                            ></span>
                            <span class="cmd-voice-state-label cmd-voice-state-dim">
                                Voice idle
                            </span>
                        {/if}
                    </div>

                    {#if voicePartialText}
                        <!-- Ghost partial — italic, muted, communicates
                             "this is in flight, not final yet". -->
                        <div class="cmd-voice-partial">
                            <Mic class="cmd-voice-partial-ico" />
                            <span>{voicePartialText}…</span>
                        </div>
                    {/if}

                    {#if voiceOutcome}
                        <!-- Last command outcome (self-clears after 4s). -->
                        <div class="cmd-voice-outcome">
                            {voiceOutcome}
                        </div>
                    {/if}

                    <!-- Mode-specific content. -->
                    {#if voiceSubMode === 'transcribe'}
                        {#if query.trim()}
                            <div class="cmd-voice-hint">
                                <Sparkles class="cmd-voice-hint-ico" />
                                <span>
                                    Search results below update as you speak.
                                </span>
                            </div>
                            <!-- Reuse the default-mode result sections so
                                 voice-driven search behaves identically. -->
                            {#if dedupedLaunchResults.length}
                                <div class="cmd-voice-results-block">
                                    <div class="cmd-section-label">
                                        <AppWindow class="cmd-section-label-ico" />
                                        <span>INSTALLED APPS</span>
                                    </div>
                                    {#each dedupedLaunchResults.slice(0, 5) as app (app.id)}
                                        <button
                                            type="button"
                                            class="cmd-row"
                                            onclick={() => void launchApp(app.path)}
                                        >
                                            <span class="cmd-row-icon" aria-hidden="true">
                                                <AppWindow class="cmd-row-icon-svg" />
                                            </span>
                                            <span class="cmd-row-text">
                                                <span class="cmd-row-title">{app.name}</span>
                                                <span class="cmd-row-sub">{app.path}</span>
                                            </span>
                                        </button>
                                    {/each}
                                </div>
                            {/if}
                            <!-- KeepItLocal tool matches in voice Transcribe —
                                 dictating "JSON formatter" should surface the
                                 actual tool just like typing it does. Without
                                 this section the voice user only sees apps +
                                 files, which is asymmetric vs default mode. -->
                            {#if keepitlocalMatches.length}
                                <div class="cmd-voice-results-block">
                                    <div class="cmd-section-label">
                                        <Wrench class="cmd-section-label-ico" />
                                        <span>KEEPITLOCAL TOOLS</span>
                                    </div>
                                    {#each keepitlocalMatches.slice(0, 5) as tool (tool.id)}
                                        {@const resolved = toolForId(tool.id)}
                                        {@const Icon = resolved?.icon ?? Wrench}
                                        <button
                                            type="button"
                                            class="cmd-row"
                                            onclick={() => void openMainAtTool(tool.id)}
                                        >
                                            <span class="cmd-row-icon" aria-hidden="true">
                                                <Icon class="cmd-row-icon-svg" />
                                            </span>
                                            <span class="cmd-row-text">
                                                <span class="cmd-row-title">{tool.name}</span>
                                                <span class="cmd-row-sub">{tool.description}</span>
                                            </span>
                                        </button>
                                    {/each}
                                </div>
                            {/if}
                            {#if fileResults.length}
                                <div class="cmd-voice-results-block">
                                    <div class="cmd-section-label">
                                        <FileText class="cmd-section-label-ico" />
                                        <span>FILES</span>
                                    </div>
                                    {#each dedupedFileResults.slice(0, 6) as file (file.path)}
                                        <button
                                            type="button"
                                            class="cmd-row"
                                            onclick={() => void openFile(file.path)}
                                        >
                                            <span class="cmd-row-icon" aria-hidden="true">
                                                {#if file.entryType === 'folder'}
                                                    <Folder class="cmd-row-icon-svg" />
                                                {:else}
                                                    <FileText class="cmd-row-icon-svg" />
                                                {/if}
                                            </span>
                                            <span class="cmd-row-text">
                                                <span class="cmd-row-title">
                                                    {file.fileName ||
                                                        file.path.split(/[\\/]/).pop()}
                                                </span>
                                                <span class="cmd-row-sub">{file.path}</span>
                                            </span>
                                        </button>
                                    {/each}
                                </div>
                            {/if}
                        {:else}
                            <div class="cmd-empty">
                                <Mic class="cmd-empty-ico cmd-empty-ico-accent" />
                                <p class="cmd-empty-text">Say what you want to find</p>
                                <p class="cmd-empty-sub">
                                    e.g., "annual report", "open chrome", "tax pdf"
                                </p>
                            </div>
                        {/if}
                    {:else if voiceSubMode === 'command'}
                        <!-- Command mode — discovery hints + recent
                             outcome (shown above). The actual command
                             routing is wired through commandRegistry's
                             executeVoiceCommand. -->
                        <div class="cmd-voice-hint">
                            <Zap class="cmd-voice-hint-ico" />
                            <span>
                                Say a command to control KeepItLocal.
                            </span>
                        </div>
                        <div class="cmd-voice-cmd-hints">
                            <div class="cmd-section-label">EXAMPLES</div>
                            <div class="cmd-voice-cmd-row">"Open Chrome"</div>
                            <div class="cmd-voice-cmd-row">"Switch to clipboard"</div>
                            <div class="cmd-voice-cmd-row">"Scroll down"</div>
                            <div class="cmd-voice-cmd-row">"Go to settings"</div>
                            <div class="cmd-voice-cmd-row">"Search for invoices"</div>
                        </div>

                        <!-- Continuous (hands-free) command mode.
                             Restores the ON switch that lived in the voice
                             overlay until Cleanup Wave 1 (87306ef, 2026-05-28)
                             deleted that route — leaving commandMode.ts fully
                             built with a stop button and no way to start.
                             The sub-mode above is one-shot + unconstrained;
                             this is continuous + grammar-constrained, so they
                             are different jobs, not duplicates.
                             requestToggleCommandMode is safe from any window:
                             it emits a request the main-window coordinator
                             owns (it refuses to start while main is in the
                             tray — no background recording). -->
                        <button
                            type="button"
                            class="cmd-voice-cont"
                            class:is-on={$commandMode.active}
                            onclick={() => requestToggleCommandMode()}
                            aria-pressed={$commandMode.active}
                        >
                            <Radio class="cmd-voice-cont-ico" />
                            <span class="cmd-voice-cont-label">
                                {$commandMode.active
                                    ? 'Listening continuously — click to stop'
                                    : 'Keep listening for commands'}
                            </span>
                        </button>
                        <p class="cmd-voice-cont-note">
                            Hands-free until you stop it or the app goes to the tray. Never
                            persists across restarts.
                        </p>
                    {:else}
                        <!-- Cleanup Wave 1 (2026-05-28): dictate mode body —
                             speech goes to the previously focused app, not
                             the palette. Hint explains the workflow. The
                             actual dictate-to-paste plumbing is in
                             voiceUnlistenTranscript above. -->
                        <div class="cmd-voice-hint">
                            <Tag class="cmd-voice-hint-ico" />
                            <span>
                                Speak a sentence — it'll be pasted into whatever app
                                you were just using when you opened the palette.
                            </span>
                        </div>
                        <div class="cmd-voice-cmd-hints">
                            <div class="cmd-section-label">EDITING COMMANDS</div>
                            <div class="cmd-voice-cmd-row">"comma" → ,</div>
                            <div class="cmd-voice-cmd-row">"new line" → ↵</div>
                            <div class="cmd-voice-cmd-row">"scratch that" → undo last phrase</div>
                            <div class="cmd-voice-cmd-row">"period" → .</div>
                        </div>
                    {/if}
                </div>
            {/if}
        </div>
        {/key}
        </div>

        {#if previewOpen}
            <!-- ─── Preview pane ─────────────────────────────────────
                 Right-side pane, 320 px wide, shows kind-specific
                 detail for the selected row. macOS QuickLook feel. -->
            <aside class="cmd-preview" aria-label="Preview">
                {#if !previewItem}
                    <div class="cmd-preview-empty">
                        <Eye class="cmd-preview-empty-ico" />
                        <p class="cmd-preview-empty-text">
                            Select an item to preview
                        </p>
                    </div>
                {:else if previewItem.kind === 'clipboard'}
                    {@const e = previewItem.entry}
                    <header class="cmd-preview-head">
                        <span class="cmd-preview-kind">Clipboard</span>
                        <span class="cmd-preview-meta-line">
                            {clipboardCategoryLabel(e.category)}
                            · {clipboardFormatRelative(e.capturedAtMs)}
                        </span>
                    </header>
                    {#if (e.sensitiveKinds?.length ?? 0) > 0}
                        <!-- Mirrors the row badge — surfaces the same
                             warning at the top of the preview so the
                             user knows the unblurred content below is
                             potentially-sensitive. The preview pane
                             intentionally shows the full plaintext
                             value: this is the "open it to verify"
                             affordance the row-blur points at. -->
                        <div class="cmd-preview-sensitive">
                            <span class="cmd-preview-sensitive-pill">Sensitive</span>
                            <span class="cmd-preview-sensitive-text">
                                Detected: {e.sensitiveKinds.join(', ')}
                            </span>
                        </div>
                    {/if}
                    {#if e.category === 'color'}
                        <!-- Color category: render the value as an actual
                             swatch. CSS parses any valid color string
                             (#RRGGBB, rgb(), hsl(), named) via inline
                             background. Inner ring + light/dark checker
                             tints keep light or transparent values from
                             blending into the panel. -->
                        <div class="cmd-preview-color-wrap">
                            <div
                                class="cmd-preview-color-swatch"
                                style={`background: ${e.text.trim()};`}
                                aria-label={`Color preview ${e.text.trim()}`}
                            ></div>
                        </div>
                        <div class="cmd-preview-color-value">
                            {e.text.trim()}
                        </div>
                        <dl class="cmd-preview-meta">
                            <div class="cmd-preview-meta-row">
                                <dt>Format</dt>
                                <dd>{detectColorFormat(e.text)}</dd>
                            </div>
                            <div class="cmd-preview-meta-row">
                                <dt>Length</dt>
                                <dd>{e.text.length} chars</dd>
                            </div>
                            {#if e.sourceApp}
                                <div class="cmd-preview-meta-row">
                                    <dt>From</dt>
                                    <dd class="cmd-source-from">
                                        {#if e.sourceAppPath && entryIcons[e.sourceAppPath]}
                                            <img
                                                class="cmd-source-icon"
                                                src={entryIcons[e.sourceAppPath]}
                                                alt={e.sourceApp}
                                                title={e.sourceApp}
                                                draggable="false"
                                            />
                                        {:else}
                                            <AppWindow class="cmd-source-icon-glyph" />
                                        {/if}
                                    </dd>
                                </div>
                            {/if}
                        </dl>
                    {:else if e.kind === 'image' && (e.thumbnailPath || e.imagePath)}
                        <!-- Click to zoom, mirroring the file-image preview.
                             Was a bare <img> with no handler, so clicking a
                             copied screenshot did nothing. -->
                        <button
                            type="button"
                            class="cmd-preview-image-wrap"
                            onclick={togglePreviewFullscreen}
                            title={previewFullscreen
                                ? 'Exit fullscreen (Esc)'
                                : 'Fullscreen (Esc to exit)'}
                        >
                            <img
                                src={convertFileSrc(e.imagePath ?? e.thumbnailPath ?? '')}
                                alt={`Clipboard ${e.imageFormat ?? ''} from ${e.sourceApp ?? 'unknown source'}`}
                                class="cmd-preview-image"
                                draggable="false"
                            />
                        </button>
                        <dl class="cmd-preview-meta">
                            {#if e.imageFormat}
                                <div class="cmd-preview-meta-row">
                                    <dt>Format</dt>
                                    <dd>{e.imageFormat.toUpperCase()}</dd>
                                </div>
                            {/if}
                            {#if e.imageWidth && e.imageHeight}
                                <div class="cmd-preview-meta-row">
                                    <dt>Dimensions</dt>
                                    <dd>{e.imageWidth} × {e.imageHeight}</dd>
                                </div>
                            {/if}
                            {#if e.imageSizeBytes}
                                <div class="cmd-preview-meta-row">
                                    <dt>Size</dt>
                                    <dd>{formatPreviewBytes(e.imageSizeBytes)}</dd>
                                </div>
                            {/if}
                            {#if e.sourceApp}
                                <div class="cmd-preview-meta-row">
                                    <dt>From</dt>
                                    <dd class="cmd-source-from">
                                        {#if e.sourceAppPath && entryIcons[e.sourceAppPath]}
                                            <img
                                                class="cmd-source-icon"
                                                src={entryIcons[e.sourceAppPath]}
                                                alt={e.sourceApp}
                                                title={e.sourceApp}
                                                draggable="false"
                                            />
                                        {:else}
                                            <AppWindow class="cmd-source-icon-glyph" />
                                        {/if}
                                    </dd>
                                </div>
                            {/if}
                        </dl>
                    {:else}
                        <pre class="cmd-preview-text">{e.text}</pre>
                        <dl class="cmd-preview-meta">
                            <div class="cmd-preview-meta-row">
                                <dt>Characters</dt>
                                <dd>{e.text.length.toLocaleString('en-US')}</dd>
                            </div>
                            <div class="cmd-preview-meta-row">
                                <dt>Lines</dt>
                                <dd>{e.text.split(/\r?\n/).length}</dd>
                            </div>
                            {#if e.sourceApp}
                                <div class="cmd-preview-meta-row">
                                    <dt>From</dt>
                                    <dd class="cmd-source-from">
                                        {#if e.sourceAppPath && entryIcons[e.sourceAppPath]}
                                            <img
                                                class="cmd-source-icon"
                                                src={entryIcons[e.sourceAppPath]}
                                                alt={e.sourceApp}
                                                title={e.sourceApp}
                                                draggable="false"
                                            />
                                        {:else}
                                            <AppWindow class="cmd-source-icon-glyph" />
                                        {/if}
                                    </dd>
                                </div>
                            {/if}
                            {#if e.sensitiveKinds && e.sensitiveKinds.length > 0}
                                <div class="cmd-preview-meta-row">
                                    <dt>Flags</dt>
                                    <dd class="cmd-preview-warn">
                                        Sensitive content detected
                                    </dd>
                                </div>
                            {/if}
                        </dl>
                    {/if}
                    {@render previewActionBar()}
                {:else if previewItem.kind === 'file'}
                    {@const f = previewItem.result}
                    {@const name = f.fileName || f.path.split(/[\\/]/).pop()}
                    <!-- V2: preview header + path removed. Open and Fullscreen
                         live in the Ctrl+Space actions menu; Name/Where/Type/Size
                         are in the metadata list below the media. -->

                    <!-- Folders now flow through fileStage too (folder-content
                         listing branch), so the fullscreen overlay — which
                         re-renders fileStage — shows the same listing. -->
                    <div class="cmd-stage-host">
                        {@render fileStage(f, name)}
                    </div>

                    {#if f.entryType !== 'folder'}
                        <!-- V2: a clean metadata list below the media — Name /
                             Where / Type / Size — so the preview reads as
                             "designed", not dumped. Hairline dividers, grey
                             labels, right-aligned values. -->
                        <dl class="cmd-preview-meta cmd-preview-meta-file">
                            <div class="cmd-preview-meta-row">
                                <dt>Name</dt>
                                <dd>{name}</dd>
                            </div>
                            <div class="cmd-preview-meta-row">
                                <dt>Where</dt>
                                <dd class="cmd-preview-path">
                                    {f.path.replace(/[\\/][^\\/]*$/, '') || f.path}
                                </dd>
                            </div>
                            {#if f.extension}
                                <div class="cmd-preview-meta-row">
                                    <dt>Type</dt>
                                    <dd>{previewLanguageLabel(f.extension)}</dd>
                                </div>
                            {/if}
                            {#if f.size > 0}
                                <div class="cmd-preview-meta-row">
                                    <dt>Size</dt>
                                    <dd class="cmd-preview-num">{formatBytes(f.size)}</dd>
                                </div>
                            {/if}
                        </dl>
                    {/if}
                    {@render previewActionBar()}
                {:else if previewItem.kind === 'app'}
                    {@const a = previewItem.result}
                    <header class="cmd-preview-head">
                        <span class="cmd-preview-kind">Installed app</span>
                    </header>
                    <h3 class="cmd-preview-title">{a.name}</h3>
                    <dl class="cmd-preview-meta">
                        <div class="cmd-preview-meta-row">
                            <dt>Path</dt>
                            <dd class="cmd-preview-path">{a.path}</dd>
                        </div>
                        {#if a.kind}
                            <div class="cmd-preview-meta-row">
                                <dt>Kind</dt>
                                <dd>{a.kind}</dd>
                            </div>
                        {/if}
                    </dl>
                    {@render previewActionBar()}
                {:else if previewItem.kind === 'tool'}
                    <header class="cmd-preview-head">
                        <span class="cmd-preview-kind">KeepItLocal tool</span>
                    </header>
                    <h3 class="cmd-preview-title">{previewItem.name}</h3>
                    <p class="cmd-preview-desc">{previewItem.description}</p>
                    {@render previewActionBar()}
                {:else if previewItem.kind === 'core'}
                    {@const p = previewItem.pillar}
                    {@const Icon = p.icon}
                    <header class="cmd-preview-head">
                        <span class="cmd-preview-kind">Core pillar</span>
                    </header>
                    <div class="cmd-preview-hero">
                        <span class="cmd-preview-hero-icon" aria-hidden="true">
                            <Icon class="cmd-preview-hero-icon-svg" />
                        </span>
                        <h3 class="cmd-preview-title">{p.name}</h3>
                    </div>
                    <p class="cmd-preview-desc">{p.description}</p>
                    <dl class="cmd-preview-meta">
                        <div class="cmd-preview-meta-row">
                            <dt>Shortcut</dt>
                            <dd>
                                <span class="cmd-preview-kbd">{p.hotkey}</span>
                            </dd>
                        </div>
                    </dl>
                    {@render previewActionBar()}
                {:else if previewItem.kind === 'summary'}
                    {@const detail = primarySelectedAction?.hint}
                    <header class="cmd-preview-head">
                        <span class="cmd-preview-kind">{previewItem.kindLabel}</span>
                    </header>
                    <h3 class="cmd-preview-title">{previewItem.label}</h3>
                    {#if detail}
                        <p class="cmd-preview-desc">{detail}</p>
                    {/if}
                    {@render previewActionBar()}
                {:else if previewItem.kind === 'command-category'}
                    <!-- cmd-stage-fill: the category pane is a LIST surface, not a
                         file preview — it fills the pane and scrolls inside itself
                         (exempt from the 200px per-child preview cap below). -->
                    <div class="cmd-stage-host cmd-stage-fill">
                        {@render commandCategoryPane(previewItem.category)}
                    </div>
                {/if}
            </aside>
        {/if}
        </div>

        <!-- ─── Action panel (Ctrl+Space) ────────────────────────
             Raycast-style floating popup anchored bottom-right of
             the body. Lists every verb available for the currently
             selected row. Closed via Esc, or by activating an
             action (terminal actions only — copies keep the panel
             open so the "Copied!" feedback line is visible). -->
        {#if actionsOpen && availableActions.length}
            <!-- Backdrop catches outside clicks. Click-through is
                 disabled so accidental clicks on the body don't
                 launch the selected app instead of closing the
                 panel. -->
            <button
                type="button"
                class="cmd-actions-backdrop"
                onclick={closeActionsPanel}
                aria-label="Close actions"
                tabindex="-1"
            ></button>
            <div
                class="cmd-actions"
                role="group"
                aria-label="Actions for selected item"
            >
                <header class="cmd-actions-head">
                    <span class="cmd-actions-title">Actions</span>
                    <span class="cmd-actions-hint">
                        <kbd>↑</kbd><kbd>↓</kbd> <kbd>↵</kbd> <kbd>Esc</kbd>
                    </span>
                </header>
                <ul class="cmd-actions-list">
                    {#each availableActions as action, i (action.id)}
                        {@const ActionIcon = action.icon}
                        <li>
                            <button
                                type="button"
                                class="cmd-actions-item"
                                class:is-selected={i === actionsSelectedIndex}
                                onmousedown={(e) => {
                                    // Same focus-theft fix as clipboard rows —
                                    // some actions (paste) need the originally
                                    // focused app to keep focus so SendInput
                                    // targets it.
                                    e.preventDefault();
                                }}
                                onclick={() => {
                                    void action.activate();
                                    if (!action.id.startsWith('copy')) {
                                        closeActionsPanel();
                                    }
                                }}
                                onmouseenter={() => {
                                    actionsSelectedIndex = i;
                                }}
                                onfocus={() => {
                                    actionsSelectedIndex = i;
                                }}
                            >
                                <span class="cmd-actions-icon" aria-hidden="true">
                                    <ActionIcon class="cmd-actions-icon-svg" />
                                </span>
                                <span class="cmd-actions-text">
                                    <span class="cmd-actions-label">{action.label}</span>
                                    {#if action.hint}
                                        <span class="cmd-actions-sublabel">{action.hint}</span>
                                    {/if}
                                </span>
                                <ChevronRight class="cmd-actions-chevron" aria-hidden="true" />
                            </button>
                        </li>
                    {/each}
                </ul>
                {#if actionFeedback}
                    <div class="cmd-actions-feedback" role="status" aria-live="polite">
                        ✓ {actionFeedback}
                    </div>
                {/if}
            </div>
        {/if}

        <!-- ─── Footer — contextual hints + persistent offline ──── -->
        <footer class="cmd-foot">
            <div class="cmd-foot-hints">
                <!-- V2: two hints only — the verbs that matter in context. The
                     full shortcut set still works (Ctrl+P preview, Ctrl+/ syntax,
                     Ctrl+Alt+A design, Ctrl+Alt+←/→ Files/Inside); it's just no
                     longer a persistent cheat sheet crowding the footer. -->
                <span class="cmd-hint">
                    <kbd>↵</kbd>
                    {mode === 'clipboard'
                        ? 'Paste'
                        : mode === 'default' && selectedSelectable && !selectedSelectable.opens
                          ? 'Copy'
                          : 'Open'}
                </span>
                {#if availableActions.length > 1}
                    <span class="cmd-hint" class:cmd-hint-active={actionsOpen}>
                        <kbd>Ctrl+Space</kbd>
                        {actionsOpen ? 'Close' : 'Actions'}
                    </span>
                {/if}
                <!-- The one permanent hint: the way to find every other
                     binding. Keeps the footer calm while making the rest
                     discoverable — that was the gap the 9→2 cut left. -->
                <button
                    type="button"
                    class="cmd-hint cmd-hint-btn"
                    class:cmd-hint-active={shortcutsOpen}
                    onclick={toggleShortcutsPanel}
                >
                    <kbd>Ctrl+Alt+I</kbd>
                    Shortcuts
                </button>
            </div>
            <div class="cmd-foot-status">
                {#if mode === 'voice' && $voiceSession.listening}
                    <span class="cmd-foot-listening">Listening…</span>
                {/if}
                <span class="cmd-foot-offline">
                    <Lock class="cmd-foot-offline-ico" />
                    Offline
                </span>
            </div>
        </footer>

        <!-- Clipboard label editor (Alt+L). Overlays the panel; a label
             pins the entry (the backend keeps pin_label only on pinned
             entries). Enter saves / Esc cancels via onKeydown's guard. -->
        {#if labelEditEntry}
            <div
                class="cmd-label-backdrop"
                role="presentation"
                onclick={(event) => { if (event.target === event.currentTarget) cancelLabel(); }}
            >
                <div
                    class="cmd-label-modal"
                    bind:this={labelDialogEl}
                    role="dialog"
                    aria-modal="true"
                    aria-label="Label clipboard entry"
                    tabindex="-1"
                >
                    <div class="cmd-label-title">Label this entry</div>
                    <input
                        bind:this={labelInputEl}
                        bind:value={labelDraft}
                        class="cmd-label-input"
                        placeholder="e.g. Work email signature"
                        spellcheck="false"
                        autocomplete="off"
                        maxlength="80"
                        aria-label="Clipboard label"
                    />
                    <div class="cmd-label-hint">Labeling pins this entry so it stays in your history.</div>
                    <div class="cmd-label-actions">
                        <button type="button" class="cmd-label-btn" onclick={() => cancelLabel()}>Cancel</button>
                        <button
                            type="button"
                            class="cmd-label-btn cmd-label-btn-primary"
                            onclick={() => void saveLabel()}
                        >
                            Save
                        </button>
                    </div>
                </div>
            </div>
        {/if}

        <!-- ─── Fullscreen preview overlay (2026-05-31) ───────────────
             Covers the palette window so any previewable file fills the
             whole window — without closing the palette (USER RULE). Same
             `fileStage` content as the inline pane, just unconstrained.
             Esc exits (onKeydown guard runs before the palette-close path);
             the ✕ button and clicking an image also exit. -->
        {#if previewFullscreen && previewItem && previewItem.kind === 'file'}
            {@const f = previewItem.result}
            {@const name = f.fileName || f.path.split(/[\\/]/).pop()}
            <div
                class="cmd-preview-full"
                bind:this={fullscreenDialogEl}
                role="dialog"
                aria-modal="true"
                aria-label="Fullscreen preview"
                tabindex="-1"
            >
                <header class="cmd-preview-full-bar">
                    <div class="cmd-preview-full-meta">
                        <span class="cmd-preview-full-name">{name}</span>
                        {#if f.extension && !isNoteFile(f.extension)}
                            <span class="cmd-preview-ext">{f.extension.toUpperCase()}</span>
                        {/if}
                    </div>
                    <div class="cmd-preview-full-actions">
                        <button
                            type="button"
                            class="cmd-preview-full-btn"
                            onclick={() => void openFile(f.path)}
                            title="Open in default app"
                        >
                            <ExternalLink class="cmd-preview-full-btn-ico" />
                            Open
                        </button>
                        <button
                            type="button"
                            class="cmd-preview-full-btn cmd-preview-full-close"
                            bind:this={fullscreenCloseEl}
                            onclick={closePreviewFullscreen}
                            title="Exit fullscreen (Esc)"
                            aria-label="Exit fullscreen"
                        >
                            <XIcon class="cmd-preview-full-btn-ico" />
                        </button>
                    </div>
                </header>
                <div class="cmd-preview-full-stage cmd-stage-host">
                    {@render fileStage(f, name)}
                </div>
            </div>
        {/if}

        {#if previewFullscreen && previewItem?.kind === 'clipboard' && previewItem.entry.kind === 'image' && (previewItem.entry.imagePath || previewItem.entry.thumbnailPath)}
            {@const c = previewItem.entry}
            <!-- Fullscreen zoom for a copied image. The file overlay above
                 reads previewItem.result (a FileSearchResultItem); a clipboard
                 entry has no such shape, so it needs its own branch reading
                 previewItem.entry. -->
            <div
                class="cmd-preview-full"
                bind:this={fullscreenDialogEl}
                role="dialog"
                aria-modal="true"
                aria-label="Fullscreen clipboard image"
                tabindex="-1"
            >
                <header class="cmd-preview-full-bar">
                    <div class="cmd-preview-full-meta">
                        <span class="cmd-preview-full-name"
                            >Clipboard image{c.sourceApp ? ` — ${c.sourceApp}` : ''}</span
                        >
                        {#if c.imageFormat}
                            <span class="cmd-preview-ext">{c.imageFormat.toUpperCase()}</span>
                        {/if}
                    </div>
                    <div class="cmd-preview-full-actions">
                        <button
                            type="button"
                            class="cmd-preview-full-btn cmd-preview-full-close"
                            bind:this={fullscreenCloseEl}
                            onclick={closePreviewFullscreen}
                            title="Exit fullscreen (Esc)"
                            aria-label="Exit fullscreen"
                        >
                            <XIcon class="cmd-preview-full-btn-ico" />
                        </button>
                    </div>
                </header>
                <div class="cmd-preview-full-stage cmd-stage-host">
                    <img
                        src={convertFileSrc(c.imagePath ?? c.thumbnailPath ?? '')}
                        alt={`Clipboard ${c.imageFormat ?? ''} from ${c.sourceApp ?? 'unknown source'}`}
                        class="cmd-preview-image"
                        draggable="false"
                    />
                </div>
            </div>
        {/if}

        <!-- ToastContainer (Wave 2.2 fix, 2026-05-26). Mounted only on the
             standalone palette window — when the palette is embedded inside
             the main window, the main window's existing ToastContainer
             handles output, so adding ours there would duplicate every
             toast. Without this, every toast(…) call from the palette's
             system-command handlers landed in the palette webview's local
             store with no subscriber to render it, which looked like
             "nothing happened" to the user. -->
        {#if isCommandWindow}
            <ToastContainer />
        {/if}

        <!-- Destructive system-command confirmation. An IN-APP modal (not a
             native dialog) because native dialogs are unreliable from this
             transparent always-on-top window. It opens focused on Cancel;
             Escape and the backdrop both cancel. -->
        {#if pendingConfirm}
            <div
                class="cmd-confirm-backdrop"
                role="presentation"
                onclick={(event) => { if (event.target === event.currentTarget) cancelPendingSystemCommand(); }}
            >
                <div
                    class="cmd-confirm-modal"
                    bind:this={confirmDialogEl}
                    role="alertdialog"
                    aria-modal="true"
                    aria-labelledby="cmd-confirm-title"
                    aria-describedby="cmd-confirm-desc"
                    tabindex="-1"
                >
                    <div class="cmd-confirm-icon" aria-hidden="true">
                        <AlertTriangle class="cmd-confirm-glyph" />
                    </div>
                    <div class="cmd-confirm-title" id="cmd-confirm-title">
                        {pendingConfirm.title}
                    </div>
                    <div class="cmd-confirm-desc" id="cmd-confirm-desc">
                        {pendingConfirm.description}
                    </div>
                    <div class="cmd-confirm-actions">
                        <button
                            type="button"
                            bind:this={confirmCancelEl}
                            class="cmd-confirm-btn"
                            onclick={() => cancelPendingSystemCommand()}
                        >
                            Cancel
                        </button>
                        <button
                            type="button"
                            class="cmd-confirm-btn"
                            class:cmd-confirm-btn-danger={pendingConfirm.danger}
                            onclick={() => confirmPendingSystemCommand()}
                        >
                            {pendingConfirm.title.replace(/\?$/, '')}
                        </button>
                    </div>
                    <div class="cmd-confirm-hint">
                        <kbd>Enter</kbd> activates focused action · <kbd>Esc</kbd> cancel
                    </div>
                </div>
            </div>
        {/if}

        <!-- Live appearance editor (Ctrl+Alt+A). Bottom-anchored (no scrim) so
             the palette stays visible behind it and every tweak — opacity,
             accent, blur — previews instantly. Edits the same store as
             Settings → Appearance, so changes persist + sync there too. -->
        {#if appearanceOpen}
            <div class="cmd-appear" role="dialog" aria-label="Palette appearance">
                <div class="cmd-appear-head">
                    <Palette class="cmd-appear-head-ico" />
                    <span class="cmd-appear-title">Palette appearance</span>
                    <span class="cmd-appear-live">live</span>
                    <button
                        type="button"
                        class="cmd-appear-x"
                        bind:this={appearanceCloseEl}
                        onclick={closeAppearancePanel}
                        aria-label="Close appearance editor"
                    >
                        <X class="cmd-appear-x-ico" />
                    </button>
                </div>

                <!-- Palette Appearance Wave E (2026-05-27): style presets row.
                     One click applies a bundle of theme + density +
                     animation + glow + width settings. Lives at the top
                     of the editor as a fast on-ramp; the individual
                     controls below let the user fine-tune. -->
                <div class="cmd-appear-row cmd-appear-row-presets">
                    <span class="cmd-appear-label">Quick style</span>
                    <div class="cmd-appear-presets">
                        {#each STYLE_PRESETS as preset (preset.id)}
                            <button
                                type="button"
                                class="cmd-appear-preset"
                                onclick={() => applyStylePreset(preset)}
                                title={preset.description}
                            >
                                {preset.label}
                            </button>
                        {/each}
                    </div>
                </div>

                <div class="cmd-appear-row">
                    <span class="cmd-appear-label">Opacity</span>
                    <input
                        type="range"
                        class="cmd-appear-range"
                        min={OPACITY_MIN}
                        max={OPACITY_MAX}
                        step="0.01"
                        value={$commandAppearance.opacity}
                        oninput={(e) => setCommandOpacity(Number(e.currentTarget.value))}
                        aria-label="Panel opacity"
                    />
                    <span class="cmd-appear-val">{Math.round($commandAppearance.opacity * 100)}%</span>
                </div>

                <!-- Palette Appearance Wave B (2026-05-27): Theme picker.
                     The theme is app-wide (same setting Settings → Appearance
                     uses), so picking here changes the main window's theme
                     too — exactly what users expect. -->
                <div class="cmd-appear-row">
                    <span class="cmd-appear-label">Theme</span>
                    <div class="cmd-appear-themes">
                        {#each THEME_OPTIONS as opt (opt.id)}
                            <button
                                type="button"
                                class="cmd-appear-theme"
                                class:is-on={$settings.theme === opt.id}
                                onclick={() => applyPaletteTheme(opt.id)}
                                title={opt.description}
                                aria-label={opt.label}
                            >
                                <span
                                    class="cmd-appear-theme-swatch"
                                    style="--s-bg: {opt.swatch[0]}; --s-panel: {opt.swatch[1]}; --s-accent: {opt.swatch[2]};"
                                ></span>
                                <span class="cmd-appear-theme-label">{opt.label}</span>
                            </button>
                        {/each}
                    </div>
                </div>

                <div class="cmd-appear-row">
                    <span class="cmd-appear-label">Accent</span>
                    <div class="cmd-appear-swatches">
                        {#each CMD_ACCENT_PRESETS as preset (preset.id)}
                            <button
                                type="button"
                                class="cmd-appear-swatch"
                                class:is-on={$commandAppearance.accent === preset.hex}
                                style="--sw: {preset.hex}"
                                onclick={() => setCommandAccent(preset.hex)}
                                aria-label={`Accent ${preset.label}`}
                                title={preset.label}
                            ></button>
                        {/each}
                        <button
                            type="button"
                            class="cmd-appear-swatch cmd-appear-swatch-theme"
                            class:is-on={!$commandAppearance.accent}
                            onclick={() => setCommandAccent(null)}
                            title="Use the active theme's accent"
                            aria-label="Use the active theme's accent"
                        >
                            Aa
                        </button>
                        <input
                            type="color"
                            class="cmd-appear-color"
                            value={$commandAppearance.accent ?? '#10b981'}
                            oninput={(e) => setCommandAccent(e.currentTarget.value)}
                            aria-label="Custom accent color"
                            title="Custom color"
                        />
                    </div>
                </div>

                <!-- Palette Appearance Wave B: Density.
                     CSS variable `--cmd-row-height` (set on .cmd-root) is
                     read by the row styling; values are 28/32/36 px. -->
                <div class="cmd-appear-row">
                    <span class="cmd-appear-label">Density</span>
                    <div class="cmd-appear-segmented" role="group" aria-label="Row density">
                        {#each DENSITY_OPTIONS as opt (opt.id)}
                            <button
                                type="button"
                                class="cmd-appear-seg"
                                class:is-on={$commandAppearance.density === opt.id}
                                aria-pressed={$commandAppearance.density === opt.id}
                                onclick={() => setCommandDensity(opt.id)}
                            >
                                {opt.label}
                            </button>
                        {/each}
                    </div>
                </div>

                <div class="cmd-appear-row">
                    <span class="cmd-appear-label">Desktop blur</span>
                    <button
                        type="button"
                        class="cmd-appear-toggle"
                        class:is-on={$commandAppearance.desktopBlur}
                        onclick={() => setCommandDesktopBlur(!$commandAppearance.desktopBlur)}
                        aria-pressed={$commandAppearance.desktopBlur}
                        aria-label="Toggle desktop blur"
                        title={$commandAppearance.desktopBlur
                            ? 'Acrylic ON — corners go square to avoid wedge leaks'
                            : 'Acrylic OFF — rounded corners restored'}
                    >
                        <span class="cmd-appear-toggle-knob"></span>
                    </button>
                </div>

                <!-- Palette Appearance Wave C (2026-05-27): window width. -->
                <div class="cmd-appear-row">
                    <span class="cmd-appear-label">Width</span>
                    <div class="cmd-appear-segmented" role="group" aria-label="Palette width">
                        {#each WIDTH_OPTIONS as opt (opt.id)}
                            <button
                                type="button"
                                class="cmd-appear-seg"
                                class:is-on={$commandAppearance.width === opt.id}
                                aria-pressed={$commandAppearance.width === opt.id}
                                onclick={() => setCommandWidth(opt.id as PaletteWidth)}
                                title={`${opt.widthPx}px`}
                            >
                                {opt.label}
                            </button>
                        {/each}
                    </div>
                </div>

                <!-- Palette Appearance Wave C / H / I: window position.
                     Three sharp picks — Top / Center / Bottom — every
                     one produces obviously different on-screen position. -->
                <div class="cmd-appear-row">
                    <span class="cmd-appear-label">Position</span>
                    <div class="cmd-appear-segmented" role="group" aria-label="Palette position">
                        {#each [
                            { id: 'top', label: 'Top' },
                            { id: 'center', label: 'Center' },
                            { id: 'bottom', label: 'Bottom' },
                        ] as opt (opt.id)}
                            <button
                                type="button"
                                class="cmd-appear-seg"
                                class:is-on={$commandAppearance.position === opt.id}
                                aria-pressed={$commandAppearance.position === opt.id}
                                onclick={() => setCommandPosition(opt.id as PalettePosition)}
                            >
                                {opt.label}
                            </button>
                        {/each}
                    </div>
                </div>

                <!-- Palette Appearance Wave H (2026-05-27): show / hide
                     the category chip row beneath the search input. -->
                <div class="cmd-appear-row">
                    <span class="cmd-appear-label">Category chips</span>
                    <button
                        type="button"
                        class="cmd-appear-toggle"
                        class:is-on={$commandAppearance.showChips}
                        onclick={() => setCommandShowChips(!$commandAppearance.showChips)}
                        aria-pressed={$commandAppearance.showChips}
                        aria-label="Toggle category chips"
                        title="Show or hide scope navigation"
                    >
                        <span class="cmd-appear-toggle-knob"></span>
                    </button>
                </div>

                <!-- Palette Appearance Wave D / H / I: animation level.
                     Wave I (2026-05-27): also a visible DEMO badge — a
                     small dot that pulses with the current motion
                     duration. So picking Reduced/Default/Lively, the
                     user can SEE the pulse speed change in real time
                     without having to hover a row. -->
                <div class="cmd-appear-row">
                    <span class="cmd-appear-label">Animations</span>
                    <div class="cmd-appear-animations">
                        <div class="cmd-appear-segmented" role="group" aria-label="Animation level">
                            {#each ANIMATION_OPTIONS as opt (opt.id)}
                                <button
                                    type="button"
                                    class="cmd-appear-seg"
                                    class:is-on={$commandAppearance.animationLevel === opt.id}
                                    aria-pressed={$commandAppearance.animationLevel === opt.id}
                                    onclick={() => {
                                        setCommandAnimationLevel(opt.id as PaletteAnimationLevel);
                                        toast(`Animations: ${opt.label}`, 'success', 1500);
                                    }}
                                >
                                    {opt.label}
                                </button>
                            {/each}
                        </div>
                        <!-- Live pulse demo. The dot's transition uses
                             var(--dur-enter) which Wave H scales per
                             animation level — so its pulse speed
                             visibly mirrors the active setting. -->
                        <span
                            class="cmd-appear-pulse"
                            title="The dot's pulse uses the current animation duration — pick Reduced / Default / Lively above to see it change."
                        ></span>
                    </div>
                </div>

                <!-- Palette Appearance Wave D: accent glow toggle.
                     Adds a subtle accent-tinted outer-shadow to selected
                     rows + focus rings when ON. Off by default for the
                     calm-by-default aesthetic; on for users who want
                     their selection to feel alive. -->
                <div class="cmd-appear-row">
                    <span class="cmd-appear-label">Accent glow</span>
                    <button
                        type="button"
                        class="cmd-appear-toggle"
                        class:is-on={$commandAppearance.accentGlow}
                        onclick={() => setCommandAccentGlow(!$commandAppearance.accentGlow)}
                        aria-pressed={$commandAppearance.accentGlow}
                        aria-label="Toggle accent glow"
                        title="Subtle outer-shadow on selected items"
                    >
                        <span class="cmd-appear-toggle-knob"></span>
                    </button>
                </div>

                <!-- Palette Appearance Wave C: hide sections.
                     Checkboxes that hide individual sections the user
                     never uses. Persists in commandAppearance.
                     hiddenSections so the choice survives restart. -->
                <details class="cmd-appear-details">
                    <summary class="cmd-appear-summary">
                        <span class="cmd-appear-label">Hide sections</span>
                        <span class="cmd-appear-summary-count">
                            {$commandAppearance.hiddenSections.length} hidden
                        </span>
                    </summary>
                    <div class="cmd-appear-hide-grid">
                        {#each HIDABLE_SECTIONS as section (section.id)}
                            {@const checked = !$commandAppearance.hiddenSections.includes(section.id)}
                            <label class="cmd-appear-hide-row" title={section.hint}>
                                <input
                                    type="checkbox"
                                    {checked}
                                    onchange={() => toggleHiddenSection(section.id)}
                                />
                                <span class="cmd-appear-hide-label">{section.label}</span>
                            </label>
                        {/each}
                    </div>
                </details>

                <div class="cmd-appear-foot">
                    <button
                        type="button"
                        class="cmd-appear-reset"
                        onclick={() => resetCommandAppearance()}
                    >
                        Reset
                    </button>
                    <span class="cmd-appear-note">
                        <kbd>Ctrl+Alt+A</kbd> to close · also in Settings → Appearance
                    </span>
                </div>
            </div>
        {/if}

        <!-- Keyboard shortcuts (Ctrl+Alt+I). Same bottom-anchored, scrimless
             dialog shape as the appearance editor above — the palette stays
             visible behind it, so you can read a binding and try it without
             closing. Deliberately separate from Ctrl+/ ("Search syntax"):
             that one is what you can TYPE, this one is what you can PRESS.
             Every binding listed here is verified against onKeydown — if you
             add or change a shortcut there, update this list too. -->
        {#if shortcutsOpen}
            <div class="cmd-appear cmd-keys" role="dialog" aria-label="Keyboard shortcuts">
                <div class="cmd-appear-head">
                    <Keyboard class="cmd-appear-head-ico" />
                    <span class="cmd-appear-title">Keyboard shortcuts</span>
                    <button
                        type="button"
                        class="cmd-appear-x"
                        bind:this={shortcutsCloseEl}
                        onclick={closeShortcutsPanel}
                        aria-label="Close keyboard shortcuts"
                    >
                        <X class="cmd-appear-x-ico" />
                    </button>
                </div>

                <div class="cmd-keys-body">
                    <section class="cmd-keys-group">
                        <h3 class="cmd-keys-gtitle">Navigate</h3>
                        <div class="cmd-keys-row"><kbd>↑</kbd><kbd>↓</kbd><span>Move selection</span></div>
                        <div class="cmd-keys-row"><kbd>Home</kbd><kbd>End</kbd><span>Jump to first / last result</span></div>
                        <div class="cmd-keys-row"><kbd>Tab</kbd><kbd>Shift+Tab</kbd><span>Cycle palette scopes</span></div>
                        <div class="cmd-keys-row"><kbd>↵</kbd><span>Open, copy, or paste the selection</span></div>
                        <div class="cmd-keys-row">
                            <kbd>Esc</kbd><span>Step back one level — exit fullscreen, close a panel, clear the scope, then close the palette</span>
                        </div>
                    </section>

                    <section class="cmd-keys-group">
                        <h3 class="cmd-keys-gtitle">Panels</h3>
                        <div class="cmd-keys-row"><kbd>Ctrl+Space</kbd><span>Actions for the selected item</span></div>
                        <div class="cmd-keys-row"><kbd>Ctrl+P</kbd><span>Show / hide the preview pane</span></div>
                        <div class="cmd-keys-row"><kbd>Ctrl+/</kbd><span>Search syntax — what you can type</span></div>
                        <div class="cmd-keys-row"><kbd>Ctrl+Alt+A</kbd><span>Appearance editor (live)</span></div>
                        <div class="cmd-keys-row"><kbd>Ctrl+Alt+I</kbd><span>This list</span></div>
                    </section>

                    <section class="cmd-keys-group">
                        <h3 class="cmd-keys-gtitle">Search</h3>
                        <div class="cmd-keys-row">
                            <kbd>Ctrl+Alt+←</kbd><kbd>Ctrl+Alt+→</kbd><span>Switch between Files and Inside (file contents)</span>
                        </div>
                    </section>

                    <section class="cmd-keys-group">
                        <h3 class="cmd-keys-gtitle">Commands</h3>
                        <div class="cmd-keys-row"><kbd>→</kbd><span>Drill into the selected category</span></div>
                        <div class="cmd-keys-row"><kbd>←</kbd><span>Back out to the category list</span></div>
                    </section>

                    <section class="cmd-keys-group">
                        <h3 class="cmd-keys-gtitle">Clipboard</h3>
                        <div class="cmd-keys-row"><kbd>Alt+P</kbd><span>Pin / unpin the selected entry</span></div>
                        <div class="cmd-keys-row"><kbd>Alt+D</kbd><span>Delete the selected entry</span></div>
                        <div class="cmd-keys-row"><kbd>Alt+L</kbd><span>Label the selected entry</span></div>
                    </section>

                    <section class="cmd-keys-group">
                        <h3 class="cmd-keys-gtitle">Voice</h3>
                        <div class="cmd-keys-row">
                            <kbd>Ctrl+Alt+←</kbd><kbd>Ctrl+Alt+→</kbd><span>Cycle Transcribe / Command mode</span>
                        </div>
                    </section>
                </div>

                <div class="cmd-appear-foot">
                    <span class="cmd-appear-note">
                        <kbd>Ctrl+Alt+I</kbd> or <kbd>Esc</kbd> to close · global hotkeys live in Settings → Shortcuts
                    </span>
                </div>
            </div>
        {/if}
    </div>
</div>

<style>
    /* ─── Root + panel ────────────────────────────────────────────
       The palette is a centered card with a rounded 16-px radius.
       During development this renders inside the main window; once
       Tauri windowing is wired (3.6.6), the surrounding background
       becomes transparent. macOS feel: soft shadows, accent reserved
       for indicators, generous spacing. */
    .cmd-root {
        display: flex;
        align-items: flex-start;
        justify-content: center;
        padding: 60px 24px 24px;
        min-height: 100vh;
        background: var(--color-bg);
    }
    /* Overlay-in-main mode (Phase 3.6.5 test binding): the palette is
       layered on top of the live workspace, so instead of a solid
       background we use a translucent scrim that lets the workspace
       show through. Fixed to the viewport, above the app shell. A
       light backdrop blur softens the workspace behind so the frosted
       panel stays the focal point without fully hiding context. */
    .cmd-root.is-overlay {
        position: fixed;
        inset: 0;
        z-index: 1000;
        min-height: 0;
        background: color-mix(in srgb, var(--color-bg) 38%, transparent);
        backdrop-filter: blur(7px) saturate(120%);
        -webkit-backdrop-filter: blur(7px) saturate(120%);
        /* Gentle fade-in for the scrim so the overlay doesn't pop. The
           panel itself doesn't animate (per the overlay-window rules:
           panels mount visible to avoid flicker), only the scrim. */
        animation: cmd-scrim-in calc(140ms * var(--cmd-motion-mult, 1)) var(--ease-out, ease) both;
    }
    @keyframes cmd-scrim-in {
        from {
            opacity: 0;
        }
        to {
            opacity: 1;
        }
    }
    @media (prefers-reduced-motion: reduce) {
        .cmd-root.is-overlay {
            animation: none;
        }
    }
    /* Real dedicated Tauri overlay window (Phase 3.6.6). The OS window is
       transparent + sized exactly to the palette (760×560), so the panel
       must FILL the window — no centering padding, no max-width gap, or
       you get a frame of empty space around the panel and the bottom of
       the panel clipped (the base `.cmd-root` padding pushed it out of
       the window). This mirrors the search overlay's `.overlay-root` /
       `.overlay-panel` (panel = window minus a 2px transparent rim for
       the rounded corners). */
    .cmd-root.is-window {
        padding: 0;
        min-height: 0;
        height: 100vh;
        align-items: stretch;
        background: transparent;
        backdrop-filter: none;
        -webkit-backdrop-filter: none;
        isolation: isolate;
        overflow: hidden;
    }
    /* Panel fills the window. SOLID surface — a transparent OS window
       can't blur the desktop behind it via backdrop-filter (the desktop
       isn't in the page's compositing tree; only DWM acrylic could, and
       that's banned for leaking square corners into the rounded panel).
       So we drop the see-through + blur and use the search overlay's
       recipe: a solid panel with a CSS glass edge (hairline border +
       inset top-lit highlight) that reads as refined frosted glass
       without any real blur. 2px margin leaves a transparent rim so the
       16px rounded corners render cleanly against the desktop. */
    .cmd-root.is-window .cmd-panel {
        width: calc(100vw - 4px);
        max-width: none;
        height: calc(100vh - 4px);
        max-height: none;
        margin: 2px;
        /* Subtly translucent (~90% opaque) so a faint hint of the desktop
           tints through for a glassy feel, plus a BEST-EFFORT backdrop
           blur. Caveat: a fully-transparent OS window can't always feed
           the desktop into the webview's compositing tree, so on Windows
           WebView2 this blur may gracefully no-op to just the tint below
           (only DWM acrylic guarantees a desktop blur, and that's banned
           for leaking square corners). Where the compositor DOES support
           webview backdrop sampling, this frosts the desktop like macOS
           Vibrancy. Kept gentle (90% opaque) so a busy desktop never
           shows through sharply even if the blur no-ops. */
        /* Opacity is user-tunable (Settings → Command Palette) via the
           --cmd-panel-opacity variable set on .cmd-root; falls back to 93%. */
        background: color-mix(
            in srgb,
            var(--color-panel) var(--cmd-panel-opacity, 93%),
            transparent
        );
        backdrop-filter: blur(40px) saturate(170%);
        -webkit-backdrop-filter: blur(40px) saturate(170%);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 8%, transparent),
            0 6px 16px rgba(0, 0, 0, 0.32);
    }
    /* Preview-open shouldn't widen past the fixed window in window mode
       (the OS window is 760 wide) — keep the panel pinned to the window. */
    .cmd-root.is-window .cmd-panel:has(.cmd-body-wrap.has-preview) {
        max-width: none;
        width: calc(100vw - 4px);
    }
    /* Desktop-blur (DWM acrylic) on: let the panel fill the whole window and
       go SQUARE. The Palette Appearance phase (2026-05-27) flipped policy —
       acrylic + rounded corners leaks dark square wedges into the rounded
       edges on most Windows builds, which can't be fixed from CSS. So when
       blur is on, the backend tells DWM to use DWMWCP_DONOTROUND on the
       window, and this rule drops the CSS border-radius to 0 to match. No
       rounded corners = no wedges to leak into. The blur-off path keeps
       the 16px CSS radius + DWMWCP_DEFAULT. */
    .cmd-root.is-window.has-desktop-blur .cmd-panel {
        width: 100vw;
        max-width: none;
        height: 100vh;
        margin: 0;
        border-radius: 0;
    }
    /* macOS Vibrancy-style frosted panel. Static backdrop-filter on a
       CSS element (NOT on the OS window) — this is architecturally
       different from the DWM acrylic ban in DesignPro:
         - DWM acrylic = Windows-level effect on the Tauri window.
           Leaks square corners into rounded panels. Banned.
         - CSS backdrop-filter = pure browser compositing on a small
           element. No window-level coupling, no corner leak. Safe.
       The filter-blur ban in DesignPro is about `filter: blur` (which
       blurs the element's own pixels and is animated) — `backdrop-
       filter: blur` here is static and acts on what's BEHIND, which
       is how macOS Vibrancy is rendered. Different mechanism. Safe.
       Once Tauri windowing lands in 3.6.6, the panel will float over
       a transparent window and frost the user's actual desktop — same
       feel as Spotlight. In dev preview it frosts the main app behind. */
    .cmd-panel {
        /* Fixed dimensions — the palette no longer grows / shrinks
           with content. Stable size matches Raycast / Spotlight feel
           and prevents disorienting reflow as results arrive or the
           user switches modes. Width clamped at 100% so smaller
           displays still fit. The preview-pane open variant below
           bumps the width to accommodate the right pane. */
        width: 100%;
        /* Kept in sync with COMMAND_WINDOW_WIDTH in src-tauri/src/lib.rs. */
        max-width: 900px;
        height: 560px;
        /* Anchor for the absolutely-positioned fullscreen preview overlay. */
        position: relative;
        /* Opacity is fully owned by the Settings → Command Palette slider via
           --cmd-panel-opacity (the same var the window panel reads); the 64%
           here is only a pre-hydration fallback. Deep blur + saturation keep
           the frost reading as glass where backdrop-filter actually works
           (in-app overlay / dev preview). */
        background: color-mix(
            in srgb,
            var(--color-panel) var(--cmd-panel-opacity, 64%),
            transparent
        );
        backdrop-filter: blur(40px) saturate(200%);
        -webkit-backdrop-filter: blur(40px) saturate(200%);
        border: 1px solid color-mix(in srgb, var(--color-text) 14%, transparent);
        border-radius: 16px;
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 14%, transparent),
            0 20px 56px rgba(0, 0, 0, 0.55);
        overflow: hidden;
        display: flex;
        flex-direction: column;
        /* Positioning context for the absolutely-positioned action
           panel (Ctrl+Space) which anchors to bottom-right of this
           container. Without `position: relative` it would escape
           up to the document body and float in the wrong spot. */
        position: relative;
    }

    /* ─── Top bar ───────────────────────────────────────────────── */
    .cmd-top {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 10px 16px;
    }

    /* ─── Meta strip (Files / Inside + timing) ───────────────
       Sits between the top bar and the body. Only mounts in default
       mode with an active query — empty intro and other modes don't
       have a filename-vs-content axis so the strip would be noise. */
    .cmd-meta {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        padding: 6px 16px;
        border-bottom: 1px solid color-mix(in srgb, var(--color-border) 40%, transparent);
        background: transparent;
        font-size: 12px;
        color: var(--color-text-secondary);
    }
    .cmd-meta-modes {
        display: inline-flex;
        align-items: center;
        gap: 2px;
        padding: 2px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-pill);
    }
    .cmd-meta-mode {
        display: inline-flex;
        align-items: center;
        height: 22px;
        padding: 0 10px;
        background: transparent;
        border: none;
        border-radius: 999px;
        color: var(--color-text-secondary);
        font-size: 11.5px;
        font-weight: 500;
        cursor: pointer;
        transition: color var(--dur-micro) var(--ease-out);
    }
    .cmd-meta-mode:hover {
        color: var(--color-text);
    }
    /* "On" treatment — accent-soft background, accent text. Same
       segmented-pill visual the FileSearch page uses for its mode
       toggle, so muscle memory carries between surfaces. */
    .cmd-meta-mode.is-on {
        background: color-mix(in srgb, var(--color-text) 10%, transparent);
        color: var(--color-text);
        cursor: default;
    }

    /* ─── Scope chips (Palette Appearance Wave A, 2026-05-27) ──────
       Category filter row directly below the search input. Active chip
       uses the user's chosen accent (var(--color-accent)) tinted, so
       it matches whatever swatch they pick in Appearance. Inactive
       chips are subtle text-only — the row reads as scannable
       navigation, not a noisy toolbar. */
    .cmd-chips {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 6px 14px 8px;
        border-bottom: 1px solid color-mix(in srgb, var(--color-border) 35%, transparent);
        position: relative;
        z-index: 10;
    }
    .cmd-scope-tabs {
        display: flex;
        align-items: center;
        flex: 1;
        min-width: 0;
        gap: 4px;
        flex-wrap: wrap;
    }
    .cmd-chip {
        appearance: none;
        border: 0;
        background: transparent;
        color: var(--color-text-secondary);
        padding: 4px 12px;
        font-size: 12px;
        font-weight: 500;
        border-radius: 999px;
        cursor: pointer;
        white-space: nowrap;
        /* Keep each primary destination readable instead of shrinking labels
           to make a crowded scope strip fit. The specialist destinations live
           in More, so the primary row does not need horizontal scrolling. */
        flex: none;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .cmd-chip:hover {
        color: var(--color-text);
        background: var(--color-panel-2);
    }
    .cmd-chip.is-on {
        position: relative;
        background: var(--color-panel-2);
        color: var(--color-text);
        font-weight: 500;
    }
    .cmd-chip.is-on::before {
        content: '';
        position: absolute;
        left: 5px;
        top: 7px;
        bottom: 7px;
        width: 2px;
        border-radius: var(--radius-pill, 999px);
        background: var(--color-accent);
    }
    .cmd-more-scopes {
        position: relative;
        flex: none;
    }
    .cmd-chip-more {
        display: inline-flex;
        align-items: center;
        gap: 4px;
    }
    :global(.cmd-chip-more-ico) {
        width: 13px;
        height: 13px;
        transition: transform var(--dur-micro) var(--ease-out);
    }
    .cmd-chip-more.is-open :global(.cmd-chip-more-ico) {
        transform: rotate(180deg);
    }
    .cmd-more-scope-menu {
        position: absolute;
        top: calc(100% + 6px);
        right: 0;
        z-index: 35;
        display: grid;
        grid-template-columns: repeat(2, minmax(92px, 1fr));
        gap: 2px;
        min-width: 214px;
        padding: 5px;
        border: 1px solid var(--color-border);
        border-radius: 10px;
        background: var(--color-panel);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            var(--shadow-lg);
    }
    .cmd-more-scope-option {
        position: relative;
        min-width: 0;
        border: 0;
        border-radius: 7px;
        padding: 7px 9px;
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12px;
        font-weight: 500;
        text-align: left;
        cursor: pointer;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .cmd-more-scope-option:hover,
    .cmd-more-scope-option:focus-visible {
        background: var(--color-panel-2);
        color: var(--color-text);
        outline: none;
    }
    .cmd-more-scope-option.is-on {
        background: var(--color-panel-2);
        color: var(--color-text);
        padding-left: 12px;
    }
    .cmd-more-scope-option.is-on::before {
        content: '';
        position: absolute;
        left: 4px;
        top: 7px;
        bottom: 7px;
        width: 2px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .cmd-chip:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: 2px;
    }

    /* 2026-05-27 polish (Model C): "Showing only X matching 'Y' · Esc
       to broaden" hint that surfaces when the user types a query while
       a non-All chip is active. Small accent-tinted strip; designed to
       be informative but unobtrusive. */
    /* V2: dev-telemetry chrome removed — the "Showing only X matching 'Y' ·
       Esc to broaden" banner is hidden (scope, if any, reads from the active
       category tag, not a full banner). */
    .cmd-scope-hint {
        display: none;
    }
    .cmd-scope-hint-text strong {
        color: var(--color-accent);
        font-weight: 600;
    }
    .cmd-scope-hint-q {
        color: var(--color-text);
        font-weight: 500;
    }
    .cmd-scope-hint-broaden {
        appearance: none;
        border: 0;
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 11.5px;
        padding: 2px 6px;
        border-radius: 6px;
        cursor: pointer;
        white-space: nowrap;
        display: inline-flex;
        align-items: center;
        gap: 6px;
        transition: color var(--dur-micro) var(--ease-out),
            background var(--dur-micro) var(--ease-out);
    }
    .cmd-scope-hint-broaden:hover {
        color: var(--color-text);
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    }
    .cmd-scope-hint-broaden kbd {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 10px;
        padding: 0 6px;
        border-radius: 4px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-bottom-width: 2px;
        color: var(--color-text);
    }

    /* V2: kill the "Found in N ms" dev timer — internal-pride signal, not user
       value. */
    .cmd-meta-time {
        display: none;
    }
    /* Right side of the meta strip — groups the "Updating…" spinner
       (when in flight) with the "Found in N ms" chip (when settled).
       Both can be visible simultaneously during a re-search over
       existing results: spinner says "new numbers coming", timing
       chip shows last-known so the user has a baseline. */
    .cmd-meta-right {
        display: inline-flex;
        align-items: center;
        gap: 10px;
    }
    .cmd-meta-updating {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        color: var(--color-text-secondary);
        font-size: 11.5px;
    }
    .cmd-meta-spinner {
        width: 10px;
        height: 10px;
        border: 1.5px solid var(--color-border);
        border-top-color: var(--color-accent);
        border-radius: 50%;
        animation: cmd-meta-spin 800ms linear infinite;
    }
    @keyframes cmd-meta-spin {
        to {
            transform: rotate(360deg);
        }
    }
    @media (prefers-reduced-motion: reduce) {
        .cmd-meta-spinner {
            animation-duration: 2400ms;
        }
    }
    :global(.cmd-top-leading-ico) {
        width: 18px;
        height: 18px;
        color: var(--color-muted);
        flex-shrink: 0;
    }
    .cmd-back {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 28px;
        height: 28px;
        border: none;
        background: transparent;
        border-radius: var(--radius-control);
        color: var(--color-text-secondary);
        cursor: pointer;
        flex-shrink: 0;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .cmd-back:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    :global(.cmd-back-ico) {
        width: 15px;
        height: 15px;
    }
    .cmd-mode-pill {
        display: inline-flex;
        align-items: center;
        height: 26px;
        padding: 0 12px;
        border-radius: var(--radius-pill);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        color: var(--color-text);
        font-size: 12px;
        font-weight: 500;
        white-space: nowrap;
        flex-shrink: 0;
    }
    /* Rounded "chip" search input — macOS Spotlight-style. A soft
       panel-2 background, hairline border, accent border on focus.
       The leading Search icon + trailing X / mic stay as sibling
       elements outside this chip so the row reads as
       `[icon] [chip] [actions]` — same anatomy as System Settings'
       search row. */
    .cmd-input {
        flex: 1;
        min-width: 0;
        height: 38px;
        padding: 0 14px;
        /* Input chip is a TOUCH more opaque than the surrounding
           translucent panel so it still reads as a distinct surface
           — but only slightly, so the Vibrancy feel carries through. */
        background: color-mix(in srgb, var(--color-panel-2) 50%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-border) 70%, transparent);
        border-radius: var(--radius-control, 8px);
        color: var(--color-text);
        font-size: 15px;
        letter-spacing: -0.005em;
        outline: none;
        transition:
            border-color var(--dur-micro) var(--ease-out),
            background-color var(--dur-micro) var(--ease-out),
            box-shadow var(--dur-micro) var(--ease-out);
    }
    .cmd-input::placeholder {
        color: var(--color-muted);
    }
    .cmd-input:hover:not(:focus) {
        border-color: color-mix(in srgb, var(--color-border-strong) 80%, transparent);
    }
    .cmd-input:focus {
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        background: color-mix(in srgb, var(--color-panel-2) 70%, transparent);
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 16%, transparent);
    }
    .cmd-input-clear {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 24px;
        height: 24px;
        border: none;
        background: transparent;
        border-radius: 6px;
        color: var(--color-muted);
        cursor: pointer;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .cmd-input-clear:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    :global(.cmd-input-clear-ico) {
        width: 13px;
        height: 13px;
    }
    /* Icon buttons in the top bar — preview toggle, mic. Same chip
       shape so they read as a matched pair on the right edge of
       the search row. Accent-tinted when active. */
    .cmd-icon-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 32px;
        height: 32px;
        border: 1px solid color-mix(in srgb, var(--color-border) 80%, transparent);
        background: color-mix(in srgb, var(--color-panel-2) 60%, transparent);
        border-radius: var(--radius-control);
        color: var(--color-text-secondary);
        cursor: pointer;
        flex-shrink: 0;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .cmd-icon-btn:hover {
        border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
        color: var(--color-text);
    }
    .cmd-icon-btn.is-active {
        background: var(--color-accent-soft);
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        color: var(--color-accent);
    }
    :global(.cmd-icon-btn-ico) {
        width: 15px;
        height: 15px;
    }

    /* ─── Body wrap — preview opens INSIDE the existing palette ────
       Important UX rule: toggling preview must not resize the command
       palette. The launcher shell remains stable; only the internal
       body split changes. Fullscreen preview is the only state allowed
       to grow the window. */
    .cmd-body-wrap {
        display: flex;
        flex: 1;
        min-height: 0;
        min-width: 0;
    }
    .cmd-body-wrap.has-preview .cmd-body {
        min-width: 0;
        flex: 0 0 clamp(300px, 38%, 360px);
        max-width: 360px;
    }
    /* Raycast-grade detail: every chrome scroller shares one thin, themed
       scrollbar (the note preview already had this; the main list, preview
       pane, commands drill-in and actions panel fell back to the default
       OS bar, which breaks the calm surface). */
    .cmd-body,
    .cmd-preview,
    .cmd-cat-rows,
    .cmd-actions-list {
        scrollbar-width: thin;
        scrollbar-color: var(--color-border) transparent;
    }
    .cmd-body::-webkit-scrollbar,
    .cmd-preview::-webkit-scrollbar,
    .cmd-cat-rows::-webkit-scrollbar,
    .cmd-actions-list::-webkit-scrollbar {
        width: 8px;
    }
    .cmd-body::-webkit-scrollbar-thumb,
    .cmd-preview::-webkit-scrollbar-thumb,
    .cmd-cat-rows::-webkit-scrollbar-thumb,
    .cmd-actions-list::-webkit-scrollbar-thumb {
        background: var(--color-border);
        border-radius: 999px;
    }
    .cmd-body::-webkit-scrollbar-track,
    .cmd-preview::-webkit-scrollbar-track,
    .cmd-cat-rows::-webkit-scrollbar-track,
    .cmd-actions-list::-webkit-scrollbar-track {
        background: transparent;
    }
    .cmd-panel {
        transition: none;
    }

    /* ─── Body ──────────────────────────────────────────────────── */
    .cmd-body {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        padding: 6px 8px;
        scrollbar-gutter: stable;
    }
    /* Mode transition — the `{#key mode}` wrapper remounts the body
       content on every mode change, so this animation fires exactly
       once per switch (default → clipboard, clipboard → voice, etc.).
       Within a mode, updates (search re-runs, clipboard refreshes)
       don't trigger the keyed remount, so the entrance doesn't replay
       on every keystroke — same lesson learned from FileSearch
       (Phase 3.3.7). 200 ms fade + small downward slide. */
    .cmd-body-content {
        animation: cmd-body-in calc(200ms * var(--cmd-motion-mult, 1)) var(--ease-out, ease) both;
    }
    @keyframes cmd-body-in {
        from {
            opacity: 0;
            transform: translate3d(0, 6px, 0);
        }
        to {
            opacity: 1;
            transform: translate3d(0, 0, 0);
        }
    }
    @media (prefers-reduced-motion: reduce) {
        .cmd-body-content {
            animation: none;
        }
    }

    /* ─── Preview pane ─────────────────────────────────────────── */
    .cmd-preview {
        /* Inline preview consumes the remaining space inside the existing
           palette width. No min-width that can force the shell wider. */
        flex: 1 1 0;
        min-width: 0;
        max-width: none;
        border-left: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
        background: color-mix(in srgb, var(--color-panel) 30%, transparent);
        /* Scroll instead of clip: the hero media keeps a generous height (see
           .cmd-stage-host min-height), so on a short/wide window the metadata
           below scrolls into view rather than getting cut off. */
        overflow-y: auto;
        overflow-x: hidden;
        padding: 12px 14px 14px;
        display: flex;
        flex-direction: column;
        gap: 12px;
        scrollbar-gutter: stable;
    }
    .cmd-preview-head {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-wrap: wrap;
    }
    .cmd-preview-kind {
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.08em;
        text-transform: uppercase;
        color: var(--color-muted);
    }
    .cmd-preview-ext {
        font-size: 10px;
        font-weight: 600;
        padding: 1px 6px;
        border-radius: 4px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        color: var(--color-text);
        letter-spacing: 0.04em;
    }
    .cmd-preview-meta-line {
        font-size: 11.5px;
        color: var(--color-muted);
    }
    .cmd-preview-title {
        margin: 0;
        font-size: 19px;
        font-weight: 600;
        color: var(--color-text);
        letter-spacing: -0.015em;
        line-height: 1.2;
        word-break: break-word;
    }
    .cmd-preview-desc {
        margin: 0;
        font-size: 13px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }
    .cmd-preview-hero {
        display: flex;
        align-items: center;
        gap: 12px;
    }
    .cmd-preview-hero-icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 44px;
        height: 44px;
        border-radius: 10px;
        background: var(--color-accent-soft);
        color: var(--color-accent);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 4%, transparent);
        flex: none;
    }
    :global(.cmd-preview-hero-icon-svg) {
        width: 22px;
        height: 22px;
    }


    /* Image-content preview (clipboard image or image file) */
    /* Image preview is a <button> so it's click-to-zoom (fit ⇄ actual).
       Reset the native button chrome, then keep the framed-thumb look. */
    /* Image preview is a <button> → click toggles fullscreen. Inside the
       stage host it fills the available height; the image is contained. */
    .cmd-preview-image-wrap {
        appearance: none;
        width: 100%;
        font: inherit;
        color: inherit;
        cursor: zoom-in;
        background: var(--color-bg);
        border: 1px solid var(--color-divider, var(--color-border));
        border-radius: 8px;
        padding: 8px;
        display: flex;
        align-items: center;
        justify-content: center;
        min-height: 160px;
        overflow: hidden;
        transition: border-color 160ms var(--ease-out, ease);
    }
    .cmd-preview-image-wrap:hover {
        border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
    }

    /* Color-content preview — full-size swatch. The wrap has the
       transparency-checker pattern as its base background; the
       swatch overlays the wrap at full size and covers it for
       opaque colors. For alpha / transparent colors, the checker
       shows THROUGH the swatch's alpha — the correct alpha-aware
       visualization. Zero padding = no checker stripe ring around
       opaque swatches (user fix). */
    .cmd-preview-color-wrap {
        position: relative;
        border: 1px solid var(--color-divider, var(--color-border));
        border-radius: 8px;
        height: 128px;
        overflow: hidden;
        background-color: var(--color-bg);
        background-image:
            linear-gradient(45deg, rgba(255, 255, 255, 0.04) 25%, transparent 25%),
            linear-gradient(-45deg, rgba(255, 255, 255, 0.04) 25%, transparent 25%),
            linear-gradient(45deg, transparent 75%, rgba(255, 255, 255, 0.04) 75%),
            linear-gradient(-45deg, transparent 75%, rgba(255, 255, 255, 0.04) 75%);
        background-size: 14px 14px;
        background-position: 0 0, 0 7px, 7px -7px, -7px 0;
    }
    .cmd-preview-color-swatch {
        position: absolute;
        inset: 0;
        border-radius: 7px;
        /* Inset rings define the edge against light or dark swatches —
           dark outer, light inner. Matches the row swatch recipe. */
        box-shadow:
            inset 0 0 0 1px rgba(0, 0, 0, 0.20),
            inset 0 0 0 2px rgba(255, 255, 255, 0.08);
    }
    .cmd-preview-color-value {
        text-align: center;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 15px;
        font-weight: 600;
        color: var(--color-text);
        padding: 4px 0;
        letter-spacing: 0;
        user-select: text;
    }
    .cmd-preview-image {
        /* Fill the stage box (contain), so a photo is the big, calm hero of the
           preview rather than a small centered thumbnail. */
        width: 100%;
        height: 100%;
        min-height: 0;
        object-fit: contain;
        border-radius: 4px;
    }

    /* In-palette PDF preview — WebView2's built-in (Edge/Chromium) viewer
       renders the actual pages inside an iframe over the asset: URL. */
    .cmd-preview-pdf-wrap {
        margin-bottom: 0;
        border: 1px solid var(--color-divider, var(--color-border));
        border-radius: 8px;
        overflow: hidden;
        background: var(--color-bg);
        min-height: 320px;
    }
    .cmd-preview-pdf {
        display: block;
        width: 100%;
        height: 100%;
        min-height: 320px;
        border: 0;
        background: #fff;
    }

    /* In-palette media player (audio / video) for file previews. */
    .cmd-preview-media-wrap {
        margin-bottom: 0;
        min-height: 0;
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .cmd-preview-audio {
        width: 100%;
        max-width: 480px;
        height: 40px;
    }
    .cmd-preview-video {
        /* Fill the stage box and letterbox to aspect, so the player is as large
           as the preview area allows — not a short full-width strip with empty
           space above and below it. */
        width: 100%;
        min-height: 0;
        object-fit: contain;
        border-radius: 8px;
        background: #000;
        outline: none;
    }

    /* ─── Stage host (2026-05-31 preview overhaul) ──────────────────
       The previewable content (image/video/pdf/reader/note) lives in a
       flex column that fills the available height, so the reader/iframe/
       image grow to use the pane instead of collapsing to one line. Used
       by BOTH the inline pane and the fullscreen overlay. */
    .cmd-stage-host {
        flex: 1 1 auto;
        display: flex;
        flex-direction: column;
    }
    /* Visual content fills the host; the reader scrolls inside itself. */
    .cmd-stage-host > .cmd-preview-image-wrap,
    .cmd-stage-host > .cmd-preview-pdf-wrap,
    .cmd-stage-host > .cmd-preview-reader,
    .cmd-stage-host > .cmd-preview-reader-dim,
    .cmd-stage-host > .cmd-folder-list {
        flex: 1 1 0;
        min-height: 0;
    }
    .cmd-stage-host > .cmd-preview-reader {
        max-height: none;
    }
    .cmd-stage-host > .cmd-note-preview {
        flex: 1 1 auto;
        min-height: 0;
        overflow: auto;
    }

    /* V2: INLINE preview only. The fullscreen overlay also carries
       `.cmd-stage-host` (plus `.cmd-preview-full-stage`), so scope by
       `:not(.cmd-preview-full-stage)` to leave fullscreen unconstrained. Size
       the inline stage to its content and cap each non-video preview at 200px,
       so long PDFs / text / code / docs scroll inside themselves and the
       metadata stays visible right below. Movies (video) are exempt and size
       naturally. */
    .cmd-stage-host:not(.cmd-preview-full-stage):not(.cmd-stage-fill) {
        flex: 0 0 auto;
    }
    .cmd-stage-host:not(.cmd-preview-full-stage):not(.cmd-stage-fill)
        > :not(.cmd-preview-media-wrap) {
        flex: 0 0 auto;
        min-height: 0;
        max-height: 200px;
        overflow: auto;
    }
    /* cmd-stage-fill (Commands drill-in): a LIST pane, not a file preview —
       fill the preview column so `.cmd-cat-rows` / `.cmd-preview-sysinfo`
       flex-fill and scroll inside themselves instead of being clamped to the
       200px per-child preview cap (which clipped long Control Panel /
       Running-apps lists to a short strip with dead space below). */
    .cmd-stage-host.cmd-stage-fill {
        flex: 1 1 0;
        min-height: 0;
    }
    .cmd-stage-host:not(.cmd-preview-full-stage) > .cmd-preview-pdf-wrap {
        min-height: 0;
    }
    .cmd-stage-host:not(.cmd-preview-full-stage) .cmd-preview-pdf {
        height: 200px;
        min-height: 0;
    }


    /* ─── Fullscreen preview overlay ────────────────────────────────
       Covers the palette window (anchored to .cmd-panel, which is
       position:relative). The palette stays mounted underneath — only
       the overlay is on top — so fullscreening never closes the palette. */
    .cmd-preview-full {
        position: absolute;
        inset: 0;
        z-index: 220;
        display: flex;
        flex-direction: column;
        background: var(--color-panel);
        border-radius: inherit;
        overflow: hidden;
    }
    .cmd-preview-full-bar {
        flex: none;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 12px 16px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        background: color-mix(in srgb, var(--color-panel-2) 60%, var(--color-panel));
    }
    .cmd-preview-full-meta {
        display: flex;
        align-items: center;
        gap: 10px;
        min-width: 0;
    }
    .cmd-preview-full-name {
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .cmd-preview-full-actions {
        display: flex;
        align-items: center;
        gap: 8px;
        flex: none;
    }
    .cmd-preview-full-btn {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 6px 12px;
        border-radius: 8px;
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;
        transition:
            background var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out);
    }
    .cmd-preview-full-btn:hover {
        background: var(--color-panel);
        border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
    }
    .cmd-preview-full-close {
        padding: 6px 8px;
    }
    :global(.cmd-preview-full-btn-ico) {
        width: 15px;
        height: 15px;
    }
    .cmd-preview-full-stage {
        flex: 1 1 auto;
        min-height: 0;
        padding: 16px;
    }
    /* In fullscreen the image fills generously and the wrap can scroll if
       the natural image is taller than the stage. */
    .cmd-preview-full-stage > .cmd-preview-image-wrap {
        cursor: zoom-out;
    }
    /* 2026-05-27 polish: file reader inside the Ctrl+P preview pane.
       Premium dark surface — refined panel-2 background, soft border,
       text-primary contrast (the old text-secondary was unreadable
       against the dim preview pane on busy themes). Two modes:
         • Default — proportional Inter, line-height 1.6 — reads like
           a document for plain text / Markdown / .ki notes.
         • Monospace (when extension is code-like) — JetBrains Mono.
       Both modes respect the `cmd-preview-reader-nowrap` modifier:
       wrap ON (default) wraps long lines; OFF leaves them so the
       horizontal scrollbar lets users see structure. */
    .cmd-preview-reader-toolbar {
        display: flex;
        gap: 8px;
        align-items: center;
        justify-content: space-between;
        margin: 0 0 8px;
        min-height: 24px;
    }
    .cmd-preview-reader-stats {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        min-width: 0;
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 500;
        white-space: nowrap;
    }
    .cmd-preview-reader-stats span {
        padding: 3px 7px;
        border-radius: 999px;
        border: 1px solid color-mix(in srgb, var(--color-border) 72%, transparent);
        background: color-mix(in srgb, var(--color-panel-2) 68%, transparent);
    }
    .cmd-preview-reader-controls {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        flex: none;
    }
    .cmd-preview-reader-btn {
        appearance: none;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        color: var(--color-text-secondary);
        padding: 3px 10px;
        border-radius: 6px;
        font-size: 10px;
        font-weight: 500;
        cursor: pointer;
        transition:
            background var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out);
    }
    .cmd-preview-reader-btn:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
        border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
    }
    .cmd-preview-reader {
        position: relative;
        margin: 0;
        padding: 12px 14px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 10px;
        max-height: none;
        overflow: auto;
        font-family: var(--font-sans, 'Inter', system-ui, sans-serif);
        /* Content surface — reading size is deliberate, not UI-chrome scale. */
        font-size: 12.5px;
        line-height: 1.62;
        color: var(--color-text);
        white-space: pre-wrap;
        word-break: break-word;
        tab-size: 4;
    }
    .cmd-preview-reader-mono {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        line-height: 1.55;
    }

    /* Plain text gets a light paper surface, closer to Notepad++/classic
       document reading: black text on a calm white page inside the dark UI. */
    .cmd-preview-reader-paper {
        padding: 18px 20px;
        background: #fbfaf6;
        border-color: rgba(15, 23, 42, 0.14);
        color: #111827;
        box-shadow:
            inset 0 1px 0 rgba(255, 255, 255, 0.85),
            0 10px 26px rgba(0, 0, 0, 0.18);
        font-size: 13px;
        line-height: 1.72;
        letter-spacing: 0.002em;
    }
    .cmd-preview-reader-paper .cmd-rln-no {
        color: #9ca3af;
    }
    .cmd-preview-reader-paper .cmd-rln-hit {
        background: color-mix(in srgb, var(--color-accent) 16%, transparent);
        box-shadow: inset 2px 0 0 var(--color-accent);
    }

    /* Generic rendered document surface used by Markdown and Office-like
       previews. It intentionally looks like a page, not a code/text dump. */
    .cmd-document-page {
        margin: 0;
        padding: 22px 24px;
        max-height: none;
        overflow: auto;
        background: #ffffff;
        color: #111827;
        border: 1px solid rgba(15, 23, 42, 0.14);
        border-radius: 10px;
        box-shadow:
            inset 0 1px 0 rgba(255, 255, 255, 0.9),
            0 12px 32px rgba(0, 0, 0, 0.2);
        font-family: Georgia, 'Times New Roman', serif;
        font-size: 13.4px;
        line-height: 1.74;
    }
    .cmd-markdown-preview {
        font-family: var(--font-sans, 'Inter', system-ui, sans-serif);
        line-height: 1.66;
    }
    .cmd-markdown-preview :global(h1),
    .cmd-markdown-preview :global(h2),
    .cmd-markdown-preview :global(h3) {
        margin: 0.65em 0 0.38em;
        color: #0f172a;
        line-height: 1.2;
    }
    .cmd-markdown-preview :global(h1) { font-size: 22px; }
    .cmd-markdown-preview :global(h2) { font-size: 18px; }
    .cmd-markdown-preview :global(h3) { font-size: 15px; }
    .cmd-markdown-preview :global(p) { margin: 0 0 0.8em; }
    .cmd-markdown-preview :global(ul),
    .cmd-markdown-preview :global(ol) { margin: 0 0 0.85em 1.3em; padding: 0; }
    .cmd-markdown-preview :global(blockquote) {
        margin: 0 0 0.9em;
        padding: 8px 12px;
        border-left: 3px solid var(--color-accent);
        background: #f8fafc;
        color: #334155;
        border-radius: 6px;
    }
    .cmd-markdown-preview :global(code) {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 12px;
        padding: 1px 6px;
        background: #eef2f7;
        border: 1px solid #e2e8f0;
        border-radius: 4px;
        color: #0f172a;
    }
    .cmd-markdown-preview :global(pre) {
        margin: 0 0 0.9em;
        padding: 11px 13px;
        overflow-x: auto;
        background: #0f172a;
        color: #e5e7eb;
        border-radius: 8px;
        border: 1px solid #1e293b;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        line-height: 1.55;
    }
    .cmd-markdown-preview :global(pre code) {
        padding: 0;
        border: 0;
        background: transparent;
        color: inherit;
    }
    .cmd-markdown-preview :global(table) {
        width: 100%;
        border-collapse: collapse;
        margin: 0 0 0.9em;
        /* Content surface — document table size is deliberate. */
        font-size: 12.5px;
    }
    .cmd-markdown-preview :global(th),
    .cmd-markdown-preview :global(td) {
        padding: 6px 8px;
        border: 1px solid #e2e8f0;
        text-align: left;
    }
    .cmd-markdown-preview :global(th) { background: #f8fafc; font-weight: 700; }

    .cmd-word-preview {
        margin: 0;
        padding: 18px;
        overflow: auto;
        background:
            linear-gradient(90deg, rgba(37, 99, 235, 0.10), color-mix(in srgb, var(--color-accent) 8%, transparent)) 0 0 / 100% 34px no-repeat,
            #e8edf5;
        border: 1px solid rgba(148, 163, 184, 0.35);
        border-radius: 12px;
    }
    .cmd-word-page {
        position: relative;
        margin: 0 auto;
        padding: 38px 44px 42px;
        width: min(100%, 680px);
        min-height: 420px;
        background: #ffffff;
        color: #111827;
        border: 1px solid #d5dbe7;
        border-radius: 2px;
        box-shadow:
            0 1px 0 rgba(15, 23, 42, 0.08),
            0 18px 44px rgba(15, 23, 42, 0.18);
        font-family: Cambria, Georgia, 'Times New Roman', serif;
        font-size: 14px;
        line-height: 1.78;
    }
    .cmd-word-page-topline {
        position: absolute;
        top: 18px;
        left: 44px;
        right: 44px;
        height: 1px;
        background: #e5e7eb;
    }
    .cmd-word-page h1 {
        margin: 0 0 18px;
        color: #111827;
        font-size: 22px;
        line-height: 1.25;
        font-weight: 700;
    }
    .cmd-word-page p {
        margin: 0 0 0.95em;
        text-align: left;
    }
    /* Word-document typography (2026-06-13 polish). The .docx is converted
       to HTML and injected inside `.cmd-word-page`, so the children are
       :global. These rules (placed after the generic markdown block so they
       win on equal specificity) give the rendered doc a clean, restrained
       document feel: a sensible heading scale, comfortable spacing, proper
       list markers + indentation, bordered tables, and a tasteful blockquote
       / inline-code treatment — all on the serif paper. */
    .cmd-word-page :global(h1),
    .cmd-word-page :global(h2),
    .cmd-word-page :global(h3),
    .cmd-word-page :global(h4) {
        color: #111827;
        font-weight: 700;
        line-height: 1.25;
        margin: 1.1em 0 0.45em;
    }
    .cmd-word-page :global(h1) {
        font-size: 1.55em;
        margin-top: 0;
        padding-bottom: 0.18em;
        border-bottom: 1px solid #e5e7eb;
    }
    .cmd-word-page :global(h2) {
        font-size: 1.3em;
    }
    .cmd-word-page :global(h3) {
        font-size: 1.12em;
    }
    .cmd-word-page :global(h4) {
        font-size: 1em;
        color: #374151;
    }
    .cmd-word-page :global(p) {
        margin: 0 0 0.85em;
        line-height: 1.55;
        text-align: left;
    }
    .cmd-word-page :global(ul),
    .cmd-word-page :global(ol) {
        margin: 0 0 0.95em;
        padding-left: 1.7em;
    }
    .cmd-word-page :global(ul) {
        list-style: disc;
    }
    .cmd-word-page :global(ol) {
        list-style: decimal;
    }
    .cmd-word-page :global(li) {
        margin: 0.12em 0;
        line-height: 1.55;
    }
    .cmd-word-page :global(li > ul),
    .cmd-word-page :global(li > ol) {
        margin: 0.2em 0 0.3em;
    }
    .cmd-word-page :global(blockquote) {
        margin: 0 0 0.95em;
        padding: 0.4em 0 0.4em 1em;
        border-left: 3px solid #cbd5e1;
        color: #475569;
        font-style: italic;
        background: transparent;
        border-radius: 0;
    }
    .cmd-word-page :global(code) {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 0.85em;
        padding: 0.1em 0.4em;
        background: #f1f5f9;
        border: 1px solid #e2e8f0;
        border-radius: 4px;
        color: #0f172a;
    }
    .cmd-word-page :global(pre) {
        margin: 0 0 0.95em;
        padding: 0.8em 1em;
        overflow-x: auto;
        background: #f8fafc;
        border: 1px solid #e2e8f0;
        border-radius: 6px;
        color: #0f172a;
        font-size: 0.85em;
        line-height: 1.6;
    }
    .cmd-word-page :global(pre code) {
        padding: 0;
        border: 0;
        background: transparent;
    }
    .cmd-word-page :global(table) {
        width: 100%;
        border-collapse: collapse;
        margin: 0 0 1em;
        font-size: 0.92em;
    }
    .cmd-word-page :global(th),
    .cmd-word-page :global(td) {
        padding: 8px 9px;
        border: 1px solid #d1d5db;
        text-align: left;
        vertical-align: top;
    }
    .cmd-word-page :global(th) {
        background: #eef1f5;
        font-weight: 700;
        color: #111827;
        border-bottom: 2px solid #9ca3af;
    }
    .cmd-word-page :global(tr:nth-child(even) td) {
        background: #fafbfc;
    }
    .cmd-word-page :global(img) {
        max-width: 100%;
        height: auto;
        border-radius: 2px;
    }
    .cmd-word-page :global(a) {
        color: #1d4ed8;
        text-decoration: underline;
    }
    .cmd-word-page :global(hr) {
        margin: 1.4em 0;
        border: 0;
        border-top: 1px solid #e5e7eb;
    }
    .cmd-word-page :global(strong) {
        font-weight: 700;
    }

    /* Folder-content listing (2026-06-13). Clean rows on the preview surface;
       dirs first (Folder glyph), files show a trailing size. */
    .cmd-folder-list {
        display: flex;
        flex-direction: column;
        gap: 2px;
        padding: 8px;
        overflow: auto;
        background: var(--panel-2, rgba(148, 163, 184, 0.06));
        border: 1px solid rgba(148, 163, 184, 0.28);
        border-radius: 12px;
    }
    .cmd-folder-list-head {
        padding: 6px 8px 8px;
        font-size: 11.5px;
        font-weight: 600;
        letter-spacing: 0.03em;
        text-transform: uppercase;
        color: var(--text-muted, #94a3b8);
    }
    .cmd-folder-row {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 6px 8px;
        border-radius: 8px;
        font-size: 13px;
        color: var(--text, #e2e8f0);
    }
    .cmd-folder-row:hover {
        background: var(--panel-2, rgba(148, 163, 184, 0.1));
    }
    .cmd-folder-row-ico {
        display: inline-flex;
        flex: 0 0 auto;
        color: var(--text-muted, #94a3b8);
    }
    :global(.cmd-folder-row-ico-svg) {
        width: 16px;
        height: 16px;
    }
    .cmd-folder-row-name {
        flex: 1 1 auto;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .cmd-folder-row-size {
        flex: 0 0 auto;
        font-size: 11.5px;
        font-variant-numeric: tabular-nums;
        color: var(--text-muted, #94a3b8);
    }

    .cmd-sheet-preview {
        overflow: auto;
        background: #f3f6fb;
        border: 1px solid rgba(148, 163, 184, 0.35);
        border-radius: 12px;
        box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.8);
    }
    .cmd-sheet-grid-wrap {
        min-width: max-content;
        padding: 10px;
    }
    .cmd-sheet-grid {
        border-collapse: collapse;
        background: #ffffff;
        color: #111827;
        font-family: 'Aptos', 'Segoe UI', system-ui, sans-serif;
        font-size: 12px;
        box-shadow: 0 8px 24px rgba(15, 23, 42, 0.12);
    }
    .cmd-sheet-grid td,
    .cmd-sheet-grid th {
        min-width: 92px;
        max-width: 260px;
        padding: 6px 8px;
        border: 1px solid #dbe3ee;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
    .cmd-sheet-rowhead {
        position: sticky;
        left: 0;
        z-index: 1;
        min-width: 36px !important;
        width: 36px;
        background: #eef2f7;
        color: #64748b;
        text-align: right;
        font-weight: 600;
    }

    .cmd-code-preview {
        overflow: auto;
        padding: 10px 0;
        background: #0d1117;
        color: #dbe7ff;
        border: 1px solid rgba(125, 140, 165, 0.22);
        border-radius: 12px;
        font-family: 'JetBrains Mono', 'Cascadia Code', Consolas, ui-monospace, monospace;
        font-size: 12px;
        line-height: 1.62;
        box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
        tab-size: 4;
    }

    /* These nodes are injected through {@html}, so Svelte's scoped CSS
       will not match them unless the selectors are global. */
    :global(.cmd-code-line) {
        display: grid;
        grid-template-columns: 58px minmax(max-content, 1fr);
        width: max-content;
        min-width: 100%;
        align-items: start;
        scroll-margin-block: 96px;
    }
    :global(.cmd-code-line:hover) { background: rgba(255, 255, 255, 0.035); }
    :global(.cmd-code-line.has-hit) {
        background: rgba(234, 179, 8, 0.07);
    }
    :global(.cmd-code-line.is-hit) {
        background: linear-gradient(
            90deg,
            color-mix(in srgb, var(--color-accent) 20%, transparent),
            color-mix(in srgb, var(--color-accent) 8%, transparent)
        );
        box-shadow: inset 3px 0 0 var(--color-accent);
    }
    :global(.cmd-code-ln) {
        user-select: none;
        padding: 0 12px 0 8px;
        color: #667085;
        text-align: right;
        border-right: 1px solid rgba(148, 163, 184, 0.16);
        background: rgba(13, 17, 23, 0.98);
        position: sticky;
        left: 0;
        z-index: 2;
        font-variant-numeric: tabular-nums;
    }
    :global(.cmd-code-src) {
        display: block;
        min-width: max-content;
        padding: 0 16px;
        white-space: pre;
        word-break: normal;
        overflow-wrap: normal;
    }
    .cmd-code-nowrap :global(.cmd-code-src) {
        white-space: pre;
        overflow-wrap: normal;
        word-break: normal;
    }
    /* Wrap mode (toggle ON): the source column shrinks to the container and
       soft-wraps long lines instead of forcing horizontal scroll. */
    .cmd-code-preview:not(.cmd-code-nowrap) :global(.cmd-code-line) {
        grid-template-columns: 58px minmax(0, 1fr);
        width: 100%;
        min-width: 0;
    }
    .cmd-code-preview:not(.cmd-code-nowrap) :global(.cmd-code-src) {
        min-width: 0;
        white-space: pre-wrap;
        overflow-wrap: anywhere;
    }
    :global(.tok-kw) { color: #ff7b72; font-weight: 650; }
    :global(.tok-str) { color: #a5d6ff; }
    :global(.tok-num) { color: #79c0ff; }
    :global(.tok-com) { color: #8b949e; font-style: italic; }
    :global(.tok-hit) {
        color: #071013;
        background: #34d399;
        border-radius: 4px;
        padding: 0 2px;
        box-shadow: 0 0 0 1px rgba(52, 211, 153, 0.3);
    }

    .cmd-deck-preview {
        background: #111827;
        color: #e5e7eb;
        border-color: rgba(255, 255, 255, 0.14);
        font-size: 13px;
        line-height: 1.72;
    }

    /* Code preview: make it visibly code-first instead of generic text. */
    .cmd-preview-reader-mono {
        background: #0d1117;
        border-color: rgba(125, 140, 165, 0.22);
        color: #dbe7ff;
        box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
    }
    .cmd-preview-reader-mono .cmd-rln-no {
        color: #667085;
    }
    .cmd-preview-reader-mono .cmd-rln-hit {
        background: color-mix(in srgb, var(--color-accent) 16%, transparent);
        box-shadow: inset 2px 0 0 var(--color-accent);
    }
    .cmd-preview-reader-nowrap {
        white-space: pre;
        word-break: normal;
        overflow-x: auto;
    }
    .cmd-preview-reader-dim {
        display: flex;
        align-items: center;
        gap: 8px;
        font-style: italic;
        color: var(--color-text-secondary);
    }
    .cmd-preview-reader-spinner {
        width: 11px;
        height: 11px;
        border: 2px solid color-mix(in srgb, var(--color-accent) 30%, transparent);
        border-top-color: var(--color-accent);
        border-radius: 50%;
        animation: cmd-preview-spin 720ms linear infinite;
        flex-shrink: 0;
    }
    @keyframes cmd-preview-spin {
        to { transform: rotate(360deg); }
    }

    /* Honest "this is extracted text, not the rendered document" banner
       above the reader for Office / e-book formats WebView2 can't render. */
    .cmd-preview-reader-note {
        margin: 0 0 8px;
        padding: 7px 10px;
        border-radius: 8px;
        background: color-mix(in srgb, var(--color-accent) 8%, var(--color-panel-2));
        border: 1px solid color-mix(in srgb, var(--color-accent) 22%, var(--color-border));
        color: var(--color-text-secondary);
        font-size: 11.5px;
        line-height: 1.45;
    }

    /* ─── Line-numbered reader (code + jump-to-match) ───────────────
       Used for code-like files and live-grep hits. Each line is a row
       with a fixed gutter (line number) + the text; the matched line
       (cmd-rln-hit) gets an accent highlight and is scrolled to center
       via the `scrollHit` action. Wrap toggle still applies to the text
       column. */
    .cmd-preview-reader-lines {
        display: block;
        padding: 8px 0;
    }
    .cmd-rln {
        display: flex;
        align-items: flex-start;
        gap: 12px;
        padding: 0 14px;
    }
    .cmd-rln-no {
        flex: none;
        width: 4ch;
        text-align: right;
        color: var(--color-muted);
        opacity: 0.6;
        user-select: none;
        font-variant-numeric: tabular-nums;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 10px;
        /* nudge the number onto the text baseline of the first wrapped row */
        padding-top: 0.12em;
    }
    /* Hide the gutter when not requested (kept for symmetry — the reader
       only renders lines when a gutter is wanted, but this is defensive). */
    .cmd-preview-reader-lines:not(.cmd-preview-reader-gutter) .cmd-rln-no {
        display: none;
    }
    .cmd-rln-tx {
        flex: 1 1 auto;
        min-width: 0;
        white-space: inherit;
        word-break: inherit;
    }
    .cmd-rln-hit {
        background: color-mix(in srgb, var(--color-accent) 16%, transparent);
        box-shadow: inset 2px 0 0 var(--color-accent);
        border-radius: 0 4px 4px 0;
    }
    .cmd-rln-hit .cmd-rln-no {
        color: var(--color-accent);
        opacity: 1;
    }
    /* No-wrap: each row grows to its content width (min full-width) so the
       container's horizontal scroll reveals long lines and the hit
       highlight spans the whole line. */
    .cmd-preview-reader-lines.cmd-preview-reader-nowrap .cmd-rln {
        width: max-content;
        min-width: 100%;
    }

    /* ─── .ki note preview (Wave J, 2026-05-27) ────────────────────
       Notes get a dedicated rendered view instead of a `<pre>` dump.
       The container holds three things stacked: (1) a metadata row
       (pinned chip + updated date + tags), (2) the note title, and
       (3) the markdown body rendered into safe HTML by
       `$lib/notes/preview.ts`. The body has its own prose-like
       typography rules below so headings, lists, code blocks, and
       quotes render with refined dark-theme styling — no generic
       `prose-invert` Tailwind class needed (which wouldn't read
       theme tokens like accent / panel-2 anyway).
       ─────────────────────────────────────────────────────────── */
    .cmd-note-preview {
        margin: 0 0 12px;
        padding: 14px 16px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 12px;
        max-height: none;
        overflow-y: auto;
        overflow-x: hidden;
        /* Refined scrollbar — matches the file-reader feel. */
        scrollbar-width: thin;
        scrollbar-color: var(--color-border) transparent;
    }
    .cmd-note-preview::-webkit-scrollbar {
        width: 8px;
    }
    .cmd-note-preview::-webkit-scrollbar-thumb {
        background: var(--color-border);
        border-radius: 4px;
    }
    .cmd-note-preview::-webkit-scrollbar-track {
        background: transparent;
    }
    .cmd-note-meta-row {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 6px 8px;
        margin: 0 0 8px;
        font-size: 11.5px;
        color: var(--color-muted);
    }
    .cmd-note-meta-chip {
        display: inline-flex;
        align-items: center;
        padding: 2px 8px;
        background: color-mix(in srgb, var(--color-text) 5%, transparent);
        border-radius: 999px;
        color: var(--color-text-secondary);
        font-size: 10px;
        letter-spacing: 0.01em;
        white-space: nowrap;
    }
    .cmd-note-pinned {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        padding: 2px 8px;
        background: color-mix(in srgb, var(--color-accent) 12%, transparent);
        color: var(--color-accent);
        border-radius: 999px;
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.02em;
    }
    /* :global() because Lucide forwards the class to its inner <svg>
       at runtime — Svelte's static analyzer can't see that and
       would otherwise prune this selector as "unused". */
    :global(.cmd-note-pinned-ico) {
        width: 10px;
        height: 10px;
    }
    .cmd-note-tags {
        display: inline-flex;
        flex-wrap: wrap;
        gap: 4px;
    }
    .cmd-note-tag {
        display: inline-flex;
        padding: 2px 7px;
        background: color-mix(in srgb, var(--color-accent) 9%, transparent);
        color: color-mix(in srgb, var(--color-accent) 80%, var(--color-text));
        border-radius: 6px;
        font-size: 10px;
        font-weight: 500;
    }
    .cmd-note-title {
        margin: 0 0 12px;
        padding: 0;
        font-size: 17px;
        font-weight: 600;
        line-height: 1.3;
        color: var(--color-text);
        letter-spacing: -0.005em;
        /* Long titles wrap rather than truncating — the user expects
           to see the full title in the preview. */
        overflow-wrap: anywhere;
    }
    .cmd-note-body {
        font-family: var(--font-sans, 'Inter', system-ui, sans-serif);
        font-size: 13px;
        line-height: 1.62;
        color: var(--color-text);
    }
    /* Body typography — keep the styling list explicit (no `@apply` /
       Tailwind prose) so it reads theme tokens at runtime and looks
       right across all 14 themes.

       :global() because the body HTML is injected via `{@html}` and
       Svelte's scoped-style pruning would otherwise drop these
       selectors as "unused" (they don't appear in the static
       template). The selectors are namespaced to `.cmd-note-body`
       so they can't leak outside the note preview. */
    .cmd-note-body :global(h1),
    .cmd-note-body :global(h2),
    .cmd-note-body :global(h3),
    .cmd-note-body :global(h4),
    .cmd-note-body :global(h5),
    .cmd-note-body :global(h6) {
        margin: 16px 0 6px;
        font-weight: 600;
        line-height: 1.3;
        color: var(--color-text);
    }
    .cmd-note-body :global(h1) { font-size: 18px; }
    .cmd-note-body :global(h2) { font-size: 16px; }
    .cmd-note-body :global(h3) { font-size: 14.5px; }
    .cmd-note-body :global(h4),
    .cmd-note-body :global(h5),
    .cmd-note-body :global(h6) { font-size: 13.5px; }
    .cmd-note-body :global(h1):first-child,
    .cmd-note-body :global(h2):first-child,
    .cmd-note-body :global(h3):first-child {
        margin-top: 0;
    }
    .cmd-note-body :global(p) {
        margin: 0 0 10px;
    }
    .cmd-note-body :global(ul),
    .cmd-note-body :global(ol) {
        margin: 0 0 10px;
        padding-left: 22px;
    }
    .cmd-note-body :global(li) {
        margin: 2px 0;
    }
    .cmd-note-body :global(li > ul),
    .cmd-note-body :global(li > ol) {
        margin: 2px 0;
    }
    .cmd-note-body :global(blockquote) {
        margin: 0 0 10px;
        padding: 6px 12px;
        border-left: 3px solid color-mix(in srgb, var(--color-accent) 65%, var(--color-border));
        background: color-mix(in srgb, var(--color-accent) 6%, transparent);
        color: var(--color-text-secondary);
        border-radius: 0 6px 6px 0;
        font-style: italic;
    }
    .cmd-note-body :global(blockquote) :global(p:last-child) {
        margin-bottom: 0;
    }
    .cmd-note-body :global(hr) {
        margin: 14px 0;
        border: 0;
        border-top: 1px solid var(--color-border);
    }
    .cmd-note-body :global(a) {
        color: var(--color-accent);
        text-decoration: underline;
        text-underline-offset: 2px;
        text-decoration-thickness: 1px;
    }
    .cmd-note-body :global(a:hover) {
        text-decoration-thickness: 2px;
    }
    /* Inline code — small chip with subtle background. */
    .cmd-note-body :global(code) {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        padding: 1px 6px;
        background: color-mix(in srgb, var(--color-text) 7%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-text) 6%, transparent);
        border-radius: 4px;
        color: var(--color-text);
        overflow-wrap: anywhere;
    }
    /* Code block — refined dark surface with horizontal scroll for
       long lines (we don't wrap code; that breaks indentation). */
    .cmd-note-body :global(pre) {
        margin: 0 0 10px;
        padding: 10px 12px;
        background: color-mix(in srgb, var(--color-panel) 60%, #000);
        border: 1px solid var(--color-border);
        border-radius: 8px;
        overflow-x: auto;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        line-height: 1.55;
        color: var(--color-text);
        tab-size: 4;
    }
    .cmd-note-body :global(pre) :global(code) {
        padding: 0;
        background: transparent;
        border: 0;
        font-size: inherit;
        color: inherit;
    }
    .cmd-note-body :global(strong),
    .cmd-note-body :global(b) {
        font-weight: 700;
        color: var(--color-text);
    }
    .cmd-note-body :global(em),
    .cmd-note-body :global(i) {
        font-style: italic;
    }
    .cmd-note-body :global(mark) {
        background: color-mix(in srgb, var(--color-accent) 32%, transparent);
        color: var(--color-text);
        padding: 0 2px;
        border-radius: 3px;
    }
    .cmd-note-body :global(img) {
        max-width: 100%;
        height: auto;
        border-radius: 6px;
        margin: 6px 0;
    }
    .cmd-note-body :global(table) {
        width: 100%;
        margin: 0 0 10px;
        border-collapse: collapse;
        font-size: 12px;
    }
    .cmd-note-body :global(th),
    .cmd-note-body :global(td) {
        padding: 6px 8px;
        border: 1px solid var(--color-border);
        text-align: left;
    }
    .cmd-note-body :global(th) {
        background: color-mix(in srgb, var(--color-text) 5%, transparent);
        font-weight: 600;
    }
    /* ─── highlight.js token colors (Wave K, 2026-05-28) ─────────
       hljs emits `<pre><code class="hljs language-X">…</code></pre>`
       with inner `<span class="hljs-keyword">…</span>` etc. The
       sanitizer allows `class` on span/code/pre (those classes are
       styling-only — no executable surface), so the tokens reach
       the DOM intact. Theme: a balanced dark palette that mixes the
       active accent for the most prominent tokens and falls back to
       neutral grays elsewhere — so the highlighting harmonizes with
       all 14 KIL themes instead of clashing.

       All selectors are `:global()` because hljs injects them at
       render time and Svelte's static analyzer can't see them in
       the template. They are nested under `.cmd-note-body` so they
       can't leak outside the note preview. */
    .cmd-note-body :global(.hljs) {
        color: var(--color-text);
        background: transparent;
    }
    .cmd-note-body :global(.hljs-keyword),
    .cmd-note-body :global(.hljs-selector-tag),
    .cmd-note-body :global(.hljs-literal),
    .cmd-note-body :global(.hljs-tag .hljs-name),
    .cmd-note-body :global(.hljs-meta-keyword) {
        color: color-mix(in srgb, var(--color-accent) 80%, #fff);
        font-weight: 600;
    }
    .cmd-note-body :global(.hljs-string),
    .cmd-note-body :global(.hljs-attr),
    .cmd-note-body :global(.hljs-regexp),
    .cmd-note-body :global(.hljs-template-variable),
    .cmd-note-body :global(.hljs-symbol),
    .cmd-note-body :global(.hljs-bullet) {
        color: #98d982; /* soft green — Words/strings */
    }
    .cmd-note-body :global(.hljs-number),
    .cmd-note-body :global(.hljs-meta),
    .cmd-note-body :global(.hljs-link) {
        color: #f0b96b; /* warm amber — numbers + meta */
    }
    .cmd-note-body :global(.hljs-comment),
    .cmd-note-body :global(.hljs-quote),
    .cmd-note-body :global(.hljs-deletion) {
        color: var(--color-muted);
        font-style: italic;
    }
    .cmd-note-body :global(.hljs-built_in),
    .cmd-note-body :global(.hljs-type),
    .cmd-note-body :global(.hljs-class .hljs-title),
    .cmd-note-body :global(.hljs-title.class_),
    .cmd-note-body :global(.hljs-section) {
        color: #6cb6ff; /* sky blue — types/classes */
        font-weight: 500;
    }
    .cmd-note-body :global(.hljs-function .hljs-title),
    .cmd-note-body :global(.hljs-title.function_),
    .cmd-note-body :global(.hljs-name),
    .cmd-note-body :global(.hljs-attribute) {
        color: #d2a8ff; /* light purple — functions/properties */
    }
    .cmd-note-body :global(.hljs-variable),
    .cmd-note-body :global(.hljs-property),
    .cmd-note-body :global(.hljs-params) {
        color: var(--color-text);
    }
    .cmd-note-body :global(.hljs-operator),
    .cmd-note-body :global(.hljs-punctuation) {
        color: var(--color-text-secondary);
    }
    .cmd-note-body :global(.hljs-addition) {
        color: #98d982;
        background: color-mix(in srgb, #98d982 12%, transparent);
    }
    .cmd-note-body :global(.hljs-emphasis) { font-style: italic; }
    .cmd-note-body :global(.hljs-strong) { font-weight: 700; }

    /* Fallback when the markdown parser threw — render raw text in a
       monospace pane so the user still sees their content. */
    .cmd-note-body-fallback {
        margin: 0;
        padding: 10px 12px;
        background: color-mix(in srgb, var(--color-panel) 60%, #000);
        border: 1px solid var(--color-border);
        border-radius: 8px;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        line-height: 1.55;
        color: var(--color-text-secondary);
        white-space: pre-wrap;
        word-break: break-word;
        max-height: 280px;
        overflow: auto;
    }

    /* Text-content preview (clipboard text) */
    .cmd-preview-text {
        margin: 0;
        padding: 10px 12px;
        background: var(--color-bg);
        border: 1px solid var(--color-divider, var(--color-border));
        border-radius: 8px;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        line-height: 1.55;
        color: var(--color-text);
        white-space: pre-wrap;
        word-break: break-word;
        max-height: 220px;
        overflow-y: auto;
    }

    /* Metadata list — label/value rows */
    .cmd-preview-meta {
        margin: 0;
        display: flex;
        flex-direction: column;
        gap: 0;
    }
    .cmd-preview-meta-row {
        display: grid;
        grid-template-columns: 64px 1fr;
        align-items: baseline;
        gap: 10px;
        padding: 7px 2px;
        border-bottom: 1px solid color-mix(in srgb, var(--color-border) 45%, transparent);
    }
    .cmd-preview-meta-row:last-child {
        border-bottom: none;
    }
    .cmd-preview-meta dt {
        font-size: 10px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }
    .cmd-preview-meta dd {
        margin: 0;
        font-size: 12px;
        color: var(--color-text);
        word-break: break-word;
        text-align: right;
    }
    /* V2: tabular numerals so sizes/counts don't jitter. */
    .cmd-preview-num {
        font-variant-numeric: tabular-nums;
    }
    /* Clipboard "From": the source app's real icon (resolved via
       ensure_launcher_icon), right-aligned like other metadata values. */
    .cmd-source-from {
        display: flex;
        justify-content: flex-end;
    }
    .cmd-source-icon {
        width: 18px;
        height: 18px;
        object-fit: contain;
        border-radius: 4px;
    }
    :global(.cmd-source-icon-glyph) {
        width: 16px;
        height: 16px;
        color: var(--color-text-secondary);
    }
    /* No-inline-preview fallback — the file's shell icon, centered. */
    .cmd-preview-noprev {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 12px;
        padding: 24px;
        color: var(--color-text-secondary);
        text-align: center;
    }
    .cmd-preview-noprev-icon {
        width: 72px;
        height: 72px;
        object-fit: contain;
    }
    :global(.cmd-preview-noprev-glyph) {
        width: 56px;
        height: 56px;
        color: var(--color-muted);
    }
    .cmd-preview-noprev-label {
        font-size: 12px;
    }
    /* The path value reads left-to-right; keep it left-aligned even though the
       other metadata values are right-aligned. */
    .cmd-preview-path {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        color: var(--color-text-secondary);
        text-align: left;
    }
    /* The file metadata list sits below the hero media, separated by a hairline
       and a little breathing room. */
    .cmd-preview-meta-file {
        flex: none;
        margin-top: 12px;
        padding-top: 6px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 45%, transparent);
    }
    .cmd-preview-warn {
        color: var(--color-warning);
        font-weight: 500;
    }
    .cmd-preview-kbd {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        height: 20px;
        padding: 0 8px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-bottom-width: 2px;
        border-radius: 5px;
        color: var(--color-text-secondary);
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 10px;
        font-weight: 500;
    }

    /* Action buttons in preview */
    .cmd-preview-actions {
        margin-top: auto;
        padding-top: 12px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 45%, transparent);
        display: flex;
        gap: 8px;
        flex-wrap: wrap;
    }
    .cmd-preview-btn {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        height: 32px;
        padding: 0 14px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-size: 13px;
        font-weight: 500;
        cursor: pointer;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out);
    }
    .cmd-preview-btn:hover {
        background: var(--color-panel-3);
        border-color: var(--color-border-strong);
    }
    .cmd-preview-btn-primary {
        background: var(--color-accent);
        color: var(--color-accent-contrast);
        border-color: transparent;
        font-weight: 600;
    }
    .cmd-preview-btn-primary:hover {
        background: var(--color-accent-hover);
        border-color: transparent;
    }
    :global(.cmd-preview-btn-ico) {
        width: 13px;
        height: 13px;
    }

    /* Empty preview state */
    .cmd-preview-empty {
        margin: auto 0;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 8px;
        text-align: center;
        padding: 24px 8px;
    }
    :global(.cmd-preview-empty-ico) {
        width: 26px;
        height: 26px;
        color: var(--color-muted);
    }
    .cmd-preview-empty-text {
        margin: 0;
        font-size: 13px;
        color: var(--color-muted);
    }

    /* Active footer hint when preview is on. */
    .cmd-hint-active {
        color: var(--color-accent);
    }
    :global(.cmd-hint-ico) {
        width: 11px;
        height: 11px;
    }
    .cmd-section {
        padding: 4px 0 10px;
    }
    .cmd-section-label {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        padding: 8px 14px 6px;
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.08em;
        color: var(--color-muted);
        text-transform: uppercase;
    }
    :global(.cmd-section-label-ico) {
        width: 12px;
        height: 12px;
        color: var(--color-accent);
    }
    .cmd-section-count {
        font-size: 10px;
        font-weight: 500;
        text-transform: none;
        letter-spacing: 0;
        color: var(--color-muted);
    }

    /* ─── Row with inline actions (clipboard mode, Cleanup Wave 1.1) ───
       Wraps a paste-on-click row + an absolutely-positioned action strip
       that hovers over the right edge. The strip's buttons stopPropagate
       so action clicks don't trigger the row's paste. .cmd-row-with-actions
       gets extra right padding so the row text doesn't overlap the strip
       on rows wide enough to show both. The strip itself only fades in on
       row hover/focus to keep the default look clean (matches the workspace
       ClipboardHistory page's :hover-reveal pattern). */
    .cmd-row-wrap {
        position: relative;
        display: block;
    }
    .cmd-row.cmd-row-with-actions {
        /* Reserve space on the right so the action strip never overlaps
           the row's category meta line on narrow widths. */
        padding-right: 132px;
    }
    .cmd-row-actions {
        position: absolute;
        right: 8px;
        top: 50%;
        transform: translateY(-50%);
        display: flex;
        align-items: center;
        gap: 4px;
        opacity: 0;
        pointer-events: none;
        transition: opacity calc(var(--dur-micro) * var(--cmd-motion-mult, 1))
            var(--ease-out);
    }
    .cmd-row-wrap:hover .cmd-row-actions,
    .cmd-row-wrap:focus-within .cmd-row-actions,
    .cmd-row-wrap.is-selected-wrap .cmd-row-actions {
        opacity: 1;
        pointer-events: auto;
    }
    .cmd-row-action-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 26px;
        height: 26px;
        border-radius: 6px;
        border: 1px solid transparent;
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
        cursor: pointer;
        transition:
            color 140ms ease,
            background-color 140ms ease,
            border-color 140ms ease;
    }
    .cmd-row-action-btn:hover {
        color: var(--color-accent);
        border-color: color-mix(in srgb, var(--color-accent) 40%, transparent);
        background: var(--color-panel-3, var(--color-panel-2));
    }
    .cmd-row-action-btn.is-danger:hover {
        color: var(--color-error);
        border-color: color-mix(in srgb, var(--color-error) 40%, transparent);
    }
    :global(.cmd-row-action-ico) {
        width: 13px;
        height: 13px;
    }

    /* ─── Row ──────────────────────────────────────────────────── */
    .cmd-row {
        position: relative;
        display: flex;
        align-items: center;
        gap: 12px;
        width: 100%;
        /* Wave B (2026-05-27): density-driven vertical padding. The CSS
           variables `--cmd-row-height` (36/42/48 px) and `--cmd-row-pad-y`
           are set on `.cmd-root` from the active density choice; rows fall
           back to the cozy default if the variable isn't set yet.
           2026-07-16: restored the variables — the V2 polish pass had
           hardcoded these to 42px/9px, which left the Density control in
           Ctrl+Alt+A rendering and clicking but doing nothing. Cozy = 42px,
           so the default look is unchanged. */
        min-height: var(--cmd-row-height, 42px);
        padding: var(--cmd-row-pad-y, 9px) 14px;
        background: transparent;
        border: none;
        border-radius: var(--radius-control);
        color: var(--color-text);
        text-align: left;
        cursor: pointer;
        /* Wave D: --cmd-motion-mult scales every transition duration
           uniformly. 0 collapses everything to instant (reduced); 1.6
           feels lively. Falls back to 1 if not set. */
        transition: background-color calc(var(--dur-micro) * var(--cmd-motion-mult, 1))
            var(--ease-out);
    }

    /* Wave D (2026-05-27): reduced motion - kill transitions entirely
       when the user picks "Reduced". Same spirit as
       prefers-reduced-motion. */
    .cmd-root[style*='--cmd-motion-mult: 0'] *,
    .cmd-root[style*='--cmd-motion-mult: 0'] *::before,
    .cmd-root[style*='--cmd-motion-mult: 0'] *::after {
        transition-duration: 0ms !important;
        animation-duration: 0ms !important;
    }

    /* Wave D (2026-05-27): accent glow on selected rows when toggled
       on. Subtle outer ring tinted by the active accent — feels
       "alive" without being decorative noise. */
    /* Canonical selected-item pattern (app-wide binding): neutral panel-2
       surface + accent rounded-pill left strip (inset via ::before) + accent
       icon, regular-weight text — same language as the Sidebar and the
       actions panel below. (Replaces the earlier V2 "calm selection" tint so
       selection reads identically everywhere.) */
    .cmd-root.has-accent-glow .cmd-row.is-selected {
        box-shadow: none;
    }
    .cmd-row:hover:not(.is-selected) {
        background: color-mix(in srgb, var(--color-text) 4%, transparent);
    }
    .cmd-row.is-selected {
        background: var(--color-panel-2);
    }
    .cmd-row.is-selected::before {
        content: '';
        position: absolute;
        left: 3px;
        top: 8px;
        bottom: 8px;
        width: 3px;
        border-radius: var(--radius-pill, 999px);
        background: var(--color-accent);
    }
    .cmd-row.is-selected .cmd-row-icon {
        color: var(--color-accent);
    }
    /* Keyboard a11y: rows are focusable buttons — real keyboard focus gets a
       visible ring (arrow-key selection uses .is-selected; this covers Tab). */
    .cmd-row:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: -2px;
    }

    .cmd-row-icon {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 32px;
        height: 32px;
        border-radius: 8px;
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
        transition: color var(--dur-micro) var(--ease-out);
    }
    .cmd-row.is-selected .cmd-row-icon {
        background: var(--color-panel-2);
    }
    /* Running indicator (Tinycast-style): a small accent dot on the app icon
       when the app has a live window. `position: relative` scopes the dot to
       the icon box; the dot sits bottom-right with a panel-colored ring so it
       reads clearly over both light and dark icons. */
    .cmd-row-icon.is-running {
        position: relative;
    }
    .cmd-row-icon.is-running::after {
        content: '';
        position: absolute;
        right: -2px;
        bottom: -2px;
        width: 9px;
        height: 9px;
        border-radius: 50%;
        background: var(--color-accent);
        box-shadow: 0 0 0 2px var(--color-panel);
    }
    /* Right-aligned muted state label — echoes Tinycast's row type label,
       here reused to spell out the running state for a non-color cue. */
    .cmd-row-running {
        flex: none;
        margin-left: auto;
        padding-left: 12px;
        font-size: 11px;
        font-weight: 500;
        letter-spacing: 0.02em;
        color: var(--color-text-secondary);
    }
    :global(.cmd-row-icon-svg) {
        width: 16px;
        height: 16px;
    }
    /* Emoji grid. Fixed 12 columns — must match EMOJI_COLS in the script,
       which is what ↑/↓ step by. Colour comes from the system colour-emoji
       font, so the glyph itself has nothing to tokenise. */
    .cmd-emoji-grid {
        display: grid;
        grid-template-columns: repeat(12, 1fr);
        gap: 2px;
        padding: 4px 8px 8px;
    }
    .cmd-emoji-cell {
        aspect-ratio: 1;
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 22px;
        line-height: 1;
        border: 1px solid transparent;
        border-radius: var(--radius-control);
        background: transparent;
        cursor: pointer;
        transition: background var(--dur-micro) var(--ease-out);
    }
    .cmd-emoji-cell:hover {
        background: var(--color-panel-2);
    }
    /* Grid analogue of the list's selected state: same panel-2 fill, but
       the accent reads as a ring rather than the left strip a row uses —
       a 2px strip on a square cell is invisible at this size. */
    .cmd-emoji-cell.is-selected {
        background: var(--color-panel-2);
        border-color: var(--color-accent);
    }
    /* Name of the highlighted glyph. Lives in the section header because
       the cells are glyph-only. */
    .cmd-emoji-hint {
        color: var(--color-text-secondary);
        text-transform: none;
        letter-spacing: 0;
    }
    @media (prefers-reduced-motion: reduce) {
        .cmd-emoji-cell {
            transition: none;
        }
    }
    /* Real Windows shell icon (img) — replaces the Lucide fallback
       once `ensure_launcher_icon` resolves the PNG. Sized identical
       to the SVG so the row's vertical rhythm doesn't shift when
       the image swaps in. */
    .cmd-row-icon-img {
        width: 20px;
        height: 20px;
        object-fit: contain;
        display: block;
        image-rendering: -webkit-optimize-contrast;
    }
    /* NOTE (2026-07-27): packaged (Store) apps deliberately have NO size override
       here. They briefly did, to compensate for the shell's padded tile image —
       but the real fix landed in the backend: packaged apps now resolve to their
       actual .exe and use its embedded icon (which fills its canvas exactly like
       any classic app), and the rare exe-less UWP component has its tile cropped
       to the mark. So every icon arrives pre-normalised and one rule sizes them
       all. Re-adding a packaged special case would make Store icons LARGER than
       classic ones, not equal. */
    /* Content-search row variant — taller so the snippet fits, and
       the icon aligns to the top instead of vertically-centered (the
       snippet pushes the row tall and the icon next to the title
       reads better than icon next to the snippet middle). */
    .cmd-row-content {
        align-items: flex-start;
    }
    .cmd-row-content .cmd-row-icon {
        margin-top: 2px;
    }
    /* Wave 3.3.2 — match highlight on live-grep snippets. Subtle
       accent-tinted background, not a bright yellow `<mark>` default
       (which would scream and clash with the palette aesthetic).
       Padding pulls the highlight slightly off the surrounding text
       so it reads as a distinct span. */
    /* V2: calm match highlight — a faint neutral tint + slight weight, not a
       loud accent mark. Used for the matched substring in result names and the
       Inside-files snippets. */
    .cmd-livegrep-mark {
        background: color-mix(in srgb, var(--color-text) 10%, transparent);
        color: var(--color-text);
        padding: 0 2px;
        border-radius: 3px;
        font-weight: 600;
    }

    /* ─── Wave 4.1b-3 (2026-05-27): shell command streaming panel ────
       Sits at the top of the body when a `shell` My Command runs.
       Status row + monospace output area + cancel/close actions.
       Color-codes border + status icon by state (running / success /
       error / cancelled). Stderr lines render red. */
    .cmd-shell {
        margin: 6px 8px 12px;
        border: 1px solid var(--color-border);
        border-radius: 10px;
        background: var(--color-panel-2);
        overflow: hidden;
        animation: cmd-shell-in calc(160ms * var(--cmd-motion-mult, 1)) var(--ease-out, ease-out);
    }
    @keyframes cmd-shell-in {
        from {
            opacity: 0;
            transform: translateY(-4px);
        }
        to {
            opacity: 1;
            transform: translateY(0);
        }
    }
    .cmd-shell.is-running {
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
    }
    .cmd-shell.is-error {
        border-color: color-mix(in srgb, var(--color-error) 55%, var(--color-border));
    }
    .cmd-shell.is-cancelled {
        border-color: color-mix(in srgb, var(--color-text-secondary) 55%, var(--color-border));
    }
    .cmd-shell-head {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 10px 12px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .cmd-shell-head-status {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 22px;
        height: 22px;
        color: var(--color-accent);
        flex-shrink: 0;
    }
    .cmd-shell.is-error .cmd-shell-head-status {
        color: var(--color-error);
    }
    .cmd-shell.is-cancelled .cmd-shell-head-status {
        color: var(--color-text-secondary);
    }
    .cmd-shell-head-status :global(.cmd-shell-icon) {
        width: 16px;
        height: 16px;
    }
    .cmd-shell-spinner {
        width: 14px;
        height: 14px;
        border: 2px solid color-mix(in srgb, var(--color-accent) 30%, transparent);
        border-top-color: var(--color-accent);
        border-radius: 50%;
        animation: cmd-shell-spin 720ms linear infinite;
    }
    @keyframes cmd-shell-spin {
        to {
            transform: rotate(360deg);
        }
    }
    .cmd-shell-head-text {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
    .cmd-shell-title {
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .cmd-shell-kind {
        font-size: 10px;
        font-weight: 500;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        text-transform: lowercase;
        color: var(--color-text-secondary);
        background: var(--color-panel-3);
        padding: 1px 6px;
        border-radius: 4px;
    }
    .cmd-shell-sub {
        font-size: 11.5px;
        color: var(--color-text-secondary);
    }
    .cmd-shell-actions {
        display: inline-flex;
        gap: 6px;
        flex-shrink: 0;
    }
    .cmd-shell-btn {
        appearance: none;
        border: 1px solid var(--color-border);
        background: transparent;
        color: var(--color-text);
        font-size: 11.5px;
        padding: 4px 10px;
        border-radius: 6px;
        cursor: pointer;
        transition: background 120ms ease, border-color 120ms ease;
    }
    .cmd-shell-btn:hover {
        background: var(--color-panel-3);
    }
    .cmd-shell-btn-cancel {
        border-color: color-mix(in srgb, var(--color-error) 50%, var(--color-border));
        color: var(--color-error);
    }
    .cmd-shell-btn-cancel:hover {
        background: color-mix(in srgb, var(--color-error) 12%, transparent);
    }
    .cmd-shell-out {
        margin: 0;
        padding: 10px 12px;
        max-height: 280px;
        overflow-y: auto;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        line-height: 1.5;
        color: var(--color-text);
        background: var(--color-panel-1);
        white-space: pre;
    }
    .cmd-shell-line {
        display: block;
    }
    .cmd-shell-line.is-err {
        color: var(--color-error);
    }
    .cmd-shell-empty {
        margin: 0;
        padding: 12px;
        font-size: 12px;
        color: var(--color-text-secondary);
        text-align: center;
    }

    /* Snippet inside content-search rows. Two-line clamp, muted, with
       `<mark>` highlighting for matched keywords (escapes are applied
       on the script side; this is the visual treatment). */
    .cmd-row-snippet {
        display: -webkit-box;
        -webkit-line-clamp: 2;
        line-clamp: 2;
        -webkit-box-orient: vertical;
        overflow: hidden;
        font-size: 12px;
        line-height: 1.45;
        color: var(--color-text-secondary);
        padding: 2px 0 1px;
    }
    .cmd-row-snippet :global(mark) {
        background: color-mix(in srgb, var(--color-accent) 30%, transparent);
        color: var(--color-text);
        padding: 1px 3px;
        border-radius: 3px;
        font-weight: 600;
    }
    /* Image entries in clipboard mode show a real thumbnail inside
       the row icon slot. Same 32×32 frame as the icon container so
       rows stay aligned. */
    .cmd-row-thumb {
        width: 28px;
        height: 28px;
        border-radius: 6px;
        object-fit: cover;
        border: 1px solid color-mix(in srgb, var(--color-border) 70%, transparent);
    }
    /* Color-category entries: the row's icon tile IS the swatch.
       Inset rings keep the swatch defined against the panel (light
       colors don't blend into the row bg; dark ones don't disappear
       on the panel-2 background). Inner outer-ring = subtle dark
       outline; inner inner-ring = subtle light highlight. Standard
       color-picker treatment. */
    .cmd-row-icon.is-color-swatch {
        background: transparent;
        box-shadow:
            inset 0 0 0 1px rgba(0, 0, 0, 0.30),
            inset 0 0 0 2px rgba(255, 255, 255, 0.10);
    }
    /* "Pinned" badge inside clipboard row subtitle. */
    .cmd-row-pinned {
        color: var(--color-accent);
        font-weight: 500;
    }
    /* Sensitive-content banner inside the preview pane head. Red-
       tinted box that mirrors the row badge so the user keeps the
       "this is sensitive" signal as they move from list → preview.
       The actual value renders normally underneath — preview IS the
       reveal surface. */
    .cmd-preview-sensitive {
        display: flex;
        align-items: center;
        gap: 8px;
        margin: 8px 16px 12px;
        padding: 8px 10px;
        background: color-mix(in srgb, var(--color-error, #fb7185) 12%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-error, #fb7185) 30%, transparent);
        border-radius: 8px;
        font-size: 11.5px;
        color: var(--color-text-secondary);
    }
    .cmd-preview-sensitive-pill {
        display: inline-flex;
        align-items: center;
        padding: 1px 6px;
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.04em;
        text-transform: uppercase;
        color: var(--color-error, #fb7185);
        background: color-mix(in srgb, var(--color-error, #fb7185) 18%, transparent);
        border-radius: 4px;
    }
    .cmd-preview-sensitive-text {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    /* Sensitive value wrapper inside clipboard row title — blurred
       by default when the row is `.is-sensitive`. The badge sits
       OUTSIDE this wrapper so it stays readable. To inspect the
       actual value the user opens the preview pane (Ctrl+P), which
       renders unblurred since the preview DOM doesn't carry the
       `is-sensitive` class.

       The filter:blur ban from DesignPro.md applies to animated /
       full-window blur. This is a static filter on a small inline
       element, applied once at mount and never animated — same
       pattern color swatches use. No DWM coupling. */
    .cmd-row.is-sensitive .cmd-row-secret {
        filter: blur(5px);
        /* Compensate for blur "leaking" past the text edges so the
           neighbouring badge doesn't appear muddied. */
        padding: 0 4px;
    }
    /* Cursor + faint warning underline cue that there's hidden text. */
    .cmd-row.is-sensitive .cmd-row-secret {
        text-decoration: underline dotted color-mix(in srgb, var(--color-error, #fb7185) 50%, transparent);
        text-underline-offset: 3px;
    }

    /* Sensitive-content badge — surfaces inline next to the file
       title. Red-tinted so it reads as a warning without being a
       full destructive treatment (no border, just background+color).
       Click-through to the row's onclick stays intact since this is
       a `<span>`. */
    .cmd-row-sensitive {
        display: inline-flex;
        align-items: center;
        margin-left: 6px;
        padding: 1px 6px;
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.04em;
        text-transform: uppercase;
        color: var(--color-error, #fb7185);
        background: color-mix(in srgb, var(--color-error, #fb7185) 14%, transparent);
        border-radius: 4px;
        vertical-align: 1px;
    }
    /* +N matches pill on content rows — accent-tinted so dense
       matches catch the eye. Smaller + bolder than the .cmd-row-meta
       byte-count chip so the two coexist visually. */
    .cmd-row-pill {
        flex: none;
        display: inline-flex;
        align-items: center;
        height: 20px;
        padding: 0 8px;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        font-weight: 600;
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
        border-radius: 999px;
        white-space: nowrap;
    }
    .cmd-row-text {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 2px;
        /* Optical alignment: text carries more visual weight below its
           mathematical center — a 1px push-down balances it against the
           icon tile (the Raycast trick). */
        margin-top: 1px;
    }
    .cmd-row-title {
        font-size: 13px;
        font-weight: 500;
        color: var(--color-text);
        letter-spacing: -0.005em;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .cmd-row-sub {
        /* V2: rows are single-line (icon + name only); the secondary path/desc
           line is dropped — that detail lives in the preview instead. */
        display: none;
    }
    .cmd-row-meta {
        flex: none;
        font-size: 11.5px;
        color: var(--color-muted);
        font-variant-numeric: tabular-nums;
        white-space: nowrap;
    }
    /* Count chip on the Commands category rows (2026-06-13 redesign).
       Neutral/muted — it's a quiet count, not an accent call-to-action. */
    .cmd-row-count {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 20px;
        height: 20px;
        padding: 0 7px;
        font-size: 11.5px;
        font-weight: 600;
        font-variant-numeric: tabular-nums;
        color: var(--color-muted);
        background: color-mix(in srgb, var(--color-text) 8%, transparent);
        border-radius: 999px;
    }
    :global(.cmd-row-chevron) {
        flex: none;
        width: 15px;
        height: 15px;
        color: var(--color-muted);
        opacity: 0.65;
    }
    /* Master-detail item rows rendered INSIDE the preview pane for a
       Commands category. Plain column of `.cmd-row`s, scrollable. */
    .cmd-cat-rows {
        display: flex;
        flex-direction: column;
        gap: 2px;
        /* Fill the remaining height of the .cmd-stage-host column and scroll
           inside itself — long Control Panel / Running-apps lists must not
           overflow the pane. */
        flex: 1 1 0;
        min-height: 0;
        overflow-y: auto;
        margin: 6px -6px 0;
        padding: 0 6px;
    }
    /* System-info card laid out inside the preview pane (no card chrome —
       the pane itself is the surface). Scrolls if the disk list is long. */
    .cmd-preview-sysinfo {
        margin-top: 10px;
        flex: 1 1 0;
        min-height: 0;
        overflow-y: auto;
    }
    /* ─── Quick-action split card (calculator / unit conversion) ──
       Mirrors /overlay's `.overlay-quick-card`: 22-px values centred
       on each side, "→" arrow between, a small chip ("Expression" /
       "Input" / "Result") under each value. Single keyboard / click
       target — the whole card is a button so Enter activates it.
       Accent-tinted background + border to read as "actionable",
       lifts to a heavier accent on selected. */
    .cmd-quick-card {
        display: flex;
        align-items: stretch;
        width: 100%;
        padding: 18px 12px;
        margin-bottom: 4px;
        background: color-mix(in srgb, var(--color-text) 4%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-accent) 25%, var(--color-border));
        border-radius: var(--radius-card, 12px);
        /* Subtle lift — the quick card is the one actionable "result card"
           in the list, so it floats slightly above the flat rows. */
        box-shadow: var(--shadow-sm);
        cursor: pointer;
        text-align: left;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            border-color var(--dur-micro, 130ms) var(--ease-out, ease),
            box-shadow var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .cmd-quick-card:hover {
        background: color-mix(in srgb, var(--color-text) 6%, transparent);
        border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
    }
    /* Canonical selected pattern (see .cmd-row.is-selected): panel-2 surface
       + accent rounded-pill strip via ::before — replaces the accent-tinted
       fill + square inset bar. */
    .cmd-quick-card.is-selected {
        position: relative;
        background: var(--color-panel-2);
        border-color: color-mix(in srgb, var(--color-accent) 60%, var(--color-border));
    }
    .cmd-quick-card.is-selected::before {
        content: '';
        position: absolute;
        left: 3px;
        top: 8px;
        bottom: 8px;
        width: 3px;
        border-radius: var(--radius-pill, 999px);
        background: var(--color-accent);
    }
    /* System-info label/value grid (Commands → System Info). Rendered in
       the preview pane now; the surrounding `.cmd-preview-sysinfo` supplies
       the top margin. */
    .cmd-sysinfo-grid {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .cmd-sysinfo-cell {
        display: flex;
        align-items: baseline;
        gap: 12px;
        min-width: 0;
    }
    .cmd-sysinfo-label {
        flex: 0 0 80px;
        font-size: 11.5px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        color: var(--color-muted);
    }
    .cmd-sysinfo-value {
        flex: 1;
        min-width: 0;
        font-size: 13px;
        color: var(--color-text);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-variant-numeric: tabular-nums;
    }
    .cmd-quick-side {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 10px;
        padding: 4px 14px;
        text-align: center;
    }
    .cmd-quick-value {
        font-size: 22px;
        font-weight: 600;
        line-height: 1.2;
        letter-spacing: -0.012em;
        color: var(--color-text);
        max-width: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-variant-numeric: tabular-nums;
    }
    /* The result side gets the accent treatment so the eye lands on
       the answer first — same convention Spotlight/Raycast use. */
    .cmd-quick-value-result {
        color: var(--color-accent);
        font-family: 'JetBrains Mono', ui-monospace, monospace;
    }
    /* Chip beneath each value labelling what side it is. Muted so
       the value reads as the headline. */
    .cmd-quick-chip {
        display: inline-flex;
        align-items: center;
        padding: 3px 9px;
        font-size: 11.5px;
        font-weight: 500;
        color: var(--color-text-secondary);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 5px;
        line-height: 1;
    }
    .cmd-quick-arrow {
        align-self: center;
        font-size: 22px;
        color: var(--color-muted);
        padding: 0 4px;
    }
    /* "Copied!" indicator inside the section label after the user
       activates the quick-action. Self-clears with quickActionCopied
       reset after 1.4 s. */
    .cmd-section-copied {
        margin-left: auto;
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.04em;
        color: var(--color-accent);
        animation: cmd-section-copied-in calc(160ms * var(--cmd-motion-mult, 1)) var(--ease-out) both;
    }
    @keyframes cmd-section-copied-in {
        from {
            opacity: 0;
            transform: translateY(-2px);
        }
        to {
            opacity: 1;
            transform: translateY(0);
        }
    }

    /* ─── Voice mode: section container ─────────────────────── */
    .cmd-voice-section {
        display: flex;
        flex-direction: column;
        gap: 10px;
        padding: 4px 0 12px;
    }

    /* ─── Voice sub-mode toggle ───────────────────────────────── */
    .cmd-voice-sub {
        align-self: flex-start;
        display: inline-flex;
        align-items: center;
        gap: 4px;
        margin: 4px 8px 6px;
        padding: 3px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
    }
    .cmd-voice-sub-btn {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        height: 28px;
        padding: 0 12px;
        background: transparent;
        border: none;
        border-radius: 6px;
        color: var(--color-text-secondary);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .cmd-voice-sub-btn:hover:not(.is-on) {
        color: var(--color-text);
    }
    .cmd-voice-sub-btn.is-on {
        background: var(--color-panel);
        color: var(--color-text);
        box-shadow: var(--shadow-sm);
    }
    :global(.cmd-voice-sub-ico) {
        width: 12px;
        height: 12px;
    }

    /* ─── Voice listening state strip ──────────────────────────
       A single-row indicator under the sub-toggle. Pulses red when
       the mic is actually capturing audio; sits muted-accent when
       armed-but-quiet. */
    .cmd-voice-state {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        margin: 0 8px;
    }
    .cmd-voice-state-label {
        font-size: 12px;
        font-weight: 500;
        color: var(--color-error);
    }
    .cmd-voice-state-dim {
        color: var(--color-text-secondary);
        font-weight: 400;
    }
    /* Quiet armed state — a small accent dot. */
    .cmd-voice-dot {
        display: inline-block;
        width: 8px;
        height: 8px;
        border-radius: 50%;
        background: var(--color-accent);
        box-shadow: 0 0 0 1.5px color-mix(in srgb, var(--color-accent) 30%, transparent);
    }
    .cmd-voice-dot-off {
        background: var(--color-muted);
        box-shadow: none;
    }
    /* Live capture — error-red pulse, universal "recording now" cue. */
    .cmd-voice-pulse {
        display: inline-block;
        width: 10px;
        height: 10px;
        border-radius: 50%;
        background: var(--color-error);
        animation: cmd-voice-pulse 1.2s ease-in-out infinite;
    }
    @keyframes cmd-voice-pulse {
        0%, 100% {
            transform: scale(0.85);
            opacity: 0.65;
            box-shadow: 0 0 0 0 color-mix(in srgb, var(--color-error) 60%, transparent);
        }
        50% {
            transform: scale(1.25);
            opacity: 1;
            box-shadow: 0 0 0 6px color-mix(in srgb, var(--color-error) 0%, transparent);
        }
    }

    /* ─── Live partial transcript — italic ghost line under the
           listening strip. Cleared on every final transcript. */
    .cmd-voice-partial {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        margin: 0 8px;
        padding: 6px 10px;
        background: var(--color-panel-2);
        border: 1px dashed color-mix(in srgb, var(--color-accent) 30%, var(--color-border));
        border-radius: var(--radius-control);
        font-size: 13px;
        font-style: italic;
        color: var(--color-text-secondary);
        align-self: flex-start;
    }
    :global(.cmd-voice-partial-ico) {
        width: 11px;
        height: 11px;
        color: var(--color-accent);
    }

    /* ─── Outcome chip (command mode) ──────────────────────────
       Self-clearing message ("✓ Opened Chrome", "No matching command…")
       displayed under the listening strip for ~4s. Accent-toned so
       it reads as a deliberate notification, not an error. */
    .cmd-voice-outcome {
        margin: 0 8px;
        padding: 8px 12px;
        background: var(--color-accent-soft);
        border: 1px solid color-mix(in srgb, var(--color-accent) 35%, var(--color-border));
        border-radius: var(--radius-control);
        font-size: 12px;
        color: var(--color-accent);
        align-self: flex-start;
        animation: cmd-voice-outcome-in 180ms var(--ease-out) both;
    }
    @keyframes cmd-voice-outcome-in {
        from {
            opacity: 0;
            transform: translate3d(0, -4px, 0);
        }
        to {
            opacity: 1;
            transform: translate3d(0, 0, 0);
        }
    }

    /* ─── Mode-help hint above results / examples ─────────────── */
    .cmd-voice-hint {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        margin: 0 8px;
        font-size: 11.5px;
        color: var(--color-muted);
    }
    :global(.cmd-voice-hint-ico) {
        width: 12px;
        height: 12px;
        color: var(--color-accent);
    }

    /* Block wrapper for voice transcribe results so the sections look
       slightly more compact than default-mode rendering. */
    .cmd-voice-results-block {
        margin-top: 6px;
    }

    /* Command-mode example list — discoverable cheat sheet of phrases
       that should match through `executeVoiceCommand`. */
    .cmd-voice-cmd-hints {
        margin: 0 8px;
        padding: 8px 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-divider, var(--color-border));
        border-radius: var(--radius-control);
        display: flex;
        flex-direction: column;
        gap: 4px;
    }
    .cmd-voice-cmd-row {
        padding: 4px 0;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 12px;
        color: var(--color-text-secondary);
    }

    /* ─── Continuous command mode toggle ───────────────────────── */
    .cmd-voice-cont {
        display: flex;
        align-items: center;
        gap: 8px;
        width: 100%;
        margin-top: 12px;
        padding: 9px 12px;
        border: 1px solid var(--color-border);
        border-radius: 8px;
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
        font-size: 12.5px;
        font-weight: 500;
        text-align: left;
        cursor: pointer;
        transition:
            background-color calc(var(--dur-micro) * var(--cmd-motion-mult, 1)) var(--ease-out),
            border-color calc(var(--dur-micro) * var(--cmd-motion-mult, 1)) var(--ease-out);
    }
    .cmd-voice-cont:hover {
        color: var(--color-text);
        border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
    }
    /* Active = the mic is genuinely open. This has to read as unmistakably ON
       — an always-listening state that looks idle is exactly the thing the
       privacy design refuses to ship. */
    .cmd-voice-cont.is-on {
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        color: var(--color-accent);
    }
    .cmd-voice-cont :global(.cmd-voice-cont-ico) {
        flex: none;
        width: 14px;
        height: 14px;
    }
    .cmd-voice-cont-label {
        flex: 1;
    }
    .cmd-voice-cont-note {
        margin: 6px 2px 0;
        font-size: 11px;
        line-height: 1.45;
        color: var(--color-muted);
    }

    /* ─── Empty state ──────────────────────────────────────────── */
    .cmd-empty {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 8px;
        padding: 36px 16px;
        text-align: center;
    }
    :global(.cmd-empty-ico) {
        width: 28px;
        height: 28px;
        color: var(--color-muted);
    }
    :global(.cmd-empty-ico-accent) {
        color: var(--color-accent);
    }
    /* Loading variant: the icon breathes so "still working" reads differently
       from a final empty state. A functional indicator, so (like the spinners)
       it deliberately doesn't scale with the motion multiplier — the mult:0
       kill-rule still flattens it for Reduced. */
    .cmd-empty.is-loading :global(.cmd-empty-ico) {
        animation: cmd-empty-pulse 1.1s ease-in-out infinite alternate;
    }
    @keyframes cmd-empty-pulse {
        from {
            opacity: 0.35;
        }
        to {
            opacity: 0.9;
        }
    }
    .cmd-empty-text {
        margin: 0;
        font-size: 13px;
        color: var(--color-text);
    }
    .cmd-empty-sub {
        margin: 0;
        font-size: 12px;
        color: var(--color-muted);
    }

    /* ─── Footer ────────────────────────────────────────────────── */
    /* ─── Syntax cheatsheet ───────────────────────────────────
       Replaces the body when open. Plain typographic layout —
       no row chrome, no selection state. The point is reading,
       not navigation. Code samples in mono with a faint
       background; descriptions in regular muted text. */
    .cmd-cheatsheet {
        padding: 4px 6px 8px;
    }
    .cmd-cheatsheet-head {
        padding: 6px 12px 14px;
        border-bottom: 1px solid color-mix(in srgb, var(--color-border) 50%, transparent);
        margin-bottom: 6px;
    }
    .cmd-cheatsheet-title {
        margin: 0 0 4px;
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
        letter-spacing: -0.005em;
    }
    .cmd-cheatsheet-sub {
        margin: 0;
        font-size: 12px;
        color: var(--color-muted);
        line-height: 1.5;
    }
    .cmd-cheatsheet-sub kbd {
        display: inline-flex;
        align-items: center;
        height: 18px;
        padding: 0 6px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-bottom-width: 2px;
        border-radius: 4px;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 10px;
        font-weight: 500;
        color: var(--color-text-secondary);
    }
    .cmd-cheatsheet-group {
        padding: 10px 12px;
    }
    .cmd-cheatsheet-gtitle {
        margin: 0 0 8px;
        font-size: 10px;
        font-weight: 600;
        letter-spacing: 0.08em;
        text-transform: uppercase;
        color: var(--color-text-secondary);
    }
    .cmd-cheatsheet-note {
        margin: -4px 0 8px;
        font-size: 11.5px;
        color: var(--color-muted);
        font-style: italic;
    }
    .cmd-cheatsheet-row {
        display: grid;
        grid-template-columns: minmax(140px, 200px) 1fr;
        align-items: baseline;
        gap: 12px;
        padding: 5px 0;
        font-size: 13px;
        color: var(--color-text);
    }
    .cmd-cheatsheet-row code {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        font-weight: 500;
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 8%, var(--color-panel-2));
        padding: 2px 6px;
        border-radius: 4px;
        max-width: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    /* Inline code inside descriptions (e.g., "Also: <code>uuid</code>")
       stays small + no accent so it doesn't fight with the leading
       example code. */
    .cmd-cheatsheet-row span code {
        font-size: 10px;
        color: var(--color-text-secondary);
        background: var(--color-panel-2);
    }
    .cmd-cheatsheet-row span {
        color: var(--color-text-secondary);
        line-height: 1.5;
    }

    /* ─── System-command row variant ─────────────────────────
       Destructive commands (shutdown / restart / signout) get a
       subtle red-tinged icon so the user sees "this is the kind
       of thing that will reboot my machine" before pressing
       Enter. Non-destructive (lock / sleep / settings / terminal)
       stay neutral. */
    .cmd-row-system.is-destructive .cmd-row-icon {
        color: var(--color-error, #fb7185);
        background: color-mix(in srgb, var(--color-error, #fb7185) 14%, var(--color-panel-2));
    }
    .cmd-row-system.is-destructive.is-selected .cmd-row-icon {
        background: color-mix(in srgb, var(--color-error, #fb7185) 22%, var(--color-panel-2));
    }

    /* ─── Load-more affordance for paginated file results ─────
       Sits below the last visible file/content row. Wave I
       (2026-05-27): button removed in favour of pure scroll-to-load
       (Raycast pattern). The row now just shows a quiet status hint
       — spinner while a fetch is in flight, "Scroll for more (X of
       Y)" otherwise — so the user knows MORE exists even before
       reaching the auto-trigger zone at the body bottom. */
    .cmd-loadmore {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 8px;
        padding: 12px 8px;
        font-size: 12px;
        color: var(--color-muted);
    }
    .cmd-loadmore-hint {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-size: 11.5px;
        color: var(--color-muted);
        font-style: italic;
        letter-spacing: 0.01em;
        opacity: 0.85;
    }
    .cmd-loadmore-spinner {
        width: 12px;
        height: 12px;
        border: 1.5px solid var(--color-border);
        border-top-color: var(--color-accent);
        border-radius: 50%;
        animation: cmd-loadmore-spin 800ms linear infinite;
    }
    @keyframes cmd-loadmore-spin {
        to {
            transform: rotate(360deg);
        }
    }
    @media (prefers-reduced-motion: reduce) {
        .cmd-loadmore-spinner {
            animation-duration: 2400ms;
        }
    }

    /* ─── Action panel (Ctrl+Space) ─────────────────────────
       Floating popup anchored bottom-right inside the .cmd-panel.
       Same frosted-glass treatment as the panel itself so it
       reads as part of the same surface. Backdrop catches outside
       clicks. The panel slides up + fades in on open. */
    .cmd-actions-backdrop {
        position: absolute;
        inset: 0;
        background: transparent;
        border: none;
        padding: 0;
        cursor: default;
        z-index: 50;
    }
    .cmd-actions {
        position: absolute;
        bottom: 56px;
        right: 12px;
        width: 320px;
        max-height: min(420px, 60vh);
        display: flex;
        flex-direction: column;
        background: color-mix(in srgb, var(--color-panel) 92%, transparent);
        backdrop-filter: blur(32px) saturate(180%);
        -webkit-backdrop-filter: blur(32px) saturate(180%);
        border: 1px solid color-mix(in srgb, var(--color-text) 18%, transparent);
        border-radius: 12px;
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 16%, transparent),
            0 24px 60px rgba(0, 0, 0, 0.55);
        overflow: hidden;
        z-index: 51;
        animation: cmd-actions-in calc(160ms * var(--cmd-motion-mult, 1)) var(--ease-out) both;
    }
    @keyframes cmd-actions-in {
        from {
            opacity: 0;
            transform: translate3d(0, 8px, 0);
        }
        to {
            opacity: 1;
            transform: translate3d(0, 0, 0);
        }
    }
    .cmd-actions-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 10px 12px;
        border-bottom: 1px solid color-mix(in srgb, var(--color-border) 50%, transparent);
    }
    .cmd-actions-title {
        font-size: 11.5px;
        font-weight: 600;
        letter-spacing: 0.06em;
        text-transform: uppercase;
        color: var(--color-text-secondary);
    }
    .cmd-actions-hint {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        color: var(--color-muted);
        font-size: 11.5px;
    }
    .cmd-actions-hint kbd {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 18px;
        height: 18px;
        padding: 0 6px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-bottom-width: 2px;
        border-radius: 4px;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 10px;
        font-weight: 500;
    }
    .cmd-actions-list {
        flex: 1;
        margin: 0;
        padding: 6px;
        list-style: none;
        overflow-y: auto;
        scrollbar-gutter: stable;
    }
    .cmd-actions-item {
        position: relative;
        display: flex;
        align-items: center;
        gap: 10px;
        width: 100%;
        padding: 8px 10px;
        background: transparent;
        border: none;
        border-radius: 8px;
        color: var(--color-text);
        text-align: left;
        cursor: pointer;
        transition: background-color var(--dur-micro) var(--ease-out);
    }
    /* Hover already moves selectedIndex (onmouseenter), so visual hover
       collapses into .is-selected. No separate :hover rule needed. */
    .cmd-actions-item.is-selected {
        background: var(--color-panel-2);
    }
    .cmd-actions-item.is-selected::before {
        content: '';
        position: absolute;
        left: 3px;
        top: 8px;
        bottom: 8px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .cmd-actions-icon {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 28px;
        height: 28px;
        border-radius: 7px;
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
        transition: color var(--dur-micro) var(--ease-out);
    }
    .cmd-actions-item.is-selected .cmd-actions-icon {
        color: var(--color-accent);
    }
    :global(.cmd-actions-icon-svg) {
        width: 14px;
        height: 14px;
    }
    .cmd-actions-text {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 1px;
    }
    .cmd-actions-label {
        font-size: 13px;
        font-weight: 500;
        color: var(--color-text);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .cmd-actions-sublabel {
        font-size: 11.5px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    :global(.cmd-actions-chevron) {
        flex: none;
        width: 14px;
        height: 14px;
        color: var(--color-muted);
        opacity: 0;
        transition: opacity var(--dur-micro) var(--ease-out);
    }
    .cmd-actions-item.is-selected :global(.cmd-actions-chevron) {
        opacity: 1;
    }
    .cmd-actions-feedback {
        padding: 8px 12px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 50%, transparent);
        font-size: 11.5px;
        font-weight: 500;
        color: var(--color-accent);
        animation: cmd-actions-feedback-in 140ms var(--ease-out) both;
    }
    @keyframes cmd-actions-feedback-in {
        from {
            opacity: 0;
            transform: translateY(-2px);
        }
        to {
            opacity: 1;
            transform: translateY(0);
        }
    }
    @media (prefers-reduced-motion: reduce) {
        .cmd-actions,
        .cmd-actions-feedback {
            animation: none;
        }
    }

    .cmd-foot {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 9px 16px;
        font-size: 11.5px;
        color: var(--color-muted);
        border-top: 1px solid color-mix(in srgb, var(--color-border) 55%, transparent);
        flex-shrink: 0;
    }
    .cmd-foot-hints {
        display: flex;
        align-items: center;
        /* At the widened 900px window the 5–6 hint chips (Navigate / Execute
           / Close / Actions / Syntax / Files-Inside) sit on ONE line. Tight
           gap keeps them there; flex-wrap stays only as a last-resort safety
           net so a narrower future layout degrades gracefully instead of
           clipping. */
        flex: 1 1 auto;
        min-width: 0;
        flex-wrap: wrap;
        column-gap: 10px;
        row-gap: 6px;
    }
    .cmd-hint {
        white-space: nowrap;
        display: inline-flex;
        align-items: center;
        gap: 6px;
    }
    .cmd-foot kbd {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 18px;
        height: 18px;
        padding: 0 6px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-bottom-width: 2px;
        border-radius: 5px;
        color: var(--color-text-secondary);
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 10px;
        font-weight: 500;
        line-height: 1;
    }
    .cmd-foot-status {
        display: inline-flex;
        align-items: center;
        gap: 12px;
        flex: none;
        padding-left: 12px;
        border-left: 1px solid color-mix(in srgb, var(--color-border) 45%, transparent);
    }
    .cmd-foot-listening {
        color: var(--color-accent);
        font-weight: 500;
    }
    .cmd-foot-offline {
        display: inline-flex;
        align-items: center;
        gap: 6px;
    }
    :global(.cmd-foot-offline-ico) {
        width: 11px;
        height: 11px;
    }

    /* ─── Clipboard label editor modal (Alt+L) ─────────────────────── */
    .cmd-label-backdrop {
        position: absolute;
        inset: 0;
        z-index: 60;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 24px;
        background: color-mix(in srgb, var(--color-bg) 70%, transparent);
        backdrop-filter: blur(4px);
        border-radius: inherit;
    }
    .cmd-label-modal {
        width: 340px;
        max-width: 100%;
        display: flex;
        flex-direction: column;
        gap: 10px;
        padding: 16px 18px;
        border-radius: var(--radius-card, 12px);
        border: 1px solid var(--color-border);
        background: var(--color-panel);
        box-shadow: var(--shadow-lg);
    }
    .cmd-label-title {
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
    }
    .cmd-label-input {
        height: 36px;
        border-radius: var(--radius-control, 8px);
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text);
        padding: 0 10px;
        font-size: 13px;
        outline: none;
    }
    .cmd-label-input:focus {
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
    }
    .cmd-label-hint {
        font-size: 11.5px;
        color: var(--color-muted);
        line-height: 1.45;
    }
    .cmd-label-actions {
        display: flex;
        justify-content: flex-end;
        gap: 8px;
        margin-top: 2px;
    }
    .cmd-label-btn {
        height: 30px;
        padding: 0 12px;
        border-radius: var(--radius-control, 8px);
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-size: 13px;
        font-weight: 500;
        cursor: pointer;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .cmd-label-btn:hover {
        background: var(--color-panel-3);
    }
    .cmd-label-btn-primary {
        background: var(--color-accent);
        color: var(--color-accent-contrast);
        border-color: var(--color-accent);
    }
    .cmd-label-btn-primary:hover {
        background: var(--color-accent-hover, var(--color-accent));
        filter: brightness(1.05);
    }

    /* ─── Destructive system-command confirm modal ──────────────── */
    .cmd-confirm-backdrop {
        position: absolute;
        inset: 0;
        z-index: 70;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 24px;
        background: color-mix(in srgb, var(--color-bg) 72%, transparent);
        backdrop-filter: blur(4px);
        border-radius: inherit;
    }
    .cmd-confirm-modal {
        width: 360px;
        max-width: 100%;
        display: flex;
        flex-direction: column;
        align-items: center;
        text-align: center;
        gap: 8px;
        padding: 22px 22px 16px;
        border-radius: var(--radius-card, 12px);
        border: 1px solid var(--color-border);
        background: var(--color-panel);
        box-shadow: var(--shadow-lg);
        outline: none;
    }
    .cmd-confirm-icon {
        width: 46px;
        height: 46px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border-radius: 50%;
        background: color-mix(in srgb, var(--color-error) 16%, var(--color-panel-2));
        border: 1px solid color-mix(in srgb, var(--color-error) 30%, var(--color-border));
        color: var(--color-error);
        margin-bottom: 2px;
    }
    :global(.cmd-confirm-glyph) {
        width: 22px;
        height: 22px;
    }
    .cmd-confirm-title {
        font-size: 15px;
        font-weight: 600;
        letter-spacing: -0.01em;
        color: var(--color-text);
    }
    .cmd-confirm-desc {
        font-size: 13px;
        line-height: 1.5;
        color: var(--color-text-secondary);
        max-width: 280px;
    }
    .cmd-confirm-actions {
        display: flex;
        justify-content: center;
        gap: 8px;
        margin-top: 6px;
        width: 100%;
    }
    .cmd-confirm-btn {
        flex: 1;
        height: 34px;
        padding: 0 14px;
        border-radius: var(--radius-control, 8px);
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-size: 13px;
        font-weight: 600;
        cursor: pointer;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            border-color var(--dur-micro, 130ms) var(--ease-out, ease),
            filter var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .cmd-confirm-btn:hover {
        background: var(--color-panel-3);
        border-color: var(--color-border-strong);
    }
    /* Danger uses accent-contrast ink — the error surface is a light rose in
       dark themes where white text fails contrast (same rule as kit Button). */
    .cmd-confirm-btn-danger {
        background: var(--color-error);
        color: var(--color-accent-contrast);
        border-color: var(--color-error);
    }
    .cmd-confirm-btn-danger:hover {
        filter: brightness(1.08);
        border-color: var(--color-error);
        background: var(--color-error);
    }
    .cmd-confirm-hint {
        margin-top: 8px;
        font-size: 11.5px;
        color: var(--color-muted);
    }
    .cmd-confirm-hint kbd {
        font-family: inherit;
        font-size: 10px;
        padding: 1px 6px;
        border-radius: 4px;
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
    }

    /* ─── Live appearance editor (Ctrl+Alt+A) ──────────────────────── */
    .cmd-appear {
        position: absolute;
        left: 50%;
        /* Wave F (2026-05-27): editor grew with Wave A-E settings — vertically
           anchor top + bottom so it never overflows the palette. The flex
           column scrolls internally when its content exceeds max-height. */
        top: 12px;
        bottom: 12px;
        transform: translateX(-50%);
        width: min(480px, calc(100% - 28px));
        max-height: calc(100% - 24px);
        z-index: 40;
        display: flex;
        flex-direction: column;
        gap: 10px;
        padding: 14px 16px 13px;
        background: var(--color-panel-2);
        border: 1px solid color-mix(in srgb, var(--color-accent) 32%, var(--color-border));
        border-radius: 14px;
        box-shadow: var(--shadow-lg, 0 16px 40px rgba(0, 0, 0, 0.5));
        overflow-y: auto;
        scrollbar-width: thin;
    }
    .cmd-appear-head {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .cmd-appear :global(.cmd-appear-head-ico) {
        width: 15px;
        height: 15px;
        color: var(--color-accent);
    }
    .cmd-appear-title {
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
    }
    .cmd-appear-live {
        font-size: 10px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-accent) 30%, var(--color-border));
        padding: 1px 6px;
        border-radius: 999px;
    }
    .cmd-appear-x {
        margin-left: auto;
        display: grid;
        place-items: center;
        width: 24px;
        height: 24px;
        border: none;
        border-radius: 7px;
        background: transparent;
        color: var(--color-muted);
        cursor: pointer;
    }
    .cmd-appear-x:hover {
        background: var(--color-panel);
        color: var(--color-text);
    }
    .cmd-appear :global(.cmd-appear-x-ico) {
        width: 14px;
        height: 14px;
    }
    .cmd-appear-row {
        display: flex;
        align-items: center;
        gap: 12px;
    }
    .cmd-appear-label {
        flex: none;
        width: 84px;
        font-size: 12px;
        font-weight: 500;
        color: var(--color-text-secondary);
    }
    .cmd-appear-range {
        flex: 1;
        min-width: 0;
        accent-color: var(--color-accent);
        cursor: pointer;
    }
    .cmd-appear-val {
        flex: none;
        width: 40px;
        text-align: right;
        font-size: 12px;
        font-variant-numeric: tabular-nums;
        color: var(--color-text-secondary);
    }
    .cmd-appear-swatches {
        display: flex;
        align-items: center;
        gap: 6px;
        flex-wrap: wrap;
    }
    .cmd-appear-swatch {
        width: 22px;
        height: 22px;
        padding: 0;
        border-radius: 999px;
        border: 2px solid transparent;
        background: var(--sw, var(--color-accent));
        cursor: pointer;
    }
    .cmd-appear-swatch.is-on {
        border-color: var(--color-text);
        box-shadow: 0 0 0 2px var(--color-panel-2);
    }
    .cmd-appear-swatch-theme {
        display: grid;
        place-items: center;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        font-size: 10px;
        font-weight: 600;
        color: var(--color-text-secondary);
    }
    .cmd-appear-swatch-theme.is-on {
        border-color: var(--color-accent);
        color: var(--color-accent);
    }
    .cmd-appear-color {
        width: 26px;
        height: 26px;
        padding: 0;
        border: 1px solid var(--color-border);
        border-radius: 7px;
        background: var(--color-panel);
        cursor: pointer;
    }
    .cmd-appear-toggle {
        position: relative;
        width: 40px;
        height: 22px;
        padding: 0;
        border-radius: 999px;
        border: 1px solid var(--color-border);
        background: var(--color-panel);
        cursor: pointer;
        transition:
            background-color 130ms ease,
            border-color 130ms ease;
    }
    .cmd-appear-toggle.is-on {
        background: var(--color-accent);
        border-color: var(--color-accent);
    }
    .cmd-appear-toggle-knob {
        position: absolute;
        top: 50%;
        left: 2px;
        width: 16px;
        height: 16px;
        border-radius: 999px;
        background: #fff;
        transform: translateY(-50%);
        transition: left 130ms ease;
    }
    .cmd-appear-toggle.is-on .cmd-appear-toggle-knob {
        left: 20px;
    }
    /* ─── Palette Appearance Wave E (2026-05-27): style presets ───── */
    .cmd-appear-row-presets {
        flex-wrap: wrap;
    }
    .cmd-appear-presets {
        display: inline-flex;
        flex-wrap: wrap;
        gap: 6px;
        flex: 1;
        justify-content: flex-end;
    }
    .cmd-appear-preset {
        appearance: none;
        border: 1px solid var(--color-border);
        background: color-mix(in srgb, var(--color-accent) 8%, var(--color-panel));
        color: var(--color-text);
        font-size: 11.5px;
        font-weight: 500;
        padding: 4px 12px;
        border-radius: 999px;
        cursor: pointer;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out),
            transform var(--dur-micro) var(--ease-out);
    }
    .cmd-appear-preset:hover {
        background: color-mix(in srgb, var(--color-accent) 22%, var(--color-panel));
        border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
        transform: translateY(-1px);
    }
    .cmd-appear-preset:active {
        transform: translateY(0);
    }

    /* ─── Palette Appearance Wave B (2026-05-27): theme + density ───── */

    .cmd-appear-themes {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
        flex: 1;
        justify-content: flex-end;
    }
    .cmd-appear-theme {
        appearance: none;
        border: 1px solid var(--color-border);
        background: var(--color-panel);
        color: var(--color-text);
        font-size: 11.5px;
        padding: 3px 8px 3px 4px;
        border-radius: 999px;
        cursor: pointer;
        display: inline-flex;
        align-items: center;
        gap: 6px;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out);
    }
    .cmd-appear-theme:hover {
        background: var(--color-panel-2);
    }
    .cmd-appear-theme.is-on {
        border-color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    }
    /* Tiny 3-band swatch — bg / panel / accent — to make the picker
       readable at a glance without forcing a real-world preview. */
    .cmd-appear-theme-swatch {
        display: inline-block;
        width: 24px;
        height: 12px;
        border-radius: 4px;
        background: linear-gradient(
            to right,
            var(--s-bg) 0%,
            var(--s-bg) 33%,
            var(--s-panel) 33%,
            var(--s-panel) 66%,
            var(--s-accent) 66%,
            var(--s-accent) 100%
        );
        border: 1px solid color-mix(in srgb, var(--color-border) 50%, transparent);
        flex-shrink: 0;
    }
    .cmd-appear-theme-label {
        font-weight: 500;
    }

    .cmd-appear-segmented {
        display: inline-flex;
        align-items: center;
        gap: 2px;
        padding: 2px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 999px;
    }
    .cmd-appear-seg {
        appearance: none;
        border: none;
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 11.5px;
        font-weight: 500;
        padding: 4px 12px;
        border-radius: 999px;
        cursor: pointer;
        transition: color var(--dur-micro) var(--ease-out);
    }
    .cmd-appear-seg:hover {
        color: var(--color-text);
    }
    .cmd-appear-seg.is-on {
        background: var(--color-accent);
        color: var(--color-accent-contrast);
        cursor: default;
    }

    /* Wave I (2026-05-27): live demo dot for the Animations row. The
       pulse's animation-duration multiplies by --cmd-motion-mult (set
       by Wave H on .cmd-root) so the user can SEE the speed change as
       they pick Reduced / Default / Lively. Reduced is special-cased
       to `animation: none` so it's clearly "instant / no motion". */
    .cmd-appear-animations {
        display: inline-flex;
        align-items: center;
        gap: 10px;
    }
    .cmd-appear-pulse {
        width: 14px;
        height: 14px;
        border-radius: 999px;
        background: var(--color-accent);
        opacity: 0.55;
        animation: cmd-appear-pulse calc(900ms * var(--cmd-motion-mult, 1)) ease-in-out infinite;
        flex-shrink: 0;
    }
    /* Reduced motion → no pulse, just a static dot. The dur-micro = 0
       override Wave H sets on .cmd-root doubles as the "off" signal
       here. */
    .cmd-root[style*='--cmd-motion-mult: 0'] .cmd-appear-pulse {
        animation: none;
        opacity: 0.4;
    }
    @keyframes cmd-appear-pulse {
        0%, 100% {
            transform: scale(0.7);
            opacity: 0.45;
            box-shadow: 0 0 0 0 color-mix(in srgb, var(--color-accent) 0%, transparent);
        }
        50% {
            transform: scale(1.1);
            opacity: 0.95;
            box-shadow: 0 0 0 6px color-mix(in srgb, var(--color-accent) 0%, transparent);
        }
    }

    /* ─── Wave C (2026-05-27): hide-sections collapsible card ─────────
       <details>/<summary> gives free expand+collapse without a state
       variable in JS. */
    .cmd-appear-details {
        display: block;
        padding: 8px 10px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 10px;
        font-size: 12px;
    }
    .cmd-appear-summary {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        cursor: pointer;
        list-style: none;
        color: var(--color-text);
    }
    .cmd-appear-summary::-webkit-details-marker {
        display: none;
    }
    .cmd-appear-summary-count {
        font-size: 11.5px;
        color: var(--color-text-secondary);
        font-variant-numeric: tabular-nums;
    }
    .cmd-appear-hide-grid {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: 4px 12px;
        margin-top: 10px;
    }
    .cmd-appear-hide-row {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-size: 12px;
        color: var(--color-text);
        cursor: pointer;
        padding: 2px 4px;
        border-radius: 6px;
    }
    .cmd-appear-hide-row:hover {
        background: var(--color-panel-2);
    }
    .cmd-appear-hide-row input[type='checkbox'] {
        accent-color: var(--color-accent);
        cursor: pointer;
    }
    .cmd-appear-hide-label {
        flex: 1;
    }

    .cmd-appear-foot {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        margin-top: 2px;
        padding-top: 10px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
    }

    /* ─── Keyboard shortcuts (Ctrl+Alt+I) ─────────────────────────
       Rides on .cmd-appear for the dialog shell (position, surface,
       head/foot) and only adds what differs: a slightly wider column
       for two-key rows, and the scrolling list itself. */
    .cmd-keys {
        width: min(520px, calc(100% - 28px));
    }
    .cmd-keys-body {
        /* The dialog is a flex column with a fixed head/foot; this is the
           only part that scrolls, so a long list never pushes the close
           button off-screen. */
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        display: flex;
        flex-direction: column;
        gap: 14px;
        padding-right: 2px;
    }
    .cmd-keys-group {
        display: flex;
        flex-direction: column;
        gap: 4px;
    }
    .cmd-keys-gtitle {
        margin: 0 0 2px;
        font-size: 11px;
        font-weight: 600;
        letter-spacing: 0.06em;
        text-transform: uppercase;
        color: var(--color-text-tertiary, var(--color-muted));
    }
    .cmd-keys-row {
        display: flex;
        align-items: baseline;
        gap: 6px;
        padding: 3px 0;
        font-size: 12.5px;
        color: var(--color-text-secondary);
    }
    .cmd-keys-row kbd {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 20px;
        padding: 2px 6px;
        border: 1px solid var(--color-border);
        border-radius: 5px;
        background: var(--color-panel-2);
        color: var(--color-text);
        font-family: var(--font-mono, monospace);
        font-size: 11px;
        line-height: 1.5;
        font-variant-numeric: tabular-nums;
    }
    .cmd-keys-row span {
        /* Takes the remaining width and wraps; the kbd chips stay put. */
        margin-left: 4px;
        color: var(--color-text-secondary);
    }

    /* Footer "Shortcuts" hint is a real <button>, so strip the UA chrome
       and let it inherit .cmd-hint's layout exactly — it must sit flush
       with the static hints beside it. */
    .cmd-hint-btn {
        border: none;
        background: none;
        padding: 0;
        margin: 0;
        font: inherit;
        color: inherit;
        cursor: pointer;
    }
    .cmd-hint-btn:hover {
        color: var(--color-text);
    }
    .cmd-appear-reset {
        font-size: 12px;
        font-weight: 500;
        color: var(--color-text-secondary);
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 8px;
        padding: 5px 12px;
        cursor: pointer;
    }
    .cmd-appear-reset:hover {
        color: var(--color-text);
        border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
    }
    .cmd-appear-note {
        font-size: 10px;
        color: var(--color-muted);
    }
    .cmd-appear-note kbd {
        font-family: inherit;
        font-size: 10px;
        padding: 1px 6px;
        border-radius: 4px;
        border: 1px solid var(--color-border);
        background: var(--color-panel);
        color: var(--color-text-secondary);
    }

    :global(.cmd-preview-hitmark) {
        border-radius: 4px;
        padding: 0 2px;
        background: color-mix(in srgb, var(--color-accent) 22%, transparent);
        color: inherit;
        box-shadow: inset 0 -1px 0 color-mix(in srgb, var(--color-accent) 45%, transparent);
    }

    .cmd-word-preview .is-doc-hit {
        border-radius: 8px;
        background: color-mix(in srgb, var(--color-accent) 9%, transparent);
        outline: 1px solid color-mix(in srgb, var(--color-accent) 22%, transparent);
    }

    .cmd-sheet-grid tr.is-sheet-hit td,
    .cmd-sheet-grid tr.is-sheet-hit th {
        background: color-mix(in srgb, var(--color-accent) 11%, white);
    }

    .cmd-sr-only {
        position: absolute;
        width: 1px;
        height: 1px;
        padding: 0;
        margin: -1px;
        overflow: hidden;
        clip: rect(0, 0, 0, 0);
        white-space: nowrap;
        border: 0;
    }

    .cmd-panel :is(button, input, summary):focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: 2px;
    }
    .cmd-panel .cmd-row:focus-visible {
        outline-offset: -2px;
    }

    @media (prefers-reduced-motion: reduce) {
        .cmd-root *,
        .cmd-root *::before,
        .cmd-root *::after {
            animation-duration: 0.01ms !important;
            animation-iteration-count: 1 !important;
            transition-duration: 0.01ms !important;
            scroll-behavior: auto !important;
        }
    }
</style>
