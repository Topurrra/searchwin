<script lang="ts">
    import { convertFileSrc } from '@tauri-apps/api/core';
    import { open } from '@tauri-apps/plugin-dialog';
    import { onMount } from 'svelte';
    import { CopyPlus, Droplets, FolderOpen, ImagePlus, ListFilter, RefreshCw, Type, X, Play, Plus, Image as ImageIcon } from '@lucide/svelte';
    import LoadingState from '$lib/components/LoadingState.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import ToolCancelButton from '$lib/components/ToolCancelButton.svelte';
    import { ToolPage } from '$lib/ui';
    import {
        applyActiveWatermarkToAll,
        cancelImageWatermark,
        clearWatermarkState,
        defaultWatermarkConfig,
        fileName,
        fmtBytes,
        initImageWatermarkStore,
        removeWatermarkItem,
        resetActiveWatermarkConfig,
        runImageWatermark,
        setWatermarkActiveIndex,
        updateActiveWatermarkConfig,
        addWatermarkItems,
        watermarkActiveIndex,
        watermarkCancelling,
        watermarkCurrentJob,
        watermarkItems,
        watermarkOutputDir,
        watermarkProcessing,
        watermarkProgressByPath,
        watermarkResults,
        type WatermarkConfig,
        type WatermarkPosition,
    } from '$lib/stores/imageWatermark';

    type Position = WatermarkPosition;

    const positions: { id: Position; label: string }[] = [
        { id: 'bottom-right', label: 'Bottom right' },
        { id: 'bottom-left', label: 'Bottom left' },
        { id: 'top-right', label: 'Top right' },
        { id: 'top-left', label: 'Top left' },
        { id: 'center', label: 'Center' },
    ];

    onMount(() => {
        initImageWatermarkStore();
    });

    let activeItem = $derived($watermarkItems[$watermarkActiveIndex] ?? null);
    let activeConfig = $derived(activeItem?.config ?? defaultWatermarkConfig());
    let selectedCount = $derived($watermarkItems.length);
    let doneCount = $derived($watermarkResults.filter((r) => r.success).length);
    let failedCount = $derived($watermarkResults.filter((r) => !r.success).length);
    let totalProgress = $derived(selectedCount ? Math.round(Object.values($watermarkProgressByPath).reduce((sum, p) => sum + (p.progress || 0), 0) / selectedCount) : 0);

    function basename(path: string): string {
        return path.split(/[\\/]/).pop() ?? path;
    }

    async function pickImages() {
        const picked = await open({ multiple: true, filters: [{ name: 'Images', extensions: ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'tif', 'tiff'] }] });
        if (!picked) return;
        const selected = Array.isArray(picked) ? picked : [picked];
        addWatermarkItems(selected, (path) => convertFileSrc(path));
    }

    async function pickWatermark() {
        const picked = await open({ multiple: false, filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp'] }] });
        if (typeof picked !== 'string') return;
        updateActiveWatermarkConfig({ watermarkPath: picked, watermarkPreview: convertFileSrc(picked), mode: 'image' });
    }

    async function pickOutputDir() {
        const picked = await open({ directory: true, multiple: false });
        if (typeof picked === 'string') watermarkOutputDir.set(picked);
    }

    function patch(patch: Partial<WatermarkConfig>) {
        updateActiveWatermarkConfig(patch);
    }
</script>

<ToolPage
    icon={Droplets}
    iconTint="#06b6d4"
    title="Stamp text or a logo onto your images"
    description="Per-image watermark settings — text or logo, position, opacity, scale and tiling. Live preview shows what each export will look like."
    width="wide"
    fill={false}
>
    <!-- Toolbar -->
    <div class="watermark-toolbar flex flex-wrap items-center gap-3 rounded-2xl border border-border bg-panel p-3 md:p-4">
        <button
            type="button"
            onclick={pickImages}
            disabled={$watermarkProcessing}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
        >
            <Plus class="h-4 w-4" />
            Add images
        </button>

        <button
            type="button"
            onclick={pickOutputDir}
            disabled={$watermarkProcessing}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
            title={$watermarkOutputDir ?? 'Save alongside originals'}
        >
            <FolderOpen class="h-4 w-4" />
            {$watermarkOutputDir ? `Output: ${basename($watermarkOutputDir)}` : 'Output: alongside originals'}
        </button>

        <button
            type="button"
            onclick={applyActiveWatermarkToAll}
            disabled={!$watermarkItems.length || $watermarkProcessing}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
        >
            <ListFilter class="h-4 w-4" />
            Apply settings to all
        </button>

        {#if $watermarkItems.length > 0 && !$watermarkProcessing}
            <button
                type="button"
                onclick={clearWatermarkState}
                class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-error-strong"
            >
                Clear all ({selectedCount})
            </button>
        {/if}

        <div class="flex-1"></div>

        <ToolCancelButton
            running={$watermarkProcessing}
            cancelling={$watermarkCancelling}
            onCancel={() => cancelImageWatermark()}
        />
        <button
            type="button"
            onclick={runImageWatermark}
            disabled={!$watermarkItems.length || $watermarkProcessing}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl bg-accent px-4 text-sm font-medium text-accent-contrast hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed"
        >
            {#if $watermarkProcessing}
                <LoadingState variant="inline" label="Applying…" />
            {:else}
                <Play class="h-4 w-4" />
                Apply watermark
            {/if}
        </button>
    </div>

    <!-- Progress -->
    {#if $watermarkProcessing || Object.keys($watermarkProgressByPath).length > 0}
        <div class="watermark-progress bg-panel border border-border rounded-2xl p-4 space-y-2">
            <div class="flex items-center justify-between gap-3 text-sm">
                {#if $watermarkProcessing}
                    <LoadingState variant="inline" label={$watermarkCancelling ? 'Cancelling…' : `Processing ${$watermarkCurrentJob || 'images'}…`} />
                {:else}
                    <span class="font-medium text-text">Last watermark job</span>
                {/if}
                <span class="text-muted text-xs shrink-0">{doneCount} done · {failedCount} failed · {totalProgress}%</span>
            </div>
            <div class="h-1.5 bg-panel-2 rounded overflow-hidden">
                <div class="h-full bg-accent transition-all" style="width: {totalProgress}%"></div>
            </div>
        </div>
    {/if}

    <!-- Work area: preview/canvas (left) + sidebar (right) -->
    <div class="watermark-workspace grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]">
        <!-- Left: preview + settings -->
        <div class="watermark-main space-y-3">
            <!-- Preview canvas -->
            <div class="watermark-preview rounded-2xl border border-border bg-panel p-4 md:p-5">
                <div class="flex items-center justify-between gap-2 mb-3">
                    <div>
                        <h2 class="text-sm font-medium text-text">Live preview</h2>
                        <div class="text-xs text-muted truncate" title={activeItem?.path ?? 'No file selected'}>{activeItem ? fileName(activeItem.path) : 'No file selected'}</div>
                    </div>
                    {#if $watermarkProcessing && $watermarkCurrentJob}
                        <div class="text-xs text-muted truncate max-w-[200px]">Working on {$watermarkCurrentJob}</div>
                    {/if}
                </div>

                <div class="aspect-video bg-panel-2 border border-border rounded-xl overflow-hidden relative flex items-center justify-center max-h-[60vh]">
                    {#if activeItem}
                        <img src={activeItem.preview} alt="Preview" class="w-full h-full object-contain" />
                        {#if activeConfig.mode === 'text'}
                            <div class="absolute px-2 py-1 rounded bg-black/30 text-white font-bold tracking-wide max-w-[80%] truncate" style="opacity: {activeConfig.opacity / 100}; color: {activeConfig.color}; font-size: {Math.max(10, activeConfig.scalePercent)}px; {activeConfig.position.includes('bottom') ? 'bottom' : activeConfig.position === 'center' ? 'top' : 'top'}: {activeConfig.position === 'center' ? '50%' : `${Math.max(8, activeConfig.margin / 3)}px`}; {activeConfig.position.includes('right') ? 'right' : activeConfig.position === 'center' ? 'left' : 'left'}: {activeConfig.position === 'center' ? '50%' : `${Math.max(8, activeConfig.margin / 3)}px`}; transform: {activeConfig.position === 'center' ? 'translate(-50%, -50%)' : 'none'};">
                                {activeConfig.text || 'Watermark'}
                            </div>
                        {:else if activeConfig.watermarkPreview}
                            <img src={activeConfig.watermarkPreview} alt="Watermark preview" class="absolute object-contain pointer-events-none" style="opacity: {activeConfig.opacity / 100}; width: {Math.max(8, activeConfig.scalePercent)}%; max-height: 70%; {activeConfig.position.includes('bottom') ? 'bottom' : activeConfig.position === 'center' ? 'top' : 'top'}: {activeConfig.position === 'center' ? '50%' : `${Math.max(8, activeConfig.margin / 3)}px`}; {activeConfig.position.includes('right') ? 'right' : activeConfig.position === 'center' ? 'left' : 'left'}: {activeConfig.position === 'center' ? '50%' : `${Math.max(8, activeConfig.margin / 3)}px`}; transform: {activeConfig.position === 'center' ? 'translate(-50%, -50%)' : 'none'};" />
                        {/if}
                        {#if activeConfig.tiled}
                            <div class="absolute top-2 left-2 rounded bg-panel text-[10px] border border-border px-1.5 py-0.5 text-muted">Tiled preview simplified</div>
                        {/if}
                    {:else}
                        <EmptyState
                            icon={ImageIcon}
                            title="Pick images to watermark"
                            description="Drop or browse JPEG, PNG, WebP, BMP, TIFF."
                            variant="compact"
                        >
                            {#snippet actions()}
                                <button onclick={pickImages} class="inline-flex items-center gap-1.5 rounded-lg border border-accent/40 bg-accent/10 px-3 py-1.5 text-sm text-accent hover:bg-accent/20">
                                    <FolderOpen class="h-4 w-4" />
                                    Browse files
                                </button>
                            {/snippet}
                        </EmptyState>
                    {/if}
                </div>
            </div>

            <!-- Settings -->
            {#if activeItem}
                <div class="watermark-inspector rounded-2xl border border-border bg-panel p-4 md:p-5 space-y-3">
                    <div class="flex items-center justify-between gap-2 mb-1">
                        <h2 class="text-xs font-semibold uppercase tracking-wider text-muted">Watermark settings (current image)</h2>
                        <button onclick={resetActiveWatermarkConfig} disabled={!activeItem || $watermarkProcessing} class="inline-flex items-center gap-1 text-xs text-muted hover:text-text disabled:opacity-50"><RefreshCw class="w-3 h-3" /> Reset</button>
                    </div>

                    <div class="grid grid-cols-2 gap-2 text-xs">
                        <button onclick={() => patch({ mode: 'text' })} disabled={!activeItem || $watermarkProcessing} class="watermark-mode h-9 rounded-lg border text-sm flex items-center justify-center gap-1 disabled:opacity-50 bg-panel-2 border-border hover:border-accent/60" class:is-active={activeConfig.mode === 'text'}><Type class="w-3.5 h-3.5" /> Text</button>
                        <button onclick={() => patch({ mode: 'image' })} disabled={!activeItem || $watermarkProcessing} class="watermark-mode h-9 rounded-lg border text-sm flex items-center justify-center gap-1 disabled:opacity-50 bg-panel-2 border-border hover:border-accent/60" class:is-active={activeConfig.mode === 'image'}><ImagePlus class="w-3.5 h-3.5" /> Image</button>
                    </div>

                    <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-4 gap-3">
                        <label class="space-y-1">
                            <span class="text-xs uppercase tracking-wider text-muted font-semibold">Suffix</span>
                            <input value={activeConfig.suffix} oninput={(e) => patch({ suffix: e.currentTarget.value })} disabled={!activeItem || $watermarkProcessing} class="w-full h-9 bg-panel-2 border border-border rounded-lg px-2 text-sm disabled:opacity-60" />
                        </label>

                        <label class="space-y-1">
                            <span class="text-xs uppercase tracking-wider text-muted font-semibold">Position</span>
                            <select value={activeConfig.position} onchange={(e) => patch({ position: e.currentTarget.value as Position })} disabled={!activeItem || $watermarkProcessing} class="w-full h-9 bg-panel-2 border border-border rounded-lg px-2 text-sm disabled:opacity-60">
                                {#each positions as p}
                                    <option value={p.id}>{p.label}</option>
                                {/each}
                            </select>
                        </label>

                        <label class="space-y-1"><span class="text-xs uppercase tracking-wider text-muted font-semibold">Opacity ({activeConfig.opacity}%)</span><input type="range" value={activeConfig.opacity} min="1" max="100" oninput={(e) => patch({ opacity: Number(e.currentTarget.value) })} disabled={!activeItem || $watermarkProcessing} class="w-full h-9 accent-emerald-500 disabled:opacity-60" /></label>
                        <label class="space-y-1"><span class="text-xs uppercase tracking-wider text-muted font-semibold">Scale ({activeConfig.scalePercent}%)</span><input type="range" value={activeConfig.scalePercent} min="4" max="80" oninput={(e) => patch({ scalePercent: Number(e.currentTarget.value) })} disabled={!activeItem || $watermarkProcessing} class="w-full h-9 accent-emerald-500 disabled:opacity-60" /></label>
                        <label class="space-y-1"><span class="text-xs uppercase tracking-wider text-muted font-semibold">Margin ({activeConfig.margin}px)</span><input type="range" value={activeConfig.margin} min="0" max="160" oninput={(e) => patch({ margin: Number(e.currentTarget.value) })} disabled={!activeItem || $watermarkProcessing} class="w-full h-9 accent-emerald-500 disabled:opacity-60" /></label>
                        <label class="space-y-1"><span class="text-xs uppercase tracking-wider text-muted font-semibold">Quality ({activeConfig.quality})</span><input type="range" value={activeConfig.quality} min="1" max="100" oninput={(e) => patch({ quality: Number(e.currentTarget.value) })} disabled={!activeItem || $watermarkProcessing} class="w-full h-9 accent-emerald-500 disabled:opacity-60" /></label>
                        <label class="flex items-center gap-2 text-sm self-end h-9"><input type="checkbox" checked={activeConfig.tiled} onchange={(e) => patch({ tiled: e.currentTarget.checked })} disabled={!activeItem || $watermarkProcessing} /> Tile watermark</label>

                        {#if activeConfig.mode === 'text'}
                            <label class="space-y-1 md:col-span-2 xl:col-span-4">
                                <span class="text-xs uppercase tracking-wider text-muted font-semibold">Text</span>
                                <input value={activeConfig.text} oninput={(e) => patch({ text: e.currentTarget.value })} disabled={!activeItem || $watermarkProcessing} class="w-full h-9 bg-panel-2 border border-border rounded-lg px-2 text-sm disabled:opacity-60" />
                            </label>

                            <label class="space-y-1 md:col-span-2">
                                <span class="text-xs uppercase tracking-wider text-muted font-semibold">Color</span>
                                <div class="flex gap-2">
                                    <input type="color" value={activeConfig.color} oninput={(e) => patch({ color: e.currentTarget.value })} disabled={!activeItem || $watermarkProcessing} class="h-9 w-11 bg-panel-2 border border-border rounded-lg disabled:opacity-60" />
                                    <input value={activeConfig.color} oninput={(e) => patch({ color: e.currentTarget.value })} disabled={!activeItem || $watermarkProcessing} class="min-w-0 flex-1 h-9 bg-panel-2 border border-border rounded-lg px-2 text-sm disabled:opacity-60" />
                                </div>
                            </label>
                        {:else}
                            <div class="space-y-1 md:col-span-2 xl:col-span-4">
                                <span class="text-xs uppercase tracking-wider text-muted font-semibold">Watermark image</span>
                                <button onclick={pickWatermark} disabled={!activeItem || $watermarkProcessing} class="w-full h-9 bg-panel-2 border border-border rounded-lg px-2 text-sm text-left truncate disabled:opacity-60" title={activeConfig.watermarkPath ?? 'Choose PNG/logo watermark'}>{activeConfig.watermarkPath ? fileName(activeConfig.watermarkPath) : 'Choose PNG/logo watermark'}</button>
                                {#if activeConfig.watermarkPath}<div class="text-[11px] text-muted path-wrap" title={activeItem?.path ?? 'current image'}>Used only for {activeItem ? activeItem.path : 'current image'}</div>{/if}
                            </div>
                        {/if}
                    </div>
                </div>
            {/if}

            <!-- Results -->
            {#if $watermarkResults.length > 0}
                <div class="watermark-results rounded-2xl border border-border bg-panel p-4 md:p-5">
                    <div class="mb-3 flex items-center justify-between">
                        <h2 class="text-sm uppercase tracking-wider text-muted font-semibold">Results</h2>
                        <div class="text-xs">
                            <span class="text-success">{doneCount} done</span>
                            {#if failedCount > 0}
                                <span class="text-muted mx-2">·</span>
                                <span class="text-error">{failedCount} failed</span>
                            {/if}
                        </div>
                    </div>
                    <div class="space-y-2 max-h-80 overflow-y-auto">
                        {#each $watermarkResults as r}
                            <div class="rounded-lg border {r.success ? 'border-border bg-panel-2' : 'border-error-strong bg-error-soft'} p-3">
                                <div class="flex items-start justify-between gap-3">
                                    <div class="min-w-0 flex-1">
                                        <div class="text-sm text-text truncate" title={r.source_path}>{fileName(r.source_path)}</div>
                                        {#if r.success}
                                            <div class="text-xs text-muted truncate" title={r.output_path}>{fmtBytes(r.original_size)} → {fmtBytes(r.new_size)} · {r.output_path}</div>
                                        {:else}
                                            <div class="text-xs text-error truncate" title={r.error}>{r.error}</div>
                                        {/if}
                                    </div>
                                    <span class="text-xs px-2 py-0.5 rounded uppercase font-semibold
                                                 {r.success ? 'bg-success-soft text-success border border-success-strong' : 'bg-error-soft text-error border border-error-strong'}">
                                        {r.success ? 'OK' : 'FAIL'}
                                    </span>
                                </div>
                            </div>
                        {/each}
                    </div>
                </div>
            {/if}
        </div>

        <!-- Right: image list -->
        <aside class="watermark-queue rounded-2xl border border-border bg-panel p-3">
            <div class="mb-2 flex items-center justify-between">
                <h3 class="text-xs font-semibold uppercase tracking-wider text-muted">Images ({selectedCount})</h3>
                <button onclick={pickImages} disabled={$watermarkProcessing} class="inline-flex items-center gap-1 text-xs text-muted hover:text-text disabled:opacity-50"><CopyPlus class="w-3 h-3" /> Add</button>
            </div>

            <div class="max-h-[70vh] overflow-y-auto space-y-1.5 pr-1">
                {#if $watermarkItems.length}
                    {#each $watermarkItems as item, index}
                        {@const progress = $watermarkProgressByPath[item.path]}
                        <div
                            role="button"
                            tabindex="0"
                            onclick={() => setWatermarkActiveIndex(index)}
                            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setWatermarkActiveIndex(index); } }}
                            class="watermark-queue-row rounded-lg border px-2.5 py-2 transition-colors cursor-pointer bg-panel-2 border-border hover:border-accent/60"
                        >
                            <div class="flex items-center justify-between gap-2">
                                <span class="truncate text-sm font-medium" title={item.path}>{fileName(item.path)}</span>
                                <button onclick={(e) => { e.stopPropagation(); removeWatermarkItem(item.path); }} disabled={$watermarkProcessing} class="text-muted hover:text-error disabled:opacity-40" aria-label="Remove image"><X class="w-3.5 h-3.5" /></button>
                            </div>
                            <div class="mt-1 flex items-center justify-between gap-2 text-[11px] text-muted">
                                <span>{item.config.mode === 'text' ? 'Text' : 'Image'} · {item.config.position}</span>
                                <span>{Math.round(progress?.progress ?? 0)}%</span>
                            </div>
                            {#if progress}
                                <div class="mt-1 h-1 bg-bg rounded overflow-hidden"><div class="h-full bg-accent" style="width: {Math.round(progress.progress)}%"></div></div>
                            {/if}
                        </div>
                    {/each}
                {:else}
                    <div class="rounded-lg border border-border border-dashed bg-panel-2/40 p-4 text-center text-xs text-muted">
                        Click "Add images" above to begin.
                    </div>
                {/if}
            </div>
        </aside>
    </div>
</ToolPage>
<style>
    .watermark-toolbar {
        gap: 8px 10px;
        padding: 0 0 12px;
        border: 0;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        border-radius: 0;
        background: transparent;
    }

    .watermark-toolbar > button {
        border-radius: var(--radius-control);
    }

    .watermark-progress {
        padding: 10px 12px;
        border-radius: var(--radius-control);
        border-color: color-mix(in srgb, var(--color-accent) 28%, var(--color-border));
        background: color-mix(in srgb, var(--color-accent) 4%, var(--color-panel));
    }

    .watermark-workspace {
        align-items: start;
    }

    .watermark-preview,
    .watermark-inspector,
    .watermark-queue,
    .watermark-results {
        border-color: var(--color-border);
        background: var(--color-panel);
        box-shadow: inset 0 1px 0 color-mix(in srgb, var(--color-text) 4%, transparent);
    }

    .watermark-preview :global(.aspect-video) {
        border-color: color-mix(in srgb, var(--color-text) 8%, var(--color-border));
        background: color-mix(in srgb, var(--color-panel-2) 92%, transparent);
    }

    .watermark-mode {
        position: relative;
        background: transparent;
        color: var(--color-text-secondary);
    }

    .watermark-mode:hover:not(:disabled) {
        background: var(--color-panel-2);
        color: var(--color-text);
    }

    .watermark-mode.is-active {
        padding-left: 18px;
        border-color: var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-weight: 500;
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }

    .watermark-mode.is-active::before {
        content: '';
        position: absolute;
        left: 7px;
        top: 9px;
        bottom: 9px;
        width: 2px;
        border-radius: 999px;
        background: var(--color-accent);
    }

    .watermark-inspector :global(input[type='checkbox']),
    .watermark-inspector :global(input[type='range']) {
        accent-color: var(--color-accent);
    }

    .watermark-queue {
        min-width: 0;
    }

    .watermark-queue-row {
        position: relative;
    }

    .watermark-queue-row:hover:not(.is-active) {
        background: color-mix(in srgb, var(--color-text) 4%, var(--color-panel-2));
    }

    .watermark-queue :global(.watermark-queue-row.is-active) {
        padding-left: 15px;
        border-color: var(--color-border);
        background: var(--color-panel-2);
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }

    .watermark-queue :global(.watermark-queue-row.is-active)::before {
        content: '';
        position: absolute;
        left: 5px;
        top: 8px;
        bottom: 8px;
        width: 2px;
        border-radius: 999px;
        background: var(--color-accent);
    }

    @media (min-width: 1024px) {
        .watermark-workspace {
            grid-template-columns: minmax(184px, 220px) minmax(0, 1fr) minmax(260px, 320px);
        }

        .watermark-main {
            display: contents;
        }

        .watermark-preview {
            grid-column: 2;
            grid-row: 1;
            min-width: 0;
        }

        .watermark-inspector {
            grid-column: 3;
            grid-row: 1;
            position: sticky;
            top: 0;
            align-self: start;
        }

        .watermark-queue {
            grid-column: 1;
            grid-row: 1 / span 2;
            position: sticky;
            top: 0;
            align-self: start;
        }

        .watermark-results {
            grid-column: 2 / span 2;
            grid-row: 2;
            min-width: 0;
        }
    }

    @media (max-width: 640px) {
        .watermark-toolbar {
            align-items: stretch;
        }

        .watermark-toolbar > button {
            justify-content: center;
        }
    }
</style>
