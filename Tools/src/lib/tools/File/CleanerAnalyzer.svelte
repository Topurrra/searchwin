<script lang="ts">
    /*
      Cleaner / Analyzer — recomposed (2026-06) from the foundation kit.

      A deliberate in-suite convenience: one click to analyze + clean, no need
      to install WizTree. Whole-drive disk-usage map across ALL fixed drives
      (pure WalkDir, no admin/MFT), large-old-file review, startup inventory,
      disk + RAM health, and safe cache cleanup. The backend streams within a
      low-end memory budget, sizes its walk concurrency to live free RAM, can be
      cancelled mid-scan, and emits coarse stage progress.

      Safety model (unchanged + binding): ONLY known cache/temp targets are
      cleanable, and cleanup requires typing CONFIRM. User files are review-only.

      The previous hand-rolled component is preserved as CleanerAnalyzer.old.svelte.
    */
    import { invoke } from '@tauri-apps/api/core';
    import { listen, type UnlistenFn } from '@tauri-apps/api/event';
    import { onMount } from 'svelte';
    import {
        AlertTriangle,
        Ban,
        CheckCircle2,
        Copy,
        Cpu,
        ExternalLink,
        FolderOpen,
        HardDrive,
        MemoryStick,
        RotateCw,
        Search,
        Trash2,
        XCircle,
    } from '@lucide/svelte';
    import { get } from 'svelte/store';
    import { formatBytes } from '$lib/stores/fileSearch';
    import { notify } from '$lib/stores/notifications';
    import { toast } from '$lib/stores/toasts';
    import { reportBusy } from '$lib/stores/globalBusy';
    import {
        cleanerReport,
        cleanerSelectedTargetIds,
        cleanerLastCleanResult,
        cleanerDetailTab,
        cleanerRuntime,
        type CleanerRuntimeState,
        type CleanerAnalyzeReport,
        type CleanerCleanResult,
        type FileTypeBreakdownEntry,
        type TopUserDirEntry,
    } from '$lib/stores/cleanerAnalyzer';
    import { escToClear } from '$lib/actions/escToClear';
    import LoadingState from '$lib/components/LoadingState.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import { ToolPage, ToolPanel, Tabs, Button, ErrorState } from '$lib/ui';

    // ─── Backend data shapes (serde camelCase) ───────────────────────
    // The result/report shapes now live in the cleanerAnalyzer store
    // (imported above) so they survive navigation. CleanerProgress stays
    // local — it's transient per-op live progress, never persisted.
    type CleanerProgress = {
        operationId: string;
        stage: string;
        stageIndex: number;
        stageTotal: number;
        entriesScanned: number;
        currentPath: string;
    };

    // ─── State ───────────────────────────────────────────────────────
    // Transient (local) — in-flight UI + the CONFIRM gate. Deliberately
    // NOT persisted; see cleanerAnalyzer.ts for the rationale.
    let analyzing = $derived($cleanerRuntime.analyzing);
    let cancelling = $derived($cleanerRuntime.cancelling);
    let cleaning = $derived($cleanerRuntime.cleaning);
    let includeCacheScan = $state(true);
    let includeOldFileReview = $state(true);
    let oldFileDays = $state(365);
    let maxOldFiles = $state(60);
    let confirmText = $state('');
    let errorMessage = $derived($cleanerRuntime.error);
    let progress = $derived($cleanerRuntime.progress);

    // Persisted RESULT/selection — hydrate from the store on mount, mirror
    // back via $effect, so a completed analysis survives navigation
    // (the app remounts tools via {#key}, wiping component-local $state).
    let report = $derived($cleanerReport);
    let selectedTargetIds = $derived($cleanerSelectedTargetIds);
    let lastCleanResult = $derived($cleanerLastCleanResult);

    let activeOpId = $derived($cleanerRuntime.operationId ?? '');
    let unlistenProgress: UnlistenFn | null = null;

    function setRuntime(next: CleanerRuntimeState) {
        cleanerRuntime.set(next);
    }

    function updateRuntime(patch: Partial<CleanerRuntimeState>) {
        setRuntime({ ...get(cleanerRuntime), ...patch });
    }
    // Plain string so it binds cleanly to <Tabs active> (a string bindable).
    // Persisted so the user returns to the same detail tab.
    let detailTab = $state<string>(get(cleanerDetailTab));
    $effect(() => { cleanerDetailTab.set(detailTab); });

    // ─── Progress subscription (per-op, throttled events from backend) ─
    onMount(() => {
        let disposed = false;
        void listen<CleanerProgress>('cleaner-analyze-progress', (event) => {
            const operationId = get(cleanerRuntime).operationId;
            if (!operationId || event.payload.operationId !== operationId) return;
            updateRuntime({ progress: event.payload });
        })
            .then((unlisten) => {
                if (disposed) {
                    unlisten();
                    return;
                }
                unlistenProgress = unlisten;
            })
            .catch(() => {});
        return () => {
            disposed = true;
            unlistenProgress?.();
            unlistenProgress = null;
        };
    });

    // 0..1 coarse determinate fill from stage index; null before the first
    // event (renders an indeterminate shimmer until stage 1 lands).
    let progressFraction = $derived.by<number | null>(() => {
        if (!progress || progress.stageTotal <= 0) return null;
        return Math.min(1, Math.max(0, progress.stageIndex / progress.stageTotal));
    });

    // ─── Derived layouts ─────────────────────────────────────────────
    let cleanableTargets = $derived(report?.cleanupTargets ?? []);
    let selectedTargets = $derived.by(() =>
        cleanableTargets.filter((t) => selectedTargetIds.includes(t.id)),
    );
    let selectedBytes = $derived.by(() => selectedTargets.reduce((s, t) => s + t.bytes, 0));
    let maxTargetBytes = $derived(cleanableTargets.reduce((m, t) => Math.max(m, t.bytes), 0));
    let maxOldFileBytes = $derived(
        (report?.oldLargeFiles ?? []).reduce((m, f) => Math.max(m, f.bytes), 0),
    );

    // All-drive "what's eating my disk" treemap. Tiles sized by percent of the
    // surveyed total; hue is stable per drive so a drive's folders cluster.
    let topUserDirs = $derived(report?.topUserDirs ?? []);
    let topUserDirsTotal = $derived(topUserDirs.reduce((s, d) => s + d.bytes, 0));
    let topUserDirsLayout = $derived.by(() => {
        const total = topUserDirsTotal;
        if (total === 0) return [] as Array<TopUserDirEntry & { percent: number; hueDeg: number }>;
        return topUserDirs.map((d) => ({
            ...d,
            percent: (d.bytes / total) * 100,
            hueDeg: hueForParent(d.parentLabel || d.label),
        }));
    });
    function hueForParent(parent: string): number {
        let h = 0;
        for (let i = 0; i < parent.length; i++) h = (h * 31 + parent.charCodeAt(i)) >>> 0;
        return h % 360;
    }

    let fileTypeBreakdown = $derived(report?.fileTypeBreakdown ?? []);
    let fileTypeTotal = $derived(fileTypeBreakdown.reduce((s, r) => s + r.bytes, 0));
    let fileTypeLayout = $derived.by(() => {
        const total = fileTypeTotal;
        if (total === 0)
            return [] as Array<FileTypeBreakdownEntry & { percent: number; color: string; emoji: string }>;
        return fileTypeBreakdown.map((r) => ({
            ...r,
            percent: (r.bytes / total) * 100,
            color: FILE_TYPE_COLORS[r.category] ?? FILE_TYPE_COLORS.Other,
            emoji: FILE_TYPE_EMOJI[r.category] ?? FILE_TYPE_EMOJI.Other,
        }));
    });
    const FILE_TYPE_COLORS: Record<string, string> = {
        Videos: 'rgb(244, 114, 182)',
        Images: 'rgb(96, 165, 250)',
        Audio: 'rgb(167, 139, 250)',
        Documents: 'rgb(251, 191, 36)',
        Code: 'rgb(52, 211, 153)',
        Archives: 'rgb(248, 113, 113)',
        Installers: 'rgb(251, 146, 60)',
        'Disk images': 'rgb(232, 121, 249)',
        Other: 'rgb(148, 163, 184)',
    };
    const FILE_TYPE_EMOJI: Record<string, string> = {
        Videos: '🎬',
        Images: '🖼️',
        Audio: '🎵',
        Documents: '📄',
        Code: '💻',
        Archives: '📦',
        Installers: '⚙️',
        'Disk images': '💿',
        Other: '📁',
    };

    const detailTabs = [
        { id: 'caches', label: 'Cache & temp' },
        { id: 'types', label: 'File types' },
        { id: 'old', label: 'Large old files' },
        { id: 'startup', label: 'Startup' },
        { id: 'tips', label: 'Tips' },
    ];

    // ─── Actions ─────────────────────────────────────────────────────
    async function analyze() {
        if (get(cleanerRuntime).analyzing) return;
        const operationId =
            globalThis.crypto?.randomUUID?.() ?? `cleaner-${Date.now()}-${Math.random()}`;
        updateRuntime({
            analyzing: true,
            cancelling: false,
            operationId,
            progress: null,
            error: null,
        });
        cleanerLastCleanResult.set(null);
        const stopBusy = reportBusy('cleaner-analyzer', 'Analyzing system');
        try {
            const nextReport = await invoke<CleanerAnalyzeReport>('analyze_system_cleaner', {
                options: {
                    includeCacheScan,
                    includeOldFileReview,
                    oldFileDays,
                    maxOldFiles,
                    operationId,
                },
            });
            cleanerReport.set(nextReport);
            const nextSelectedTargetIds = nextReport.cleanupTargets
                .filter((t) => t.safeToClean && t.bytes > 0)
                .map((t) => t.id);
            cleanerSelectedTargetIds.set(nextSelectedTargetIds);
            toast('Analysis complete', 'success');
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            if (/cancel/i.test(message)) {
                toast('Scan cancelled', 'info');
            } else {
                updateRuntime({ error: message });
                toast(message, 'error');
            }
        } finally {
            if (get(cleanerRuntime).operationId === operationId) {
                updateRuntime({
                    analyzing: false,
                    cancelling: false,
                    operationId: null,
                    progress: null,
                });
            }
            stopBusy();
        }
    }

    async function cancelScan() {
        if (!analyzing || !activeOpId || cancelling) return;
        updateRuntime({ cancelling: true });
        try {
            await invoke('cancel_cleaner_operation', { operationId: activeOpId });
            toast('Cancelling…', 'info');
        } catch {
            if (get(cleanerRuntime).operationId === activeOpId) updateRuntime({ cancelling: false });
        }
    }

    function toggleTarget(id: string) {
        cleanerSelectedTargetIds.set(selectedTargetIds.includes(id)
            ? selectedTargetIds.filter((t) => t !== id)
            : [...selectedTargetIds, id]);
    }
    function selectAllCleanable() {
        cleanerSelectedTargetIds.set(cleanableTargets
            .filter((t) => t.safeToClean && t.bytes > 0)
            .map((t) => t.id));
    }
    function clearSelection() {
        cleanerSelectedTargetIds.set([]);
    }

    let canClean = $derived(
        !cleaning && selectedTargetIds.length > 0 && confirmText.trim() === 'CONFIRM',
    );

    async function cleanSelected() {
        if (!canClean) return;
        updateRuntime({ cleaning: true });
        const stopBusy = reportBusy('cleaner-analyzer', 'Cleaning selected');
        try {
            const nextCleanResult = await invoke<CleanerCleanResult>('clean_system_cache', {
                payload: { targetIds: selectedTargetIds, confirm: confirmText },
            });
            cleanerLastCleanResult.set(nextCleanResult);
            toast(`Cleaned ${formatBytes(nextCleanResult.deletedBytes)}`, 'success');
            notify({
                level: nextCleanResult.success ? 'success' : 'warning',
                title: nextCleanResult.success
                    ? 'Cleaner finished'
                    : 'Cleaner finished with skipped items',
                message: `Deleted ${formatBytes(nextCleanResult.deletedBytes)} from ${nextCleanResult.deletedEntries} entries.`,
                toolId: 'cleaner-analyzer',
            });
            confirmText = '';
            await analyze();
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            toast(message, 'error');
        } finally {
            updateRuntime({ cleaning: false });
            stopBusy();
        }
    }

    async function copyPath(path: string) {
        await navigator.clipboard.writeText(path);
        toast('Path copied', 'success');
    }
    async function openPath(path: string) {
        try {
            await invoke('open_search_result_path', { path });
        } catch (error) {
            toast(error instanceof Error ? error.message : String(error), 'error');
        }
    }
    async function copyReport() {
        if (!report) return;
        const lines = [
            'Cleaner / Analyzer Report',
            `Generated: ${new Date(report.generatedAtMs).toLocaleString()}`,
            `Cleanable cache: ${formatBytes(report.totalCleanableBytes)}`,
            `Old large files: ${report.oldLargeFiles.length}`,
            `Startup items: ${report.startupItems.length}`,
            '',
            'Disk usage (all fixed drives):',
            ...report.topUserDirs.map(
                (d) => `- ${d.label} · ${d.parentLabel}: ${formatBytes(d.bytes)}`,
            ),
            '',
            'Cleanup targets:',
            ...report.cleanupTargets.map((t) => `- ${t.label}: ${formatBytes(t.bytes)} (${t.path})`),
            '',
            'Tips:',
            ...report.tips.map((t) => `- ${t}`),
        ];
        await navigator.clipboard.writeText(lines.join('\n'));
        toast('Report copied', 'success');
    }

    function formatPercent(value: number | null) {
        if (value === null || !Number.isFinite(value)) return 'Unknown';
        return `${value.toFixed(0)}%`;
    }
    function diskSeverity(percent: number | null): 'low' | 'mid' | 'high' {
        const p = percent ?? 0;
        if (p >= 90) return 'high';
        if (p >= 70) return 'mid';
        return 'low';
    }
    function formatDate(value: number | null) {
        if (!value) return 'Unknown';
        return new Date(value).toLocaleDateString();
    }
</script>

<ToolPage
    icon={HardDrive}
    iconTint="#64748b"
    title="See what's really filling your disk"
    description="Only known-safe cache targets are cleanable here; your files are review-only."
    width="wide"
    fill={false}
>
    {#snippet actions()}
        <Button variant="secondary" icon={Copy} onclick={copyReport} disabled={!report || analyzing}>
            Copy report
        </Button>
        {#if analyzing}
            <Button variant="danger" icon={Ban} onclick={cancelScan} disabled={cancelling} loading={cancelling}>
                {cancelling ? 'Cancelling' : 'Cancel'}
            </Button>
        {:else}
            <Button variant="primary" icon={Search} onclick={analyze}>Analyze</Button>
        {/if}
    {/snippet}

    <div class="ca-shell">
        <!-- Scan options -->
        <ToolPanel padding="sm" tone="panel-2">
            <div class="ca-scanbar-head">
                <span class="ca-step" aria-hidden="true">1</span>
                <span>Scan scope</span>
                <span class="ca-scanbar-note">Applies to the next analysis</span>
            </div>
            <div class="ca-options">
                <label class="ca-check">
                    <input type="checkbox" bind:checked={includeCacheScan} disabled={analyzing} />
                    <span>Scan cache &amp; temp</span>
                </label>
                <label class="ca-check">
                    <input type="checkbox" bind:checked={includeOldFileReview} disabled={analyzing} />
                    <span>Review large old files</span>
                </label>
                <label class="ca-field">
                    <span>Older than (days)</span>
                    <input type="number" min="30" max="3650" bind:value={oldFileDays} disabled={analyzing} />
                </label>
                <label class="ca-field">
                    <span>Max old files</span>
                    <input type="number" min="10" max="200" bind:value={maxOldFiles} disabled={analyzing} />
                </label>
            </div>
        </ToolPanel>

        {#if analyzing}
            <!-- LOADING: determinate-by-stage bar + live tail -->
            <ToolPanel padding="lg">
                <div class="ca-loading">
                    <LoadingState variant="inline" label={progress?.stage ?? 'Starting scan…'} />
                    <div
                        class="ca-progress"
                        role="progressbar"
                        aria-label="Analysis progress"
                        aria-valuemin={0}
                        aria-valuemax={100}
                        aria-valuenow={progressFraction === null ? undefined : Math.round(progressFraction * 100)}
                    >
                        {#if progressFraction === null}
                            <div class="ca-progress-fill ca-indeterminate"></div>
                        {:else}
                            <div class="ca-progress-fill" style="width:{(progressFraction * 100).toFixed(0)}%"></div>
                        {/if}
                    </div>
                    <div class="ca-progress-meta">
                        {#if progress}
                            <span>Stage {progress.stageIndex}/{progress.stageTotal}</span>
                            <span class="ca-dot">·</span>
                            <span>{progress.entriesScanned.toLocaleString()} files scanned</span>
                        {:else}
                            <span>Preparing…</span>
                        {/if}
                    </div>
                    {#if progress?.currentPath}
                        <div class="ca-progress-path" title={progress.currentPath}>{progress.currentPath}</div>
                    {/if}
                </div>
            </ToolPanel>
        {:else if errorMessage}
            <!-- ERROR: inline, recoverable -->
            <ErrorState
                title="Analysis failed"
                description={errorMessage}
                retry={analyze}
                retryLabel="Try again"
            />
        {:else if !report}
            <!-- EMPTY -->
            <ToolPanel padding="lg">
                <EmptyState
                    icon={Search}
                    title="Ready to analyze"
                    description="Scan all fixed drives to see what's using your space, find large old files, and reclaim cache. Nothing leaves your machine."
                >
                    {#snippet actions()}
                        <Button variant="primary" icon={Search} onclick={analyze}>Analyze now</Button>
                    {/snippet}
                </EmptyState>
            </ToolPanel>
        {:else}
            <!-- RESULTS -->
            <!-- Reclaimable hero + quick stats -->
            <ToolPanel padding="md">
                <div class="ca-hero">
                    <div class="ca-hero-main">
                        <div class="ca-hero-label">
                            <span class="ca-step" aria-hidden="true">2</span>
                            <span>Review overview</span>
                        </div>
                        <div class="ca-hero-value">{formatBytes(report.totalCleanableBytes)}</div>
                        <div class="ca-hero-sub">
                            {formatBytes(selectedBytes)} selected · {selectedTargetIds.length} of {cleanableTargets.length} targets
                        </div>
                    </div>
                    <div class="ca-stats">
                        <div class="ca-stat">
                            <div class="ca-stat-num">{report.oldLargeFiles.length}</div>
                            <div class="ca-stat-label">large old files</div>
                        </div>
                        <div class="ca-stat">
                            <div class="ca-stat-num">{report.startupItems.length}</div>
                            <div class="ca-stat-label">startup apps</div>
                        </div>
                        <div class="ca-stat">
                            <div class="ca-stat-num">{report.diskSummaries.length}</div>
                            <div class="ca-stat-label">drives</div>
                        </div>
                    </div>
                </div>
            </ToolPanel>

            <!-- Disk + RAM health -->
            <ToolPanel padding="md">
                <div class="ca-section-head"><HardDrive class="ca-head-ico" /><span>System health</span></div>
                <div class="ca-health">
                    {#each report.diskSummaries as disk (disk.path)}
                        <div class="ca-meter">
                            <div class="ca-meter-top">
                                <span class="ca-meter-name">{disk.path}</span>
                                <span class="ca-meter-val">
                                    {formatPercent(disk.usedPercent)}
                                    {#if disk.availableBytes != null}· {formatBytes(disk.availableBytes)} free{/if}
                                </span>
                            </div>
                            <div class="ca-bar">
                                <div
                                    class="ca-bar-fill sev-{diskSeverity(disk.usedPercent)}"
                                    style="width:{Math.min(100, disk.usedPercent ?? 0).toFixed(0)}%"
                                ></div>
                            </div>
                        </div>
                    {/each}
                    <div class="ca-meter">
                        <div class="ca-meter-top">
                            <span class="ca-meter-name"><MemoryStick class="ca-inline-ico" /> RAM</span>
                            <span class="ca-meter-val">
                                {formatPercent(report.memorySummary.usedPercent)}
                                {#if report.memorySummary.availableBytes != null}
                                    · {formatBytes(report.memorySummary.availableBytes)} free
                                {/if}
                            </span>
                        </div>
                        <div class="ca-bar">
                            <div
                                class="ca-bar-fill sev-{diskSeverity(report.memorySummary.usedPercent)}"
                                style="width:{Math.min(100, report.memorySummary.usedPercent ?? 0).toFixed(0)}%"
                            ></div>
                        </div>
                    </div>
                </div>
            </ToolPanel>

            <!-- All-drive disk-usage treemap -->
            {#if topUserDirsLayout.length > 0}
                <ToolPanel padding="md">
                    <div class="ca-section-head"><Cpu class="ca-head-ico" /><span>What's eating your disk — all drives</span></div>
                    <div class="ca-treemap">
                        {#each topUserDirsLayout as dir (dir.path)}
                            <button
                                type="button"
                                class="ca-tile"
                                style="flex-grow:{Math.max(1, dir.percent)}; --tile-hue:{dir.hueDeg};"
                                title={dir.path}
                                aria-label={`${dir.label} on ${dir.parentLabel}: ${formatBytes(dir.bytes)}. Open in Explorer.`}
                                onclick={() => openPath(dir.path)}
                            >
                                <span class="ca-tile-label">{dir.label}</span>
                                <span class="ca-tile-bytes">{formatBytes(dir.bytes)}</span>
                                <span class="ca-tile-parent">{dir.parentLabel}</span>
                            </button>
                        {/each}
                    </div>
                </ToolPanel>
            {/if}

            <!-- Detail tabs -->
            <ToolPanel padding="md">
                <div class="ca-details-head">
                    <div class="ca-section-title">
                        <span class="ca-step" aria-hidden="true">3</span>
                        <h3 class="ca-details-title">Review findings</h3>
                    </div>
                </div>
                <Tabs tabs={detailTabs} bind:active={detailTab} ariaLabel="Cleaner detail sections" />

                <div class="ca-tabbody">
                    {#if detailTab === 'caches'}
                        {#if cleanableTargets.length === 0}
                            <EmptyState icon={Trash2} title="No cache targets found" description="Nothing safe to clean was detected." variant="compact" />
                        {:else}
                            <div class="ca-tab-toolbar">
                                <Button size="sm" variant="ghost" onclick={selectAllCleanable}>Select all</Button>
                                <Button size="sm" variant="ghost" onclick={clearSelection}>Clear</Button>
                            </div>
                            <div class="ca-rows">
                                {#each cleanableTargets as target (target.id)}
                                    {@const sel = selectedTargetIds.includes(target.id)}
                                    <label class="ca-row" class:is-selected={sel}>
                                        <input type="checkbox" checked={sel} onchange={() => toggleTarget(target.id)} />
                                        <div class="ca-row-bg" style="width:{maxTargetBytes > 0 ? ((target.bytes / maxTargetBytes) * 100).toFixed(1) : 0}%"></div>
                                        <div class="ca-row-main">
                                            <div class="ca-row-title">{target.label}</div>
                                            <div class="ca-row-sub">{target.category} · {target.path}</div>
                                        </div>
                                        <div class="ca-row-meta">
                                            <span class="ca-row-bytes">{formatBytes(target.bytes)}</span>
                                            <span class="ca-row-count">{target.fileCount.toLocaleString()} files</span>
                                        </div>
                                    </label>
                                {/each}
                            </div>

                            <!-- CONFIRM-gated cleanup (the ONLY destructive surface) -->
                            <div class="ca-clean">
                                <div class="ca-clean-warn">
                                    <AlertTriangle class="ca-warn-ico" />
                                    <span>Deletes the contents of the selected cache/temp targets. Apps rebuild these on next launch. Your documents are never touched.</span>
                                </div>
                                <div class="ca-clean-row">
                                    <input
                                        class="ca-confirm"
                                        type="text"
                                        placeholder="Type CONFIRM to enable"
                                        aria-label="Type CONFIRM to enable cleanup"
                                        bind:value={confirmText}
                                        use:escToClear={() => (confirmText = '')}
                                    />
                                    <Button
                                        variant="danger"
                                        icon={Trash2}
                                        onclick={cleanSelected}
                                        disabled={!canClean}
                                        loading={cleaning}
                                    >
                                        Clean selected ({formatBytes(selectedBytes)})
                                    </Button>
                                </div>
                            </div>

                            {#if lastCleanResult}
                                <div class="ca-clean-result" class:ok={lastCleanResult.success}>
                                    {#if lastCleanResult.success}
                                        <CheckCircle2 class="ca-res-ico ok" />
                                    {:else}
                                        <AlertTriangle class="ca-res-ico warn" />
                                    {/if}
                                    <span>
                                        Deleted {formatBytes(lastCleanResult.deletedBytes)} from
                                        {lastCleanResult.deletedEntries.toLocaleString()} entries.
                                        {#if !lastCleanResult.success}Some items were skipped (locked or in use).{/if}
                                    </span>
                                </div>
                            {/if}
                        {/if}
                    {:else if detailTab === 'types'}
                        {#if fileTypeLayout.length === 0}
                            <EmptyState icon={FolderOpen} title="No file-type data" description="No home content folders were accessible." variant="compact" />
                        {:else}
                            <div class="ca-rows">
                                {#each fileTypeLayout as row (row.category)}
                                    <div class="ca-type">
                                        <div class="ca-type-top">
                                            <span class="ca-type-label"><span class="ca-emoji" aria-hidden="true">{row.emoji}</span>{row.category}</span>
                                            <span class="ca-type-val">{formatBytes(row.bytes)} · {row.percent.toFixed(0)}%</span>
                                        </div>
                                        <div class="ca-bar">
                                            <div class="ca-bar-fill" style="width:{row.percent.toFixed(1)}%; background:{row.color}"></div>
                                        </div>
                                    </div>
                                {/each}
                            </div>
                        {/if}
                    {:else if detailTab === 'old'}
                        {#if (report.oldLargeFiles ?? []).length === 0}
                            <EmptyState icon={FolderOpen} title="No large old files" description="No files over the size/age threshold were found." variant="compact" />
                        {:else}
                            <div class="ca-rows">
                                {#each report.oldLargeFiles as file (file.path)}
                                    <div class="ca-row is-review">
                                        <div class="ca-row-bg" style="width:{maxOldFileBytes > 0 ? ((file.bytes / maxOldFileBytes) * 100).toFixed(1) : 0}%"></div>
                                        <div class="ca-row-main">
                                            <div class="ca-row-title">{file.fileName}</div>
                                            <div class="ca-row-sub">{file.path}</div>
                                        </div>
                                        <div class="ca-row-meta">
                                            <span class="ca-row-bytes">{formatBytes(file.bytes)}</span>
                                            <span class="ca-row-count">
                                                {#if file.daysSinceModified != null}{file.daysSinceModified}d ago{:else}{formatDate(file.modifiedMs)}{/if}
                                            </span>
                                        </div>
                                        <div class="ca-row-actions">
                                            <button type="button" class="ca-icon-btn" title="Open location" aria-label="Open file location" onclick={() => openPath(file.path)}>
                                                <ExternalLink class="ca-act-ico" />
                                            </button>
                                            <button type="button" class="ca-icon-btn" title="Copy path" aria-label="Copy path" onclick={() => copyPath(file.path)}>
                                                <Copy class="ca-act-ico" />
                                            </button>
                                        </div>
                                    </div>
                                {/each}
                            </div>
                            <p class="ca-note">Review-only — the Cleaner never deletes your files.</p>
                        {/if}
                    {:else if detailTab === 'startup'}
                        {#if (report.startupItems ?? []).length === 0}
                            <EmptyState icon={RotateCw} title="No startup items" description="Nothing runs at startup from the locations we inspect." variant="compact" />
                        {:else}
                            <div class="ca-rows">
                                {#each report.startupItems as item (item.source + item.name)}
                                    <div class="ca-row is-review">
                                        <div class="ca-row-main">
                                            <div class="ca-row-title">{item.name}</div>
                                            <div class="ca-row-sub">{item.source} · {item.commandOrPath}</div>
                                        </div>
                                        <div class="ca-row-actions">
                                            <button type="button" class="ca-icon-btn" title="Copy command" aria-label="Copy command" onclick={() => copyPath(item.commandOrPath)}>
                                                <Copy class="ca-act-ico" />
                                            </button>
                                        </div>
                                    </div>
                                {/each}
                            </div>
                            <p class="ca-note">Manage these in Windows Settings → Apps → Startup. Listed for review only.</p>
                        {/if}
                    {:else if detailTab === 'tips'}
                        {#if (report.tips ?? []).length === 0}
                            <EmptyState icon={CheckCircle2} title="No tips" description="Nothing notable to flag right now." variant="compact" />
                        {:else}
                            <ul class="ca-tips">
                                {#each report.tips as tip}
                                    <li><CheckCircle2 class="ca-tip-ico" /><span>{tip}</span></li>
                                {/each}
                            </ul>
                        {/if}
                    {/if}
                </div>
            </ToolPanel>
        {/if}
    </div>
</ToolPage>

<style>
    .ca-shell {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }

    /* ── Scan options ── */
    .ca-options {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 16px;
    }
    .ca-scanbar-head {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
        margin-bottom: 8px;
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.05em;
        color: var(--color-text-secondary);
    }
    .ca-scanbar-note {
        margin-left: auto;
        font-size: 11px;
        font-weight: 400;
        text-transform: none;
        letter-spacing: 0;
        color: var(--color-muted);
    }
    .ca-step {
        flex: none;
        display: inline-grid;
        width: 18px;
        height: 18px;
        place-items: center;
        border: 1px solid var(--color-border);
        border-radius: 999px;
        color: var(--color-text-secondary);
        font-size: 10px;
        font-weight: 600;
        line-height: 1;
    }
    .ca-check {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        font-size: 13px;
        color: var(--color-text);
        cursor: pointer;
    }
    .ca-field {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        font-size: 12px;
        color: var(--color-text-secondary);
    }
    .ca-field input {
        width: 84px;
        height: 30px;
        padding: 0 8px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        color: var(--color-text);
        font-size: 13px;
    }

    /* ── Loading + progress ── */
    .ca-loading {
        display: flex;
        flex-direction: column;
        gap: 10px;
    }
    .ca-progress {
        height: 8px;
        border-radius: 999px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        overflow: hidden;
        position: relative;
    }
    .ca-progress-fill {
        height: 100%;
        border-radius: 999px;
        background: var(--color-accent);
        transition: width 320ms cubic-bezier(0.22, 1, 0.36, 1);
    }
    .ca-indeterminate {
        width: 38%;
        animation: ca-slide 1300ms linear infinite;
    }
    @keyframes ca-slide {
        from {
            transform: translate3d(-120%, 0, 0);
        }
        to {
            transform: translate3d(330%, 0, 0);
        }
    }
    .ca-progress-meta {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: 12px;
        color: var(--color-text-secondary);
    }
    .ca-dot {
        opacity: 0.5;
    }
    .ca-progress-path {
        font-size: 11px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        max-width: 100%;
    }

    /* ── Hero ── */
    .ca-hero {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        justify-content: space-between;
        gap: 16px;
    }
    .ca-hero-label {
        display: flex;
        align-items: center;
        gap: 8px;
        font-size: 11.5px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.05em;
        color: var(--color-muted);
    }
    .ca-hero-value {
        font-size: 28px;
        font-weight: 700;
        color: var(--color-accent);
        line-height: 1.1;
    }
    .ca-hero-sub {
        margin-top: 2px;
        font-size: 12px;
        color: var(--color-text-secondary);
    }
    .ca-stats {
        display: flex;
        gap: 0;
        border-top: 1px solid var(--color-border);
        border-bottom: 1px solid var(--color-border);
    }
    .ca-stat {
        min-width: 84px;
        padding: 8px 12px;
        text-align: center;
    }
    .ca-stat + .ca-stat {
        border-left: 1px solid var(--color-border);
    }
    .ca-stat-num {
        font-size: 18px;
        font-weight: 600;
        color: var(--color-text);
    }
    .ca-stat-label {
        font-size: 11px;
        color: var(--color-muted);
    }

    /* ── Section heads ── */
    .ca-section-head {
        display: flex;
        align-items: center;
        gap: 8px;
        margin-bottom: 12px;
        font-size: 11.5px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.05em;
        color: var(--color-text-secondary);
    }
    .ca-details-head {
        margin-bottom: 12px;
    }
    .ca-section-title {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .ca-details-title {
        margin: 0;
        color: var(--color-text);
        font-size: 13px;
        font-weight: 600;
    }

    .ca-section-head :global(.ca-head-ico) {
        width: 14px;
        height: 14px;
    }

    /* ── Health meters ── */
    .ca-health {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .ca-meter-top {
        display: flex;
        justify-content: space-between;
        gap: 8px;
        margin-bottom: 5px;
        font-size: 12px;
    }
    .ca-meter-name {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-weight: 500;
        color: var(--color-text);
    }
    .ca-meter-val {
        color: var(--color-text-secondary);
    }
    .ca-meter-name :global(.ca-inline-ico) {
        width: 13px;
        height: 13px;
    }
    .ca-bar {
        height: 8px;
        border-radius: 999px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        overflow: hidden;
    }
    .ca-bar-fill {
        height: 100%;
        border-radius: 999px;
        background: var(--color-accent);
        transition: width 300ms var(--ease-out, ease);
    }
    .sev-low {
        background: var(--color-success);
    }
    .sev-mid {
        background: var(--color-warning);
    }
    .sev-high {
        background: var(--color-error);
    }

    /* ── Treemap ── */
    .ca-treemap {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
    }
    .ca-tile {
        flex: 1 1 120px;
        min-width: 110px;
        min-height: 64px;
        padding: 10px 12px;
        display: flex;
        flex-direction: column;
        justify-content: center;
        gap: 2px;
        text-align: left;
        border-radius: var(--radius-control, 8px);
        border: 1px solid color-mix(in srgb, hsl(var(--tile-hue, 210) 60% 55%) 40%, var(--color-border));
        background: color-mix(in srgb, hsl(var(--tile-hue, 210) 60% 50%) 14%, var(--color-panel-2));
        color: var(--color-text);
        cursor: pointer;
        transition: transform 130ms var(--ease-out, ease), border-color 130ms var(--ease-out, ease);
    }
    .ca-tile:hover {
        transform: translate3d(0, -1px, 0);
        border-color: color-mix(in srgb, hsl(var(--tile-hue, 210) 60% 55%) 70%, var(--color-border));
    }
    .ca-tile:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: 2px;
    }
    .ca-tile-label {
        font-size: 12.5px;
        font-weight: 600;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .ca-tile-bytes {
        font-size: 12px;
        color: var(--color-text-secondary);
    }
    .ca-tile-parent {
        font-size: 10.5px;
        color: var(--color-muted);
    }

    /* ── Tab body ── */
    .ca-tabbody {
        margin-top: 12px;
    }
    .ca-tab-toolbar {
        display: flex;
        gap: 6px;
        margin-bottom: 8px;
    }
    .ca-rows {
        display: flex;
        flex-direction: column;
        gap: 6px;
    }

    /* ── Rows (canonical selected pattern: panel-2 + accent pill strip) ── */
    .ca-row {
        position: relative;
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 10px 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel);
        overflow: hidden;
        cursor: pointer;
    }
    .ca-row.is-review {
        cursor: default;
    }
    .ca-row.is-selected {
        background: var(--color-panel-2);
    }
    .ca-row.is-selected::before {
        content: '';
        position: absolute;
        left: 3px;
        top: 8px;
        bottom: 8px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .ca-row input[type='checkbox'] {
        flex: none;
        position: relative;
        z-index: 1;
    }
    .ca-row-bg {
        position: absolute;
        inset: 0 auto 0 0;
        background: color-mix(in srgb, var(--color-accent) 8%, transparent);
        pointer-events: none;
    }
    .ca-row-main {
        position: relative;
        z-index: 1;
        min-width: 0;
        flex: 1;
    }
    .ca-row-title {
        font-size: 13px;
        font-weight: 500;
        color: var(--color-text);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .ca-row-sub {
        font-size: 11px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .ca-row-meta {
        position: relative;
        z-index: 1;
        flex: none;
        text-align: right;
    }
    .ca-row-bytes {
        display: block;
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
    }
    .ca-row-count {
        display: block;
        font-size: 11px;
        color: var(--color-muted);
    }
    .ca-row-actions {
        position: relative;
        z-index: 1;
        flex: none;
        display: flex;
        gap: 4px;
    }
    .ca-icon-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 28px;
        height: 28px;
        border-radius: var(--radius-control, 8px);
        border: 1px solid transparent;
        background: transparent;
        color: var(--color-muted);
        cursor: pointer;
    }
    .ca-icon-btn:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .ca-icon-btn :global(.ca-act-ico) {
        width: 15px;
        height: 15px;
    }

    /* ── File-type rows ── */
    .ca-type-top {
        display: flex;
        justify-content: space-between;
        gap: 8px;
        margin-bottom: 5px;
        font-size: 12px;
    }
    .ca-type-label {
        display: inline-flex;
        align-items: center;
        gap: 7px;
        font-weight: 500;
        color: var(--color-text);
    }
    .ca-emoji {
        font-size: 14px;
    }
    .ca-type-val {
        color: var(--color-text-secondary);
    }

    /* ── Cleanup gate ── */
    .ca-clean {
        margin-top: 14px;
        display: flex;
        flex-direction: column;
        gap: 10px;
        padding-top: 14px;
        border-top: 1px solid var(--color-border);
    }
    .ca-clean-warn {
        display: flex;
        gap: 8px;
        align-items: flex-start;
        font-size: 12px;
        line-height: 1.5;
        color: var(--color-warning);
    }
    .ca-clean-warn :global(.ca-warn-ico) {
        flex: none;
        width: 15px;
        height: 15px;
        margin-top: 1px;
    }
    .ca-clean-row {
        display: flex;
        flex-wrap: wrap;
        gap: 10px;
        align-items: center;
    }
    .ca-confirm {
        flex: 1;
        min-width: 200px;
        height: 36px;
        padding: 0 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        color: var(--color-text);
        font-size: 13px;
        letter-spacing: 0.04em;
    }
    .ca-confirm:focus-visible {
        outline: 2px solid var(--color-error);
        outline-offset: 1px;
    }
    .ca-clean-result {
        margin-top: 10px;
        display: flex;
        gap: 8px;
        align-items: center;
        padding: 10px 12px;
        border-radius: var(--radius-control, 8px);
        border: 1px solid color-mix(in srgb, var(--color-warning) 40%, var(--color-border));
        background: color-mix(in srgb, var(--color-warning) 8%, transparent);
        font-size: 12.5px;
        color: var(--color-text);
    }
    .ca-clean-result.ok {
        border-color: color-mix(in srgb, var(--color-success) 40%, var(--color-border));
        background: color-mix(in srgb, var(--color-success) 8%, transparent);
    }
    .ca-clean-result :global(.ca-res-ico) {
        flex: none;
        width: 16px;
        height: 16px;
    }
    .ca-clean-result :global(.ca-res-ico.ok) {
        color: var(--color-success);
    }
    .ca-clean-result :global(.ca-res-ico.warn) {
        color: var(--color-warning);
    }

    /* ── Tips + notes ── */
    .ca-tips {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .ca-tips li {
        display: flex;
        gap: 8px;
        align-items: flex-start;
        font-size: 13px;
        line-height: 1.5;
        color: var(--color-text);
    }
    .ca-tips :global(.ca-tip-ico) {
        flex: none;
        width: 15px;
        height: 15px;
        margin-top: 2px;
        color: var(--color-accent);
    }
    .ca-note {
        margin: 10px 0 0;
        font-size: 11.5px;
        color: var(--color-muted);
    }

    @media (max-width: 560px) {
        .ca-scanbar-note {
            flex-basis: 100%;
            margin-left: 26px;
        }
        .ca-hero {
            align-items: stretch;
        }
        .ca-stats {
            width: 100%;
        }
        .ca-stat {
            flex: 1;
            min-width: 0;
            padding-inline: 8px;
        }
    }

    @media (prefers-reduced-motion: reduce) {
        .ca-indeterminate {
            animation: none;
            width: 100%;
            opacity: 0.5;
        }
        .ca-progress-fill,
        .ca-bar-fill,
        .ca-tile {
            transition: none;
        }
    }
</style>
