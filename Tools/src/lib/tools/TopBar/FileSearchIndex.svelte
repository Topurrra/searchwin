<script lang="ts">
    import { onMount, untrack } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { open } from '@tauri-apps/plugin-dialog';
    import {
        Activity,
        Clock,
        Database,
        File,
        FileText,
        FolderOpen,
        PauseCircle,
        Play,
        RotateCcw,
        Search,
        Settings2,
        ShieldCheck,
        Sparkles,
        X,
        XCircle,
    } from '@lucide/svelte';

    import { toast } from '$lib/stores/toasts';
    import { _ } from 'svelte-i18n';
    import { t } from '$lib/i18n';
    import {
        addFileSearchExcludeExtensions,
        addFileSearchExcludeFolders,
        addFileSearchFilenameRoots,
        addFileSearchRoots,
        cancelFileSearchIndexBuild,
        clearFileSearchFilenameRoots,
        clearFileSearchRoots,
        fileSearchCancelling,
        fileSearchCommitEvery,
        fileSearchExcludeExtensions,
        fileSearchExcludeFolders,
        fileSearchFilenameIndexMessage,
        fileSearchFilenameRoots,
        fileSearchIncludeHidden,
        fileSearchContentIndexingEnabled,
        setFileSearchContentIndexingEnabled,
        // Semantic search (beta, 2026-07-01): embedding-based retrieval opt-in.
        fileSearchSemanticEnabled,
        setFileSearchSemanticEnabled,
        fileSearchIndexing,
        fileSearchLastMessage,
        fileSearchLastScheduledRebuildAtMs,
        fileSearchMaxContentKb,
        fileSearchPerformanceMode,
        fileSearchProgress,
        fileSearchRebuildIntervalHours,
        fileSearchRebuildScheduleEnabled,
        fileSearchRoots,
        fileSearchStatus,
        fileSearchWatcherEnabled,
        formatBytes,
        initFileSearchStore,
        refreshFileSearchStatus,
        removeFileSearchExcludeExtension,
        removeFileSearchExcludeFolder,
        removeFileSearchFilenameRoot,
        removeFileSearchRoot,
        resetFileSearchExclusions,
        saveFileSearchIndexOptions,
        saveFileSearchRebuildSchedule,
        startContentSearchIndex,
        startFilenameSearchIndex,
        stopFileSearchWatcher,
        // Wave 8 / task #88 (2026-05-28): OCR-on-Index opt-in.
        fileSearchOcrEnabled,
        fileSearchOcrFolders,
        fileSearchOcrMaxPagesPerFile,
        setFileSearchOcrEnabled,
        addFileSearchOcrFolder,
        removeFileSearchOcrFolder,
        setFileSearchOcrMaxPages,
        // Wave 8.2 (2026-05-28): smart-skip heuristic knobs.
        fileSearchOcrMinImageDim,
        fileSearchOcrMaxAspectRatio,
        fileSearchOcrPerFileTimeoutSecs,
        setFileSearchOcrMinImageDim,
        setFileSearchOcrMaxAspectRatio,
        setFileSearchOcrTimeoutSecs,
    } from '$lib/stores/fileSearch';

    let {
        selected = $bindable(),
        /** When set, the page shows ONLY this index (filename or content) and
         *  hides the File/Content tab toggle — used by the standalone File Index
         *  and Content Index screens. Unset = the combined page (both tabs),
         *  still used where the page is embedded (e.g. Settings). */
        fixedTab = undefined,
    }: { selected: string; fixedTab?: 'file' | 'content' } = $props();

    // Which settings tab is active. Each tab scopes a distinct backend index:
    // 'file'    -> filename/path index (engine selector lives here)
    // 'content' -> full-text content index
    // `fixedTab` is a constant per page instance (the File/Content wrappers each
    // pass a literal), so capturing its initial value here is exactly right.
    // svelte-ignore state_referenced_locally
    let activeTab = $state<'file' | 'content'>(fixedTab ?? 'file');

    let excludeFolderDraft = $state('');
    let excludeExtensionDraft = $state('');

    // Sprint 1 — Task #64 (2026-05-27): "0 / 0 / 0" on first-run.
    // When the build is in progress but the worker hasn't written its
    // first status file yet (typical 1–3 s window on app start), all
    // three counters are zero AND there's no currentPath. Rendering
    // "0 / 0 / 0" reads as broken; rendering a "Starting…" message
    // reads as "the indexer is spinning up". As soon as ANY counter
    // becomes non-zero, we switch to the live numbers.
    let progressIsStarting = $derived.by(() => {
        if (!$fileSearchProgress || !$fileSearchIndexing) return false;
        const p = $fileSearchProgress;
        return (
            !p.currentPath &&
            (p.indexedFiles ?? 0) === 0 &&
            (p.scannedEntries ?? 0) === 0 &&
            (p.skippedFiles ?? 0) === 0
        );
    });

    // Wave 5 (2026-05-28) — UxAudit UX-A-04: first-run hero state.
    // When the user lands on this page for the first time there is no
    // index yet, no folders configured, and no build in flight — the
    // power-user surface (engine selector, watcher modes, advanced
    // sidesheet) makes no sense to them. Detect that combined empty
    // state per tab so we can swap the body for a friendly hero card
    // that explains WHY indexing matters and offers a single "Pick a
    // folder" CTA. As soon as a folder is added OR an index exists,
    // the normal power-user UI returns.
    let isFirstRunForFile = $derived.by(() => {
        return (
            $fileSearchFilenameRoots.length === 0 &&
            ($fileSearchStatus?.filenameIndexedFiles ?? 0) === 0 &&
            !$fileSearchIndexing
        );
    });
    let isFirstRunForContent = $derived.by(() => {
        return (
            $fileSearchRoots.length === 0 &&
            ($fileSearchStatus?.indexedFiles ?? 0) === 0 &&
            !$fileSearchIndexing
        );
    });

    // Wave 5 (2026-05-28) — UxAudit UX-A-05: current-activity copy.
    // The previous "Indexing in progress" + 3 raw counters told the
    // user nothing about WHAT was happening. The backend reports the
    // active path it's reading; surface just the folder name so the
    // user sees "Reading: Documents" without the full path noise.
    function shortFolderName(path: string | null | undefined): string {
        if (!path) return '';
        const cleaned = path.replace(/[\\/]+$/, '');
        const idx = Math.max(cleaned.lastIndexOf('/'), cleaned.lastIndexOf('\\'));
        const last = idx >= 0 ? cleaned.slice(idx + 1) : cleaned;
        return last || cleaned;
    }

    // Wave 5.5 (2026-05-28): true determinate progress.
    //
    // Cheapest path to a real % + ETA without a backend pre-walk pass
    // (which would add 1-3s to every build start): use the backend's
    // already-emitted `scannedEntries` as a live total estimate.
    //
    // How `scannedEntries` relates to "total":
    //   - The walker reports every path it sees, indexed or skipped.
    //   - Late in the build, scanning plateaus and `scannedEntries`
    //     becomes the actual final count.
    //   - During the walk, it grows alongside `indexedFiles`. The
    //     ratio `indexed / scanned` stays roughly stable for filename
    //     indexing (one-pass walk + index) and grows monotonically
    //     for content indexing (walker discovers, indexer trails).
    //
    // To avoid the bar dipping when the walker discovers new files
    // faster than the indexer commits, we track a "stable total"
    // that ratchets upward — once we've seen a higher count we
    // never go back below it. The bar fill is `indexed / stableTotal`,
    // clamped [0, 1].
    //
    // ETA: a 6-sample rolling window of `(timestamp, indexedFiles)`
    // gives us files/sec; remaining = `stableTotal - indexed`; ETA
    // shows only when rate > 0.5 files/sec AND the stable total has
    // settled for at least 1.5 s (so the early "files/sec spike" from
    // worker startup doesn't surface a misleading "12 hours remaining").

    type ProgressSample = { ms: number; indexed: number };
    let progressSamples = $state<ProgressSample[]>([]);
    let progressStableTotal = $state(0);
    let progressStableTotalSetAtMs = $state(0);
    let lastBuildKey = $state(''); // resets the throughput window per build
    /** Wave 7.7 (2026-05-28): peak fraction — the maximum `indexed /
     *  stableTotal` we've seen during the current build. Used by the
     *  bar so it never decreases. Bug it fixes: during a rebuild on
     *  an already-indexed corpus, the walker visits every file but
     *  only RE-indexes the ones that changed. So `scannedEntries`
     *  grows fast while `indexedFiles` grows slowly (or barely at
     *  all), making the raw ratio fall as the walk progresses.
     *  Ratcheting the peak hides that artifact — the bar can only
     *  ever move forward. Resets to 0 on build end / new build. */
    let peakFraction = $state(0);

    // Wave 7.6 bug fix (2026-05-28): the original effect read AND wrote
    // to `progressSamples` / `progressStableTotal` / `lastBuildKey`,
    // which Svelte 5 treats as a dependency-loop trigger — the write
    // would re-fire the effect, causing `effect_update_depth_exceeded`
    // the moment a build started. Fix: the effect only TRACKS the
    // upstream signals (`$fileSearchProgress`, `$fileSearchIndexing`),
    // and runs the entire mutating body inside `untrack()` so our own
    // writes don't subscribe us to our own state. Deriveds downstream
    // still react to those writes via their normal signal propagation —
    // the only thing we suppress is the EFFECT's self-trigger.
    $effect(() => {
        const p = $fileSearchProgress;
        const indexing = $fileSearchIndexing;
        untrack(() => {
            if (!p || !indexing) {
                if (progressSamples.length > 0) {
                    progressSamples = [];
                    progressStableTotal = 0;
                    progressStableTotalSetAtMs = 0;
                }
                // Always reset peak when not indexing — independent of
                // the samples-length gate above, so a build->idle->build
                // cycle starts fresh even if samples were already empty.
                if (peakFraction !== 0) peakFraction = 0;
                return;
            }
            // New build → reset (detected by an indexedFiles drop or a
            // kind switch).
            const buildKey = `${p.indexKind}:${p.stage}`;
            if (
                buildKey !== lastBuildKey ||
                (progressSamples.length > 0 &&
                    p.indexedFiles < progressSamples.at(-1)!.indexed)
            ) {
                lastBuildKey = buildKey;
                progressSamples = [];
                progressStableTotal = 0;
                progressStableTotalSetAtMs = 0;
                peakFraction = 0;
            }
            const now = performance.now();
            progressSamples = [
                ...progressSamples,
                { ms: now, indexed: p.indexedFiles },
            ].slice(-6);
            // Ratchet the total upward — combined `indexed + skipped` is
            // the floor (every reported file must be one or the other);
            // the walker's `scanned` is the ceiling-so-far. We pick the
            // LARGER so the bar never overshoots, and never lower it
            // (avoid jitter).
            const observedTotal = Math.max(
                p.indexedFiles + p.skippedFiles,
                p.scannedEntries,
            );
            if (observedTotal > progressStableTotal) {
                progressStableTotal = observedTotal;
                progressStableTotalSetAtMs = now;
            }
            // Wave 7.7 (2026-05-28): track the peak raw fraction so the
            // bar never decreases. See the comment on `peakFraction` for
            // the rebuild-with-mostly-unchanged-files scenario this
            // fixes.
            if (progressStableTotal > 0) {
                const raw = Math.max(
                    0,
                    Math.min(1, p.indexedFiles / progressStableTotal),
                );
                if (raw > peakFraction) peakFraction = raw;
            }
        });
    });

    /** 0–1 fill ratio for the determinate bar. Returns null while the
     *  build hasn't yielded enough signal to plot anything useful.
     *  Wave 7.7 (2026-05-28): now reads `peakFraction` (monotonic
     *  ratchet) instead of computing the raw ratio inline, so the bar
     *  can never visually go backward during a rebuild. */
    let progressFraction = $derived.by<number | null>(() => {
        if (!$fileSearchIndexing) return null;
        const p = $fileSearchProgress;
        if (!p) return null;
        if (progressStableTotal <= 0) return null;
        if (progressIsStarting) return null;
        if (peakFraction <= 0) return null;
        return peakFraction;
    });

    /** Files per second from the rolling sample window. Returns null
     *  when we don't have enough samples yet OR when the rate is
     *  meaningless (zero or negative). */
    let progressRate = $derived.by<number | null>(() => {
        if (progressSamples.length < 2) return null;
        const first = progressSamples[0];
        const last = progressSamples.at(-1)!;
        const elapsedSec = (last.ms - first.ms) / 1000;
        if (elapsedSec <= 0.25) return null;
        const delta = last.indexed - first.indexed;
        if (delta <= 0) return null;
        return delta / elapsedSec;
    });

    /** ETA in seconds — only computed when the total has been stable
     *  for ~1.5 s AND we have a positive rate. Hidden during the
     *  early "walker burst" when remaining keeps spiking. */
    let progressEtaSeconds = $derived.by<number | null>(() => {
        const total = progressStableTotal;
        const rate = progressRate;
        const p = $fileSearchProgress;
        if (!p || !rate || rate < 0.5 || total <= 0) return null;
        // Confidence gate: total must have settled for a moment so we
        // don't quote ETA off a still-growing denominator. 1.5 s is
        // long enough to filter the walker-burst startup spike but
        // short enough to feel responsive on small folders.
        if (performance.now() - progressStableTotalSetAtMs < 1500) return null;
        const remaining = total - p.indexedFiles;
        if (remaining <= 0) return null;
        return remaining / rate;
    });

    /** Format an ETA in seconds as "Xh Ym", "Xm", or "Xs". Caps the
     *  longest form at "1h+" so a misestimated huge total doesn't
     *  spook the user with "23 hours". */
    function formatEta(seconds: number): string {
        const s = Math.max(1, Math.round(seconds));
        if (s < 60) return `~${s}s remaining`;
        const m = Math.round(s / 60);
        if (m < 60) return `~${m}m remaining`;
        const h = Math.floor(m / 60);
        if (h >= 2) return '~1h+ remaining';
        const remM = m - 60;
        return remM > 0 ? `~${h}h ${remM}m remaining` : `~${h}h remaining`;
    }

    let progressActivityText = $derived.by(() => {
        if (progressIsStarting) return $_('page.fileSearchIndex.progress.starting');
        const p = $fileSearchProgress;
        if (!p) return $_('page.fileSearchIndex.progress.title');
        let base: string;
        if (p.currentPath) {
            const folder = shortFolderName(p.currentPath);
            base = folder
                ? $_('page.fileSearchIndex.progress.readingFolder', {
                      values: { folder },
                  })
                : (p.message || $_('page.fileSearchIndex.progress.title'));
        } else {
            base = p.message || $_('page.fileSearchIndex.progress.title');
        }
        // Append ETA when we have one — keeps the activity string as
        // the canonical "what + when" line so the user reads one place.
        if (progressEtaSeconds !== null) {
            return `${base} · ${formatEta(progressEtaSeconds)}`;
        }
        return base;
    });

    // Drive-source banners apply to the content index sources.
    let driveRootCount = $derived($fileSearchRoots.filter(isDriveRoot).length);
    let hasDriveOnlySources = $derived($fileSearchRoots.length > 0 && driveRootCount === $fileSearchRoots.length);
    let hasMixedSources = $derived($fileSearchRoots.length > 0 && driveRootCount > 0 && !hasDriveOnlySources);
    // The watcher status is tab-aware: the File tab reports the filename
    // index's own watcher (its worker becomes a notify-watch loop / USN tailer
    // after a build), the Content tab reports the content-index watcher.
    let watcherLabel = $derived.by(() => {
        if (activeTab === 'file') {
            if ($fileSearchStatus?.filenameWatching)
                return $_('page.fileSearchIndex.watcherState.fileWatching');
            if (!$fileSearchWatcherEnabled)
                return $_('page.fileSearchIndex.watcherState.disabled');
            // If an index already exists (files were indexed at some point) but the
            // watcher isn't running, say "Indexed — watching inactive" rather than
            // "Rebuild to activate" (which implies a rebuild would fix it, and is
            // also confusing right after a successful build finishes).
            const hasIndex = ($fileSearchStatus?.filenameIndexedFiles ?? 0) > 0;
            if (hasIndex)
                return $_('page.fileSearchIndex.watcherState.fileIndexedNoWatch');
            return $_('page.fileSearchIndex.watcherState.fileInactive');
        }
        return $fileSearchStatus?.watching
            ? $_('page.fileSearchIndex.watcherState.watching')
            : $fileSearchStatus?.watcherPaused
              ? $_('page.fileSearchIndex.watcherState.paused')
              : $fileSearchWatcherEnabled
                ? $_('page.fileSearchIndex.watcherState.foldersOnly')
                : $_('page.fileSearchIndex.watcherState.disabled');
    });
    let watcherTone = $derived.by(() => {
        if (activeTab === 'file') {
            if ($fileSearchStatus?.filenameWatching) return 'text-success';
            if (!$fileSearchWatcherEnabled) return 'text-muted';
            return 'text-warning';
        }
        return $fileSearchStatus?.watching
            ? 'text-success'
            : $fileSearchStatus?.watcherPaused
              ? 'text-warning'
              : 'text-muted';
    });

    // The shared "Indexed files" stat card reflects whichever index the active
    // tab governs — the filename index on the File tab, the content index on
    // the Content tab — so each engine reports its own count and build time.
    let shownIndexedFiles = $derived(
        activeTab === 'file'
            ? $fileSearchStatus?.filenameIndexedFiles
            : $fileSearchStatus?.indexedFiles,
    );
    let shownLastIndexedAtMs = $derived(
        activeTab === 'file'
            ? $fileSearchStatus?.filenameLastIndexedAtMs
            : $fileSearchStatus?.lastIndexedAtMs,
    );
    let shownIndexBytes = $derived(
        activeTab === 'file'
            ? $fileSearchStatus?.filenameIndexBytes
            : $fileSearchStatus?.contentIndexBytes,
    );

    onMount(async () => {
        await initFileSearchStore();
        await refreshFileSearchStatus();
    });

    async function pickFilenameFolders() {
        const picked = await open({ directory: true, multiple: true });
        if (Array.isArray(picked)) addFileSearchFilenameRoots(picked);
        else if (typeof picked === 'string') addFileSearchFilenameRoots([picked]);
    }

    async function pickFilenameFiles() {
        const picked = await open({ directory: false, multiple: true });
        if (Array.isArray(picked)) addFileSearchFilenameRoots(picked);
        else if (typeof picked === 'string') addFileSearchFilenameRoots([picked]);
    }

    async function pickContentFolders() {
        const picked = await open({ directory: true, multiple: true });
        const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
        addContentRootsFiltered(paths);
    }

    /** Wave 8 / task #88 (2026-05-28): folder picker for OCR-on-Index.
     *  Limited to folders (no individual files) because the OCR fallback
     *  is path-prefix matched in the worker — the user picks a folder
     *  containing scanned PDFs (Receipts, Scanned IDs, Old Contracts)
     *  and every PDF in/below that folder is OCR'd if its text layer is
     *  empty. */
    async function pickOcrFolders() {
        const picked = await open({ directory: true, multiple: true });
        const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
        for (const p of paths) addFileSearchOcrFolder(p);
    }

    async function pickContentFiles() {
        const picked = await open({ directory: false, multiple: true });
        const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
        addContentRootsFiltered(paths);
    }

    /** Filter out drive roots (C:\, D:\, …) before adding content index sources.
     *  Content indexing on a drive root crawls millions of files and makes the
     *  live watcher useless — only specific folders are allowed. */
    function addContentRootsFiltered(paths: string[]) {
        const rejected = paths.filter(isDriveRoot);
        const accepted = paths.filter((p) => !isDriveRoot(p));
        if (rejected.length) {
            const listed = rejected.join(', ');
            toast(
                `Drive roots can't be added as content sources (${listed}). Pick a specific folder inside the drive instead.`,
                'info',
                6000,
            );
        }
        if (accepted.length) addFileSearchRoots(accepted);
    }

    function addExcludeFolderDraft() {
        if (!excludeFolderDraft.trim()) return;
        addFileSearchExcludeFolders([excludeFolderDraft]);
        excludeFolderDraft = '';
        toast(t('page.fileSearchIndex.toast.folderExcluded'), 'info');
    }

    function addExcludeExtensionDraft() {
        if (!excludeExtensionDraft.trim()) return;
        addFileSearchExcludeExtensions([excludeExtensionDraft]);
        excludeExtensionDraft = '';
        toast(t('page.fileSearchIndex.toast.extensionExcluded'), 'info');
    }

    function restoreSearchExclusionDefaults() {
        resetFileSearchExclusions();
        toast(t('page.fileSearchIndex.toast.exclusionsRestored'), 'success');
    }

    async function saveSearchRebuildSchedule() {
        await saveFileSearchRebuildSchedule();
    }

    // Header Build button is tab-aware: each tab rebuilds its own index.
    function buildActiveIndex() {
        if (activeTab === 'file') void startFilenameSearchIndex();
        else void startContentSearchIndex();
    }

    async function openQuickSearchOverlay() {
        try {
            await invoke('show_overlay_window_command');
        } catch (error) {
            console.error(error);
            toast(t('page.fileSearchIndex.toast.overlayOpenFailed'), 'error');
        }
    }

    function isDriveRoot(path: string) {
        return /^[a-zA-Z]:[\\/]*$/.test(path.trim());
    }

    function fileName(path: string) {
        const index = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
        return index >= 0 ? path.slice(index + 1) || path : path;
    }

    function formatCount(value: number | null | undefined) {
        return Number(value ?? 0).toLocaleString();
    }

    function formatDate(value: number | null | undefined) {
        if (!value) return t('page.fileSearchIndex.never');
        return new Date(value).toLocaleString();
    }

    function nextScheduledRebuildTime() {
        if (!$fileSearchRebuildScheduleEnabled) return null;
        const reference =
            $fileSearchLastScheduledRebuildAtMs ?? $fileSearchStatus?.lastIndexedAtMs ?? null;
        if (!reference) return null;
        const intervalMs =
            Math.max(1, Math.round($fileSearchRebuildIntervalHours || 24)) * 60 * 60 * 1000;
        return reference + intervalMs;
    }

    // ── UI-only view layer additions (kit migration) ─────────────────
    // Everything above this line is the ORIGINAL, untouched script
    // (backend wiring, stores, handlers). Below: the only additions —
    // the UI-kit component imports (presentational only, no behavior)
    // and a single local flag for the "Tuning & schedule" SideSheet
    // open/close. No backend contract is affected.
    import {
        ToolPage,
        ToolPanel,
        SectionHeader,
        Tabs,
        Button,
        Checkbox,
        Select,
        TextInput,
        ResultList,
        ResultRow,
        SideSheet,
    } from '$lib/ui';

    let sideSheetOpen = $state(false);
</script>

<ToolPage
    icon={Database}
    title={fixedTab === 'file'
        ? $_('page.fileSearchIndex.tabs.file')
        : fixedTab === 'content'
          ? $_('page.fileSearchIndex.tabs.content')
          : $_('page.fileSearchIndex.title')}
    description={$_('page.fileSearchIndex.intro')}
    width="wide"
>
    {#snippet actions()}
        <Button
            variant="secondary"
            icon={Search}
            onclick={() => (selected = 'file-search')}
        >
            {$_('page.fileSearchIndex.openSearch')}
        </Button>
        <Button
            variant="ghost"
            icon={Settings2}
            onclick={() => (sideSheetOpen = true)}
        >
            {$_('page.fileSearchIndex.advanced.title')}
        </Button>
        {#if $fileSearchIndexing}
            <Button
                variant="danger"
                icon={XCircle}
                onclick={cancelFileSearchIndexBuild}
                disabled={$fileSearchCancelling}
            >
                {$fileSearchCancelling ? $_('page.fileSearchIndex.cancelling') : $_('page.fileSearchIndex.cancelBuild')}
            </Button>
        {:else}
            <Button
                variant="primary"
                icon={Play}
                onclick={buildActiveIndex}
                disabled={activeTab === 'file' ? !$fileSearchFilenameRoots.length : !$fileSearchRoots.length}
            >
                {$_('page.fileSearchIndex.buildRebuild')}
            </Button>
        {/if}
    {/snippet}

    <!-- Identity badge — preserved from the original header. The kit's
         ToolPage header has no badge slot, so it rides as a small
         tokenized caption pill at the top of the body. -->
    <div class="fsi-badge-row">
        <span class="fsi-badge">
            <Database class="fsi-badge-ico" />
            {$_('page.fileSearchIndex.badge')}
        </span>
    </div>

    <!-- ─── Stat tiles ─────────────────────────────────────────────
         Four glanceable stats. Calm type scale (kit restrained: ~19px
         numbers) rather than the old text-2xl. -->
    <ToolPanel padding="md">
        <div class="fsi-stat-grid">
            <div class="fsi-stat">
                <div class="fsi-stat-head">
                    <span class="fsi-stat-label">{$_('page.fileSearchIndex.stats.indexedFiles')}</span>
                    <Database class="fsi-stat-ico" />
                </div>
                <div class="fsi-stat-value">{formatCount(shownIndexedFiles)}</div>
                <div class="fsi-stat-meta">{$_('page.fileSearchIndex.stats.lastBuild', { values: { date: formatDate(shownLastIndexedAtMs) } })}</div>
                <div class="fsi-stat-meta">{$_('page.fileSearchIndex.stats.onDisk', { values: { size: formatBytes(shownIndexBytes ?? 0) } })}</div>
            </div>

            <div class="fsi-stat">
                <div class="fsi-stat-head">
                    <span class="fsi-stat-label">{$_('page.fileSearchIndex.stats.indexWorker')}</span>
                    <Activity class="fsi-stat-ico" />
                </div>
                <div class="fsi-stat-value">
                    {$fileSearchIndexing ? $_('page.fileSearchIndex.stats.building') : ($fileSearchStatus?.diagnostics.indexWorker ?? $_('page.fileSearchIndex.stats.idle'))}
                </div>
                <div class="fsi-stat-meta fsi-truncate" title={$fileSearchLastMessage ?? ''}>
                    {$fileSearchLastMessage ?? $_('page.fileSearchIndex.stats.ready')}
                </div>
            </div>

            <div class="fsi-stat">
                <div class="fsi-stat-head">
                    <span class="fsi-stat-label">{$_('page.fileSearchIndex.stats.liveWatcher')}</span>
                    <ShieldCheck class="fsi-stat-ico" />
                </div>
                <div class="fsi-stat-value {watcherTone}">{watcherLabel}</div>
                <div class="fsi-stat-meta fsi-truncate" title={$fileSearchStatus?.diagnostics.lastWorkerMessage ?? ''}>
                    {$fileSearchStatus?.diagnostics.lastWorkerMessage ?? $_('page.fileSearchIndex.stats.driveRootsNotWatched')}
                </div>
            </div>

            <div class="fsi-stat">
                <div class="fsi-stat-head">
                    <span class="fsi-stat-label">{$_('page.fileSearchIndex.stats.scheduledRebuild')}</span>
                    <Clock class="fsi-stat-ico" />
                </div>
                <div class="fsi-stat-value">
                    {$fileSearchRebuildScheduleEnabled ? `${$fileSearchRebuildIntervalHours}h` : $_('page.fileSearchIndex.stats.manual')}
                </div>
                <div class="fsi-stat-meta fsi-truncate">
                    {$_('page.fileSearchIndex.stats.next', { values: { date: nextScheduledRebuildTime() ? formatDate(nextScheduledRebuildTime()) : $_('page.fileSearchIndex.stats.notScheduled') } })}
                </div>
            </div>
        </div>
    </ToolPanel>

    {#if $fileSearchProgress && $fileSearchIndexing}
        <div class="fsi-progress">
            <div class="fsi-progress-row">
                <div class="fsi-progress-text">
                    <div class="fsi-progress-title">{$_('page.fileSearchIndex.progress.title')}</div>
                    <!-- Wave 5 (2026-05-28): plain-English activity string
                         ("Reading: Documents") in place of the raw
                         worker message. UxAudit UX-A-05. -->
                    <div class="fsi-progress-msg fsi-truncate" title={$fileSearchProgress.currentPath}>
                        {progressActivityText}
                    </div>
                </div>
                {#if progressIsStarting}
                    <!-- Task #64 (2026-05-27): worker hasn't reported
                         yet — show a spinner instead of "0/0/0". -->
                    <div class="fsi-progress-starting">
                        <span class="fsi-progress-spinner" aria-hidden="true"></span>
                    </div>
                {:else}
                    <div class="fsi-progress-stats">
                        <div class="fsi-progress-stat">
                            <div class="fsi-progress-stat-label">{$_('page.fileSearchIndex.progress.indexed')}</div>
                            <div class="fsi-progress-stat-value">{formatCount($fileSearchProgress.indexedFiles)}</div>
                        </div>
                        <div class="fsi-progress-stat">
                            <div class="fsi-progress-stat-label">{$_('page.fileSearchIndex.progress.scanned')}</div>
                            <div class="fsi-progress-stat-value">{formatCount($fileSearchProgress.scannedEntries)}</div>
                        </div>
                        <div class="fsi-progress-stat">
                            <div class="fsi-progress-stat-label">{$_('page.fileSearchIndex.progress.skipped')}</div>
                            <div class="fsi-progress-stat-value">{formatCount($fileSearchProgress.skippedFiles)}</div>
                        </div>
                    </div>
                {/if}
            </div>
            <!-- Wave 5.5 (2026-05-28): true determinate progress when
                 the backend has reported enough signal, indeterminate
                 sliding gradient while the walker is still discovering.
                 UxAudit UX-A-05.

                 `progressFraction` is null during the early walker-burst
                 (when `scannedEntries` grows faster than `indexedFiles`
                 so a determinate ratio would be misleading); the bar
                 falls back to the animated indeterminate state. Once
                 the stable total ratchets up and `indexed / total`
                 becomes a reliable percent, the fill bar takes over
                 with a smooth CSS transition so the handover looks
                 like a single bar — not a glitch. -->
            {#if progressFraction !== null}
                <div
                    class="fsi-progress-bar"
                    role="progressbar"
                    aria-valuemin="0"
                    aria-valuemax="100"
                    aria-valuenow={Math.round(progressFraction * 100)}
                >
                    <div
                        class="fsi-progress-bar-determinate"
                        style="width: {(progressFraction * 100).toFixed(1)}%"
                    ></div>
                    <div class="fsi-progress-bar-percent" aria-hidden="true">
                        {Math.round(progressFraction * 100)}%
                    </div>
                </div>
            {:else}
                <div class="fsi-progress-bar" aria-hidden="true">
                    <div class="fsi-progress-bar-fill"></div>
                </div>
            {/if}
        </div>
    {/if}

    <!-- Quick action: open the overlay from this page. The full set of
         hotkey + bang config moved to Settings → Search Overlay so it
         lives in one place; this strip stays as a fast "try it now"
         affordance + a pointer to where the config lives. -->
    <ToolPanel tone="panel-2" padding="md" as="section">
        <div class="fsi-overlay-strip">
            <div class="fsi-overlay-text">
                <div class="fsi-overlay-head">
                    <Search class="fsi-overlay-ico" />
                    <h2 class="fsi-overlay-title">{$_('page.fileSearchIndex.overlay.title')}</h2>
                </div>
                <p class="fsi-overlay-desc">
                    {$_('page.fileSearchIndex.overlay.descBefore')}
                    <button
                        type="button"
                        onclick={() => (selected = 'settings')}
                        class="fsi-overlay-link"
                    >{$_('page.fileSearchIndex.overlay.descLink')}</button>{$_('page.fileSearchIndex.overlay.descAfter')}
                </p>
            </div>
            <Button variant="secondary" icon={Search} onclick={openQuickSearchOverlay}>
                {$_('page.fileSearchIndex.overlay.openNow')}
            </Button>
        </div>
    </ToolPanel>

    <!-- Two-tab split: the "File Search Index" tab governs the filename/path
         index (with its own engine selector), the "Content Search Index"
         tab governs the full-text content index. They are distinct backend
         indexes with distinct source lists. -->
    {#if !fixedTab}
        <div class="fsi-tabs-row">
            <Tabs
                tabs={[
                    { id: 'file', label: $_('page.fileSearchIndex.tabs.file'), icon: Database },
                    { id: 'content', label: $_('page.fileSearchIndex.tabs.content'), icon: FileText },
                ]}
                active={activeTab}
                onChange={(id) => (activeTab = id as 'file' | 'content')}
                ariaLabel={$_('page.fileSearchIndex.title')}
            />
        </div>
    {/if}

    {#if activeTab === 'file'}
        <!-- ============ TAB 1 — File Search Index (filename/path) ============ -->
        {#if isFirstRunForFile}
            <!-- Wave 5 (2026-05-28): first-run hero — UxAudit UX-A-04.
                 New users land here with no idea why indexing matters.
                 Replaces the power-user surface (engine cards + watcher
                 modes + advanced sidesheet) with one friendly card that
                 says WHY this exists and offers a single CTA. Once a
                 folder is added OR an index exists, the normal surface
                 returns. -->
            <ToolPanel padding="lg">
                <div class="fsi-firstrun">
                    <div class="fsi-firstrun-icon">
                        <Database class="fsi-firstrun-icon-svg" />
                    </div>
                    <h2 class="fsi-firstrun-title">{$_('page.fileSearchIndex.firstRun.fileTitle')}</h2>
                    <p class="fsi-firstrun-lede">{$_('page.fileSearchIndex.firstRun.fileLede')}</p>
                    <div class="fsi-firstrun-actions">
                        <Button
                            variant="primary"
                            icon={FolderOpen}
                            onclick={pickFilenameFolders}
                        >
                            {$_('page.fileSearchIndex.firstRun.filePickCta')}
                        </Button>
                    </div>
                    <p class="fsi-firstrun-note">{$_('page.fileSearchIndex.firstRun.fileNote')}</p>
                </div>
            </ToolPanel>
        {:else}
        <p class="fsi-tab-desc">
            {$_('page.fileSearchIndex.tabs.fileDesc')}
        </p>

        <!-- Wave 6 (2026-05-28): the MFT engine selector card + the
             "Allow root drive watcher" opt-in were removed. The walker
             is the only path now; drive-root watching is unconditional;
             system paths (C:\Windows, Program Files, Program Files (x86),
             ProgramData) are unconditionally hard-blocked at the backend
             level. See UxAudit.md Wave 6 for the full rationale. -->

        {#if $fileSearchFilenameIndexMessage}
            <ToolPanel padding="md" as="section">
                <p class="fsi-filename-status">
                    {$fileSearchFilenameIndexMessage}
                </p>
            </ToolPanel>
        {/if}

        <ToolPanel padding="lg" as="section">
            <SectionHeader
                title={$_('page.fileSearchIndex.filenameSources.title')}
                description={$_('page.fileSearchIndex.filenameSources.desc')}
            >
                {#snippet actions()}
                    <Button
                        variant="secondary"
                        size="sm"
                        icon={FolderOpen}
                        onclick={pickFilenameFolders}
                        disabled={$fileSearchIndexing}
                    >
                        {$_('page.fileSearchIndex.sources.addFolders')}
                    </Button>
                    <Button
                        variant="secondary"
                        size="sm"
                        icon={File}
                        onclick={pickFilenameFiles}
                        disabled={$fileSearchIndexing}
                    >
                        {$_('page.fileSearchIndex.sources.addFiles')}
                    </Button>
                {/snippet}
            </SectionHeader>

            <div class="fsi-sources-meta">
                <span>{$_('page.fileSearchIndex.filenameSources.count', { values: { count: $fileSearchFilenameRoots.length } })}</span>
                {#if $fileSearchFilenameRoots.length}
                    <button onclick={clearFileSearchFilenameRoots} disabled={$fileSearchIndexing} class="fsi-clear-btn">
                        {$_('page.fileSearchIndex.sources.clearAll')}
                    </button>
                {/if}
            </div>

            <div class="fsi-sources-list">
                <ResultList
                    empty={!$fileSearchFilenameRoots.length}
                    emptyIcon={FolderOpen}
                    emptyTitle={$_('page.fileSearchIndex.filenameSources.empty')}
                >
                    {#each $fileSearchFilenameRoots as root}
                        <ResultRow
                            icon={isDriveRoot(root) ? Database : FolderOpen}
                            iconTint="var(--color-accent)"
                            title={fileName(root) || root}
                            subtitle={root}
                        >
                            {#snippet trailing()}
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    iconOnly
                                    icon={X}
                                    onclick={() => removeFileSearchFilenameRoot(root)}
                                    disabled={$fileSearchIndexing}
                                    title={$_('page.fileSearchIndex.sources.removeSource')}
                                    aria-label={$_('page.fileSearchIndex.sources.removeSource')}
                                />
                            {/snippet}
                        </ResultRow>
                    {/each}
                </ResultList>
            </div>

            <ToolPanel tone="panel-2" padding="sm">
                <div class="fsi-status-row">
                    <div class="fsi-status-text">
                        <div class="fsi-stat-label">{$_('page.fileSearchIndex.filenameStatus.indexedFiles')}</div>
                        <div class="fsi-stat-value">{formatCount($fileSearchStatus?.filenameIndexedFiles)}</div>
                        <div class="fsi-stat-meta">{$_('page.fileSearchIndex.stats.lastBuild', { values: { date: formatDate($fileSearchStatus?.filenameLastIndexedAtMs) } })}</div>
                        <div class="fsi-stat-meta">{$_('page.fileSearchIndex.stats.onDisk', { values: { size: formatBytes($fileSearchStatus?.filenameIndexBytes ?? 0) } })}</div>
                    </div>
                    {@render buildCancelButton(() => void startFilenameSearchIndex(), !$fileSearchFilenameRoots.length)}
                </div>
                <div class="fsi-status-msg fsi-truncate" title={$fileSearchFilenameIndexMessage ?? ''}>
                    {$fileSearchFilenameIndexMessage ?? $_('page.fileSearchIndex.filenameStatus.idle')}
                </div>
            </ToolPanel>
        </ToolPanel>

        {@render liveWatcherSection('file')}
        {/if}
    {:else}
        <!-- ============ TAB 2 — Content Search Index (full-text) ============ -->
        {#if isFirstRunForContent}
            <!-- Wave 5 (2026-05-28): first-run hero (Content tab). -->
            <ToolPanel padding="lg">
                <div class="fsi-firstrun">
                    <div class="fsi-firstrun-icon">
                        <FileText class="fsi-firstrun-icon-svg" />
                    </div>
                    <h2 class="fsi-firstrun-title">{$_('page.fileSearchIndex.firstRun.contentTitle')}</h2>
                    <p class="fsi-firstrun-lede">{$_('page.fileSearchIndex.firstRun.contentLede')}</p>
                    <div class="fsi-firstrun-actions">
                        <Button
                            variant="primary"
                            icon={FolderOpen}
                            onclick={pickContentFolders}
                        >
                            {$_('page.fileSearchIndex.firstRun.contentPickCta')}
                        </Button>
                    </div>
                    <p class="fsi-firstrun-note">{$_('page.fileSearchIndex.firstRun.contentNote')}</p>
                </div>
            </ToolPanel>
        {:else}
        <p class="fsi-tab-desc">
            {$_('page.fileSearchIndex.tabs.contentDesc')}
        </p>

        {#if hasDriveOnlySources}
            <div class="fsi-banner fsi-banner-warning">
                {$_('page.fileSearchIndex.driveOnlyBanner')}
            </div>
        {:else if hasMixedSources}
            <div class="fsi-banner fsi-banner-neutral">
                {$_('page.fileSearchIndex.mixedSourceBanner')}
            </div>
        {/if}

        <ToolPanel padding="lg" as="section">
            <SectionHeader
                title={$_('page.fileSearchIndex.contentSources.title')}
                description={$_('page.fileSearchIndex.contentSources.desc')}
            >
                {#snippet actions()}
                    <Button
                        variant="secondary"
                        size="sm"
                        icon={FolderOpen}
                        onclick={pickContentFolders}
                        disabled={$fileSearchIndexing}
                    >
                        {$_('page.fileSearchIndex.sources.addFolders')}
                    </Button>
                    <Button
                        variant="secondary"
                        size="sm"
                        icon={File}
                        onclick={pickContentFiles}
                        disabled={$fileSearchIndexing}
                    >
                        {$_('page.fileSearchIndex.sources.addFiles')}
                    </Button>
                {/snippet}
            </SectionHeader>

            <div class="fsi-sources-meta">
                <span>{$_('page.fileSearchIndex.contentSources.count', { values: { count: $fileSearchRoots.length } })}</span>
                {#if $fileSearchRoots.length}
                    <button onclick={clearFileSearchRoots} disabled={$fileSearchIndexing} class="fsi-clear-btn">
                        {$_('page.fileSearchIndex.sources.clearAll')}
                    </button>
                {/if}
            </div>

            <div class="fsi-sources-list">
                <ResultList
                    empty={!$fileSearchRoots.length}
                    emptyIcon={FolderOpen}
                    emptyTitle={$_('page.fileSearchIndex.contentSources.empty')}
                >
                    {#each $fileSearchRoots as root}
                        <ResultRow
                            icon={isDriveRoot(root) ? Database : FolderOpen}
                            iconTint="var(--color-accent)"
                            title={fileName(root) || root}
                            subtitle={root}
                            meta={isDriveRoot(root) ? $_('page.fileSearchIndex.sources.rebuildOnly') : $_('page.fileSearchIndex.sources.watcherEligible')}
                        >
                            {#snippet trailing()}
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    iconOnly
                                    icon={X}
                                    onclick={() => removeFileSearchRoot(root)}
                                    disabled={$fileSearchIndexing}
                                    title={$_('page.fileSearchIndex.sources.removeSource')}
                                    aria-label={$_('page.fileSearchIndex.sources.removeSource')}
                                />
                            {/snippet}
                        </ResultRow>
                    {/each}
                </ResultList>
            </div>
        </ToolPanel>

        <ToolPanel padding="lg" as="section">
            <SectionHeader
                icon={FileText}
                title={$_('page.fileSearchIndex.contentStatus.title')}
                description={$_('page.fileSearchIndex.exclusions.desc')}
            />

            <div class="fsi-tuning-grid">
                <label class="fsi-field">
                    <span class="fsi-field-label">{$_('page.fileSearchIndex.advanced.maxContentKb')}</span>
                    <input
                        type="number"
                        min="8"
                        max="4096"
                        step="8"
                        bind:value={$fileSearchMaxContentKb}
                        onchange={() => void saveFileSearchIndexOptions()}
                        disabled={$fileSearchIndexing}
                        class="fsi-num-input"
                    />
                    <span class="fsi-field-hint">
                        {@html $_('page.fileSearchIndex.advanced.tiersHint')}
                    </span>
                </label>
                <label class="fsi-field">
                    <span class="fsi-field-label">{$_('page.fileSearchIndex.advanced.commitEvery')}</span>
                    <input
                        type="number"
                        min="300"
                        max="250000"
                        step="300"
                        bind:value={$fileSearchCommitEvery}
                        onchange={() => void saveFileSearchIndexOptions()}
                        disabled={$fileSearchIndexing}
                        class="fsi-num-input"
                    />
                </label>
            </div>

            <ToolPanel tone="panel-2" padding="sm">
                <div class="fsi-status-row">
                    <div class="fsi-status-text">
                        <div class="fsi-stat-label">{$_('page.fileSearchIndex.stats.indexedFiles')}</div>
                        <div class="fsi-stat-value">{formatCount($fileSearchStatus?.indexedFiles)}</div>
                        <div class="fsi-stat-meta">{$_('page.fileSearchIndex.stats.lastBuild', { values: { date: formatDate($fileSearchStatus?.lastIndexedAtMs) } })}</div>
                        <div class="fsi-stat-meta">{$_('page.fileSearchIndex.stats.onDisk', { values: { size: formatBytes($fileSearchStatus?.contentIndexBytes ?? 0) } })}</div>
                    </div>
                    {@render buildCancelButton(() => void startContentSearchIndex(), !$fileSearchRoots.length)}
                </div>
            </ToolPanel>

            {#if $fileSearchProgress && $fileSearchIndexing}
                <div class="fsi-progress">
                    <div class="fsi-progress-row">
                        <div class="fsi-progress-text">
                            <div class="fsi-progress-title">{$_('page.fileSearchIndex.progress.title')}</div>
                            <div class="fsi-progress-msg fsi-truncate" title={$fileSearchProgress.currentPath}>
                                {progressIsStarting
                                    ? $_('page.fileSearchIndex.progress.starting')
                                    : $fileSearchProgress.message}
                            </div>
                        </div>
                        {#if progressIsStarting}
                            <div class="fsi-progress-starting">
                                <span class="fsi-progress-spinner" aria-hidden="true"></span>
                            </div>
                        {:else}
                            <div class="fsi-progress-stats">
                                <div class="fsi-progress-stat">
                                    <div class="fsi-progress-stat-label">{$_('page.fileSearchIndex.progress.indexed')}</div>
                                    <div class="fsi-progress-stat-value">{formatCount($fileSearchProgress.indexedFiles)}</div>
                                </div>
                                <div class="fsi-progress-stat">
                                    <div class="fsi-progress-stat-label">{$_('page.fileSearchIndex.progress.scanned')}</div>
                                    <div class="fsi-progress-stat-value">{formatCount($fileSearchProgress.scannedEntries)}</div>
                                </div>
                                <div class="fsi-progress-stat">
                                    <div class="fsi-progress-stat-label">{$_('page.fileSearchIndex.progress.skipped')}</div>
                                    <div class="fsi-progress-stat-value">{formatCount($fileSearchProgress.skippedFiles)}</div>
                                </div>
                            </div>
                        {/if}
                    </div>
                </div>
            {/if}
        </ToolPanel>

        {@render liveWatcherSection('content')}
        {/if}
    {/if}
</ToolPage>

<!-- ─── SideSheet: Tuning & schedule ───────────────────────────────
     The heavy/secondary controls live here so the main column stays
     calm: the exclusions + indexing options (advancedIndexingSection)
     and the scheduled-rebuild card. Wave 6 (2026-05-28): the `full`
     argument no longer depends on MFT (walker is the only path). -->
<SideSheet
    open={sideSheetOpen}
    title={$_('page.fileSearchIndex.advanced.title')}
    width={480}
    onclose={() => (sideSheetOpen = false)}
>
    <div class="fsi-sheet-stack">
        {@render advancedIndexingSection(true)}

        <div class="fsi-sheet-divider"></div>

        <!-- Indexing Phase 2 / Task 4.3 (2026-06-18): content-indexing opt-in.
             "Search inside files" gates whether the content index builds at all.
             Off = filename-only search (much lighter — good for low-RAM
             machines). The filename index always builds regardless. -->
        <div class="fsi-sheet-section">
            <SectionHeader
                icon={FileText}
                title="Search inside files"
                description="Index the text inside your documents so you can search file contents, not just names. Heavier on CPU and memory — turn it off for filename-only search on low-memory machines. The index is a local searchable copy of that text — it never leaves your machine, but it's protected only by your disk encryption, so turn on BitLocker if you index sensitive files."
            >
                {#snippet actions()}
                    <label class="fsi-inline-toggle">
                        <Checkbox
                            checked={$fileSearchContentIndexingEnabled}
                            onchange={(checked) => setFileSearchContentIndexingEnabled(checked)}
                            disabled={$fileSearchIndexing}
                        />
                        <span>Search inside files</span>
                    </label>
                {/snippet}
            </SectionHeader>
        </div>

        <div class="fsi-sheet-divider"></div>

        <!-- Semantic search (beta, 2026-07-01): embedding-based retrieval that
             finds documents by MEANING, not just matching words — so "car
             insurance" can surface a file that only says "auto policy". Builds
             on top of "Search inside files" (needs the content text), so it's
             disabled when that's off. Off by default: it bundles a local model
             and adds per-document embedding at index time. Fully offline. -->
        <div class="fsi-sheet-section">
            <SectionHeader
                icon={Sparkles}
                title="Semantic search (beta)"
                description="Also find files by meaning, not just exact words — so a search for one phrase can surface a document that means the same thing in different words. Uses a small local AI model that runs entirely on your machine; nothing is sent anywhere. Slower to index. Requires “Search inside files”."
            >
                {#snippet actions()}
                    <label class="fsi-inline-toggle">
                        <Checkbox
                            checked={$fileSearchSemanticEnabled}
                            onchange={(checked) => setFileSearchSemanticEnabled(checked)}
                            disabled={$fileSearchIndexing || !$fileSearchContentIndexingEnabled}
                        />
                        <span>Enable semantic search</span>
                    </label>
                {/snippet}
            </SectionHeader>
        </div>

        <div class="fsi-sheet-divider"></div>

        <!-- Wave 8 / task #88 (2026-05-28): OCR-on-Index opt-in card.
             Off by default; the toggle reveals the folder picker + page
             cap. Plain English copy — no "Tesseract" / "PDFium" jargon
             on the toggle itself; the description explains the slow-but-
             searchable trade-off without scaring non-power users. -->
        <div class="fsi-sheet-section">
            <SectionHeader
                icon={Settings2}
                title="Search inside scanned PDFs & images"
                description="For folders with scans, screenshots, or photos of documents — a few seconds per file."
            >
                {#snippet actions()}
                    <label class="fsi-inline-toggle">
                        <Checkbox
                            checked={$fileSearchOcrEnabled}
                            onchange={(checked) => setFileSearchOcrEnabled(checked)}
                            disabled={$fileSearchIndexing}
                        />
                        <span>Enable OCR on index</span>
                    </label>
                {/snippet}
            </SectionHeader>

            {#if $fileSearchOcrEnabled}
                <div class="fsi-exclude-grid" style="grid-template-columns: 1fr;">
                    <div class="fsi-exclude-col">
                        <div class="fsi-exclude-head">
                            <span>OCR these folders</span>
                            <span>{formatCount($fileSearchOcrFolders.length)}</span>
                        </div>
                        <div class="fsi-exclude-form">
                            <Button
                                variant="secondary"
                                size="sm"
                                onclick={() => void pickOcrFolders()}
                                disabled={$fileSearchIndexing}
                            >
                                Add folder…
                            </Button>
                        </div>
                        <!-- Wave 8.4.2 (2026-05-28): always render the chip
                             box so the picker has a visible target even when
                             empty. Hint text moves INSIDE the empty box so
                             new users see exactly where their picked folders
                             will appear. -->
                        <div class="fsi-chip-box">
                            {#if $fileSearchOcrFolders.length === 0}
                                <p class="fsi-chip-empty">
                                    No folders yet — click <b>Add folder…</b> above.
                                </p>
                            {:else}
                                <div class="fsi-chip-row">
                                    {#each $fileSearchOcrFolders as folder}
                                        <button
                                            type="button"
                                            onclick={() => removeFileSearchOcrFolder(folder)}
                                            disabled={$fileSearchIndexing}
                                            title="Remove {folder} from OCR list"
                                            class="fsi-chip"
                                        >
                                            <span class="fsi-chip-text">{folder}</span>
                                        </button>
                                    {/each}
                                </div>
                            {/if}
                        </div>
                    </div>

                    <div class="fsi-exclude-col">
                        <label class="fsi-field">
                            <span class="fsi-field-label">Pages per file (cap)</span>
                            <input
                                type="number"
                                min="1"
                                max="200"
                                step="1"
                                value={$fileSearchOcrMaxPagesPerFile}
                                onchange={(event) =>
                                    setFileSearchOcrMaxPages(
                                        Number((event.currentTarget as HTMLInputElement).value) ||
                                            20,
                                    )}
                                disabled={$fileSearchIndexing}
                                class="fsi-num-input"
                            />
                            <span class="fsi-field-hint">
                                Caps OCR at this many pages per PDF. Default 20; raise for
                                long contracts.
                            </span>
                        </label>
                    </div>
                </div>

                <!-- Wave 8.2 (2026-05-28): smart-skip heuristics. Three
                     dials that filter out images before Tesseract spawns
                     on them — thumbnails, banners, runaway scans. Set any
                     to 0 to disable that particular check. -->
                <div class="fsi-exclude-grid" style="grid-template-columns: 1fr 1fr 1fr; margin-top: 12px;">
                    <div class="fsi-exclude-col">
                        <label class="fsi-field">
                            <span class="fsi-field-label">Min image size (px)</span>
                            <input
                                type="number"
                                min="0"
                                max="5000"
                                step="50"
                                value={$fileSearchOcrMinImageDim}
                                onchange={(event) =>
                                    setFileSearchOcrMinImageDim(
                                        Number((event.currentTarget as HTMLInputElement).value),
                                    )}
                                disabled={$fileSearchIndexing}
                                class="fsi-num-input"
                            />
                            <span class="fsi-field-hint">
                                Skips tiny images (icons, thumbnails). Default 400. 0 = off.
                            </span>
                        </label>
                    </div>

                    <div class="fsi-exclude-col">
                        <label class="fsi-field">
                            <span class="fsi-field-label">Max aspect ratio</span>
                            <input
                                type="number"
                                min="0"
                                max="50"
                                step="0.5"
                                value={$fileSearchOcrMaxAspectRatio}
                                onchange={(event) =>
                                    setFileSearchOcrMaxAspectRatio(
                                        Number((event.currentTarget as HTMLInputElement).value),
                                    )}
                                disabled={$fileSearchIndexing}
                                class="fsi-num-input"
                            />
                            <span class="fsi-field-hint">
                                Skips banner-shaped images (longer side ÷ shorter side).
                                Default 5. 0 = off.
                            </span>
                        </label>
                    </div>

                    <div class="fsi-exclude-col">
                        <label class="fsi-field">
                            <span class="fsi-field-label">Per-file timeout (sec)</span>
                            <input
                                type="number"
                                min="0"
                                max="600"
                                step="5"
                                value={$fileSearchOcrPerFileTimeoutSecs}
                                onchange={(event) =>
                                    setFileSearchOcrTimeoutSecs(
                                        Number((event.currentTarget as HTMLInputElement).value),
                                    )}
                                disabled={$fileSearchIndexing}
                                class="fsi-num-input"
                            />
                            <span class="fsi-field-hint">
                                Kills OCR after this many seconds. Default 30. 0 = no
                                timeout (not recommended).
                            </span>
                        </label>
                    </div>
                </div>
            {/if}
        </div>

        <div class="fsi-sheet-divider"></div>

        <!-- Scheduled rebuild card -->
        <div class="fsi-sheet-section">
            <SectionHeader
                icon={Clock}
                title={$_('page.fileSearchIndex.schedule.title')}
                description={$_('page.fileSearchIndex.schedule.desc')}
            >
                {#snippet actions()}
                    <label class="fsi-inline-toggle">
                        <Checkbox
                            bind:checked={$fileSearchRebuildScheduleEnabled}
                            disabled={$fileSearchIndexing}
                        />
                        {$_('page.fileSearchIndex.schedule.enabled')}
                    </label>
                {/snippet}
            </SectionHeader>

            <div class="fsi-schedule-grid">
                <label class="fsi-field">
                    <span class="fsi-field-label">{$_('page.fileSearchIndex.schedule.everyNHours')}</span>
                    <input
                        type="number"
                        min="1"
                        max="720"
                        step="1"
                        bind:value={$fileSearchRebuildIntervalHours}
                        disabled={!$fileSearchRebuildScheduleEnabled || $fileSearchIndexing}
                        class="fsi-num-input"
                    />
                    <span class="fsi-field-hint">{$_('page.fileSearchIndex.schedule.hoursHint')}</span>
                </label>

                <div class="fsi-schedule-next">
                    <div class="fsi-field-label">{$_('page.fileSearchIndex.schedule.nextRun')}</div>
                    <div class="fsi-schedule-next-value">
                        {nextScheduledRebuildTime() ? formatDate(nextScheduledRebuildTime()) : $_('page.fileSearchIndex.schedule.notScheduledYet')}
                    </div>
                </div>
            </div>

            <div class="fsi-schedule-save">
                <Button
                    variant="secondary"
                    size="sm"
                    onclick={saveSearchRebuildSchedule}
                    disabled={$fileSearchIndexing}
                >
                    {$_('page.fileSearchIndex.schedule.save')}
                </Button>
            </div>
        </div>
    </div>
</SideSheet>

{#snippet buildCancelButton(start: () => void, disabledWhenIdle: boolean)}
    {#if $fileSearchIndexing}
        <Button
            variant="danger"
            size="sm"
            icon={XCircle}
            onclick={cancelFileSearchIndexBuild}
            disabled={$fileSearchCancelling}
        >
            {$fileSearchCancelling ? $_('page.fileSearchIndex.cancelling') : $_('page.fileSearchIndex.cancelBuild')}
        </Button>
    {:else}
        <Button
            variant="primary"
            size="sm"
            icon={Play}
            onclick={start}
            disabled={disabledWhenIdle}
        >
            {$_('page.fileSearchIndex.buildRebuild')}
        </Button>
    {/if}
{/snippet}

{#snippet liveWatcherSection(tab: 'file' | 'content')}
    <ToolPanel padding="lg" as="section">
        <SectionHeader
            title={$_('page.fileSearchIndex.watcher.title')}
            description={$_('page.fileSearchIndex.watcher.desc')}
        >
            {#snippet actions()}
                <label class="fsi-inline-toggle">
                    <Checkbox
                        bind:checked={$fileSearchWatcherEnabled}
                        onchange={() => void saveFileSearchIndexOptions()}
                        disabled={$fileSearchIndexing}
                    />
                    {$_('page.fileSearchIndex.watcher.enabled')}
                </label>
            {/snippet}
        </SectionHeader>

        <div class="fsi-watcher-status">
            {#if tab === 'file'}
                {watcherLabel}
            {:else}
                {$fileSearchStatus?.diagnostics.lastWorkerMessage ?? $_('page.fileSearchIndex.watcher.startHint')}
            {/if}
        </div>

        {#if tab === 'content'}
            <div class="fsi-watcher-actions">
                <Button
                    variant="secondary"
                    size="sm"
                    icon={PauseCircle}
                    onclick={stopFileSearchWatcher}
                    disabled={$fileSearchIndexing || !$fileSearchStatus?.watching}
                >
                    {$_('page.fileSearchIndex.watcher.stop')}
                </Button>
            </div>
        {/if}
    </ToolPanel>
{/snippet}

{#snippet advancedIndexingSection(full: boolean)}
    <div class="fsi-sheet-section">
        <SectionHeader
            icon={Settings2}
            title={$_('page.fileSearchIndex.advanced.title')}
            description={$_('page.fileSearchIndex.advanced.desc')}
        />

        {#if full}
            <div class="fsi-exclude-grid">
                <div class="fsi-exclude-col">
                    <div class="fsi-exclude-head">
                        <span>{$_('page.fileSearchIndex.exclusions.excludedFolders')}</span>
                        <span>{formatCount($fileSearchExcludeFolders.length)}</span>
                    </div>
                    <form
                        onsubmit={(event) => {
                            event.preventDefault();
                            addExcludeFolderDraft();
                        }}
                        class="fsi-exclude-form"
                    >
                        <TextInput
                            size="sm"
                            bind:value={excludeFolderDraft}
                            placeholder="node_modules, .git, build"
                            disabled={$fileSearchIndexing}
                        />
                        <Button
                            type="submit"
                            variant="secondary"
                            size="sm"
                            disabled={!excludeFolderDraft.trim() || $fileSearchIndexing}
                        >
                            {$_('page.fileSearchIndex.exclusions.add')}
                        </Button>
                    </form>
                    <div class="fsi-chip-box">
                        <div class="fsi-chip-row">
                            {#each $fileSearchExcludeFolders as folder}
                                <button
                                    type="button"
                                    onclick={() => removeFileSearchExcludeFolder(folder)}
                                    disabled={$fileSearchIndexing}
                                    title={$_('page.fileSearchIndex.exclusions.removeFolder', { values: { folder } })}
                                    class="fsi-chip"
                                >
                                    <span class="fsi-chip-text">{folder}</span>
                                </button>
                            {/each}
                        </div>
                    </div>
                </div>

                <div class="fsi-exclude-col">
                    <div class="fsi-exclude-head">
                        <span>{$_('page.fileSearchIndex.exclusions.excludedExtensions')}</span>
                        <span>{formatCount($fileSearchExcludeExtensions.length)}</span>
                    </div>
                    <form
                        onsubmit={(event) => {
                            event.preventDefault();
                            addExcludeExtensionDraft();
                        }}
                        class="fsi-exclude-form"
                    >
                        <TextInput
                            size="sm"
                            bind:value={excludeExtensionDraft}
                            placeholder="dll, tmp, gitignore"
                            disabled={$fileSearchIndexing}
                        />
                        <Button
                            type="submit"
                            variant="secondary"
                            size="sm"
                            disabled={!excludeExtensionDraft.trim() || $fileSearchIndexing}
                        >
                            {$_('page.fileSearchIndex.exclusions.add')}
                        </Button>
                    </form>
                    <div class="fsi-chip-box">
                        <div class="fsi-chip-row">
                            {#each $fileSearchExcludeExtensions as extension}
                                <button
                                    type="button"
                                    onclick={() => removeFileSearchExcludeExtension(extension)}
                                    disabled={$fileSearchIndexing}
                                    title={$_('page.fileSearchIndex.exclusions.removeExtension', { values: { extension } })}
                                    class="fsi-chip"
                                >
                                    .{extension}
                                </button>
                            {/each}
                        </div>
                    </div>
                </div>
            </div>

            <div class="fsi-exclude-defaults">
                <Button
                    variant="secondary"
                    size="sm"
                    icon={RotateCcw}
                    onclick={restoreSearchExclusionDefaults}
                    disabled={$fileSearchIndexing}
                >
                    {$_('page.fileSearchIndex.exclusions.defaults')}
                </Button>
            </div>
        {/if}

        <div class="fsi-adv-options">
            {#if full}
                <label class="fsi-check-card">
                    <Checkbox
                        bind:checked={$fileSearchIncludeHidden}
                        onchange={() => void saveFileSearchIndexOptions()}
                        disabled={$fileSearchIndexing}
                    />
                    {$_('page.fileSearchIndex.advanced.includeHidden')}
                </label>
                <!-- Wave 7.7 (2026-05-28): "Follow symlinks" toggle
                     removed. On Windows it follows NTFS reparse points
                     (pnpm store, OneDrive placeholders, junctions),
                     inflating the index ~3× with duplicate paths and
                     triggering cloud-sync downloads. Walker is now
                     permanently non-following. -->
            {/if}
            <label class="fsi-field fsi-adv-perf">
                <span class="fsi-field-label">{$_('page.fileSearchIndex.advanced.performanceMode')}</span>
                <Select
                    bind:value={$fileSearchPerformanceMode}
                    onchange={() => void saveFileSearchIndexOptions()}
                    options={[
                        { value: 'balanced', label: $_('page.fileSearchIndex.advanced.modeBalanced') },
                        { value: 'fast', label: $_('page.fileSearchIndex.advanced.modeFast') },
                        { value: 'quiet', label: $_('page.fileSearchIndex.advanced.modeQuiet') },
                    ]}
                />
            </label>
        </div>
        {#if full}
            <p class="fsi-field-hint">
                {$_('page.fileSearchIndex.exclusions.desc')}
            </p>
        {/if}
    </div>
{/snippet}

<style>
    /* ─── Identity badge pill ───────────────────────────────────── */
    .fsi-badge-row {
        display: flex;
    }
    .fsi-badge {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 4px 10px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-pill);
        background: var(--color-panel-2);
        font-size: 11.5px;
        color: var(--color-muted);
    }
    .fsi-badge :global(.fsi-badge-ico) {
        width: 13px;
        height: 13px;
        color: var(--color-accent);
    }

    /* ─── Stat tiles ─────────────────────────────────────────────
       Calm type scale — numbers at ~19px (kit-restrained) rather than
       the old text-2xl. Tokenized throughout. */
    .fsi-stat-grid {
        display: grid;
        gap: 12px;
        grid-template-columns: 1fr;
    }
    @media (min-width: 640px) {
        .fsi-stat-grid {
            grid-template-columns: repeat(2, 1fr);
        }
    }
    @media (min-width: 1200px) {
        .fsi-stat-grid {
            grid-template-columns: repeat(4, 1fr);
        }
    }
    .fsi-stat {
        min-width: 0;
        padding: 14px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel-2);
    }
    .fsi-stat-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
    }
    .fsi-stat-label {
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.05em;
        color: var(--color-muted);
    }
    .fsi-stat :global(.fsi-stat-ico) {
        width: 15px;
        height: 15px;
        color: var(--color-accent);
        flex: none;
    }
    .fsi-stat-value {
        margin-top: 10px;
        font-size: 19px;
        font-weight: 600;
        line-height: 1.2;
        color: var(--color-text);
        letter-spacing: -0.01em;
    }
    .fsi-stat-meta {
        margin-top: 3px;
        font-size: 11.5px;
        line-height: 1.4;
        color: var(--color-muted);
    }
    .fsi-truncate {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    /* Watcher tone helpers — replace the raw text-emerald/amber classes
       the script's `watcherTone` derived value still emits, mapped onto
       tokens via :global so the dynamic class string resolves. */
    :global(.fsi-stat-value.text-success) {
        color: var(--color-success);
    }
    :global(.fsi-stat-value.text-warning) {
        color: var(--color-warning);
    }
    :global(.fsi-stat-value.text-muted) {
        color: var(--color-muted);
    }

    /* ─── Build-progress banner (warning-toned, tokenized) ──────── */
    .fsi-progress {
        padding: 16px;
        border: 1px solid var(--color-warning-strong);
        border-radius: var(--radius-card);
        background: var(--color-warning-soft);
        font-size: 13px;
        color: var(--color-text);
    }
    .fsi-progress-row {
        display: flex;
        flex-direction: column;
        gap: 10px;
    }
    @media (min-width: 640px) {
        .fsi-progress-row {
            flex-direction: row;
            align-items: center;
            justify-content: space-between;
        }
    }
    .fsi-progress-text {
        min-width: 0;
    }
    .fsi-progress-title {
        font-weight: 600;
        color: var(--color-text);
    }
    .fsi-progress-msg {
        margin-top: 3px;
        font-size: 11.5px;
        color: var(--color-text-secondary);
    }
    .fsi-progress-stats {
        display: grid;
        grid-template-columns: repeat(3, 1fr);
        gap: 8px;
        text-align: center;
    }
    @media (min-width: 640px) {
        .fsi-progress-stats {
            min-width: 18rem;
        }
    }
    .fsi-progress-stat {
        padding: 6px 8px;
        border: 1px solid color-mix(in srgb, var(--color-warning) 25%, transparent);
        border-radius: var(--radius-control);
        background: color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .fsi-progress-stat-label {
        font-size: 11px;
        color: var(--color-text-secondary);
    }
    .fsi-progress-stat-value {
        font-weight: 600;
        font-variant-numeric: tabular-nums;
    }

    /* Wave 5 (2026-05-28): first-run hero (UxAudit UX-A-04).
       Replaces the power-user engine console with a friendly card
       when no folders are configured and no index has been built.
       The text-align:center is deliberate — this is the only
       block-styled card on the page; it should read as the calm
       welcome state, not as part of the dense settings flow. */
    .fsi-firstrun {
        display: flex;
        flex-direction: column;
        align-items: center;
        text-align: center;
        gap: 12px;
        padding: 8px 0 4px;
    }
    .fsi-firstrun-icon {
        width: 56px;
        height: 56px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: color-mix(in srgb, var(--color-accent) 12%, transparent);
        border-radius: 50%;
        color: var(--color-accent);
        margin-bottom: 4px;
    }
    .fsi-firstrun :global(.fsi-firstrun-icon-svg) {
        width: 26px;
        height: 26px;
    }
    .fsi-firstrun-title {
        margin: 0;
        font-size: 18px;
        font-weight: 600;
        color: var(--color-text);
        letter-spacing: -0.005em;
    }
    .fsi-firstrun-lede {
        margin: 0;
        max-width: 56ch;
        font-size: 13.5px;
        line-height: 1.55;
        color: var(--color-text-secondary);
    }
    .fsi-firstrun-actions {
        margin: 8px 0 4px;
    }
    .fsi-firstrun-note {
        margin: 0;
        font-size: 11.5px;
        color: var(--color-muted);
        font-style: italic;
    }

    /* Wave 5 (2026-05-28): progress bar — dual-mode.
       UxAudit UX-A-05.
         - Indeterminate (`.fsi-progress-bar-fill`): sliding gradient
           shown during the early walker-burst when we don't yet have
           a reliable total. Communicates "still working".
         - Determinate (`.fsi-progress-bar-determinate`): real fill
           width = `indexed / scannedEntries` once that ratio is
           stable. Smooth CSS transition on width so the handover
           from indeterminate-to-determinate looks like one bar.
       Wave 5.5 (2026-05-28): added the determinate path + percent badge. */
    .fsi-progress-bar {
        margin-top: 12px;
        height: 8px;
        border-radius: 4px;
        overflow: hidden;
        background: color-mix(in srgb, var(--color-text) 6%, transparent);
        position: relative;
    }
    .fsi-progress-bar-fill {
        position: absolute;
        inset: 0;
        background: linear-gradient(
            90deg,
            transparent,
            var(--color-accent),
            transparent
        );
        width: 40%;
        animation: fsi-progress-bar-slide 1400ms linear infinite;
    }
    .fsi-progress-bar-determinate {
        position: absolute;
        inset: 0 auto 0 0;
        background: linear-gradient(
            90deg,
            color-mix(in srgb, var(--color-accent) 70%, transparent),
            var(--color-accent)
        );
        border-radius: 4px;
        /* Smooth growth so the bar feels alive without "jumping"
           every time a new sample lands. */
        transition: width 320ms cubic-bezier(0.22, 1, 0.36, 1);
    }
    .fsi-progress-bar-percent {
        position: absolute;
        right: 0;
        top: -18px;
        font-size: 11px;
        font-weight: 600;
        font-variant-numeric: tabular-nums;
        color: var(--color-text-secondary);
        letter-spacing: 0.02em;
    }
    @keyframes fsi-progress-bar-slide {
        from { transform: translateX(-100%); }
        to   { transform: translateX(260%); }
    }
    @media (prefers-reduced-motion: reduce) {
        .fsi-progress-bar-fill {
            animation: none;
            width: 100%;
            opacity: 0.5;
        }
        .fsi-progress-bar-determinate {
            transition: none;
        }
    }

    /* Task #64 (2026-05-27): the "Starting…" state — small inline
       spinner shown when the worker hasn't reported any numbers yet,
       in place of the 0/0/0 count grid. */
    .fsi-progress-starting {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        padding: 8px 16px;
        min-width: 64px;
    }
    .fsi-progress-spinner {
        width: 16px;
        height: 16px;
        border: 2px solid color-mix(in srgb, var(--color-accent) 28%, transparent);
        border-top-color: var(--color-accent);
        border-radius: 50%;
        animation: fsi-progress-spin 720ms linear infinite;
    }
    @keyframes fsi-progress-spin {
        to {
            transform: rotate(360deg);
        }
    }

    /* ─── Overlay quick-strip ────────────────────────────────────── */
    .fsi-overlay-strip {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    @media (min-width: 640px) {
        .fsi-overlay-strip {
            flex-direction: row;
            align-items: center;
            justify-content: space-between;
        }
    }
    .fsi-overlay-text {
        min-width: 0;
    }
    .fsi-overlay-head {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .fsi-overlay-strip :global(.fsi-overlay-ico) {
        width: 15px;
        height: 15px;
        color: var(--color-accent);
        flex: none;
    }
    .fsi-overlay-title {
        margin: 0;
        font-size: 15px;
        font-weight: 600;
        color: var(--color-text);
    }
    .fsi-overlay-desc {
        margin-top: 4px;
        font-size: 13px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }
    .fsi-overlay-link {
        font-weight: 500;
        color: var(--color-accent);
        background: none;
        border: none;
        padding: 0;
        cursor: pointer;
        text-underline-offset: 2px;
    }
    .fsi-overlay-link:hover {
        text-decoration: underline;
    }

    /* ─── Tabs row ───────────────────────────────────────────────── */
    .fsi-tabs-row {
        display: flex;
    }
    .fsi-tab-desc {
        margin: -4px 0 0;
        font-size: 13px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }

    /* ─── Banners (drive-only / mixed) ──────────────────────────── */
    .fsi-banner {
        padding: 14px 16px;
        border-radius: var(--radius-card);
        font-size: 13px;
        line-height: 1.5;
    }
    .fsi-banner-warning {
        border: 1px solid var(--color-warning-strong);
        background: var(--color-warning-soft);
        color: var(--color-text);
    }
    .fsi-banner-neutral {
        border: 1px solid var(--color-border);
        background: var(--color-panel);
        color: var(--color-text-secondary);
    }

    /* ─── Engine selector cards ─────────────────────────────────── */
    .fsi-engine-grid {
        margin-top: 16px;
        display: grid;
        gap: 12px;
    }
    @media (min-width: 640px) {
        .fsi-engine-grid {
            grid-template-columns: repeat(2, 1fr);
        }
    }
    .fsi-engine-card {
        display: flex;
        align-items: flex-start;
        gap: 12px;
        padding: 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel-2);
        cursor: pointer;
        transition: border-color var(--dur-micro) var(--ease-out);
    }
    .fsi-engine-card.is-selected {
        border-color: var(--color-accent);
    }
    .fsi-radio {
        margin-top: 2px;
        flex: none;
        accent-color: var(--color-accent);
    }
    .fsi-engine-text {
        min-width: 0;
    }
    .fsi-engine-name {
        font-size: 13px;
        font-weight: 500;
        color: var(--color-text);
    }
    .fsi-engine-hint {
        margin-top: 2px;
        font-size: 11.5px;
        line-height: 1.4;
        color: var(--color-muted);
    }

    /* ─── MFT admin sub-card ────────────────────────────────────── */
    .fsi-mft-card {
        margin-top: 12px;
        padding: 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel-2);
    }
    .fsi-mft-needsadmin {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
    }
    .fsi-mft-status {
        margin-top: 6px;
        font-size: 11.5px;
        color: var(--color-muted);
    }
    .fsi-text-success {
        font-size: 11.5px;
        color: var(--color-success);
    }
    .fsi-text-warning {
        font-size: 11.5px;
        color: var(--color-warning);
    }

    /* ─── Source lists ──────────────────────────────────────────── */
    .fsi-sources-meta {
        margin-top: 16px;
        display: flex;
        align-items: center;
        justify-content: space-between;
        font-size: 11.5px;
        color: var(--color-muted);
    }
    .fsi-clear-btn {
        background: none;
        border: none;
        padding: 0;
        cursor: pointer;
        color: var(--color-muted);
        font-size: 11.5px;
        transition: color var(--dur-micro) var(--ease-out);
    }
    .fsi-clear-btn:hover:not(:disabled) {
        color: var(--color-error);
    }
    .fsi-clear-btn:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }
    .fsi-sources-list {
        margin-top: 10px;
        max-height: 24rem;
        overflow: auto;
        margin-bottom: 16px;
    }

    /* ─── Index status sub-card ─────────────────────────────────── */
    .fsi-status-row {
        display: flex;
        flex-wrap: wrap;
        align-items: flex-end;
        justify-content: space-between;
        gap: 12px;
    }
    .fsi-status-text {
        min-width: 0;
    }
    .fsi-status-msg {
        margin-top: 8px;
        font-size: 11.5px;
        color: var(--color-muted);
    }

    /* ─── Content tuning fields ─────────────────────────────────── */
    .fsi-tuning-grid {
        margin-top: 16px;
        margin-bottom: 16px;
        display: grid;
        gap: 12px;
    }
    @media (min-width: 640px) {
        .fsi-tuning-grid {
            grid-template-columns: repeat(2, 1fr);
        }
    }
    .fsi-field {
        display: flex;
        flex-direction: column;
        gap: 5px;
    }
    .fsi-field-label {
        font-size: 11.5px;
        color: var(--color-muted);
    }
    .fsi-field-hint {
        font-size: 11px;
        line-height: 1.45;
        color: var(--color-muted);
    }
    /* Native number inputs — the kit TextInput is string-typed, so
       numeric two-way bindings (maxContentKb / commitEvery / interval)
       stay on a native control styled to the kit's tokens. Mirrors the
       ClipboardHistory SideSheet's `.ch-sheet-input-num` approach. */
    .fsi-num-input {
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
    .fsi-num-input:hover:not(:disabled):not(:focus) {
        border-color: var(--color-border-strong);
    }
    .fsi-num-input:focus {
        border-color: var(--color-accent);
    }
    .fsi-num-input:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    /* ─── Live watcher ──────────────────────────────────────────── */
    .fsi-inline-toggle {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        font-size: 12px;
        color: var(--color-text);
        cursor: pointer;
    }
    .fsi-watcher-status {
        margin-top: 16px;
        padding: 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel-2);
        font-size: 11.5px;
        color: var(--color-muted);
    }
    .fsi-watcher-actions {
        margin-top: 12px;
    }

    /* ─── SideSheet content ─────────────────────────────────────── */
    .fsi-sheet-stack {
        display: flex;
        flex-direction: column;
        gap: 20px;
    }
    .fsi-sheet-section {
        display: flex;
        flex-direction: column;
        gap: 0;
    }
    .fsi-sheet-divider {
        height: 1px;
        background: var(--color-divider, var(--color-border));
    }

    /* ─── Exclusions (chips + add forms) ────────────────────────── */
    .fsi-exclude-grid {
        margin-top: 16px;
        display: grid;
        gap: 16px;
    }
    @media (min-width: 768px) {
        .fsi-exclude-grid {
            grid-template-columns: repeat(2, 1fr);
        }
    }
    .fsi-exclude-col {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .fsi-exclude-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        font-size: 11.5px;
        color: var(--color-muted);
    }
    .fsi-exclude-form {
        display: flex;
        gap: 8px;
        align-items: center;
    }
    .fsi-exclude-form :global(.field) {
        flex: 1;
        min-width: 0;
    }
    .fsi-chip-box {
        max-height: 14rem;
        overflow: auto;
        padding: 8px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel-2);
    }
    .fsi-chip-row {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
    }
    .fsi-chip {
        max-width: 100%;
        display: inline-flex;
        align-items: center;
        padding: 4px 8px;
        border-radius: var(--radius-pill);
        border: 1px solid var(--color-border);
        background: var(--color-panel);
        color: var(--color-muted);
        font-size: 11.5px;
        cursor: pointer;
        transition:
            border-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .fsi-chip:hover:not(:disabled) {
        border-color: var(--color-error-strong, color-mix(in srgb, var(--color-error) 40%, var(--color-border)));
        color: var(--color-error);
    }
    .fsi-chip:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }
    .fsi-chip-text {
        display: inline-block;
        max-width: 11rem;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        vertical-align: bottom;
    }
    /* Wave 8.4.2: empty-state copy inside an otherwise-empty chip box.
       Same muted-text feel as .fsi-hint but constrained to the box. */
    .fsi-chip-empty {
        margin: 0;
        font-size: 12px;
        line-height: 1.5;
        color: var(--color-muted);
    }
    .fsi-chip-empty b {
        color: var(--color-text);
        font-weight: 600;
    }
    .fsi-exclude-defaults {
        margin-top: 16px;
        display: flex;
        justify-content: flex-end;
    }

    /* ─── Advanced options (checkbox cards + perf select) ───────── */
    .fsi-adv-options {
        margin-top: 16px;
        display: grid;
        gap: 12px;
    }
    @media (min-width: 640px) {
        .fsi-adv-options {
            grid-template-columns: repeat(2, 1fr);
        }
    }
    .fsi-check-card {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 8px 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel-2);
        font-size: 13px;
        color: var(--color-text);
        cursor: pointer;
    }
    .fsi-adv-perf {
        grid-column: 1 / -1;
    }

    /* ─── Schedule card ─────────────────────────────────────────── */
    .fsi-schedule-grid {
        margin-top: 16px;
        display: grid;
        gap: 12px;
    }
    .fsi-schedule-next {
        padding: 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel-2);
    }
    .fsi-schedule-next-value {
        margin-top: 4px;
        font-size: 13px;
        font-weight: 500;
        color: var(--color-text);
    }
    .fsi-schedule-save {
        margin-top: 12px;
    }
</style>
