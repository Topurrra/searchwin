<script lang="ts">
    import { onMount } from 'svelte';
    import { listen } from '@tauri-apps/api/event';
    import { invoke } from '@tauri-apps/api/core';
    import { _ } from 'svelte-i18n';
    import { t } from '$lib/i18n';
    import Sidebar from '$lib/Sidebar.svelte';
    import TopBar from '$lib/TopBar.svelte';
    import StatusBar from '$lib/StatusBar.svelte';
    import TitleBar from '$lib/TitleBar.svelte';
    import ToastContainer from '$lib/ToastContainer.svelte';
    import Skeleton from '$lib/Skeleton.svelte';
    // Wave 7.9 (2026-05-28): WelcomeSetup is no longer imported here —
    // it now lives exclusively in the dedicated /welcome route, rendered
    // in its own Tauri window. Main no longer carries the onboarding
    // branch in its template (and so doesn't need to ship the welcome
    // bundle in the main webview's initial parse cost either).
    import TourModal from '$lib/components/TourModal.svelte';
    import ToolErrorBoundary from '$lib/components/ToolErrorBoundary.svelte';
    import VoiceCommandFeedback from '$lib/components/VoiceCommandFeedback.svelte';
    import { toast } from '$lib/stores/toasts';
    import { pageScreens, getScreen, categoryScreenIdForTool } from '$lib/appScreens';
    import { categoryActiveTool, commandActiveTab, commandTabForId } from '$lib/stores/categoryNav';
    import { initOnboardingStore, onboardingLoaded, onboardingState } from '$lib/stores/onboarding';
    import { installedScreens, installedTools } from '$lib/stores/toolPacks';
    import { fileSearchMode, fileSearchQuery, ensureFirstRunDiskIndex } from '$lib/stores/fileSearch';
    import { requestSettingsSection, type SettingsSectionId } from '$lib/stores/settingsTarget';
    import { pendingNavigation } from '$lib/stores/pendingNavigation';
    import { stageToolLaunchTarget } from '$lib/stores/toolLaunchTarget';
    import { startReminderScheduler } from '$lib/stores/reminders';
    import { startTimeFocusBridge } from '$lib/stores/timeFocusBridge';
    import { initNotesStore, openNote } from '$lib/stores/notes';
    import { commandPaletteOpen, closeCommandPalette } from '$lib/stores/commandPalette';
    import PaletteV2 from './command/PaletteV2.svelte';
    import { get } from 'svelte/store';

    type NavigateToolPayload = {
        toolId?: string;
        searchQuery?: string;
        searchMode?: 'files' | 'content';
        /** Only meaningful when toolId is 'settings' — deep-links to a
         *  specific Settings section (from the command palette's Settings
         *  search results). */
        settingsSection?: string;
        /** Wave K (2026-05-28): only meaningful when toolId is 'notes'.
         *  Opens this specific `.ki` note in the Notes editor — fired
         *  when the user activates a note row in the command palette
         *  (Enter / click). Without it the palette would route to the
         *  Windows file-association (no associated app for `.ki` →
         *  silent no-op), so we route to the Notes tool here instead. */
        notePath?: string;
        /** Optional file staged by a palette action for the destination tool. */
        targetFile?: string;
    };

    // Default landing is Home, the new workspace screen. About is still
    // reachable via the TopBar Info button + the screen registry.
    let selected = $state('home');
    let currentComponent = $state<any>(null);
    let currentScreenId = $state('');
    let loading = $state(false);
    let loadError = $state<string | null>(null);
    let normalUiReady = $derived($onboardingLoaded && $onboardingState.welcomeCompleted);

    const componentCache = new Map<string, any>();
    // Cache holds component module references (not instances), so each
    // entry costs almost nothing. Bumped to 100 (from 10) so the
    // on-idle prefetch loop in the $effect below can populate the
    // ENTIRE installed-screen set up front — subsequent navigation
    // is a cache hit, never an async import, and feels instant.
    const MAX_CACHED_SCREENS = 100;
    // Set once after mount + UI ready so the prefetch effect fires
    // exactly once per session, not on every store update.
    let hasPrefetchedScreens = $state(false);

    function getInstalledScreen(screenId: string) {
        return $installedScreens.find((screen) => screen.id === screenId);
    }

    function hasInstalledScreen(screenId: string) {
        return Boolean(getInstalledScreen(screenId));
    }

    /** True for ids that navigation can act on: a standalone installed
     *  screen (pages, core file-search, category workspaces) OR a pack tool
     *  that maps to a category workspace. Pack tools are no longer their own
     *  installed screens — they live inside `category-<pack>` — so a bare
     *  `hasInstalledScreen` check wrongly dropped them (command-palette tool
     *  matches did nothing). The redirect $effect below resolves the rest:
     *  enabled pack → open the category; disabled pack → "enable it" prompt. */
    function isNavigableTool(id: string) {
        return hasInstalledScreen(id) || categoryScreenIdForTool(id) !== null;
    }

    function pruneCache(nextKeep: string | null = null) {
        if (componentCache.size <= MAX_CACHED_SCREENS) return;

        const toKeep = new Set<string>();
        if (nextKeep) {
            toKeep.add(nextKeep);
        }
        if (currentScreenId) {
            toKeep.add(currentScreenId);
        }

        for (const key of componentCache.keys()) {
            if (componentCache.size <= MAX_CACHED_SCREENS) break;
            if (!toKeep.has(key)) {
                componentCache.delete(key);
            }
        }
    }

    async function loadSelectedScreen(screenId: string) {
        const screen = getInstalledScreen(screenId);
        if (!screen) {
            currentComponent = null;
            loadError = t('errors.unknownScreen', { id: screenId });
            return;
        }

        currentScreenId = screenId;
        loadError = null;

        if (componentCache.has(screenId)) {
            currentComponent = componentCache.get(screenId) ?? null;
            loading = false;
            return;
        }

        loading = true;
        currentComponent = null;

        try {
            const module = await screen.loader();
            const component = module.default;
            componentCache.set(screenId, component);
            pruneCache(screenId);

            if (currentScreenId === screenId) {
                currentComponent = component;
            }
        } catch (error) {
            if (currentScreenId === screenId) {
                loadError = String(error);
            }
        } finally {
            if (currentScreenId === screenId) {
                loading = false;
            }
        }
    }

    function fallbackSelection() {
        return $installedTools[0]?.id ?? pageScreens[0]?.id ?? 'about';
    }

    $effect(() => {
        if (!normalUiReady) return;

        // Search / Clipboard / Voice / Snippets are merged into the single
        // 'command' page (tabs). Redirect those legacy ids there, stashing
        // which tab to open. MUST run before the category redirect below —
        // file-search is a File-category tool, so otherwise it'd be sent to
        // the File category workspace instead of the Command page's Search tab.
        const cmdTab = commandTabForId(selected);
        if (cmdTab) {
            commandActiveTab.set(cmdTab);
            selected = 'command';
            return;
        }

        // Pack tools live inside their Category Workspace now. Any direct
        // tool navigation — sidebar, command palette, deep link, frecency,
        // "open in main" — is redirected to the tool's `category-<pack>`
        // page, stashing which tool to activate. This MUST run before the
        // installed-screen gate below: an individual pack tool is no longer
        // its own installed screen (only the category page is), so the gate
        // would otherwise treat an enabled pack tool as "not installed" and
        // bounce it to Settings. Core tools (file-search) and single-tool
        // packs (Automation) return null here and render directly.
        const categoryId = categoryScreenIdForTool(selected);
        if (categoryId && hasInstalledScreen(categoryId)) {
            categoryActiveTool.set(selected);
            selected = categoryId;
            return;
        }

        if (!hasInstalledScreen(selected)) {
            // The screen the user asked for isn't in their installed pack
            // set (e.g. clicked "Automations" in the sidebar but the
            // Automation pack is disabled, or a pack tool whose category
            // isn't installed). Route the user to Settings → Tool Packs so
            // they can enable it and restart, and toast why. Without this
            // the user silently lands on File Search and wonders why their
            // click went to the wrong page.
            const wantedScreen = getScreen(selected);
            const wantedName = wantedScreen?.name ?? selected;
            if (selected !== 'settings' && selected !== 'about' && wantedScreen) {
                requestSettingsSection('toolpacks');
                toast(
                    `${wantedName} needs its Tool Pack enabled. Open the pack, save, then restart KeepItLocal.`,
                    'info',
                    6000,
                );
                selected = 'settings';
                return;
            }
            // Either the id is unknown entirely, or the user landed on
            // 'settings' / 'about' which aren't pack-gated — fall back
            // to first installed tool as before.
            selected = fallbackSelection();
            return;
        }

        void loadSelectedScreen(selected);
    });

    /* ─── Splash → main handoff (readiness-driven) ───────────────────────
       The static splash (static/splash.html) has no JS, so the MAIN window
       signals when it's actually painted: the splash closes + main shows at
       that exact moment (machine-independent), instead of after a guessed
       fixed delay that could reveal a half-loaded main (the "needs a little
       more time to load" regression). `set_ready` needs BOTH flags before it
       closes the splash; we set both here (the proven pattern from the old
       SvelteKit splash route, which had JS). A Rust-side timer (lib.rs setup)
       is the safety fallback if this never fires (e.g. a first-screen load
       error), so the app can never get stuck on the splash.

       Wave 7.9 (2026-05-28): on first-run, welcome lives in its own Tauri
       window (label "welcome") and owns this handshake — its /welcome
       +page calls set_ready when its first paint lands, and the Rust
       set_ready handler prefers showing welcome over main whenever
       welcome exists. So on first-run, main stays hidden, set_ready is
       NEVER called from this route, and welcome owns the splash close.
       Returning users (welcomeCompleted = true) still drive the handshake
       from here exactly like before. */
    let splashReadySignaled = false;
    $effect(() => {
        if (splashReadySignaled) return;
        // Wait until onboarding state is known so we don't accidentally
        // signal ready from main when welcome is going to own the
        // handshake.
        if (!$onboardingLoaded) return;
        // First-run: welcome window will handle set_ready. Mark our
        // signal as done so we don't re-evaluate every time the
        // onboarding store updates (e.g. after welcome_finished fires
        // and main becomes visible — splash is long gone by then).
        if (!$onboardingState.welcomeCompleted) {
            splashReadySignaled = true;
            return;
        }
        // Returning-user path: first meaningful paint is ready when the
        // default screen's component has finished loading, or it
        // errored (the error boundary paints).
        const firstPaintReady = currentComponent !== null || loadError !== null;
        if (!firstPaintReady) return;
        splashReadySignaled = true;

        // One rAF so the browser has a chance to paint the first tool
        // screen at the configured size before the splash close
        // reveals the main window.
        requestAnimationFrame(() => {
            void invoke('set_ready', { task: 'frontend' });
            void invoke('set_ready', { task: 'backend' });
        });
    });

    /* ─── Idle-time prefetch: warm every installed screen's module
       so subsequent navigation is instant ────────────────────────
       The dynamic imports backing each screen are bundle-split for
       startup speed — without prefetch, the FIRST click on any
       screen pays the import cost (~50-300 ms depending on chunk
       size). That's the "needs some time to load" the user sees.

       This effect waits until the visible UI is up (`normalUiReady`)
       and the installed-screens registry is populated, then walks
       every screen and pulls its module into `componentCache` on
       idle. Each load yields to the event loop after completion so
       the prefetch never blocks user input.

       `hasPrefetchedScreens` flag ensures this runs once per session,
       not every time the installed-screens store updates. */
    $effect(() => {
        if (!normalUiReady) return;
        if (hasPrefetchedScreens) return;
        if ($installedScreens.length === 0) return;
        hasPrefetchedScreens = true;

        const screensSnapshot = [...$installedScreens];
        const runPrefetch = () => {
            void (async () => {
                for (const screen of screensSnapshot) {
                    if (componentCache.has(screen.id)) continue;
                    try {
                        const module = await screen.loader();
                        componentCache.set(screen.id, module.default);
                    } catch {
                        // Silent — if the user later navigates here,
                        // the regular load path retries. Logging
                        // would just be noise during idle prefetch.
                    }
                    // Yield between loads so this never holds the
                    // main thread long enough to delay input.
                    await new Promise<void>((resolve) => setTimeout(resolve, 0));
                }
            })();
        };
        // `requestIdleCallback` is the preferred scheduler — runs the
        // work only when the browser would otherwise be idle. WebView2
        // supports it; the setTimeout fallback covers the rare case
        // where it doesn't.
        const ric = (window as any).requestIdleCallback;
        if (typeof ric === 'function') {
            ric(runPrefetch, { timeout: 1500 });
        } else {
            setTimeout(runPrefetch, 300);
        }
    });

    // (2026-07-18) The Ctrl+Shift+D V1/V2 swap hook and its localStorage flag
    // were removed along with V1 — exactly as their own comment said they
    // would be "once a design is chosen". V2 is now the only palette, so
    // there is nothing left to swap between.

    onMount(() => {
        let unlisten: (() => void) | null = null;
        let unlistenPasteFallback: (() => void) | null = null;
        let unlistenKitStateRefresh: (() => void) | null = null;
        void initOnboardingStore();

        // Wave 7.9 (2026-05-28): cross-window store re-sync.
        //
        // When the user finishes the welcome flow, the welcome window's
        // `enterApp` invokes the `welcome_finished` Rust command, which
        // emits `kit-state-refresh` BEFORE revealing main. Main has been
        // mounted in the background with the pre-welcome store state
        // (welcomeCompleted = false, default packs), so we re-read from
        // the Rust backend here — that way the first paint the user
        // sees in main reflects any pack toggles + the just-flipped
        // onboarding flag. Settings auto-syncs through its existing
        // `settings-updated` broadcast and doesn't need wiring here.
        void (async () => {
            try {
                const { reloadOnboardingStore } = await import('$lib/stores/onboarding');
                const { reloadToolPacksStore } = await import('$lib/stores/toolPacks');
                unlistenKitStateRefresh = await listen('kit-state-refresh', () => {
                    void reloadOnboardingStore();
                    void reloadToolPacksStore();
                });
            } catch (error) {
                console.warn('Could not subscribe to kit-state-refresh:', error);
            }
        })();
        // Start the reminders due-check loop app-wide, so a reminder set in a
        // previous session fires even if the user never opens the tool.
        startReminderScheduler();
        // Bridge so the command palette can start/stop timers + create
        // reminders in this (main) window.
        void startTimeFocusBridge();
        // Index existing notes app-wide so they're findable in search even if
        // the user never opens the Notes tool this session. Guarded by an
        // `initialized` flag — opening Notes later just refreshes the list.
        void initNotesStore();
        // First launch only: silently seed + build the disk index in the
        // background (filenames = all drives, content = Desktop/Downloads/
        // Documents, watch-root on). Idempotent — no-ops after the first run.
        void ensureFirstRunDiskIndex();

        // One-shot tool navigation handoff from the embedded /command
        // palette (it can't use the cross-window navigate-tool event
        // because the main route isn't mounted while /command occupies
        // the webview). Read + clear immediately. Mirrors the navigate-
        // tool handler below.
        const pending = get(pendingNavigation);
        if (pending) {
            pendingNavigation.set(null);
            if (isNavigableTool(pending.toolId)) {
                if (pending.toolId === 'file-search') {
                    if (pending.searchMode === 'files' || pending.searchMode === 'content') {
                        fileSearchMode.set(pending.searchMode);
                    }
                    if (pending.searchQuery) {
                        fileSearchQuery.set(pending.searchQuery);
                    }
                }
                // Settings deep-link (embedded-palette path).
                if (pending.toolId === 'settings' && pending.settingsSection) {
                    requestSettingsSection(pending.settingsSection as SettingsSectionId);
                }
                // Wave K (2026-05-28): the embedded-palette deep-link for
                // opening a specific note. Kick off the read BEFORE
                // selected = 'notes' so the Notes component sees the
                // freshly-set activeNote on mount instead of auto-opening
                // the first note in the list. The read is fire-and-forget
                // since we don't want to delay the route switch — by the
                // time Notes.svelte mounts the activeNote store update
                // has already landed (or arrives moments later, which
                // Notes' reactive UI picks up automatically).
                if (pending.toolId === 'notes' && pending.notePath) {
                    void openNote(pending.notePath);
                }
                if (pending.targetFile) {
                    stageToolLaunchTarget({ toolId: pending.toolId, targetFile: pending.targetFile });
                }
                selected = pending.toolId;
            }
        }
        void (async () => {
            unlisten = await listen<NavigateToolPayload>('navigate-tool', (event) => {
                const toolId = event.payload?.toolId?.trim();
                if (toolId && isNavigableTool(toolId)) {
                    if (toolId === 'file-search') {
                        const searchQuery = event.payload?.searchQuery?.trim();
                        const searchMode = event.payload?.searchMode;
                        if (searchMode === 'files' || searchMode === 'content') {
                            fileSearchMode.set(searchMode);
                        }
                        if (searchQuery) {
                            fileSearchQuery.set(searchQuery);
                        }
                    }
                    // Settings deep-link from the palette's Settings search.
                    if (toolId === 'settings' && event.payload?.settingsSection) {
                        requestSettingsSection(event.payload.settingsSection as SettingsSectionId);
                    }
                    // Wave K (2026-05-28): Notes deep-link from the palette
                    // (Enter / click on a `.ki` row). Mirror of the
                    // pending-navigation branch above — see that comment
                    // for the timing rationale.
                    if (toolId === 'notes' && event.payload?.notePath) {
                        void openNote(event.payload.notePath);
                    }
                    if (event.payload?.targetFile) {
                        stageToolLaunchTarget({ toolId, targetFile: event.payload.targetFile });
                    }
                    selected = toolId;
                }
            });
        })();

        // Auto-paste fallback toast — the clipboard backend emits this when
        // SetForegroundWindow or SendInput is refused (Win32 focus-stealing
        // prevention, AV-installed keyboard hooks, target window closed, etc).
        // In every failure case the text is still on the clipboard, so the
        // toast just nudges the user to Ctrl+V manually. Longer duration than
        // a normal info toast since the user typically needs to switch focus
        // to the target window before acting on the message.
        void (async () => {
            unlistenPasteFallback = await listen<string>('clipboard-paste-fallback', (event) => {
                const message =
                    typeof event.payload === 'string' && event.payload.length > 0
                        ? event.payload
                        : t('errors.pasteFallback');
                toast(message, 'info', 5000);
            });
        })();

        return () => {
            if (unlisten) {
                unlisten();
            }
            if (unlistenPasteFallback) {
                unlistenPasteFallback();
            }
            if (unlistenKitStateRefresh) {
                unlistenKitStateRefresh();
            }
        };
    });
</script>

{#if !$onboardingLoaded}
    <div class="flex h-screen w-screen items-center justify-center bg-bg text-text">
        <div class="space-y-3 text-center">
            <div class="mx-auto h-8 w-8 animate-spin rounded-full border-2 border-emerald-400 border-t-transparent"></div>
            <div class="text-sm text-muted">{$_('errors.preparing')}</div>
        </div>
    </div>
{:else}
    <div class="flex flex-col h-screen w-screen bg-bg text-text">
        <!-- Custom window chrome — replaces the OS title bar (disabled in
             tauri.conf.json). Mounted ONLY here in the main route so the
             overlays + splashscreen (which run with their own minimal
             borderless config) don't accidentally inherit a title bar. -->
        <TitleBar />
        <TopBar bind:selected />
        <!-- First-run product tour. Renders nothing once the user dismisses it
             (controlled by settings), so leaving it permanently mounted in the
             shell is free. Replayable from Settings → Onboarding & Tips. -->
        <TourModal />
        <div class="flex flex-1 overflow-hidden">
            <Sidebar bind:selected />
            <main class="flex-1 overflow-y-auto">
                {#key selected}
                    {@const activeScreen = getInstalledScreen(selected)}
                    <!-- `fade-in` removed in Phase 3.5.1 — paired with the
                         idle-prefetch of all installed screens above, page
                         navigation is now a cache hit + instant DOM swap.
                         Adding a 190 ms fade-in on every page change made
                         transitions feel slower than they actually were.
                         The `data-screen` / `data-screen-kind` attrs stay
                         (consumed by global styles in styles.css). -->
                    <!-- Category Workspace pages render real tools inside,
                         so they report `tool` kind to inherit the tool-host
                         global styling (input radius, focus rings, etc.). -->
                    <div class="tool-host" data-screen={selected} data-screen-kind={selected?.startsWith('category-') ? 'tool' : (activeScreen?.kind ?? 'page')}>
                        {#if loadError}
                            <div class="p-6 max-w-3xl">
                                <div class="rounded border border-red-500/30 bg-red-500/10 p-4 text-sm text-red-300">
                                    {loadError}
                                </div>
                            </div>
                        {:else if loading || currentScreenId !== selected}
                            <!-- `currentScreenId !== selected` guards the frame
                                 right after a tool switch: `selected` (and so
                                 `activeScreen`/`acceptsSelected`) updates instantly,
                                 but `currentComponent` still holds the PREVIOUS
                                 screen until the load effect runs. Rendering the
                                 stale component through the new screen's
                                 acceptsSelected branch crashed bindings (e.g.
                                 CommandWorkspace rendered without `bind:selected`
                                 → its FileSearch got `selected={undefined}`). Show
                                 the skeleton until the loaded component matches. -->
                            <div class="p-6 max-w-4xl space-y-4">
                                <Skeleton height="2rem" width="14rem" />
                                <Skeleton height="1rem" width="28rem" />
                                <div class="rounded border border-border bg-panel p-4 space-y-3">
                                    <Skeleton height="1rem" />
                                    <Skeleton height="1rem" />
                                    <Skeleton height="10rem" />
                                </div>
                            </div>
                        {:else if currentComponent}
                            {@const ScreenComponent = currentComponent}
                            <!-- Per-tool error boundary. Lives inside {#key selected}
                                 so every tool switch gets a fresh boundary; a
                                 render-time crash in one tool falls back to the
                                 inline ToolErrorBoundary instead of taking down
                                 the whole app shell. -->
                            <svelte:boundary>
                                {#snippet failed(error, reset)}
                                    <ToolErrorBoundary
                                        {error}
                                        {reset}
                                        screenId={selected}
                                        screenName={activeScreen?.name}
                                    />
                                {/snippet}
                                {#if activeScreen?.acceptsSelected}
                                    <ScreenComponent bind:selected />
                                {:else}
                                    <ScreenComponent />
                                {/if}
                            </svelte:boundary>
                        {/if}
                    </div>
                {/key}
            </main>
        </div>
        <StatusBar />
    </div>
    <!-- Voice command feedback + the #14 dangerous-command confirmation
         prompt. A self-hiding floating card; command mode (which has no
         surface of its own) reports here. Main window only. -->
    <VoiceCommandFeedback />

    <!-- Command palette OVERLAY (Phase 3.6.5 test binding). Rendered on
         top of the live workspace so the workspace stays visible behind
         the translucent scrim — no black void. Mounted only while open
         so its voice subscriptions / listeners tear down on close.
         Replaced by a dedicated transparent Tauri overlay window in
         Phase 3.6.6. -->
    {#if $commandPaletteOpen}
        <PaletteV2 asOverlay onClose={closeCommandPalette} />
    {/if}
{/if}

<ToastContainer />
