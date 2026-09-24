<script lang="ts">
    import { convertFileSrc } from '@tauri-apps/api/core';
    import { open } from '@tauri-apps/plugin-dialog';
    import { onMount } from 'svelte';
    import { AppWindow, Copy, FolderOpen, Image as ImageIcon, RotateCcw, Play } from '@lucide/svelte';
    import LoadingState from '$lib/components/LoadingState.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import ToolCancelButton from '$lib/components/ToolCancelButton.svelte';
    import { ToolPage } from '$lib/ui';
    import { toast } from '$lib/stores/toasts';
    import {
        cancelFaviconGeneration,
        clearFaviconState,
        faviconAllSizes,
        faviconAppName,
        faviconBackground,
        faviconCancelling,
        faviconGenerating,
        faviconMakeIco,
        faviconMakePng,
        faviconOutputDir,
        faviconPaddingPercent,
        faviconPreview,
        faviconProgress,
        faviconResult,
        faviconSelectedSizes,
        faviconSourcePath,
        faviconTransparent,
        initFaviconGeneratorStore,
        runFaviconGeneration,
        setFaviconSource,
        toggleFaviconSize,
    } from '$lib/stores/faviconGenerator';

    onMount(() => {
        initFaviconGeneratorStore();
    });

    let activeSizes = $derived(faviconAllSizes.filter((size) => $faviconSelectedSizes[size]));

    function basename(path: string): string {
        return path.split(/[\\/]/).pop() ?? path;
    }

    async function pickSource() {
        const picked = await open({ multiple: false, filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'tif', 'tiff'] }] });
        if (typeof picked !== 'string') return;
        setFaviconSource(picked, convertFileSrc(picked));
    }

    async function pickOutputDir() {
        const picked = await open({ directory: true, multiple: false });
        if (typeof picked === 'string') faviconOutputDir.set(picked);
    }

    async function copyHtml() {
        const html = `<link rel="icon" type="image/png" sizes="32x32" href="/favicon-32x32.png">\n<link rel="icon" type="image/png" sizes="16x16" href="/favicon-16x16.png">\n<link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png">\n<link rel="manifest" href="/site.webmanifest">`;
        await navigator.clipboard.writeText(html);
        toast('HTML snippet copied', 'success');
    }
</script>

<ToolPage
    icon={AppWindow}
    iconTint="#06b6d4"
    title="Build a complete favicon set in one click"
    description="PNG favicons, Apple touch icon, Android icons, ICO bundle, and a web manifest — all generated locally from a single source image."
    width="wide"
    fill={false}
>
    <!-- Toolbar -->
    <div class="favicon-toolbar flex flex-wrap items-center gap-3 rounded-2xl border border-border bg-panel p-3 md:p-4">
        <button
            type="button"
            onclick={pickSource}
            disabled={$faviconGenerating}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
        >
            <ImageIcon class="h-4 w-4" />
            Choose source image
        </button>

        <button
            type="button"
            onclick={pickOutputDir}
            disabled={$faviconGenerating}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
            title={$faviconOutputDir ?? 'Generates a favicon folder beside the source'}
        >
            <FolderOpen class="h-4 w-4" />
            {$faviconOutputDir ? `Output: ${basename($faviconOutputDir)}` : 'Output: beside source'}
        </button>

        <button
            type="button"
            onclick={copyHtml}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70"
        >
            <Copy class="h-4 w-4" />
            Copy HTML snippet
        </button>

        <button
            type="button"
            onclick={clearFaviconState}
            disabled={$faviconGenerating}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-error-strong disabled:opacity-50"
        >
            <RotateCcw class="h-4 w-4" />
            Reset
        </button>

        <div class="flex-1"></div>

        <ToolCancelButton
            running={$faviconGenerating}
            cancelling={$faviconCancelling}
            onCancel={() => cancelFaviconGeneration()}
        />
        <button
            type="button"
            onclick={runFaviconGeneration}
            disabled={!$faviconSourcePath || $faviconGenerating}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl bg-accent px-4 text-sm font-medium text-accent-contrast hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed"
        >
            {#if $faviconGenerating}
                <LoadingState variant="inline" label="Generating…" />
            {:else}
                <Play class="h-4 w-4" />
                Generate
            {/if}
        </button>
    </div>

    <!-- Progress -->
    {#if $faviconGenerating || $faviconProgress}
        <div class="favicon-progress bg-panel border border-border rounded-2xl p-4 space-y-2">
            <div class="flex items-center justify-between gap-3 text-sm">
                <LoadingState variant="inline" label={$faviconCancelling ? 'Cancelling favicon generation…' : ($faviconProgress?.message ?? 'Generating favicons…')} />
                <span class="text-muted text-xs shrink-0">{Math.round($faviconProgress?.progress ?? 0)}%</span>
            </div>
            <div class="h-1.5 bg-panel-2 rounded overflow-hidden">
                <div class="h-full bg-accent transition-all" style="width: {Math.round($faviconProgress?.progress ?? 0)}%"></div>
            </div>
        </div>
    {/if}

    <!-- Work area -->
    <div class="favicon-workspace grid grid-cols-1 lg:grid-cols-[280px_1fr] gap-3">
        <aside class="favicon-source rounded-2xl border border-border bg-panel p-4 space-y-3">
            {#if $faviconPreview}
                <div class="favicon-preview aspect-square bg-panel-2 rounded-xl border border-border flex items-center justify-center overflow-hidden" style="background-color: {$faviconTransparent ? 'transparent' : $faviconBackground}">
                    <img src={$faviconPreview} alt="Icon preview" class="max-w-[78%] max-h-[78%] object-contain" />
                </div>
            {:else}
                <button onclick={pickSource} class="w-full aspect-square bg-panel-2 border border-dashed border-border rounded-xl text-sm text-muted hover:text-text flex flex-col items-center justify-center gap-2">
                    <ImageIcon class="w-6 h-6" /> Pick source image
                </button>
            {/if}
            {#if $faviconSourcePath}
                <div class="favicon-source-name truncate" title={$faviconSourcePath}>{basename($faviconSourcePath)}</div>
            {/if}
            <div class="text-[11px] text-muted truncate" title={$faviconOutputDir ?? ''}>{$faviconOutputDir ?? 'Output: beside source in favicon folder'}</div>
        </aside>

        <main class="favicon-config space-y-3">
            <div class="favicon-section rounded-2xl border border-border bg-panel p-4 md:p-5">
                <div class="grid grid-cols-1 md:grid-cols-4 gap-3">
                    <label class="space-y-1 md:col-span-2">
                        <span class="text-xs uppercase tracking-wider text-muted font-semibold">App/site name</span>
                        <input bind:value={$faviconAppName} disabled={$faviconGenerating} class="w-full h-9 bg-panel-2 border border-border rounded-lg px-2 text-sm outline-none focus:border-accent disabled:opacity-60" placeholder="KeepItLocal" />
                    </label>
                    <label class="space-y-1">
                        <span class="text-xs uppercase tracking-wider text-muted font-semibold">Background</span>
                        <div class="flex gap-2"><input type="color" bind:value={$faviconBackground} disabled={$faviconGenerating} class="h-9 w-11 bg-panel-2 border border-border rounded-lg disabled:opacity-60" /><input bind:value={$faviconBackground} disabled={$faviconGenerating} class="min-w-0 flex-1 h-9 bg-panel-2 border border-border rounded-lg px-2 text-sm disabled:opacity-60" /></div>
                    </label>
                    <label class="space-y-1">
                        <span class="text-xs uppercase tracking-wider text-muted font-semibold">Padding ({$faviconPaddingPercent}%)</span>
                        <input type="range" bind:value={$faviconPaddingPercent} min="0" max="45" disabled={$faviconGenerating} class="w-full h-9 accent-emerald-500 disabled:opacity-60" />
                    </label>

                    <label class="flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={$faviconTransparent} disabled={$faviconGenerating} /> Transparent background</label>
                    <label class="flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={$faviconMakePng} disabled={$faviconGenerating} /> PNG set</label>
                    <label class="flex items-center gap-2 text-sm"><input type="checkbox" bind:checked={$faviconMakeIco} disabled={$faviconGenerating} /> favicon.ico</label>
                    <div class="text-xs text-muted self-center">{activeSizes.length} sizes selected</div>
                </div>
            </div>

            <div class="favicon-section rounded-2xl border border-border bg-panel p-4 md:p-5 space-y-2">
                <div class="text-xs font-semibold uppercase tracking-wider text-muted">Sizes</div>
                <div class="flex flex-wrap gap-2">
                    {#each faviconAllSizes as size}
                        <button onclick={() => toggleFaviconSize(size)} disabled={$faviconGenerating} class="favicon-size px-2.5 py-1 rounded-lg border text-xs disabled:opacity-50 bg-panel-2 border-border text-muted hover:border-accent/60" class:is-selected={$faviconSelectedSizes[size]}>
                            {size}×{size}
                        </button>
                    {/each}
                </div>
            </div>

            {#if $faviconResult}
                <div class="favicon-results rounded-2xl border border-border bg-panel p-4 md:p-5 space-y-2">
                    <div class="flex items-center justify-between gap-2">
                        <h2 class="text-sm uppercase tracking-wider text-muted font-semibold">Generated files</h2>
                        <div class="text-xs text-muted truncate" title={$faviconResult.output_dir}>{$faviconResult.output_dir}</div>
                    </div>
                    {#if !$faviconResult.success && $faviconResult.error}
                        <div class="text-xs text-error bg-error-soft border border-error-strong rounded-lg p-2">{$faviconResult.error}</div>
                    {/if}
                    <div class="grid grid-cols-1 md:grid-cols-2 gap-2 max-h-[260px] overflow-auto">
                        {#each $faviconResult.outputs as out}
                            <div class="bg-panel-2 border border-border rounded-lg p-2 text-xs flex items-center justify-between gap-2">
                                <span class="truncate">{out.path}</span>
                                <span class="shrink-0 text-muted uppercase">{out.kind}{out.size ? ` ${out.size}` : ''}</span>
                            </div>
                        {/each}
                    </div>
                </div>
            {:else if !$faviconSourcePath && !$faviconGenerating}
                <div class="favicon-empty rounded-2xl border border-border bg-panel p-4 md:p-5">
                    <EmptyState
                        icon={AppWindow}
                        title="Pick a source image to generate favicons"
                        description="Choose a square PNG or SVG (1024×1024 or larger ideally). Adjust padding and background, then click Generate."
                        variant="compact"
                    />
                </div>
            {/if}
        </main>
    </div>
</ToolPage>
<style>
    .favicon-toolbar {
        gap: 8px 10px;
        padding: 0 0 12px;
        border: 0;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        border-radius: 0;
        background: transparent;
    }

    .favicon-toolbar > button {
        border-radius: var(--radius-control);
    }

    .favicon-progress {
        padding: 10px 12px;
        border-radius: var(--radius-control);
        border-color: color-mix(in srgb, var(--color-accent) 28%, var(--color-border));
        background: color-mix(in srgb, var(--color-accent) 4%, var(--color-panel));
    }

    .favicon-workspace {
        align-items: start;
    }

    .favicon-source,
    .favicon-section,
    .favicon-results,
    .favicon-empty {
        border-color: var(--color-border);
        background: var(--color-panel);
        box-shadow: inset 0 1px 0 color-mix(in srgb, var(--color-text) 4%, transparent);
    }

    .favicon-preview {
        border-color: color-mix(in srgb, var(--color-text) 8%, var(--color-border));
        background-image: linear-gradient(45deg, color-mix(in srgb, var(--color-text) 4%, transparent) 25%, transparent 25%), linear-gradient(-45deg, color-mix(in srgb, var(--color-text) 4%, transparent) 25%, transparent 25%);
        background-position: 0 0, 8px 8px;
        background-size: 16px 16px;
    }

    .favicon-source-name {
        font-size: 12px;
        font-weight: 500;
        color: var(--color-text-secondary);
    }

    .favicon-section :global(input[type='checkbox']),
    .favicon-section :global(input[type='range']) {
        accent-color: var(--color-accent);
    }

    .favicon-size {
        position: relative;
        min-width: 58px;
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
    }

    .favicon-size:hover:not(:disabled) {
        color: var(--color-text);
    }

    .favicon-section :global(.favicon-size.is-selected) {
        padding-left: 16px;
        border-color: var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-weight: 500;
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }

    .favicon-section :global(.favicon-size.is-selected)::before {
        content: '';
        position: absolute;
        left: 6px;
        top: 6px;
        bottom: 6px;
        width: 2px;
        border-radius: 999px;
        background: var(--color-accent);
    }

    @media (min-width: 1024px) {
        .favicon-source {
            position: sticky;
            top: 0;
            align-self: start;
        }
    }

    @media (max-width: 640px) {
        .favicon-toolbar {
            align-items: stretch;
        }

        .favicon-toolbar > button {
            justify-content: center;
        }
    }
</style>
