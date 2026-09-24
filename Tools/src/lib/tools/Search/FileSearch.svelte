<script lang="ts">

    import {onMount} from 'svelte';
    import {convertFileSrc, invoke} from '@tauri-apps/api/core';
    import {
        Activity,
        AppWindow,
        ExternalLink,
        FileSearch as FileSearchIcon,
        FileText,
        Filter,
        Folder,
        FolderOpen,
        Keyboard,
        KeyRound,
        PanelRightOpen,
        RefreshCw,
        Search,
        Settings as SettingsIcon,
        Sparkles,
        Wrench,
        X,
        Zap,
    } from '@lucide/svelte';
    import {installedTools} from '$lib/stores/toolPacks';
    import {settings} from '$lib/stores/settings';
    import {
        type ContentSearchResultItem,
        type FileSearchResultItem,
        clearFileSearchResults,
        contentSearchResult,
        contentSearchSearching,
        FILE_SEARCH_DATE_OPTIONS,
        FILE_SEARCH_SIZE_OPTIONS,
        fileSearchDateFilter,
        fileSearchExtensionFilter,
        fileSearchLaunchResult,
        fileSearchLaunchSearching,
        fileSearchMode,
        fileSearchNaturalLanguage,
        fileSearchPathFilter,
        fileSearchQuery,
        fileSearchResult,
        fileSearchSearching,
        fileSearchSizeFilter,
        fileSearchStatus,
        formatBytes,
        formatDateTime,
        initFileSearchStore,
        openFileSearchResult,
        openLaunchTarget,
        refreshFileSearchStatus,
        refreshLaunchTargetCache,
        revealFileSearchResult,
        runContentSearch,
        runLaunchTargetSearch,
        runFileSearch,
    } from '$lib/stores/fileSearch';
    import {requestSettingsSection} from '$lib/stores/settingsTarget';
    import { escToClear } from '$lib/actions/escToClear';
    // Phase 1 kit primitives.
    import {
        ToolPage,
        ToolToolbar,
        ToolPanel,
        Button,
        Toggle,
        SideSheet,
        Kbd,
    } from '$lib/ui';

    type KeepItLocalMatch = {
        id: string;
        name: string;
        description: string;
        score: number;
    };

    type InspectableResult =
        | {kind: 'file'; item: FileSearchResultItem}
        | {kind: 'content'; item: ContentSearchResultItem};

    let {selected = $bindable('file-search')}: { selected: string } = $props();

    let autoSearchTimer: ReturnType<typeof setTimeout> | null = null;
    let keepitlocalMatches = $state<KeepItLocalMatch[]>([]);
    let loadingMoreFiles = $state(false);
    let loadTriggerArmed = $state(true);
    let filtersOpen = $state(false);
    let inspectorOpen = $state(false);
    let inspectedResult = $state<InspectableResult | null>(null);
    let entryIcons = $state<Record<string, string | null>>({});
    let previewLoadError = $state(false);
    let searchInput = $state<HTMLInputElement | null>(null);
    let resultsScrollEl = $state<HTMLDivElement | null>(null);

    // Row-level entrance gating (`isFreshSession` / `.is-cold`) is GONE
    // as of Phase 3.3.6. The entrance feel now comes from a single
    // section-level fade-in (`@keyframes fs-section-enter`, 180 ms),
    // which CSS plays once per section mount — no script-side timer
    // needed, no per-row stagger. The stable-header pattern from
    // Phase 3.3.5 keeps sections mounted across re-searches, so the
    // animation never replays inside a session.

    /** Pretty-print a list of `sensitive_kinds` labels into a human-readable
     * tooltip. Filters out the universal "sensitive" tag (we use it for the
     * presence check) and turns snake_case into Title Case. Preserved
     * verbatim from the old surface. */
    /** Escape HTML so user file content can't inject markup. The
     *  snippets come from backend over IPC and the backend may emit
     *  raw text — we render with `{@html}` so this escape is the
     *  safety net before any `<mark>` tags get inserted. */
    function escapeHtml(s: string): string {
        return s
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    }

    /** Escape regex metacharacters in a literal string so the keyword
     *  highlighter can build a safe alternation pattern. */
    function escapeRegex(s: string): string {
        return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    }

    /** Wrap matched keywords in `<mark>` tags so the user can spot the
     *  hits at a glance inside the snippet block.
     *
     *  - If the backend already wrapped matches (snippet contains
     *    `<mark>` tags), trust it and pass through unchanged.
     *  - Otherwise escape the snippet's HTML, then do a case-insensitive
     *    alternation match against the longest keywords first so
     *    multi-word terms ("annual report") win over their prefixes
     *    ("annual" / "report"). Empty / 1-char keywords are ignored
     *    (would highlight too much noise).
     *
     *  CSS for the highlight pill lives in `.fs-snippet :global(mark)`
     *  near the bottom of this file. */
    function highlightSnippet(snippet: string, keywords: string[] | undefined): string {
        if (!snippet) return '';
        if (/<mark[\s>]/i.test(snippet)) return snippet;
        const safe = escapeHtml(snippet);
        const tokens = (keywords ?? [])
            .map((k) => (k ?? '').trim())
            .filter((k) => k.length >= 2);
        if (tokens.length === 0) return safe;
        // Longest-first so "annual report" matches before "annual".
        tokens.sort((a, b) => b.length - a.length);
        const pattern = new RegExp(
            '(' + tokens.map(escapeRegex).join('|') + ')',
            'gi',
        );
        return safe.replace(pattern, '<mark>$1</mark>');
    }

    function formatSensitiveKinds(kinds: string[] | undefined): string {
        if (!kinds || kinds.length === 0) return '';
        const labels: Record<string, string> = {
            aws_access_key: 'AWS access key',
            github_token: 'GitHub token',
            slack_token: 'Slack token',
            stripe_secret_key: 'Stripe secret key',
            google_api_key: 'Google API key',
            twilio_account_sid: 'Twilio account SID',
            sendgrid_api_key: 'SendGrid API key',
            jwt_token: 'JWT token',
            private_key_pem: 'PEM private key',
            pgp_private_key: 'PGP private key',
            database_url_with_password: 'Database URL with password',
            ethereum_private_key: 'Ethereum private key',
            bitcoin_wif_private_key: 'Bitcoin private key',
            us_ssn: 'US SSN',
            credit_card: 'Credit card number',
        };
        return kinds
            .filter((k) => k !== 'sensitive')
            .map((k) => labels[k] ?? k.replace(/_/g, ' '))
            .join(', ');
    }

    let hasActiveFilters = $derived(
        Boolean(
            $fileSearchExtensionFilter.trim() ||
            $fileSearchPathFilter.trim() ||
            $fileSearchSizeFilter ||
            $fileSearchDateFilter,
        ),
    );
    const FILE_PAGE_SIZE = 50;
    const SEARCH_STOPWORDS = new Set([
        'a', 'an', 'the', 'of', 'and', 'or', 'to', 'from', 'in', 'on',
        'at', 'for', 'with', 'by', 'file', 'files', 'folder', 'folders',
        'search', 'find', 'show', 'my', 'me', 'just', 'only', 'tool',
        'tools', 'app', 'apps',
    ]);

    let keepitlocalTargets = $derived(
        $installedTools
            .filter((tool) => tool.available)
            .map((tool) => ({
                id: tool.id,
                name: tool.name,
                description: tool.description,
                tokens: `${tool.name} ${tool.description} ${tool.category}`.toLowerCase(),
            })),
    );

    let inspectedFile = $derived<FileSearchResultItem | null>(
        inspectedResult?.kind === 'file' ? inspectedResult.item : null,
    );
    let inspectedContent = $derived<ContentSearchResultItem | null>(
        inspectedResult?.kind === 'content' ? inspectedResult.item : null,
    );

    let hasAnyResults = $derived.by(() => {
        if ($fileSearchMode === 'content') {
            return ($contentSearchResult?.results.length ?? 0) > 0;
        }
        return (
            keepitlocalMatches.length > 0 ||
            ($fileSearchLaunchResult?.results.length ?? 0) > 0 ||
            ($fileSearchResult?.results.length ?? 0) > 0
        );
    });
    let hasMoreFileResults = $derived.by(() => {
        if ($fileSearchMode === 'content') {
            return Boolean(
                $contentSearchResult &&
                $contentSearchResult.results.length < $contentSearchResult.totalHits,
            );
        }
        return Boolean(
            $fileSearchResult &&
            $fileSearchResult.results.length < $fileSearchResult.totalHits,
        );
    });
    let installedAppsCount = $derived($fileSearchLaunchResult?.returned ?? 0);
    let keepitlocalToolsCount = $derived(keepitlocalMatches.length);
    let fileMatchesVisible = $derived(
        $fileSearchMode === 'content'
            ? ($contentSearchResult?.results.length ?? 0)
            : ($fileSearchResult?.results.length ?? 0),
    );
    let fileMatchesTotal = $derived(
        $fileSearchMode === 'content'
            ? ($contentSearchResult?.totalHits ?? 0)
            : ($fileSearchResult?.totalHits ?? 0),
    );
    let fileSearchTookMs = $derived(
        $fileSearchMode === 'content'
            ? ($contentSearchResult?.tookMs ?? null)
            : ($fileSearchResult?.tookMs ?? null),
    );
    let activeSearching = $derived(
        $fileSearchMode === 'content' ? $contentSearchSearching : $fileSearchSearching,
    );

    /** Pretty parts of the user's CURRENT search-overlay shortcut, for
     *  the ToolPage footer hint. Re-derives from $settings whenever
     *  they remap the hotkey so the hint stays in sync without reload. */
    let overlayShortcutParts = $derived(
        ($settings.commandOverlayShortcut || 'CommandOrControl+Alt+K')
            .split('+')
            .map((part) => part.trim())
            .filter((part) => part.length > 0)
            .map((part) =>
                part === 'CommandOrControl' || part === 'Control'
                    ? 'Ctrl'
                    : part === 'Meta' || part === 'Super'
                        ? 'Win'
                        : part,
            ),
    );
    let overlayShortcutLabel = $derived(overlayShortcutParts.join('+'));

    /** True when ANY search path is in flight (content/file/launch).
     *  Used to dim the meta line when results are stale (search running
     *  but previous results still showing). The spinner in the input
     *  is the primary "we're loading" cue; this just softens the meta. */
    let isSearchingAnything = $derived(activeSearching || $fileSearchLaunchSearching);

    let resultSummary = $derived.by(() => {
        // No "Searching…" intermediate. On fast queries the search
        // completes in 30–100 ms, so a "Searching…" → "X files" flip
        // reads as a flash. Instead: keep the previous count visible
        // (dimmed via `.is-stale`); when results actually update, the
        // numbers change in place without an extra state.
        const parts: string[] = [];
        if ($fileSearchMode === 'content') {
            if (fileMatchesTotal > 0) parts.push(`${fileMatchesTotal} content matches`);
        } else {
            if (installedAppsCount > 0) parts.push(`${installedAppsCount} apps`);
            if (keepitlocalToolsCount > 0) parts.push(`${keepitlocalToolsCount} tools`);
            if (fileMatchesTotal > 0) parts.push(`${fileMatchesTotal} files`);
        }
        return parts.join(' · ');
    });

    onMount(() => {
        void initFileSearchStore();
        // Re-fetch index status on EVERY mount. initFileSearchStore is guarded
        // (runs once per session), so without this the status goes stale across
        // remounts — and now that Search lives in a Command tab that mounts/
        // unmounts, a stale `indexedFiles: 0` made the page wrongly claim the
        // index "is not ready" even while content/file search returns results.
        void refreshFileSearchStatus();

        return () => {
            closeInspector();
            if (autoSearchTimer) {
                clearTimeout(autoSearchTimer);
                autoSearchTimer = null;
            }
            if (filterChangeTimer) {
                clearTimeout(filterChangeTimer);
                filterChangeTimer = null;
            }
        };
    });

    /* ─── Auto-search: query keystrokes ─────────────────────────
       Gated by natural-language mode — when natural is OFF, the user
       is composing an advanced expression (path:foo AND bar) and the
       page waits for Enter. Mirrors the old behavior. */
    $effect(() => {
        const query = $fileSearchQuery;
        const natural = $fileSearchNaturalLanguage;
        const mode = $fileSearchMode;

        if (autoSearchTimer) {
            clearTimeout(autoSearchTimer);
            autoSearchTimer = null;
        }

        if (!natural) return;

        if (!query.trim()) {
            keepitlocalMatches = [];
            loadingMoreFiles = false;
            loadTriggerArmed = true;
            if (mode === 'content') {
                void runContentSearch({silentEmpty: true, query});
            } else {
                void runFileSearch({silentEmpty: true, query});
                void runLaunchTargetSearch({silentEmpty: true, query});
            }
            return;
        }

        autoSearchTimer = setTimeout(() => {
            void runUnifiedSearch(true);
        }, autoSearchDelayMs());
    });

    /* ─── Auto-search: filter changes ─────────────────────────────
       Filter edits ALWAYS trigger a debounced re-search when there's
       a query — regardless of natural-language mode. The user opened
       the side sheet, changed a filter, and naturally expects results
       to update without a separate Enter press. The natural-language
       gate above only governs how QUERY text is interpreted; filters
       are a different signal. */
    let filterChangeTimer: ReturnType<typeof setTimeout> | null = null;
    $effect(() => {
        // Reactively track all 4 filter stores. Read them so Svelte
        // wires the deps; the values themselves aren't used here.
        $fileSearchExtensionFilter;
        $fileSearchPathFilter;
        $fileSearchSizeFilter;
        $fileSearchDateFilter;
        $fileSearchMode;

        if (filterChangeTimer) {
            clearTimeout(filterChangeTimer);
            filterChangeTimer = null;
        }

        // Only re-search when there's actually a query — empty-query
        // changes are handled by the keystroke effect above.
        if (!$fileSearchQuery.trim()) return;

        filterChangeTimer = setTimeout(() => {
            void runUnifiedSearch(true);
        }, 220);
    });

    function autoSearchDelayMs() {
        const indexedFiles = $fileSearchStatus?.indexedFiles ?? 0;
        if ($fileSearchMode === 'content') return indexedFiles >= 250_000 ? 600 : 420;
        return indexedFiles >= 250_000 ? 420 : 320;
    }

    function normalizeSearchText(input: string) {
        return input.toLowerCase().replace(/[^a-z0-9]+/g, ' ').trim();
    }

    function isSearchStopword(token: string) {
        return SEARCH_STOPWORDS.has(token);
    }

    function levenshtein(a: string, b: string): number {
        if (a === b) return 0;
        if (a.length === 0) return b.length;
        if (b.length === 0) return a.length;
        const prev = Array.from({length: b.length + 1}, (_, i) => i);
        const curr = new Array(b.length + 1).fill(0);
        for (let i = 1; i <= a.length; i++) {
            curr[0] = i;
            for (let j = 1; j <= b.length; j++) {
                curr[j] =
                    a[i - 1] === b[j - 1]
                        ? prev[j - 1]
                        : 1 + Math.min(prev[j - 1], prev[j], curr[j - 1]);
            }
            prev.splice(0, prev.length, ...curr);
        }
        return curr[b.length];
    }

    function fuzzyTokenMatch(toolTokens: string, queryToken: string): boolean {
        if (queryToken.length < 4) return false;
        for (const word of toolTokens.split(/\s+/)) {
            if (word.length >= 3 && levenshtein(queryToken, word) <= 1) return true;
        }
        return false;
    }

    function computeKeepItLocalMatches(queryText: string): KeepItLocalMatch[] {
        const query = normalizeSearchText(queryText);
        if (query.length < 2) return [];

        const tokens = query
            .split(/\s+/)
            .filter((token) => token.length >= 2 && !isSearchStopword(token));
        if (tokens.length === 0) return [];

        const ranked = keepitlocalTargets
            .map((tool) => {
                let score = 0;
                if (tool.name.toLowerCase() === query) score += 400;
                else if (tool.name.toLowerCase().startsWith(query)) score += 250;
                else if (tool.tokens.includes(query)) score += 120;
                for (const token of tokens) {
                    if (tool.tokens.includes(token)) score += 30;
                    else if (fuzzyTokenMatch(tool.tokens, token)) score += 15;
                }
                return {
                    id: tool.id,
                    name: tool.name,
                    description: tool.description,
                    score,
                };
            })
            .filter((tool) => tool.score >= 60)
            .sort((a, b) => b.score - a.score || a.name.localeCompare(b.name));

        return ranked.slice(0, 12);
    }

    function isImageFile(extension: string): boolean {
        return ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'avif', 'svg'].includes(
            extension.replace(/^\./, '').toLowerCase(),
        );
    }

    function isAudioFile(extension: string): boolean {
        return ['mp3', 'wav', 'ogg', 'oga', 'm4a', 'aac', 'flac', 'opus', 'weba'].includes(
            extension.replace(/^\./, '').toLowerCase(),
        );
    }

    function isVideoFile(extension: string): boolean {
        return [
            'mp4', 'm4v', 'webm', 'ogv', 'mov', 'mkv', 'avi', 'wmv', 'flv',
            'mpeg', 'mpg', 'm2ts', 'ts', 'vob', 'asf', '3gp', '3g2',
        ].includes(extension.replace(/^\./, '').toLowerCase());
    }

    function isPdfFile(extension: string): boolean {
        return extension.replace(/^\./, '').toLowerCase() === 'pdf';
    }

    const MAX_IMAGE_PREVIEW_BYTES = 50 * 1024 * 1024;
    const MAX_PDF_PREVIEW_BYTES = 100 * 1024 * 1024;

    async function fetchEntryIcon(path: string, kind: 'app' | 'folder' | 'file') {
        if (!path || path in entryIcons) return;
        entryIcons = {...entryIcons, [path]: null};
        try {
            const iconPath = await invoke<string | null>('ensure_launcher_icon', {path, kind});
            if (iconPath) entryIcons = {...entryIcons, [path]: convertFileSrc(iconPath)};
        } catch {
            // Keep the Lucide fallback when the Windows shell has no icon.
        }
    }

    $effect(() => {
        for (const item of $fileSearchLaunchResult?.results ?? []) {
            void fetchEntryIcon(item.path, 'app');
        }
        for (const item of $fileSearchResult?.results ?? []) {
            void fetchEntryIcon(item.path, item.entryType === 'folder' ? 'folder' : 'file');
        }
        for (const item of $contentSearchResult?.results ?? []) {
            void fetchEntryIcon(item.path, 'file');
        }
    });

    function openInspector(result: InspectableResult) {
        inspectedResult = result;
        previewLoadError = false;
        inspectorOpen = true;
    }

    function closeInspector() {
        document.querySelectorAll<HTMLMediaElement>('.fs-preview-stage audio, .fs-preview-stage video')
            .forEach((element) => {
                element.pause();
                element.currentTime = 0;
            });
        inspectorOpen = false;
        inspectedResult = null;
    }

    async function runUnifiedSearch(silentEmpty = false) {
        const query = $fileSearchQuery.trim();
        loadingMoreFiles = false;
        loadTriggerArmed = true;

        if ($fileSearchMode === 'content') {
            keepitlocalMatches = [];
            await runContentSearch({
                silentEmpty,
                query,
                offset: 0,
                limit: FILE_PAGE_SIZE,
                append: false,
            });
            return;
        }

        keepitlocalMatches = computeKeepItLocalMatches(query);
        await Promise.all([
            runFileSearch({
                silentEmpty,
                query,
                offset: 0,
                limit: FILE_PAGE_SIZE,
                append: false,
            }),
            runLaunchTargetSearch({silentEmpty, query}),
        ]);
    }

    async function loadMoreFileResults() {
        if (!hasMoreFileResults || loadingMoreFiles || activeSearching) return;
        const query = $fileSearchQuery.trim();
        if (!query) return;
        const currentLoaded =
            $fileSearchMode === 'content'
                ? ($contentSearchResult?.results.length ?? 0)
                : ($fileSearchResult?.results.length ?? 0);
        if (currentLoaded === 0) return;

        loadTriggerArmed = false;
        loadingMoreFiles = true;
        try {
            const runner = $fileSearchMode === 'content' ? runContentSearch : runFileSearch;
            await runner({
                silentEmpty: true,
                query,
                offset: currentLoaded,
                limit: FILE_PAGE_SIZE,
                append: true,
            });
        } finally {
            loadingMoreFiles = false;
        }
    }

    async function onResultsScroll(event: Event) {
        if (!hasMoreFileResults || loadingMoreFiles || activeSearching) return;
        const container = event.currentTarget as HTMLDivElement | null;
        if (!container) return;
        const nearBottom =
            container.scrollTop + container.clientHeight >= container.scrollHeight - 120;
        if (!nearBottom) {
            loadTriggerArmed = true;
            return;
        }
        if (!loadTriggerArmed) return;
        await loadMoreFileResults();
    }

    function onQueryKeydown(event: KeyboardEvent) {
        if (event.key === 'Enter') {
            event.preventDefault();
            void runUnifiedSearch(false);
        }
    }

    function openKeepItLocalTool(toolId: string) {
        selected = toolId;
    }

    function clearAllResults() {
        keepitlocalMatches = [];
        loadingMoreFiles = false;
        loadTriggerArmed = true;
        closeInspector();
        fileSearchQuery.set('');
        fileSearchExtensionFilter.set('');
        fileSearchPathFilter.set('');
        fileSearchSizeFilter.set('');
        fileSearchDateFilter.set('');
        clearFileSearchResults();
    }

    function clearFiltersOnly() {
        fileSearchExtensionFilter.set('');
        fileSearchPathFilter.set('');
        fileSearchSizeFilter.set('');
        fileSearchDateFilter.set('');
    }

    function setSearchMode(mode: 'files' | 'content') {
        if ($fileSearchMode === mode) return;
        fileSearchMode.set(mode);
        keepitlocalMatches = [];
        loadingMoreFiles = false;
        loadTriggerArmed = true;
        closeInspector();
        clearFileSearchResults();
        if ($fileSearchQuery.trim()) {
            void runUnifiedSearch(true);
        }
    }

    function openIndexSettings() {
        requestSettingsSection('fileIndex');
        selected = 'settings';
    }

    function openContentIndexSettings() {
        requestSettingsSection('contentIndex');
        selected = 'settings';
    }

    /** "Refresh apps" handler. `refreshLaunchTargetCache` clears the
     *  store's `fileSearchLaunchResult` and rebuilds the on-disk app
     *  cache — but does NOT re-run the launch search. Without this
     *  wrapper the user clicks Refresh and sees the apps section
     *  disappear with no replacement. We re-run the launch search
     *  immediately after so fresh results appear in-place. */
    async function handleRefreshApps() {
        await refreshLaunchTargetCache();
        const query = $fileSearchQuery.trim();
        if (query) {
            await runLaunchTargetSearch({silentEmpty: true, query});
        }
    }

    /* ─── Index-readiness derivations (drives empty-state branching) ──
       The Files mode and Content mode look at DIFFERENT indices:
         - files   → filenameIndexedFiles (the big number — every file's
                     name). Fall back to indexedFiles if the filename
                     index field isn't populated yet.
         - content → indexedFiles (the smaller, content-aware count).
       Distinct "building right now" flag too — a busy indexer should
       surface as a different message than a cold index. */
    let indexReady = $derived.by(() => {
        const s = $fileSearchStatus;
        if (!s) return false;
        if ($fileSearchMode === 'content') return s.indexedFiles > 0;
        return s.filenameIndexedFiles > 0 || s.indexedFiles > 0;
    });
    let indexBuilding = $derived.by(() => {
        const s = $fileSearchStatus;
        if (!s) return false;
        if ($fileSearchMode === 'content') return Boolean(s.indexing);
        return Boolean(s.filenameIndexing) || Boolean(s.indexing);
    });

    /** Empty-state config — driven purely by whether the input is
     *  empty and the underlying index state. Crucially, NOT gated on
     *  `activeSearching`: that was the source of the intro ↔ no-matches
     *  flicker (the two views fought for the {:else} fallback slot
     *  during the brief search window). User's rule, applied:
     *
     *    - Input empty            → intro (the welcoming examples)
     *    - Input has text:
     *        - index not built    → 'index-not-ready' (with CTA)
     *        - index building     → 'indexing'
     *        - otherwise          → 'no-matches'
     *
     *  Once the input has any text, intro is gone for good — no race
     *  with the search state, no flash back to it. Title in the
     *  no-matches view reflects the current query and updates live as
     *  the user types; once the search settles, it's accurate. */
    type EmptyStateConfig = {
        kind: 'intro' | 'no-matches' | 'index-not-ready' | 'indexing';
        icon: any;
        iconWarn: boolean;
        title: string;
        description: string;
        showExamples: boolean;
        actionLabel: string | null;
        actionIcon: any;
        actionVariant: 'primary' | 'secondary' | 'ghost';
        onAction: (() => void) | null;
    };
    let emptyStateConfig = $derived.by<EmptyStateConfig>(() => {
        const queryText = $fileSearchQuery.trim();
        if (queryText.length === 0) {
            // Input empty → intro / examples. Mode-aware copy.
            return {
                kind: 'intro',
                icon: Sparkles,
                iconWarn: false,
                title:
                    $fileSearchMode === 'content'
                        ? 'Search inside your files'
                        : 'Find files, apps, and tools instantly',
                description:
                    $fileSearchMode === 'content'
                        ? 'Type a phrase or keyword to search inside indexed document text. Try a phrase in quotes for exact matches.'
                        : 'Type a few characters and KeepItLocal ranks files, folders, installed apps, and KeepItLocal tools — natural language, prefixes, and typos all work.',
                showExamples: true,
                actionLabel: 'Start typing above',
                actionIcon: Keyboard,
                actionVariant: 'secondary',
                onAction: () => searchInput?.focus(),
            };
        }
        // From here on, the input has text — intro is OFF the table.
        // Gate on `!activeSearching`: a search in flight must never flash
        // "index not ready" (the status can be momentarily stale). Only claim
        // the index isn't ready once a search has settled and still shows no
        // signs of an index — by then `refreshFileSearchStatus` (mount + per
        // op) has given us an accurate count.
        if (!indexReady && !indexBuilding && !activeSearching) {
            return {
                kind: 'index-not-ready',
                icon: Activity,
                iconWarn: true,
                title: 'Search index is not ready yet',
                description:
                    'Build the index first — KeepItLocal walks your chosen folders and catalogues files locally so search stays fast. Nothing leaves your machine.',
                showExamples: false,
                actionLabel: 'Open Indexing settings',
                actionIcon: SettingsIcon,
                actionVariant: 'primary',
                onAction: openIndexSettings,
            };
        }
        if (indexBuilding) {
            return {
                kind: 'indexing',
                icon: Activity,
                iconWarn: false,
                title: 'Indexing in progress…',
                description:
                    'KeepItLocal is building the search index right now. Results will improve as more files are catalogued — try again in a moment.',
                showExamples: false,
                actionLabel: null,
                actionIcon: null,
                actionVariant: 'secondary',
                onAction: null,
            };
        }
        // Index healthy, input has text, nothing matched: "no matches".
        // Title reflects the current query — it updates live as the
        // user types, so the panel never reads as a stale snapshot.
        return {
            kind: 'no-matches',
            icon: Search,
            iconWarn: false,
            title: `No matches for "${queryText}"`,
            description:
                'Try fewer words, a different spelling, or a prefix — KeepItLocal matches fuzzily, but it still needs at least one overlap to rank a result.',
            showExamples: false,
            actionLabel: hasActiveFilters ? 'Clear filters' : null,
            actionIcon: hasActiveFilters ? Filter : null,
            actionVariant: 'secondary',
            onAction: hasActiveFilters ? clearFiltersOnly : null,
        };
    });
</script>


<ToolPage
        icon={FileSearchIcon}
        title={$fileSearchMode === 'content' ? 'Content search' : 'File search'}
        description=""
        width="wide"

>

    {#snippet actions()}
        {#if $fileSearchMode === 'content'}
            <Button
                    variant="ghost"
                    icon={SettingsIcon}
                    onclick={openContentIndexSettings}
                    title="Manage Content index"
            >
                Manage Content index
            </Button>
        {:else}
            <Button
                    variant="ghost"
                    icon={SettingsIcon}
                    onclick={openIndexSettings}
                    title="Manage File index"
            >
                Manage File index
            </Button>
        {/if}
    {/snippet}

    <!-- ─── Search bar + mode toggle + filters ────────────────────
         A single self-contained row: big search input on the left,
         mode toggle in the middle, filter button on the right. -->
    <div class="fs-bar">
        <div class="fs-input-wrap">
            <Search class="fs-input-icon"/>
            <input
                    bind:this={searchInput}
                    bind:value={$fileSearchQuery}
                    use:escToClear={() => fileSearchQuery.set('')}
                    onkeydown={onQueryKeydown}
                    placeholder={$fileSearchMode === 'content'
                    ? 'Search inside files…  "quarterly earnings"  or  taxes from 2023'
                    : 'Search files, folders, apps, and KeepItLocal tools…'}
                    class="fs-input"
                    autocomplete="off"
                    spellcheck="false"
            />
            {#if $fileSearchQuery}
                <button
                        type="button"
                        class="fs-input-clear"
                        onclick={clearAllResults}
                        title="Clear search"
                        aria-label="Clear search"
                >
                    <X class="fs-input-clear-ico"/>
                </button>
            {/if}
            {#if activeSearching}
                <span class="fs-input-busy">
                    <span class="fs-spinner" aria-hidden="true"></span>
                </span>
            {/if}
        </div>

        <!-- Mode segmented control — macOS pill style. -->
        <div class="fs-mode" role="group" aria-label="Search mode">
            <button
                    type="button"
                    class="fs-mode-btn"
                    class:is-on={$fileSearchMode === 'files'}
                    onclick={() => setSearchMode('files')}
            >
                <FileSearchIcon class="fs-mode-ico"/>
                Files
            </button>
            <button
                    type="button"
                    class="fs-mode-btn"
                    class:is-on={$fileSearchMode === 'content'}
                    onclick={() => setSearchMode('content')}
            >
                <FileText class="fs-mode-ico"/>
                Inside
            </button>
        </div>

        <!-- Filters button — opens the SideSheet. A dot indicator
             appears when any filter is active. -->
        <button
                type="button"
                class="fs-filter-btn"
                class:is-active={hasActiveFilters || filtersOpen}
                onclick={() => (filtersOpen = true)}
                title="Filters & search options"
                aria-label="Filters & search options"
        >
            <Filter class="fs-filter-ico"/>
            <span>Filters</span>
            {#if hasActiveFilters}
                <span class="fs-filter-dot" aria-hidden="true"></span>
            {/if}
        </button>
    </div>

    <!-- ─── Hint strip ────────────────────────────────────────────
         Compact, contextual help line under the search bar. Changes
         based on mode + natural-language state. -->
    <div class="fs-hint">
        {#if $fileSearchMode === 'content'}
            <Zap class="fs-hint-ico"/>
            <span>
                Searches inside indexed file text — phrases, AND/OR/NOT, prefix and
                fuzzy passes.
            </span>
        {:else if $fileSearchNaturalLanguage}
            <Sparkles class="fs-hint-ico"/>
            <span>
                Natural language is on — try
                <code class="fs-hint-code">large pdf from 2024</code>
            </span>
        {:else}
            <Keyboard class="fs-hint-ico"/>
            <span>
                Advanced mode — try
                <code class="fs-hint-code">path:reports AND budget</code>
            </span>
        {/if}
    </div>

    <!-- ─── Results body ──────────────────────────────────────────
         Scrolls within a single ToolPanel so the load-more pagination
         remains intact (scroll-event-driven). Each section is a
         visually separated block inside the panel. -->
    {#if hasAnyResults}
        <!-- Results meta — count summary + time + Refresh apps (files
             mode). Only rendered when there ARE results to summarize;
             during the initial search (no previous results) the meta
             is hidden entirely and the input's spinner carries the
             loading signal. When a re-search runs over existing
             results, the meta stays visible but dims via .is-stale
             so the user knows the numbers are about to update. -->
        {#if hasAnyResults}
            <div class="fs-meta" class:is-stale={isSearchingAnything}>
                <div class="fs-meta-left">
                    <span class="fs-meta-summary">{resultSummary}</span>
                    {#if fileSearchTookMs !== null && !isSearchingAnything}
                        <span class="fs-meta-time">
                            <Zap class="fs-meta-time-ico"/>
                            {fileSearchTookMs} ms
                        </span>
                    {/if}
                </div>
                <div class="fs-meta-right">
                    {#if $fileSearchMode === 'files'}
                        <Button
                                size="sm"
                                variant="ghost"
                                icon={RefreshCw}
                                onclick={() => void handleRefreshApps()}
                        >
                            Refresh apps
                        </Button>
                    {/if}
                </div>
            </div>
        {/if}

        <ToolPanel padding="sm" scroll>
            <div
                    bind:this={resultsScrollEl}
                    class="fs-results"
                    onscroll={onResultsScroll}
            >
                <!-- Installed Apps — only mounts when there are actual
                     apps to show. No speculative "skeleton-while-
                     searching" branch; the section never appears for a
                     search that will yield zero apps. The input's
                     spinner already signals "we're loading"; this
                     section's job is solely to display results. -->
                {#if $fileSearchMode === 'files' && $fileSearchLaunchResult?.results.length}
                    <div class="fs-section">
                        <div class="fs-section-head">
                            <AppWindow class="fs-section-ico"/>
                            <span>Installed Apps</span>
                            <span class="fs-section-count">
                                {$fileSearchLaunchResult.results.length}
                            </span>
                        </div>
                        {#each $fileSearchLaunchResult.results as item (item.id)}
                            <div class="fs-row">
                                <div class="fs-row-tile fs-tile-app" aria-hidden="true">
                                    {#if entryIcons[item.path]}
                                        <img
                                                class="fs-row-tile-img"
                                                src={entryIcons[item.path]}
                                                alt=""
                                                draggable="false"
                                        />
                                    {:else}
                                        <AppWindow class="fs-row-tile-ico"/>
                                    {/if}
                                </div>
                                <div class="fs-row-main">
                                    <div class="fs-row-title">{item.name}</div>
                                    <div class="fs-row-path">{item.path}</div>
                                </div>
                                <div class="fs-row-actions">
                                    <Button
                                            size="sm"
                                            variant="secondary"
                                            icon={ExternalLink}
                                            onclick={() => openLaunchTarget(item.path)}
                                    >
                                        Launch
                                    </Button>
                                </div>
                            </div>
                        {/each}
                    </div>
                {/if}

                <!-- KeepItLocal Tools (no skeleton — only shows when there
                     are matches; the match list is computed synchronously). -->
                {#if $fileSearchMode === 'files' && keepitlocalMatches.length}
                    <div class="fs-section">
                        <div class="fs-section-head">
                            <Wrench class="fs-section-ico"/>
                            <span>KeepItLocal Tools</span>
                            <span class="fs-section-count">
                                {keepitlocalMatches.length}
                            </span>
                        </div>
                        {#each keepitlocalMatches as item (item.id)}
                            <div class="fs-row">
                                <div class="fs-row-tile fs-tile-tool" aria-hidden="true">
                                    <Wrench class="fs-row-tile-ico"/>
                                </div>
                                <div class="fs-row-main">
                                    <div class="fs-row-title">{item.name}</div>
                                    <div class="fs-row-path">{item.description}</div>
                                </div>
                                <div class="fs-row-actions">
                                    <Button
                                            size="sm"
                                            variant="ghost"
                                            icon={ExternalLink}
                                            onclick={() => openKeepItLocalTool(item.id)}
                                    >
                                        Open
                                    </Button>
                                </div>
                            </div>
                        {/each}
                    </div>
                {/if}

                <!-- Content Matches — same pattern as Installed Apps:
                     only mounts when results actually exist. The input's
                     spinner carries the "loading" cue; the empty state
                     handles the "0 matches" case. -->
                {#if $fileSearchMode === 'content' && $contentSearchResult?.results.length}
                    <div class="fs-section">
                        <div class="fs-section-head">
                            <FileText class="fs-section-ico"/>
                            <span>Content Matches</span>
                            <span class="fs-section-count">
                                {$contentSearchResult.results.length} of
                                {$contentSearchResult.totalHits}
                            </span>
                            {#if $contentSearchResult.semanticActive}
                                <span
                                        class="fs-semantic-active"
                                        title="Semantic search is on: results also include files that match the meaning of your query, not just the exact words."
                                >
                                    <Sparkles class="fs-semantic-ico"/>
                                    Semantic on
                                </span>
                            {/if}
                        </div>
                        {#each $contentSearchResult.results as item (item.path)}
                            <div class="fs-row fs-row-rich">
                                <div class="fs-row-tile fs-tile-file" aria-hidden="true">
                                    {#if entryIcons[item.path]}
                                        <img
                                                class="fs-row-tile-img"
                                                src={entryIcons[item.path]}
                                                alt=""
                                                draggable="false"
                                        />
                                    {:else}
                                        <FileText class="fs-row-tile-ico"/>
                                    {/if}
                                </div>
                                <div class="fs-row-main">
                                    <div
                                            class="fs-row-title"
                                            title={item.fileName || item.path.split(/[\\/]/).pop()}
                                    >
                                        <span>
                                            {item.fileName || item.path.split(/[\\/]/).pop()}
                                        </span>
                                        {#if item.sensitiveKinds && item.sensitiveKinds.includes('sensitive')}
                                            <span
                                                    class="fs-sensitive"
                                                    title="Sensitive content detected: {formatSensitiveKinds(item.sensitiveKinds)}"
                                            >
                                                <KeyRound class="fs-sensitive-ico"/>
                                                Sensitive
                                            </span>
                                        {/if}
                                    </div>
                                    <div class="fs-row-path" title={item.path}>{item.path}</div>
                                    <div class="fs-row-meta">
                                        <span>{item.extension || '—'}</span>
                                        <span class="fs-meta-dot">·</span>
                                        <span>{formatBytes(item.size)}</span>
                                        <span class="fs-meta-dot">·</span>
                                        <span>{formatDateTime(item.modifiedMs)}</span>
                                        {#if item.matchCount > 1}
                                            <span class="fs-meta-dot">·</span>
                                            <span class="fs-row-meta-accent">
                                                {item.matchCount} matches
                                            </span>
                                        {/if}
                                        {#if item.semanticOnly}
                                            <span class="fs-meta-dot">·</span>
                                            <span
                                                    class="fs-semantic-badge"
                                                    title="Found by meaning — this file matches what you searched for without containing the exact words ({Math.round(item.semanticScore * 100)}% similar)."
                                            >
                                                <Sparkles class="fs-semantic-ico"/>
                                                Meaning match
                                            </span>
                                        {:else if item.semanticScore > 0.4}
                                            <span class="fs-meta-dot">·</span>
                                            <span
                                                    class="fs-semantic-badge fs-semantic-soft"
                                                    title="Also a strong meaning match ({Math.round(item.semanticScore * 100)}% similar)."
                                            >
                                                <Sparkles class="fs-semantic-ico"/>
                                                {Math.round(item.semanticScore * 100)}%
                                            </span>
                                        {/if}
                                    </div>
                                    {#if item.snippet}
                                        <div class="fs-snippet">
                                            {@html highlightSnippet(item.snippet, item.matchedKeywords)}
                                        </div>
                                    {/if}
                                    {#if item.snippets && item.snippets.length > 0}
                                        {#each item.snippets as extra}
                                            <div class="fs-snippet fs-snippet-extra">
                                                {@html highlightSnippet(extra, item.matchedKeywords)}
                                            </div>
                                        {/each}
                                    {/if}
                                </div>
                                <div class="fs-row-actions">
                                    <Button
                                            size="sm"
                                            variant="ghost"
                                            iconOnly
                                            icon={PanelRightOpen}
                                            title="Preview"
                                            aria-label="Preview"
                                            onclick={() => openInspector({kind: 'content', item})}
                                    />
                                    <Button
                                            size="sm"
                                            variant="ghost"
                                            iconOnly
                                            icon={ExternalLink}
                                            title="Open"
                                            aria-label="Open"
                                            onclick={() => openFileSearchResult(item.path)}
                                    />
                                    <Button
                                            size="sm"
                                            variant="ghost"
                                            iconOnly
                                            icon={FolderOpen}
                                            title="Reveal"
                                            aria-label="Reveal"
                                            onclick={() => revealFileSearchResult(item.path)}
                                    />
                                </div>
                            </div>
                        {/each}
                        {#if loadingMoreFiles}
                            <div class="fs-load-state">
                                Loading more content results…
                            </div>
                        {:else if hasMoreFileResults}
                            <button
                                    type="button"
                                    onclick={() => void loadMoreFileResults()}
                                    class="fs-load-btn"
                            >
                                Load more content results
                            </button>
                        {/if}
                    </div>
                {/if}

                <!-- Files & Folders — only mounts when results actually
                     exist. Input spinner = loading cue; empty state =
                     "0 matches" handling. -->
                {#if $fileSearchMode === 'files' && $fileSearchResult?.results.length}
                    <div class="fs-section">
                        <div class="fs-section-head">
                            <FileSearchIcon class="fs-section-ico"/>
                            <span>Files & Folders</span>
                            <span class="fs-section-count">
                                {$fileSearchResult.results.length} of
                                {$fileSearchResult.totalHits}
                            </span>
                        </div>
                        {#each $fileSearchResult.results as item (item.path)}
                            <div class="fs-row fs-row-rich">
                                <div
                                        class="fs-row-tile {item.entryType === 'folder'
                                        ? 'fs-tile-folder'
                                        : 'fs-tile-file'}"
                                        aria-hidden="true"
                                >
                                    {#if entryIcons[item.path]}
                                        <img
                                                class="fs-row-tile-img"
                                                src={entryIcons[item.path]}
                                                alt=""
                                                draggable="false"
                                        />
                                    {:else if item.entryType === 'folder'}
                                        <Folder class="fs-row-tile-ico"/>
                                    {:else}
                                        <FileText class="fs-row-tile-ico"/>
                                    {/if}
                                </div>
                                <div class="fs-row-main">
                                    <div class="fs-row-title">
                                        <span>
                                            {item.fileName || item.path.split(/[\\/]/).pop()}
                                        </span>
                                        {#if item.sensitiveKinds && item.sensitiveKinds.includes('sensitive')}
                                            <span
                                                    class="fs-sensitive"
                                                    title="Sensitive content detected: {formatSensitiveKinds(item.sensitiveKinds)}"
                                            >
                                                <KeyRound class="fs-sensitive-ico"/>
                                                Sensitive
                                            </span>
                                        {/if}
                                    </div>
                                    <div class="fs-row-path">{item.path}</div>
                                    <div class="fs-row-meta">
                                        {#if item.entryType === 'folder'}
                                            <span>Folder</span>
                                        {:else}
                                            <span>{item.extension || '—'}</span>
                                            <span class="fs-meta-dot">·</span>
                                            <span>{formatBytes(item.size)}</span>
                                        {/if}
                                        <span class="fs-meta-dot">·</span>
                                        <span>{formatDateTime(item.modifiedMs)}</span>
                                    </div>
                                    {#if item.matchReason}
                                        <div class="fs-match-reason">{item.matchReason}</div>
                                    {/if}
                                </div>
                                <div class="fs-row-actions">
                                    <Button
                                            size="sm"
                                            variant="ghost"
                                            iconOnly
                                            icon={PanelRightOpen}
                                            title="Preview"
                                            aria-label="Preview"
                                            onclick={() => openInspector({kind: 'file', item})}
                                    />
                                    <Button
                                            size="sm"
                                            variant="ghost"
                                            iconOnly
                                            icon={ExternalLink}
                                            title="Open"
                                            aria-label="Open"
                                            onclick={() => openFileSearchResult(item.path)}
                                    />
                                    <Button
                                            size="sm"
                                            variant="ghost"
                                            iconOnly
                                            icon={FolderOpen}
                                            title="Reveal"
                                            aria-label="Reveal"
                                            onclick={() => revealFileSearchResult(item.path)}
                                    />
                                </div>
                            </div>
                        {/each}
                        {#if loadingMoreFiles}
                            <div class="fs-load-state">Loading more file results…</div>
                        {:else if hasMoreFileResults}
                            <button
                                    type="button"
                                    onclick={() => void loadMoreFileResults()}
                                    class="fs-load-btn"
                            >
                                Load more file results
                            </button>
                        {/if}
                    </div>
                {/if}
            </div>
        </ToolPanel>
    {:else}
        {@const ec = emptyStateConfig}
        {@const Icon = ec.icon}
        {@const ActionIcon = ec.actionIcon}
        <ToolPanel padding="lg">
            <div class="fs-empty">
                <div
                        class="fs-empty-icon"
                        class:fs-empty-icon-warn={ec.iconWarn}
                        aria-hidden="true"
                >
                    <Icon class="fs-empty-ico"/>
                </div>
                <h2 class="fs-empty-title">{ec.title}</h2>
                <p class="fs-empty-desc">{ec.description}</p>
                {#if ec.showExamples}
                    <div class="fs-empty-grid">
                        {#if $fileSearchMode === 'content'}
                            <div class="fs-empty-card">
                                <code class="fs-empty-code">"annual report"</code>
                                <div class="fs-empty-card-sub">
                                    Exact phrase inside any file.
                                </div>
                            </div>
                            <div class="fs-empty-card">
                                <code class="fs-empty-code">tax AND deduction</code>
                                <div class="fs-empty-card-sub">
                                    Both terms must appear.
                                </div>
                            </div>
                            <div class="fs-empty-card">
                                <code class="fs-empty-code">invest</code>
                                <div class="fs-empty-card-sub">
                                    Prefix — matches investment, investor.
                                </div>
                            </div>
                            <div class="fs-empty-card">
                                <code class="fs-empty-code">budget from 2023</code>
                                <div class="fs-empty-card-sub">
                                    Content + date filter together.
                                </div>
                            </div>
                        {:else}
                            <div class="fs-empty-card">
                                <code class="fs-empty-code">large pdf from 2024</code>
                                <div class="fs-empty-card-sub">
                                    Size, type, and date filters.
                                </div>
                            </div>
                            <div class="fs-empty-card">
                                <code class="fs-empty-code">chrome</code>
                                <div class="fs-empty-card-sub">
                                    Launches installed apps inline.
                                </div>
                            </div>
                            <div class="fs-empty-card">
                                <code class="fs-empty-code">hash check</code>
                                <div class="fs-empty-card-sub">
                                    Finds matching KeepItLocal tools.
                                </div>
                            </div>
                            <div class="fs-empty-card">
                                <code class="fs-empty-code">spi</code>
                                <div class="fs-empty-card-sub">
                                    Prefixes match — spider, spike, spine.
                                </div>
                            </div>
                        {/if}
                    </div>
                {/if}
                {#if ec.actionLabel && ec.onAction}
                    <div class="fs-empty-actions">
                        <Button
                                variant={ec.actionVariant}
                                size="sm"
                                icon={ActionIcon}
                                onclick={ec.onAction}
                        >
                            {ec.actionLabel}
                        </Button>
                    </div>
                {/if}
                <!-- Wave 8.2 (2026-05-28): when a search returns no results,
                     gently suggest the OTHER mode. File mode → Content mode
                     ("nothing matched by name — maybe the words are inside
                     the file"). Content mode → File mode ("nothing inside
                     a file matched — maybe try by filename"). Surfaces the
                     two-mode model when it would actually help; quiet when
                     the input is empty or the index isn't ready. -->
                {#if ec.kind === 'no-matches'}
                    <div class="fs-cross-mode-hint">
                        {#if $fileSearchMode === 'files'}
                            <span>Looking for words <em>inside</em> files?</span>
                            <button
                                    type="button"
                                    class="fs-cross-mode-link"
                                    onclick={() => setSearchMode('content')}
                            >
                                Try Content search →
                            </button>
                        {:else}
                            <span>Looking for files by <em>name</em>?</span>
                            <button
                                    type="button"
                                    class="fs-cross-mode-link"
                                    onclick={() => setSearchMode('files')}
                            >
                                Try File search →
                            </button>
                        {/if}
                    </div>
                {/if}
            </div>
        </ToolPanel>
    {/if}

    {#snippet footer()}
        <span class="fs-footer-line">
            <Keyboard class="fs-footer-ico"/>
            <span>Press</span>
            <Kbd keys={overlayShortcutLabel}/>
            <span>anywhere to summon the quick-search overlay</span>
        </span>
    {/snippet}
</ToolPage>

<!-- ─── SideSheet: Filters & search options ───────────────────────
     Opened by the Filters button in the search bar. Holds:
       - Natural language toggle (was inline before)
       - Extension / Path / Size / Modified filters (were inline)
       - Clear filters action
     The sheet's open/close is purely local — no backend changes. -->
<SideSheet open={filtersOpen} title="Filters & options" onclose={() => (filtersOpen = false)}>
    <div class="ss-stack">
        <div class="ss-row">
            <div class="ss-row-text">
                <div class="ss-row-label">Natural language</div>
                <div class="ss-row-sub">
                    Lets you write queries like
                    <code class="fs-hint-code">large pdf from 2024</code>
                </div>
            </div>
            <Toggle
                    bind:checked={$fileSearchNaturalLanguage}
                    ariaLabel="Natural language mode"
            />
        </div>

        <div class="ss-divider"></div>

        <div class="ss-field">
            <label for="fs-ext-filter" class="ss-label">Extension</label>
            <input
                    id="fs-ext-filter"
                    bind:value={$fileSearchExtensionFilter}
                    use:escToClear={() => fileSearchExtensionFilter.set('')}
                    placeholder="pdf, docx, jpg…"
                    class="ss-input"
            />
        </div>
        <div class="ss-field">
            <label for="fs-path-filter" class="ss-label">Path contains</label>
            <input
                    id="fs-path-filter"
                    bind:value={$fileSearchPathFilter}
                    use:escToClear={() => fileSearchPathFilter.set('')}
                    placeholder="reports, 2024, downloads…"
                    class="ss-input"
            />
        </div>
        <div class="ss-field">
            <label for="fs-size-filter" class="ss-label">Size</label>
            <select
                    id="fs-size-filter"
                    bind:value={$fileSearchSizeFilter}
                    class="ss-select"
            >
                {#each FILE_SEARCH_SIZE_OPTIONS as option (option.label)}
                    <option value={option.token}>{option.label}</option>
                {/each}
            </select>
        </div>
        <div class="ss-field">
            <label for="fs-date-filter" class="ss-label">Modified</label>
            <select
                    id="fs-date-filter"
                    bind:value={$fileSearchDateFilter}
                    class="ss-select"
            >
                {#each FILE_SEARCH_DATE_OPTIONS as option (option.label)}
                    <option value={option.token}>{option.label}</option>
                {/each}
            </select>
        </div>
    </div>

    {#snippet footer()}
        <Button variant="ghost" onclick={clearFiltersOnly} disabled={!hasActiveFilters}>
            Clear filters
        </Button>
        <Button variant="primary" onclick={() => (filtersOpen = false)}>Done</Button>
    {/snippet}
</SideSheet>

<SideSheet open={inspectorOpen} title="Preview" width={620} onclose={closeInspector}>
    {#if inspectedResult}
        {@const item = inspectedResult.item}
        <div class="ss-stack fs-inspector">
            <div class="fs-inspector-hero">
                <div
                        class="fs-row-tile {inspectedFile?.entryType === 'folder'
                        ? 'fs-tile-folder'
                        : 'fs-tile-file'}"
                        aria-hidden="true"
                >
                    {#if entryIcons[item.path]}
                        <img
                                class="fs-row-tile-img"
                                src={entryIcons[item.path]}
                                alt=""
                                draggable="false"
                        />
                    {:else if inspectedFile?.entryType === 'folder'}
                        <Folder class="fs-row-tile-ico"/>
                    {:else}
                        <FileText class="fs-row-tile-ico"/>
                    {/if}
                </div>
                <div class="fs-inspector-hero-copy">
                    <h3 class="fs-inspector-name">
                        {item.fileName || item.path.split(/[\\/]/).pop()}
                    </h3>
                    <p class="fs-inspector-path">{item.path}</p>
                </div>
            </div>

            {#if inspectedFile?.entryType === 'folder'}
                <div class="fs-preview-unavailable">
                    <Folder class="fs-preview-unavailable-ico"/>
                    <span>Folder previews open in Explorer.</span>
                </div>
            {:else if isImageFile(item.extension)}
                <div class="fs-preview-stage fs-preview-image-wrap">
                    {#if item.size > MAX_IMAGE_PREVIEW_BYTES}
                        <div class="fs-preview-unavailable">
                            <span>This image is too large to preview here. Open it in your default viewer.</span>
                        </div>
                    {:else if previewLoadError}
                        <div class="fs-preview-unavailable">
                            <span>This image cannot be previewed here. Open it in your default viewer.</span>
                        </div>
                    {:else}
                        <img
                                class="fs-preview-image"
                                src={convertFileSrc(item.path, 'kilmedia')}
                                alt={item.fileName || 'Image preview'}
                                onerror={() => (previewLoadError = true)}
                        />
                    {/if}
                </div>
            {:else if isAudioFile(item.extension)}
                <div class="fs-preview-stage fs-preview-audio-wrap">
                    {#if previewLoadError}
                        <div class="fs-preview-unavailable">
                            <span>This audio file cannot play in the app. Open it in your default player.</span>
                        </div>
                    {:else}
                        <audio
                                class="fs-preview-audio"
                                controls
                                controlsList="nodownload noplaybackrate"
                                preload="metadata"
                                src={convertFileSrc(item.path, 'kilmedia')}
                                onerror={() => (previewLoadError = true)}
                        ></audio>
                    {/if}
                </div>
            {:else if isVideoFile(item.extension)}
                <div class="fs-preview-stage fs-preview-video-wrap">
                    {#if previewLoadError}
                        <div class="fs-preview-unavailable">
                            <span>This video format cannot play in the app. Open it in your default player.</span>
                        </div>
                    {:else}
                        <!-- svelte-ignore a11y_media_has_caption -- local media may not have a caption track. -->
                        <video
                                class="fs-preview-video"
                                controls
                                controlsList="nodownload noplaybackrate noremoteplayback"
                                preload="metadata"
                                disablePictureInPicture
                                src={convertFileSrc(item.path, 'kilmedia')}
                                onerror={() => (previewLoadError = true)}
                        ></video>
                    {/if}
                </div>
            {:else if isPdfFile(item.extension)}
                <div class="fs-preview-stage fs-preview-pdf-wrap">
                    {#if item.size > MAX_PDF_PREVIEW_BYTES}
                        <div class="fs-preview-unavailable">
                            <span>This PDF is too large to preview here. Open it in your default viewer.</span>
                        </div>
                    {:else if previewLoadError}
                        <div class="fs-preview-unavailable">
                            <span>This PDF cannot be previewed here. Open it in your default viewer.</span>
                        </div>
                    {:else}
                        <iframe
                                class="fs-preview-pdf"
                                title={item.fileName || 'PDF preview'}
                                src={`${convertFileSrc(item.path, 'kilmedia')}#toolbar=0&navpanes=0&view=FitH`}
                                onerror={() => (previewLoadError = true)}
                        ></iframe>
                    {/if}
                </div>
            {/if}

            <div class="ss-divider"></div>

            <div class="fs-inspector-grid">
                <div class="fs-inspector-field">
                    <span>Kind</span>
                    <strong>
                        {inspectedContent
                            ? 'Content match'
                            : inspectedFile?.entryType === 'folder'
                                ? 'Folder'
                                : 'File'}
                    </strong>
                </div>
                <div class="fs-inspector-field">
                    <span>Type</span>
                    <strong>{item.extension || '—'}</strong>
                </div>
                <div class="fs-inspector-field">
                    <span>Size</span>
                    <strong>{formatBytes(item.size)}</strong>
                </div>
                <div class="fs-inspector-field">
                    <span>Modified</span>
                    <strong>{formatDateTime(item.modifiedMs)}</strong>
                </div>
            </div>

            {#if item.sensitiveKinds?.includes('sensitive')}
                <div class="fs-inspector-sensitive">
                    <KeyRound class="fs-inspector-sensitive-ico"/>
                    <span>
                        Sensitive content detected{formatSensitiveKinds(item.sensitiveKinds)
                            ? `: ${formatSensitiveKinds(item.sensitiveKinds)}`
                            : ''}
                    </span>
                </div>
            {/if}

            {#if inspectedContent?.snippet}
                <div class="fs-inspector-section">
                    <span class="fs-inspector-label">Best matching passage</span>
                    <div class="fs-snippet">
                        {@html highlightSnippet(inspectedContent.snippet, inspectedContent.matchedKeywords)}
                    </div>
                </div>
            {:else if inspectedFile?.matchReason}
                <div class="fs-inspector-section">
                    <span class="fs-inspector-label">Why it matched</span>
                    <p class="fs-inspector-copy">{inspectedFile.matchReason}</p>
                </div>
            {/if}

            {#if item.matchedKeywords.length}
                <div class="fs-inspector-section">
                    <span class="fs-inspector-label">Matched terms</span>
                    <div class="fs-inspector-tags">
                        {#each item.matchedKeywords as keyword}
                            <span>{keyword}</span>
                        {/each}
                    </div>
                </div>
            {/if}
        </div>
    {/if}

    {#snippet footer()}
        <Button
                variant="ghost"
                icon={FolderOpen}
                disabled={!inspectedResult}
                onclick={() => void revealFileSearchResult(inspectedResult?.item.path ?? '')}
        >
            Reveal
        </Button>
        <Button
                variant="primary"
                icon={ExternalLink}
                disabled={!inspectedResult}
                onclick={() => void openFileSearchResult(inspectedResult?.item.path ?? '')}
        >
            Open
        </Button>
    {/snippet}
</SideSheet>

<style>
    /* ─── Search bar ─────────────────────────────────────────────
       Single-row composition: big input + mode segmented control +
       filters button. macOS-grade rhythm: 12px gaps, 40px control
       height, focus ring via the global rule. */
    .fs-bar {
        display: flex;
        align-items: center;
        gap: 10px;
    }

    .fs-input-wrap {
        position: relative;
        flex: 1;
        min-width: 0;
    }

    :global(.fs-input-icon) {
        position: absolute;
        left: 12px;
        top: 50%;
        transform: translateY(-50%);
        width: 16px;
        height: 16px;
        color: var(--color-muted);
        pointer-events: none;
    }

    .fs-input {
        width: 100%;
        height: 40px;
        padding: 0 80px 0 38px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 10px;
        color: var(--color-text);
        font-size: 14px;
        outline: none;
        transition: border-color var(--dur-micro) var(--ease-out),
        background-color var(--dur-micro) var(--ease-out);
    }

    .fs-input::placeholder {
        color: var(--color-muted);
    }

    .fs-input:hover {
        border-color: var(--color-border-strong);
    }

    .fs-input:focus {
        border-color: var(--color-accent);
        background: var(--color-panel-3);
    }

    .fs-input-clear {
        position: absolute;
        right: 38px;
        top: 50%;
        transform: translateY(-50%);
        width: 24px;
        height: 24px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: transparent;
        border: none;
        border-radius: 6px;
        color: var(--color-muted);
        cursor: pointer;
        transition: background-color var(--dur-micro) var(--ease-out),
        color var(--dur-micro) var(--ease-out);
    }

    .fs-input-clear:hover {
        background: var(--color-panel-3);
        color: var(--color-text);
    }

    /* Override the global `button:active { transform: translateY(0.5px) }`
       press-feedback rule. The X uses `translateY(-50%)` to stay
       centered against its absolute parent; the global rule would
       overwrite the centering and make the button shift ~50% down on
       click (the "moving down" bug). Restore the centering here. */
    .fs-input-clear:active {
        transform: translateY(-50%);
    }

    :global(.fs-input-clear-ico) {
        width: 13px;
        height: 13px;
    }

    .fs-input-busy {
        position: absolute;
        right: 12px;
        top: 50%;
        transform: translateY(-50%);
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 16px;
        height: 16px;
    }

    /* ─── Mode segmented control ─────────────────────────────── */
    .fs-mode {
        display: inline-flex;
        align-items: center;
        gap: 2px;
        padding: 3px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 10px;
        height: 40px;
        flex: none;
    }

    .fs-mode-btn {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        height: 32px;
        padding: 0 12px;
        background: transparent;
        border: none;
        border-radius: 7px;
        color: var(--color-text-secondary);
        font-size: 12.5px;
        font-weight: 500;
        cursor: pointer;
        transition: background-color var(--dur-micro) var(--ease-out),
        color var(--dur-micro) var(--ease-out);
    }

    .fs-mode-btn:hover:not(.is-on) {
        color: var(--color-text);
    }

    .fs-mode-btn.is-on {
        background: var(--color-panel);
        color: var(--color-text);
        box-shadow: 0 1px 2px var(--color-shadow),
        inset 0 1px 0 color-mix(in srgb, var(--color-text) 6%, transparent);
    }

    :global(.fs-mode-ico) {
        width: 13px;
        height: 13px;
    }

    .fs-inspector-label {
        font-size: 10.5px;
        font-weight: 600;
        letter-spacing: 0.06em;
        text-transform: uppercase;
        color: var(--color-muted);
    }

    /* ─── Filters button ─────────────────────────────────────── */
    .fs-filter-btn {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        height: 40px;
        padding: 0 14px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 10px;
        color: var(--color-text);
        font-size: 13px;
        font-weight: 500;
        cursor: pointer;
        position: relative;
        flex: none;
        transition: background-color var(--dur-micro) var(--ease-out),
        border-color var(--dur-micro) var(--ease-out);
    }

    .fs-filter-btn:hover {
        background: var(--color-panel-3);
        border-color: var(--color-border-strong);
    }

    .fs-filter-btn.is-active {
        border-color: color-mix(in srgb, var(--color-accent) 50%, var(--color-border));
        background: var(--color-accent-soft);
        color: var(--color-accent);
    }

    :global(.fs-filter-ico) {
        width: 14px;
        height: 14px;
    }

    .fs-filter-dot {
        position: absolute;
        top: 6px;
        right: 6px;
        width: 7px;
        height: 7px;
        border-radius: 50%;
        background: var(--color-accent);
        box-shadow: 0 0 0 1.5px var(--color-panel);
    }

    /* ─── Hint strip ─────────────────────────────────────────── */
    .fs-hint {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-size: 11.5px;
        color: var(--color-muted);
        padding: 0 2px;
    }

    :global(.fs-hint-ico) {
        width: 12px;
        height: 12px;
        color: var(--color-accent);
    }

    .fs-hint-code {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11px;
        padding: 1px 6px;
        border-radius: 4px;
        background: var(--color-panel-2);
        color: var(--color-accent);
    }

    /* ─── Results meta ───────────────────────────────────────── */
    .fs-meta {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 4px 4px 0;
        transition: opacity 160ms var(--ease-out);
    }

    /* While a re-search runs over existing results, fade the meta to
       0.55 — communicates "these numbers are about to update" without
       a "Searching…" flash. Returns to 1.0 the moment results land. */
    .fs-meta.is-stale {
        opacity: 0.55;
    }

    .fs-meta-left {
        display: flex;
        align-items: center;
        gap: 10px;
        flex-wrap: wrap;
    }

    .fs-meta-summary {
        font-size: 12.5px;
        font-weight: 500;
        color: var(--color-text);
    }

    .fs-meta-time {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        font-size: 11.5px;
        color: var(--color-muted);
        font-variant-numeric: tabular-nums;
    }

    :global(.fs-meta-time-ico) {
        width: 11px;
        height: 11px;
        color: var(--color-accent);
    }

    /* ─── Results body ──────────────────────────────────────── */
    .fs-results {
        display: flex;
        flex-direction: column;
        gap: 14px;
        padding: 4px;
    }

    .fs-section {
        display: flex;
        flex-direction: column;
        gap: 2px;
        /* Section-level fade-in — the single, calm motion for results
           appearing. CSS plays it once on element mount; with the
           stable-header pattern from 3.3.5 sections stay mounted
           across re-searches, so this fires only when a section first
           appears (or re-appears after disappearing). 180 ms is fast
           enough to feel like a "presence" rather than a slide. */
        animation: fs-section-enter 180ms var(--ease-out, ease) both;
    }

    @keyframes fs-section-enter {
        from {
            opacity: 0;
        }
        to {
            opacity: 1;
        }
    }

    @media (prefers-reduced-motion: reduce) {
        .fs-section {
            animation: none;
        }
    }

    .fs-section-head {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        padding: 6px 8px 4px;
        font-size: 10.5px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }

    :global(.fs-section-ico) {
        width: 13px;
        height: 13px;
    }

    .fs-section-count {
        font-size: 10.5px;
        color: var(--color-muted);
        font-variant-numeric: tabular-nums;
        font-weight: 500;
        text-transform: none;
        letter-spacing: 0;
    }

    /* ─── Result rows ────────────────────────────────────────
       macOS-style: icon tile + main content + actions. Hover
       lifts on background tint only (no border pop). Rich
       variant accommodates multi-line meta + snippets. */
    .fs-row {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 9px 10px;
        border-radius: var(--radius-control);
        background: transparent;
        transition: background-color var(--dur-micro) var(--ease-out);
        /* NO row-level entrance animation. Per-row staggered fade
           reads as flicker during rapid type-delete-type — even with
           the previous .is-cold gating, the visible 432 ms reveal
           window during the FIRST results land was perceived as
           jitter. The "elegant landing" feel now comes from a single
           short fade-in on the entire section (.fs-section), which
           CSS naturally plays once per mount. Rows snap in instantly
           inside an already-faded section — no visible flicker. */
    }

    .fs-row:hover {
        background: color-mix(in srgb, var(--color-text) 4%, transparent);
    }

    .fs-row-rich {
        align-items: flex-start;
        padding: 11px 10px;
    }

    .fs-row-tile {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 32px;
        height: 32px;
        border-radius: 8px;
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
    }

    .fs-tile-app {
        background: color-mix(in srgb, #60a5fa 14%, transparent);
        color: #60a5fa;
    }

    .fs-tile-tool {
        background: var(--color-accent-soft);
        color: var(--color-accent);
    }

    .fs-tile-folder {
        background: color-mix(in srgb, #fbbf24 14%, transparent);
        color: #fbbf24;
    }

    .fs-tile-file {
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
    }

    :global(.fs-row-tile-ico) {
        width: 16px;
        height: 16px;
    }

    .fs-row-tile-img {
        display: block;
        width: 20px;
        height: 20px;
        object-fit: contain;
        image-rendering: -webkit-optimize-contrast;
    }

    .fs-row-main {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    .fs-row-title {
        display: flex;
        align-items: center;
        gap: 8px;
        font-size: 13.5px;
        font-weight: 500;
        color: var(--color-text);
        letter-spacing: -0.005em;
        overflow: hidden;
    }

    .fs-row-title > span:first-child {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        min-width: 0;
    }

    .fs-row-path {
        font-size: 11.5px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .fs-row-meta {
        display: flex;
        align-items: center;
        gap: 6px;
        margin-top: 2px;
        font-size: 11px;
        color: var(--color-muted);
        font-variant-numeric: tabular-nums;
        flex-wrap: wrap;
    }

    .fs-meta-dot {
        opacity: 0.6;
    }

    .fs-row-meta-accent {
        color: var(--color-accent);
        font-weight: 500;
    }

    .fs-match-reason {
        margin-top: 4px;
        font-size: 11px;
        font-style: italic;
        color: var(--color-text-secondary);
    }

    .fs-snippet {
        margin-top: 6px;
        padding: 8px 10px;
        background: var(--color-bg);
        border: 1px solid var(--color-divider, var(--color-border));
        border-radius: var(--radius-control);
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        line-height: 1.55;
        color: var(--color-text-secondary);
        white-space: pre-wrap;
        word-break: break-word;
        max-height: 96px;
        overflow-y: auto;
    }

    .fs-snippet-extra {
        margin-top: 4px;
    }

    /* Highlight inside snippets. The `<mark>` tags are inserted by
       `highlightSnippet()` in the script (or by the backend when it
       provides pre-marked HTML). Accent-tinted pill with bold weight
       so matches pop visually against the muted snippet text — easy
       to scan without overwhelming the snippet itself. */
    .fs-snippet :global(mark) {
        background: color-mix(in srgb, var(--color-accent) 30%, transparent);
        color: var(--color-accent);
        padding: 1px 4px;
        border-radius: 4px;
        font-weight: 600;
        /* The bold weight subtly increases width — let the highlight
           breathe in line-height rather than crowd adjacent text. */
        line-height: 1.2;
    }

    .fs-row-actions {
        flex: none;
        display: flex;
        align-items: center;
        gap: 4px;
        opacity: 0.5;
        transition: opacity var(--dur-micro) var(--ease-out);
    }

    .fs-row:hover .fs-row-actions,
    .fs-row:focus-within .fs-row-actions {
        opacity: 1;
    }

    /* ─── Sensitive badge ────────────────────────────────────── */
    .fs-sensitive {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        height: 18px;
        padding: 0 7px;
        border-radius: var(--radius-pill);
        background: var(--color-warning-soft);
        border: 1px solid var(--color-warning-strong);
        color: var(--color-warning);
        font-size: 10px;
        font-weight: 600;
        white-space: nowrap;
    }

    :global(.fs-sensitive-ico) {
        width: 9px;
        height: 9px;
    }

    /* ─── Semantic (meaning) badges ──────────────────────────── */
    /* Query-level pill: the meaning pass actually ran for this search. */
    .fs-semantic-active {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        height: 18px;
        padding: 0 8px;
        border-radius: var(--radius-pill);
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
        color: var(--color-accent);
        font-size: 10px;
        font-weight: 600;
        white-space: nowrap;
    }

    /* Per-result badge: this row was surfaced by meaning (no keyword match). */
    .fs-semantic-badge {
        display: inline-flex;
        align-items: center;
        gap: 3px;
        color: var(--color-accent);
        font-weight: 600;
        white-space: nowrap;
    }

    /* Softer variant: a keyword hit that ALSO matched strongly by meaning. */
    .fs-semantic-soft {
        color: var(--color-text-secondary);
        font-weight: 500;
    }

    :global(.fs-semantic-ico) {
        width: 10px;
        height: 10px;
    }

    /* ─── Load more / skeleton ───────────────────────────────── */
    .fs-load-btn {
        margin-top: 8px;
        width: 100%;
        padding: 9px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-accent);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;
        transition: background-color var(--dur-micro) var(--ease-out),
        border-color var(--dur-micro) var(--ease-out);
    }

    .fs-load-btn:hover {
        background: var(--color-panel-3);
        border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
    }

    .fs-load-state {
        margin-top: 6px;
        text-align: center;
        font-size: 11px;
        color: var(--color-muted);
        padding: 8px;
    }

    /* .fs-skeleton-row + @keyframes fs-shimmer removed in Phase 3.3.7
       — sections no longer mount speculatively during search, so the
       skeleton shimmer is dead code. The input's small spinner is the
       sole loading cue; absent skeletons = no flicker. */

    /* ─── Spinner ────────────────────────────────────────────── */
    .fs-spinner {
        width: 13px;
        height: 13px;
        border: 2px solid color-mix(in srgb, var(--color-accent) 30%, transparent);
        border-top-color: var(--color-accent);
        border-radius: 50%;
        animation: fs-spin 720ms linear infinite;
    }

    @keyframes fs-spin {
        to {
            transform: rotate(360deg);
        }
    }

    /* ─── Empty state ───────────────────────────────────────
       Unified across all empty-state kinds (intro / no-matches /
       index-not-ready / indexing). The container stays mounted; only
       icon / title / desc / action swap inside per `emptyStateConfig`. */
    .fs-empty {
        display: flex;
        flex-direction: column;
        align-items: center;
        text-align: center;
        gap: 12px;
        padding: 8px 0 4px;
    }

    /* Soft cross-state transition for the changing text. The title /
       description elements stay mounted across state changes (we only
       update their textContent) — so a `transition` on opacity / color
       won't fire on text-content change alone. To get a brief settle
       on content change we instead key-animate via a CSS-only trick:
       the title gets a transition on color, which the cascade picks
       up reactively because the empty-icon's `class:fs-empty-icon-warn`
       changes alongside the text. The effect is subtle by design — we
       don't want a heavy "card-swap" feel, just a calm settle. */
    .fs-empty-title,
    .fs-empty-desc {
        transition: color 220ms var(--ease-out);
    }

    .fs-empty-icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 48px;
        height: 48px;
        border-radius: 12px;
        color: var(--color-accent);
        box-shadow: inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
        inset 0 0 0 1px color-mix(in srgb, var(--color-text) 4%, transparent);
        /* Mirrors EmptyState.svelte's icon float — Phase 7.2. */
        animation: fs-empty-icon-float 6s ease-in-out infinite;
        will-change: transform;
    }

    @keyframes fs-empty-icon-float {
        0%,
        100% {
            transform: translateY(0);
        }
        50% {
            transform: translateY(-2px);
        }
    }

    @media (prefers-reduced-motion: reduce) {
        .fs-empty-icon {
            animation: none;
        }
    }

    /* Warn-toned variant — used by the "Index not ready" state so
       the icon reads as "attention needed" rather than "go go go". */
    .fs-empty-icon-warn {
        background: var(--color-warning-soft);
        color: var(--color-warning);
    }

    .fs-empty-actions {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 8px;
        margin-top: 4px;
    }

    /* Wave 8.2 (2026-05-28): cross-mode hint at the bottom of the empty
       state. Visual hierarchy: smaller + muted than the title/description
       so it reads as a gentle suggestion, not a competing CTA. */
    .fs-cross-mode-hint {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 4px;
        margin-top: 16px;
        padding-top: 16px;
        border-top: 1px solid var(--color-border-subtle, var(--color-border));
        font-size: 13px;
        color: var(--color-text-secondary);
    }
    .fs-cross-mode-hint em {
        font-style: normal;
        color: var(--color-text);
        font-weight: 500;
    }
    .fs-cross-mode-link {
        background: none;
        border: none;
        padding: 4px 8px;
        color: var(--color-accent);
        font-size: 13px;
        font-weight: 500;
        cursor: pointer;
        border-radius: 6px;
        transition:
            background-color 140ms ease,
            color 140ms ease;
    }
    .fs-cross-mode-link:hover {
        background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    }

    :global(.fs-empty-ico) {
        width: 22px;
        height: 22px;
    }

    .fs-empty-title {
        margin: 0;
        font-size: 17px;
        font-weight: 600;
        letter-spacing: -0.012em;
        color: var(--color-text);
    }

    .fs-empty-desc {
        margin: 0;
        max-width: 56ch;
        font-size: 13px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }

    .fs-empty-grid {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 8px;
        width: 100%;
        max-width: 640px;
        margin-top: 4px;
        text-align: left;
    }

    @media (max-width: 720px) {
        .fs-empty-grid {
            grid-template-columns: 1fr;
        }
    }

    .fs-empty-card {
        padding: 10px 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-divider, var(--color-border));
        border-radius: var(--radius-control);
    }

    .fs-empty-code {
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 12px;
        color: var(--color-accent);
    }

    .fs-empty-card-sub {
        margin-top: 4px;
        font-size: 11.5px;
        line-height: 1.45;
        color: var(--color-muted);
    }

    /* ─── Footer keyboard hint ──────────────────────────────── */
    .fs-footer-line {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        flex-wrap: wrap;
    }

    :global(.fs-footer-ico) {
        width: 13px;
        height: 13px;
        color: var(--color-accent);
    }

    /* ─── SideSheet content ──────────────────────────────────
       Form fields styled to match the new conventions — soft
       borders, generous spacing, semantic tokens only. */
    .ss-stack {
        display: flex;
        flex-direction: column;
        gap: 16px;
    }

    .ss-row {
        display: flex;
        align-items: flex-start;
        gap: 12px;
        justify-content: space-between;
    }

    .ss-row-text {
        flex: 1;
        min-width: 0;
    }

    .ss-row-label {
        font-size: 13.5px;
        font-weight: 500;
        color: var(--color-text);
        letter-spacing: -0.005em;
    }

    .ss-row-sub {
        margin-top: 2px;
        font-size: 12px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }

    .ss-divider {
        height: 1px;
        background: var(--color-divider, var(--color-border));
    }

    .ss-field {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }

    .ss-label {
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }

    .ss-input,
    .ss-select {
        width: 100%;
        height: 34px;
        padding: 0 11px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-size: 13px;
        outline: none;
        transition: border-color var(--dur-micro) var(--ease-out);
    }

    .ss-input::placeholder {
        color: var(--color-muted);
    }

    .ss-input:hover,
    .ss-select:hover {
        border-color: var(--color-border-strong);
    }

    .ss-input:focus,
    .ss-select:focus {
        border-color: var(--color-accent);
    }

    .fs-inspector {
        gap: 18px;
    }

    .fs-inspector-hero {
        display: flex;
        align-items: flex-start;
        gap: 12px;
    }

    .fs-inspector-hero-copy {
        min-width: 0;
    }

    .fs-inspector-name {
        margin: 1px 0 0;
        overflow: hidden;
        color: var(--color-text);
        font-size: 14px;
        font-weight: 600;
        letter-spacing: -0.006em;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .fs-inspector-path {
        margin: 4px 0 0;
        overflow-wrap: anywhere;
        color: var(--color-muted);
        font-size: 11.5px;
        line-height: 1.45;
    }

    .fs-preview-stage {
        overflow: hidden;
        background: var(--color-bg);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
    }

    .fs-preview-image-wrap,
    .fs-preview-video-wrap {
        display: flex;
        align-items: center;
        justify-content: center;
        min-height: 180px;
    }

    .fs-preview-image {
        display: block;
        max-width: 100%;
        max-height: min(42vh, 360px);
        object-fit: contain;
    }

    .fs-preview-audio-wrap {
        padding: 14px;
    }

    .fs-preview-audio {
        display: block;
        width: 100%;
    }

    .fs-preview-video {
        display: block;
        width: 100%;
        max-height: min(42vh, 360px);
        background: var(--color-bg);
    }

    .fs-preview-pdf-wrap {
        height: min(52vh, 440px);
        min-height: 300px;
    }

    .fs-preview-pdf {
        display: block;
        width: 100%;
        height: 100%;
        border: none;
        background: var(--color-bg);
    }

    .fs-preview-unavailable {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 8px;
        min-height: 72px;
        padding: 14px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text-secondary);
        font-size: 12.5px;
        line-height: 1.45;
        text-align: center;
    }

    .fs-preview-stage .fs-preview-unavailable {
        min-height: 180px;
        border: none;
        border-radius: 0;
    }

    :global(.fs-preview-unavailable-ico) {
        flex: none;
        width: 16px;
        height: 16px;
        color: var(--color-muted);
    }

    .fs-inspector-grid {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 12px 16px;
    }

    .fs-inspector-field {
        display: flex;
        flex-direction: column;
        gap: 3px;
        min-width: 0;
    }

    .fs-inspector-field > span {
        color: var(--color-muted);
        font-size: 10.5px;
        font-weight: 600;
        letter-spacing: 0.05em;
        text-transform: uppercase;
    }

    .fs-inspector-field > strong {
        overflow: hidden;
        color: var(--color-text);
        font-size: 12.5px;
        font-weight: 500;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .fs-inspector-sensitive {
        display: flex;
        align-items: flex-start;
        gap: 8px;
        padding: 9px 10px;
        background: var(--color-warning-soft);
        border: 1px solid var(--color-warning-strong);
        border-radius: var(--radius-control);
        color: var(--color-warning);
        font-size: 12px;
        line-height: 1.45;
    }

    :global(.fs-inspector-sensitive-ico) {
        flex: none;
        width: 14px;
        height: 14px;
        margin-top: 1px;
    }

    .fs-inspector-section {
        display: flex;
        flex-direction: column;
        gap: 7px;
    }

    .fs-inspector-copy {
        margin: 0;
        color: var(--color-text-secondary);
        font-size: 12.5px;
        line-height: 1.5;
    }

    .fs-inspector-tags {
        display: flex;
        flex-wrap: wrap;
        gap: 5px;
    }

    .fs-inspector-tags span {
        max-width: 100%;
        overflow: hidden;
        padding: 3px 7px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-pill);
        color: var(--color-text-secondary);
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 10.5px;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
</style>
