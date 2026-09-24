<script lang="ts">
    /*
      Home — the landing screen users see when KeepItLocal opens.

      Distinct from About (which is the marketing / "what is this app"
      page that stays reachable from the TopBar Info button). Home is a
      WORKSPACE — fast entry point into the most-used local workflows,
      live telemetry about what KeepItLocal is doing on this machine,
      and the offline-first identity reinforcement.

      Layout:
        - Hero (badge + title + subtitle)
        - Wide search CTA bar that opens the global overlay
        - Two-column body:
          * Left (main): Recent activity or existing pinned notes and
            clipboard items
          * Right (sidebar): Local Activity stats panel + True Offline
            First reassurance card

      The default route is Recent. The same surface also renders Pinned
      when the router passes `selected = 'pinned'`.
    */
    import { onMount } from 'svelte';
    import { tweened } from 'svelte/motion';
    import { cubicOut } from 'svelte/easing';
    import { convertFileSrc, invoke } from '@tauri-apps/api/core';
    import { getScreen } from '$lib/appScreens';
    import { openCommandPalette } from '$lib/stores/commandPalette';
    import {
        AppWindow,
        ArrowUpRight,
        Clock3,
        Search,
        Sparkles,
        Clipboard as ClipboardIcon,
        FileText,
        Folder,
        NotebookPen,
        Pin,
        RefreshCw,
        Lock,
        Command,
    } from '@lucide/svelte';

    import { enabledPackIds } from '$lib/stores/toolPacks';
    import { settings } from '$lib/stores/settings';
    import { commandActiveTab } from '$lib/stores/categoryNav';
    import { openFileSearchResult, openLaunchTarget } from '$lib/stores/fileSearch';
    import { initNotesStore, notes, openNote, type NoteSummary } from '$lib/stores/notes';

    let { selected = $bindable() }: { selected: string } = $props();

    type RecentKind = 'app' | 'file' | 'folder' | 'tool';

    type RecentItem = {
        kind: RecentKind;
        path: string;
        displayName: string;
        launchCount: number;
        lastLaunchedMs: number;
        score: number;
    };

    type RecentItemsResult = {
        apps: RecentItem[];
        files: RecentItem[];
        folders: RecentItem[];
        tools: RecentItem[];
    };

    type ClipboardEntry = {
        isPinned: boolean;
    };

    let recentItems = $state<RecentItemsResult | null>(null);
    let recentLoading = $state(true);
    let recentEntryIcons = $state<Record<string, string | null>>({});
    let clipboardEntries = $state<ClipboardEntry[]>([]);
    let isPinnedView = $derived(selected === 'pinned');
    let recentList = $derived.by(() => {
        if (!recentItems) return [];
        return Object.values(recentItems)
            .flat()
            .sort((a, b) => b.lastLaunchedMs - a.lastLaunchedMs)
            .slice(0, 10);
    });
    let pinnedNotes = $derived(
        $notes
            .filter((note) => note.pinned)
            .sort((a, b) => b.modifiedMs - a.modifiedMs)
            .slice(0, 5),
    );
    let pinnedClipboardCount = $derived(clipboardEntries.filter((entry) => entry.isPinned).length);

    /* ──────────────────────────────────────────────────────────────
       Time-based greeting. Replaces the static "Workspace" title with a
       "Good morning/afternoon/evening/night, {Name}" line. The minute
       ticker keeps it correct if the home page is left open across an
       hour boundary; the name comes from Settings → System (empty until
       the user sets it, in which case we just show the greeting).
       ────────────────────────────────────────────────────────────── */
    let greetingTick = $state(Date.now());
    $effect(() => {
        const id = setInterval(() => (greetingTick = Date.now()), 60_000);
        return () => clearInterval(id);
    });
    let greeting = $derived.by(() => {
        const h = new Date(greetingTick).getHours();
        if (h >= 5 && h < 12) return 'Good morning';
        if (h >= 12 && h < 18) return 'Good afternoon';
        if (h >= 18 && h < 22) return 'Good evening';
        return 'Good night';
    });
    let userName = $derived(($settings.userName ?? '').trim());

    /** Live telemetry — fetched once on mount. Refreshes if the user
     *  navigates away and back (component remounts via {#key selected}
     *  in the parent router). We deliberately don't poll — the home
     *  page is a snapshot, not a live dashboard. */
    let indexedFiles = $state<number | null>(null);
    /** Content-index file count — separate from filename index. Smaller
     *  number because content indexing only touches text-extractable
     *  files and respects the user's `index_content` toggle in
     *  Settings → Search Index. */
    let indexedContent = $state<number | null>(null);
    let clipboardItems = $state<number | null>(null);
    let indexingActive = $state(false);

    /** Active tool packs — every enabled pack including Core. Core is
     *  always present, so the user sees at minimum "1 installed". */
    let activeToolPackCount = $derived($enabledPackIds.length);

    /* ──────────────────────────────────────────────────────────────
       Phase 7.6: count-up ticker on the four telemetry numbers.
       Each metric gets a Svelte `tweened` store that interpolates
       from 0 → value over 800 ms with a cubic-out easing — fast
       enough to feel responsive, slow enough that the eye registers
       the count climbing. We feed `tweened.set(value)` in $effects
       triggered by the underlying state; before the backend
       response lands the displayed value stays at 0 (the template
       still renders `—` for null via fmt(), see below).
       ────────────────────────────────────────────────────────────── */
    const indexedFilesTween = tweened(0, { duration: 800, easing: cubicOut });
    const indexedContentTween = tweened(0, { duration: 800, easing: cubicOut });
    const clipboardItemsTween = tweened(0, { duration: 800, easing: cubicOut });
    const activeToolPackTween = tweened(0, { duration: 500, easing: cubicOut });

    $effect(() => {
        if (indexedFiles !== null) indexedFilesTween.set(indexedFiles);
    });
    $effect(() => {
        if (indexedContent !== null) indexedContentTween.set(indexedContent);
    });
    $effect(() => {
        if (clipboardItems !== null) clipboardItemsTween.set(clipboardItems);
    });
    $effect(() => {
        activeToolPackTween.set(activeToolPackCount);
    });

    onMount(async () => {
        void initNotesStore();

        // File search status. FileSearchStatus carries TWO file counts:
        //   - `indexedFiles`           — content index (text-only files
        //                                 with full-text search). Usually
        //                                 a small subset.
        //   - `filenameIndexedFiles`   — filename index (every file's
        //                                 name regardless of type). This
        //                                 is the number the user sees on
        //                                 the File Search tab and the
        //                                 number they think of as "files
        //                                 indexed."
        // We show the filename count because it matches user expectation
        // (a user with 292k files indexed shouldn't see "0" because their
        // content index happens to be empty). Falls back to content count
        // if filename count isn't populated yet.
        try {
            const status = await invoke<{
                indexedFiles?: number;
                filenameIndexedFiles?: number;
                watcherEnabled?: boolean;
            }>('get_file_search_status');
            // Two separate counts surfaced as separate stat rows:
            //   - "Indexed Files"   = filenameIndexedFiles (every file's
            //     name regardless of type; the big number)
            //   - "Indexed Content" = indexedFiles (text files indexed
            //     for full-text search; the smaller, content-aware number)
            indexedFiles = status.filenameIndexedFiles ?? 0;
            indexedContent = status.indexedFiles ?? 0;
            // We treat "indexing active" as "watcher is enabled" if the
            // backend exposes that flag, else "has indexed files" as a
            // proxy. Telemetry should err toward reassuring.
            indexingActive =
                status.watcherEnabled ?? (indexedFiles > 0 || indexedContent > 0);
        } catch {
            indexedFiles = 0;
            indexedContent = 0;
            indexingActive = false;
        }

        // Clipboard items count. get_clipboard_history returns the
        // full Vec — we only need .length, so this is technically more
        // bytes over IPC than ideal. If perf becomes an issue we add a
        // dedicated count command; for ~thousand entries it's fine.
        try {
            const entries = await invoke<ClipboardEntry[]>('get_clipboard_history');
            clipboardEntries = Array.isArray(entries) ? entries : [];
            clipboardItems = clipboardEntries.length;
        } catch {
            clipboardEntries = [];
            clipboardItems = 0;
        }

        await refreshRecentItems();
    });

    async function refreshRecentItems() {
        recentLoading = true;
        try {
            recentItems = await invoke<RecentItemsResult>('get_recent_items', {limitPerKind: 6});
        } catch {
            recentItems = null;
        } finally {
            recentLoading = false;
        }
    }

    async function fetchRecentEntryIcon(path: string, kind: 'app' | 'folder' | 'file') {
        if (!path || path in recentEntryIcons) return;
        recentEntryIcons = {...recentEntryIcons, [path]: null};
        try {
            const iconPath = await invoke<string | null>('ensure_launcher_icon', {path, kind});
            if (iconPath) recentEntryIcons = {...recentEntryIcons, [path]: convertFileSrc(iconPath)};
        } catch {
            // Keep the fallback icon when the system does not expose one.
        }
    }

    $effect(() => {
        for (const item of recentList) {
            if (item.kind === 'tool') continue;
            void fetchRecentEntryIcon(item.path, item.kind);
        }
    });

    function recentKindLabel(kind: RecentKind): string {
        switch (kind) {
            case 'app':
                return 'Application';
            case 'file':
                return 'File';
            case 'folder':
                return 'Folder';
            default:
                return 'Tool';
        }
    }

    function relativeTime(timestamp: number): string {
        const elapsed = Math.max(0, Date.now() - timestamp);
        const minutes = Math.floor(elapsed / 60_000);
        if (minutes < 1) return 'Just now';
        if (minutes < 60) return `${minutes}m ago`;
        const hours = Math.floor(minutes / 60);
        if (hours < 24) return `${hours}h ago`;
        const days = Math.floor(hours / 24);
        if (days < 7) return `${days}d ago`;
        return new Date(timestamp).toLocaleDateString(undefined, {month: 'short', day: 'numeric'});
    }

    async function openRecent(item: RecentItem) {
        if (item.kind === 'app') {
            await openLaunchTarget(item.path);
            return;
        }
        if (item.kind === 'file' || item.kind === 'folder') {
            await openFileSearchResult(item.path);
            return;
        }
        void invoke('record_frecency_launch', {kind: 'tool', path: item.path}).catch(() => {});
        selected = item.path;
    }

    async function openPinnedNote(note: NoteSummary) {
        await openNote(note.path);
        selected = 'notes';
    }

    function openPinnedClipboard() {
        commandActiveTab.set('clipboard');
        selected = 'command';
    }

    /** Format a count with thousand-separators. Returns '—' for null
     *  so the loading state is unambiguous; '0' is a real value. */
    function fmt(n: number | null): string {
        if (n === null) return '—';
        return n.toLocaleString('en-US');
    }

    /** Open the search palette.
     *
     *  TEST BINDING (temporary, Phase 3.6.5): mounts the NEW unified
     *  `/command` palette as an OVERLAY over the live workspace so it
     *  can be A/B tested against the still-live old overlays (which
     *  remain reachable via their own global hotkeys). The workspace
     *  stays visible behind the palette (no black void) because the
     *  main route is never unmounted — the palette layers on top.
     *
     *  Phase 3.6.6 replaces this with the real transparent Tauri
     *  overlay window + global-hotkey routing, at which point this
     *  reverts to a `show_*_window_command` invoke. Old path kept here
     *  for a trivial revert:
     *      void invoke('show_overlay_window_command').catch(() => {}); */
    function openSearchOverlay() {
        openCommandPalette();
    }

    /** Render the user's actual overlay hotkey as kbd chips. Reads from
     *  the settings store so if the user remaps the hotkey in Settings,
     *  the home page chips update live. Mirrors the formatter we use in
     *  Settings.svelte and ShortcutPicker. */
    function shortcutParts(shortcut: string | null | undefined): string[] {
        if (!shortcut) return ['Ctrl', 'Alt', 'S']; // last-ditch fallback
        return shortcut
            .replace(/CommandOrControl/gi, 'Ctrl')
            .replace(/\bCmd\b/gi, 'Ctrl')
            .replace(/\bSuper\b/gi, 'Win')
            .split('+')
            .map((p) => p.trim())
            .filter(Boolean);
    }
    let overlayHotkeyParts = $derived(shortcutParts($settings.commandOverlayShortcut));

</script>

<div class="home-page">
    <!-- ─── Hero ─────────────────────────────────────────────────── -->
    <header class="hero">
        <span class="hero-badge">
            {#if isPinnedView}
                <Pin class="badge-ico" /> Pinned
            {:else}
                <Clock3 class="badge-ico" /> Recent
            {/if}
        </span>
        {#if isPinnedView}
            <h1 class="hero-title">Keep your important work within reach.</h1>
            <p class="hero-sub">
                Your existing pinned notes and clipboard items, kept local and ready when you need them.
            </p>
        {:else}
            <h1 class="hero-title">
                {greeting}{#if userName}, <span class="brand-it">{userName}</span>{/if}
            </h1>
            <p class="hero-sub">
                Pick up the apps, files, folders, and tools you used most recently.
            </p>
        {/if}
    </header>

    <!-- ─── Search CTA bar ───────────────────────────────────────── -->
    <!-- Looks like a search input, behaves like a button — clicking
         anywhere on it opens the real global overlay (which is where
         the actual typing happens). Keeps the home page lightweight
         and avoids duplicating the overlay's search infrastructure. -->
    <button
        type="button"
        class="search-cta"
        onclick={openSearchOverlay}
        title="Open the global search overlay (Ctrl+Alt+S)"
    >
        <div class="search-cta-icon" aria-hidden="true">
            <Search class="search-ico" />
        </div>
        <span class="search-cta-placeholder">
            Search local files, tools, or run commands…
        </span>
        <span class="search-cta-kbd" aria-hidden="true">
            {#each overlayHotkeyParts as part, i (i)}
                {#if i > 0}<span class="kbd-plus">+</span>{/if}
                <kbd>{part}</kbd>
            {/each}
        </span>
    </button>

    <!-- ─── Two-column body ──────────────────────────────────────── -->
    <div class="body-grid">
        <!-- Left main: continuity workspace -->
        <section class="continuity-list">
            <header class="section-head">
                <h2 class="section-title">
                    {#if isPinnedView}
                        <Pin class="section-title-ico" />
                        PINNED ITEMS
                    {:else}
                        <Clock3 class="section-title-ico" />
                        RECENTLY USED
                    {/if}
                </h2>
                {#if !isPinnedView}
                    <button
                        type="button"
                        class="section-link"
                        onclick={() => void refreshRecentItems()}
                    >
                        <RefreshCw class="refresh-ico" /> Refresh
                    </button>
                {/if}
            </header>

            {#if isPinnedView}
                {#if pinnedNotes.length > 0 || pinnedClipboardCount > 0}
                    <div class="continuity-group">
                        <div class="group-label">
                            <NotebookPen class="group-label-ico" />
                            <span>Pinned notes</span>
                        </div>
                        {#if pinnedNotes.length > 0}
                            <div class="continuity-rows">
                                {#each pinnedNotes as note (note.path)}
                                    <button
                                        type="button"
                                        class="continuity-row"
                                        onclick={() => void openPinnedNote(note)}
                                        title={`Open ${note.title}`}
                                    >
                                        <span class="continuity-icon note-icon" aria-hidden="true">
                                            <NotebookPen class="continuity-icon-svg" />
                                        </span>
                                        <span class="continuity-copy">
                                            <span class="continuity-name">{note.title}</span>
                                            <span class="continuity-meta">
                                                Note &middot; {relativeTime(note.modifiedMs)}
                                            </span>
                                        </span>
                                        <ArrowUpRight class="continuity-arrow" aria-hidden="true" />
                                    </button>
                                {/each}
                            </div>
                        {:else}
                            <p class="group-empty">No notes are pinned yet.</p>
                        {/if}
                    </div>

                    <button
                        type="button"
                        class="continuity-row pinned-clipboard-row"
                        onclick={openPinnedClipboard}
                        title="Open Clipboard History"
                    >
                        <span class="continuity-icon clipboard-icon" aria-hidden="true">
                            <ClipboardIcon class="continuity-icon-svg" />
                        </span>
                        <span class="continuity-copy">
                            <span class="continuity-name">Pinned clipboard</span>
                            <span class="continuity-meta">
                                {pinnedClipboardCount === 1
                                    ? '1 saved clipboard item'
                                    : `${pinnedClipboardCount} saved clipboard items`}
                            </span>
                        </span>
                        <ArrowUpRight class="continuity-arrow" aria-hidden="true" />
                    </button>
                {:else}
                    <div class="empty-state">
                        <Pin class="empty-state-ico" />
                        <div>
                            <strong>Nothing pinned yet</strong>
                            <p>Pin a note or clipboard item and it will stay here.</p>
                        </div>
                        <button type="button" class="section-link" onclick={() => (selected = 'notes')}>
                            Open Notes
                        </button>
                    </div>
                {/if}
            {:else if recentLoading}
                <div class="empty-state is-loading" aria-live="polite">
                    <RefreshCw class="empty-state-ico" />
                    <div>
                        <strong>Loading recent work</strong>
                        <p>Reading your local activity on this device.</p>
                    </div>
                </div>
            {:else if recentList.length > 0}
                <div class="continuity-rows">
                    {#each recentList as item (`${item.kind}:${item.path}`)}
                        {@const screen = item.kind === 'tool' ? getScreen(item.path) : null}
                        {@const displayName = screen?.name ?? item.displayName}
                        {@const ToolIcon = screen?.icon ?? Command}
                        <button
                            type="button"
                            class="continuity-row"
                            onclick={() => void openRecent(item)}
                            title={`Open ${displayName}`}
                        >
                            <span class="continuity-icon" class:app-icon={item.kind === 'app'} aria-hidden="true">
                                {#if item.kind !== 'tool' && recentEntryIcons[item.path]}
                                    <img
                                        class="continuity-icon-img"
                                        src={recentEntryIcons[item.path]}
                                        alt=""
                                        draggable="false"
                                    />
                                {:else if item.kind === 'app'}
                                    <AppWindow class="continuity-icon-svg" />
                                {:else if item.kind === 'folder'}
                                    <Folder class="continuity-icon-svg" />
                                {:else if item.kind === 'file'}
                                    <FileText class="continuity-icon-svg" />
                                {:else}
                                    <ToolIcon class="continuity-icon-svg" />
                                {/if}
                            </span>
                            <span class="continuity-copy">
                                <span class="continuity-name">{displayName}</span>
                                <span class="continuity-meta">
                                    {recentKindLabel(item.kind)} &middot; {relativeTime(item.lastLaunchedMs)}
                                </span>
                            </span>
                            <ArrowUpRight class="continuity-arrow" aria-hidden="true" />
                        </button>
                    {/each}
                </div>
            {:else}
                <div class="empty-state">
                    <Clock3 class="empty-state-ico" />
                    <div>
                        <strong>Your recent work will appear here.</strong>
                        <p>Open a file, application, folder, or tool to start building your local history.</p>
                    </div>
                    <button type="button" class="section-link" onclick={() => (selected = 'command')}>
                        Open Search
                    </button>
                </div>
            {/if}
        </section>

        <!-- Right sidebar: Local Activity + True Offline First ──── -->
        <aside class="side-col">
            <div class="telemetry-card">
                <header class="telemetry-head">
                    <Sparkles class="telemetry-head-ico" />
                    <!-- "Local Activity", NOT "Local Telemetry" (renamed 2026-07-29).
                         The status bar directly below this panel reads "All local ·
                         No telemetry", and the product's central claim is that we
                         collect none — so heading the Home screen with the word
                         "Telemetry" made the primary surface appear to contradict
                         the promise. The panel reports index/clipboard state, which
                         is activity, not telemetry. Do not rename it back. -->
                    <span>Local Activity</span>
                </header>

                <ul class="stat-list">
                    <li class="stat-row">
                        <span class="stat-label">
                            <Sparkles class="stat-row-ico" />
                            Local Indexing
                        </span>
                        {#if indexingActive}
                            <span class="stat-pill is-good">Active</span>
                        {:else}
                            <span class="stat-pill is-muted">Idle</span>
                        {/if}
                    </li>
                    <li class="stat-row">
                        <span class="stat-label">Indexed Files</span>
                        <!-- Show '—' until the backend snapshot lands; once
                             it does, the tween animates from 0 to the new
                             value over ~800 ms (Phase 7.6). -->
                        <span class="stat-value">
                            {indexedFiles === null ? '—' : fmt(Math.round($indexedFilesTween))}
                        </span>
                    </li>
                    <li class="stat-row">
                        <span class="stat-label">Indexed Content</span>
                        <span class="stat-value">
                            {indexedContent === null ? '—' : fmt(Math.round($indexedContentTween))}
                        </span>
                    </li>
                    <li class="stat-row">
                        <span class="stat-label">Clipboard Items</span>
                        <span class="stat-value">
                            {clipboardItems === null ? '—' : fmt(Math.round($clipboardItemsTween))}
                        </span>
                    </li>
                    <li class="stat-row">
                        <span class="stat-label">Active Tool Packs</span>
                        <span class="stat-value">
                            {Math.round($activeToolPackTween)} installed
                        </span>
                    </li>
                </ul>

                <button
                    type="button"
                    class="manage-row"
                    onclick={() => (selected = 'settings')}
                >
                    <span>Manage Configuration</span>
                    <Command class="manage-ico" />
                </button>
            </div>

            <div class="offline-card">
                <div class="offline-icon" aria-hidden="true">
                    <Lock class="offline-lock" />
                </div>
                <div class="offline-body">
                    <div class="offline-title">True Offline First</div>
                    <p class="offline-desc">
                        Clipboard data and snippets are encrypted in local app storage. Your search
                        index stays on this device.
                    </p>
                </div>
            </div>
        </aside>
    </div>
</div>

<style>
    /* ─── Page container ───────────────────────────────────────── */
    .home-page {
        padding: 32px 40px 56px;
        max-width: 1280px;
        margin: 0 auto;
        display: flex;
        flex-direction: column;
        gap: 28px;
    }

    /* ─── Hero ─────────────────────────────────────────────────── */
    .hero {
        display: flex;
        flex-direction: column;
        gap: 12px;
        max-width: 640px;
    }
    .hero-badge {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        align-self: flex-start;
        padding: 5px 11px;
        font-size: 11.5px;
        font-weight: 600;
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-accent) 35%, var(--color-border));
        border-radius: 999px;
    }
    .home-page :global(.badge-ico) {
        width: 13px;
        height: 13px;
    }
    /* HIERARCHY (2026-07-29): the greeting used to be 32px/700 — more than twice
       the size of the search placeholder (14px) — which made a line that says
       nothing new on the 200th launch the loudest thing on the product's home
       screen. The app's whole promise is "every tool one shortcut away", so the
       SEARCH is the product; the greeting is a courtesy. Demoted to a small
       personal line so the search bar below can be the anchor. */
    .hero-title {
        margin: 0;
        font-size: 20px;
        font-weight: 600;
        letter-spacing: -0.015em;
        color: var(--color-text);
        line-height: 1.25;
    }
    /* The brand-red IT in "Keep IT Local" — matches the TitleBar's
       `.brand-it` rule (logo crimson, not the user accent) so the
       wordmark reads identically in both places. */
    .brand-it {
        color: var(--color-brand-red);
    }
    .hero-sub {
        margin: 0;
        /* 13px, below the search placeholder's 16px: this is supporting copy,
           not a competing focal point. */
        font-size: 13px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }

    /* ─── Search CTA bar ───────────────────────────────────────── */
    /* The home screen's focal point (2026-07-29). Taller than a normal control
       and given a faintly accented edge so it reads as "start here" rather than
       as one more panel — it is the entry point the entire product is built
       around. Hover/focus states below already lift it further. */
    .search-cta {
        display: flex;
        align-items: center;
        gap: 14px;
        width: 100%;
        padding: 20px;
        background: var(--color-panel);
        border: 1px solid color-mix(in srgb, var(--color-accent) 22%, var(--color-border));
        border-radius: 14px;
        cursor: pointer;
        text-align: left;
        transition:
            border-color var(--dur-micro, 130ms) var(--ease-out, ease),
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            transform var(--dur-micro, 130ms) var(--ease-out, ease),
            box-shadow var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .search-cta:hover {
        border-color: color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
        background: var(--color-panel-2);
        transform: translateY(-1px);
        box-shadow: var(--shadow-lg);
    }
    .search-cta:active {
        transform: translateY(0);
    }
    .search-cta:focus-visible {
        outline: none;
        border-color: var(--color-accent);
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 18%, transparent);
    }
    .search-cta-icon {
        width: 36px;
        height: 36px;
        flex-shrink: 0;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border-radius: 10px;
        /* Accent-tinted, not neutral grey: this is the one primary action on the
           screen, and a muted icon made the bar read as a passive panel. */
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
        color: var(--color-accent);
    }
    .home-page :global(.search-ico) {
        width: 16px;
        height: 16px;
    }
    .search-cta-placeholder {
        flex: 1;
        min-width: 0;
        /* Largest type in the hero now (16px vs the greeting's 20px display
           line and the sub's 13px) so the eye lands on the action. */
        font-size: 16px;
        color: var(--color-text-secondary);
    }
    .search-cta-kbd {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        flex-shrink: 0;
    }
    .search-cta-kbd kbd {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 28px;
        height: 26px;
        padding: 0 8px;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 11.5px;
        font-weight: 600;
        color: var(--color-text);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-bottom-width: 2px;
        border-radius: 6px;
    }
    .kbd-plus {
        font-size: 12px;
        color: var(--color-muted);
    }

    /* ─── Two-column body ──────────────────────────────────────── */
    .body-grid {
        display: grid;
        grid-template-columns: 1fr 320px;
        gap: 24px;
        align-items: start;
    }
    @media (max-width: 900px) {
        .body-grid {
            grid-template-columns: 1fr;
        }
    }

    /* ─── Continuity section header ────────────────────────────── */
    .section-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 16px;
        margin-bottom: 14px;
    }
    .section-title {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        margin: 0;
        font-size: 11px;
        font-weight: 700;
        letter-spacing: 0.08em;
        color: var(--color-accent);
        text-transform: uppercase;
    }
    .home-page :global(.section-title-ico) {
        width: 13px;
        height: 13px;
    }
    .section-link {
        background: transparent;
        border: none;
        color: var(--color-accent);
        font-size: 12.5px;
        font-weight: 600;
        cursor: pointer;
        padding: 4px 6px;
        border-radius: 6px;
        transition: background-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .section-link:hover {
        background: color-mix(in srgb, var(--color-accent) 12%, transparent);
    }

    .continuity-list {
        min-width: 0;
    }
    .continuity-rows {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .continuity-group {
        display: flex;
        flex-direction: column;
        gap: 10px;
    }
    .group-label {
        display: inline-flex;
        align-items: center;
        gap: 7px;
        font-size: 12px;
        font-weight: 600;
        color: var(--color-text-secondary);
    }
    .home-page :global(.group-label-ico),
    .home-page :global(.refresh-ico) {
        width: 14px;
        height: 14px;
    }
    .section-link {
        display: inline-flex;
        align-items: center;
        gap: 6px;
    }
    .continuity-row {
        display: flex;
        align-items: center;
        gap: 13px;
        width: 100%;
        min-height: 68px;
        padding: 11px 13px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 12px;
        color: inherit;
        text-align: left;
        cursor: pointer;
        transition:
            border-color var(--dur-micro, 130ms) var(--ease-out, ease),
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            transform var(--dur-micro, 130ms) var(--ease-out, ease),
            box-shadow var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .continuity-row:hover {
        border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
        background: var(--color-panel-2);
        transform: translateY(-1px);
        box-shadow: var(--shadow-lg);
    }
    .continuity-row:active {
        transform: translateY(0);
    }
    .continuity-row:focus-visible {
        outline: none;
        border-color: var(--color-accent);
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 18%, transparent);
    }
    .continuity-icon {
        width: 38px;
        height: 38px;
        flex-shrink: 0;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        overflow: hidden;
        border-radius: 10px;
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
    }
    .continuity-icon.note-icon,
    .continuity-icon.clipboard-icon {
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
        color: var(--color-accent);
    }
    .home-page :global(.continuity-icon-svg),
    .home-page :global(.continuity-arrow) {
        width: 17px;
        height: 17px;
    }
    .continuity-icon-img {
        display: block;
        width: 25px;
        height: 25px;
        object-fit: contain;
    }
    .continuity-copy {
        display: flex;
        flex: 1;
        min-width: 0;
        flex-direction: column;
        gap: 3px;
    }
    .continuity-name {
        overflow: hidden;
        color: var(--color-text);
        font-size: 14px;
        font-weight: 600;
        letter-spacing: -0.008em;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .continuity-meta,
    .group-empty {
        margin: 0;
        overflow: hidden;
        color: var(--color-text-secondary);
        font-size: 12.5px;
        line-height: 1.4;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .home-page :global(.continuity-arrow) {
        flex-shrink: 0;
        color: var(--color-muted);
        transition: color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .continuity-row:hover :global(.continuity-arrow) {
        color: var(--color-accent);
    }
    .pinned-clipboard-row {
        margin-top: 10px;
    }
    .empty-state {
        display: flex;
        align-items: center;
        gap: 14px;
        min-height: 118px;
        padding: 18px;
        background: var(--color-panel);
        border: 1px dashed color-mix(in srgb, var(--color-border) 80%, transparent);
        border-radius: 12px;
        color: var(--color-text-secondary);
    }
    .empty-state strong {
        display: block;
        color: var(--color-text);
        font-size: 14px;
    }
    .empty-state p {
        margin: 3px 0 0;
        font-size: 12.5px;
        line-height: 1.45;
    }
    .empty-state .section-link {
        margin-left: auto;
        flex-shrink: 0;
    }
    .home-page :global(.empty-state-ico) {
        width: 21px;
        height: 21px;
        flex-shrink: 0;
        color: var(--color-accent);
    }
    .empty-state.is-loading :global(.empty-state-ico) {
        animation: recent-spin 1.1s linear infinite;
    }
    @keyframes recent-spin {
        to {
            transform: rotate(360deg);
        }
    }
    @media (max-width: 620px) {
        .empty-state {
            align-items: flex-start;
            flex-wrap: wrap;
        }
        .empty-state .section-link {
            width: 100%;
            margin-left: 35px;
        }
    }

    /* ─── Recent and pinned rows ───────────────────────────────── */

    /* ─── Side column (Telemetry + Offline reassurance) ────────── */
    .side-col {
        display: flex;
        flex-direction: column;
        gap: 14px;
    }

    /* ─── Telemetry card ───────────────────────────────────────── */
    .telemetry-card {
        padding: 18px 18px 6px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: 14px;
    }
    .telemetry-head {
        display: flex;
        align-items: center;
        gap: 8px;
        font-size: 14px;
        font-weight: 600;
        color: var(--color-accent);
        margin-bottom: 14px;
    }
    .home-page :global(.telemetry-head-ico) {
        width: 16px;
        height: 16px;
    }
    .stat-list {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
    }
    .stat-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 11px 0;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 55%, transparent);
        font-size: 13px;
    }
    .stat-row:first-child {
        border-top: none;
        padding-top: 4px;
    }
    .stat-label {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        color: var(--color-text-secondary);
    }
    .home-page :global(.stat-row-ico) {
        width: 13px;
        height: 13px;
        opacity: 0.8;
    }
    .stat-value {
        font-weight: 600;
        color: var(--color-text);
        font-variant-numeric: tabular-nums;
    }
    .stat-pill {
        display: inline-flex;
        align-items: center;
        padding: 3px 9px;
        font-size: 11px;
        font-weight: 600;
        border-radius: 999px;
        border: 1px solid transparent;
    }
    .stat-pill.is-good {
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
        border-color: color-mix(in srgb, var(--color-accent) 35%, var(--color-border));
    }
    .stat-pill.is-muted {
        color: var(--color-muted);
        background: var(--color-panel-2);
        border-color: var(--color-border);
    }

    .manage-row {
        margin-top: 10px;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        width: calc(100% + 36px);
        margin-left: -18px;
        margin-right: -18px;
        margin-bottom: -6px;
        padding: 12px 18px;
        background: transparent;
        border: none;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 55%, transparent);
        color: var(--color-text-secondary);
        font-size: 13px;
        cursor: pointer;
        border-radius: 0 0 14px 14px;
        transition: background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .manage-row:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .home-page :global(.manage-ico) {
        width: 14px;
        height: 14px;
    }

    /* ─── True Offline First reassurance ───────────────────────── */
    .offline-card {
        display: flex;
        align-items: flex-start;
        gap: 12px;
        padding: 14px 16px;
        background: var(--color-panel);
        border: 1px solid color-mix(in srgb, var(--color-accent) 35%, var(--color-border));
        border-left-width: 3px;
        border-radius: 12px;
    }
    .offline-icon {
        width: 30px;
        height: 30px;
        flex-shrink: 0;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border-radius: 8px;
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
        color: var(--color-accent);
    }
    .home-page :global(.offline-lock) {
        width: 14px;
        height: 14px;
    }
    .offline-title {
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
    }
    .offline-desc {
        margin: 4px 0 0;
        font-size: 12px;
        line-height: 1.45;
        color: var(--color-text-secondary);
    }
</style>
