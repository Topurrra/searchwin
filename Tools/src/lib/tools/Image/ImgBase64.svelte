<script lang="ts">
    import { onMount } from 'svelte';
    import { open } from '@tauri-apps/plugin-dialog';
    import { Binary, Copy, X, Plus, Play, ImageIcon, FolderOpen } from '@lucide/svelte';
    import DropZone from '$lib/DropZone.svelte';
    import LoadingState from '$lib/components/LoadingState.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import ToolCancelButton from '$lib/components/ToolCancelButton.svelte';
    import { ToolPage } from '$lib/ui';
    import {
        addBase64Files,
        base64Cancelling,
        base64CopyMode,
        base64Files,
        base64Processing,
        base64ProgressByPath,
        base64Results,
        base64SelectedIdx,
        cancelImageBase64,
        clearBase64State,
        copyBase64All,
        copyBase64Selected,
        initImageBase64Store,
        removeBase64File,
        runImageBase64,
    } from '$lib/stores/imageBase64';
    import { fileName, fmtBytes } from '$lib/stores/imageBatchCommon';

    const supportedExts = ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'tiff', 'tif', 'gif', 'svg', 'ico'];

    onMount(() => {
        initImageBase64Store();
    });

    async function pickFiles() {
        const selected = await open({ multiple: true, directory: false, filters: [{ name: 'Images', extensions: supportedExts }] });
        if (Array.isArray(selected)) addBase64Files(selected);
        else if (typeof selected === 'string') addBase64Files([selected]);
    }

    const selected = $derived($base64Results.length > 0 && $base64Results[$base64SelectedIdx]?.success ? $base64Results[$base64SelectedIdx] : null);
    const successCount = $derived($base64Results.filter((r) => r.success).length);
    const failCount = $derived($base64Results.filter((r) => !r.success && r.error !== 'Cancelled').length);
    const activeCount = $derived(Object.values($base64ProgressByPath).filter((p) => !['queued', 'done', 'error', 'cancelled'].includes(p.stage)).length);
</script>

<ToolPage
    icon={Binary}
    iconTint="#22c55e"
    title="Image to Base64"
    description="Drop one or many images and copy the data URI ready to paste into HTML, CSS, or JSON. Encoding runs locally — no upload."
    width="wide"
    fill={false}
>
    <!-- Toolbar -->
    <div class="base64-toolbar">
        <button
            type="button"
            onclick={pickFiles}
            disabled={$base64Processing}
            class="base64-action"
        >
            <Plus class="h-4 w-4" />
            Add images
        </button>

        {#if $base64Files.length > 0 && !$base64Processing}
            <button
                type="button"
                onclick={clearBase64State}
                class="base64-action base64-clear"
            >
                Clear all ({$base64Files.length})
            </button>
        {/if}

        <div class="flex-1"></div>

        <ToolCancelButton
            running={$base64Processing}
            cancelling={$base64Cancelling}
            onCancel={() => cancelImageBase64()}
        />
        <button
            type="button"
            onclick={runImageBase64}
            disabled={$base64Files.length === 0 || $base64Processing}
            class="base64-primary"
        >
            {#if $base64Processing}
                <LoadingState variant="inline" label="Encoding..." />
            {:else}
                <Play class="h-4 w-4" />
                Convert {$base64Files.length} image{$base64Files.length === 1 ? '' : 's'}
            {/if}
        </button>
    </div>

    <!-- File list -->
    <div class="base64-queue">
        <DropZone onFiles={addBase64Files} accept={supportedExts}>
            {#snippet children()}
                {#if $base64Files.length === 0}
                    <EmptyState
                        icon={ImageIcon}
                        title="Drop images to encode"
                        description="JPEG, PNG, WebP, GIF, BMP, SVG, ICO."
                        variant="dashed"
                    >
                        {#snippet actions()}
                            <button type="button" onclick={pickFiles} class="base64-browse">
                                <FolderOpen class="h-4 w-4" />
                                Browse files
                            </button>
                        {/snippet}
                    </EmptyState>
                {:else}
                    <div class="base64-file-list">
                        {#each $base64Files as path}
                            {@const p = $base64ProgressByPath[path]}
                            <div class="base64-queue-row">
                                <div class="flex items-center justify-between gap-3">
                                    <span class="truncate flex-1" title={path}>{fileName(path)}</span>
                                    {#if $base64Processing && p}
                                        <span class="text-[10px] text-muted uppercase shrink-0">{p.stage} - {p.progress}%</span>
                                    {:else if !$base64Processing}
                                        <button onclick={() => removeBase64File(path)} class="text-muted hover:text-error text-xs shrink-0" aria-label="Remove file"><X class="w-3.5 h-3.5" /></button>
                                    {/if}
                                </div>
                                {#if $base64Processing && p}
                                    <div class="mt-1.5 h-1.5 bg-bg rounded overflow-hidden"><div class="h-full bg-accent transition-all" style={`width: ${p.progress}%`}></div></div>
                                    {#if p.message}<div class="mt-1 text-[10px] text-error truncate" title={p.message}>{p.message}</div>{/if}
                                {/if}
                            </div>
                        {/each}
                    </div>
                {/if}
            {/snippet}
        </DropZone>
    </div>

    <!-- Progress -->
    {#if $base64Processing}
        <div class="base64-progress">
            <div class="flex items-center justify-between text-sm">
                <LoadingState variant="inline" label={$base64Cancelling ? 'Cancelling Base64 conversion...' : 'Encoding images...'} />
                <span class="text-muted text-xs">{activeCount || 1} active - {$base64Files.length} total</span>
            </div>
            <div class="h-1.5 bg-panel-2 rounded overflow-hidden"><div class="h-full w-1/2 bg-accent animate-pulse"></div></div>
            <p class="text-xs text-muted">You can switch tools. Progress and results will stay here.</p>
        </div>
    {/if}

    <!-- Results -->
    {#if $base64Results.length > 0}
        <div class="base64-results">
            <div class="base64-results-grid">
                <div class="base64-result-list">
                    <div class="base64-result-list-head">
                        <span class="text-xs uppercase tracking-wider text-muted font-semibold">Files</span>
                        <span class="text-xs text-muted">
                            <span class="text-success">{successCount}</span> -
                            {#if failCount > 0}<span class="text-error">{failCount}</span>{:else}<span class="text-muted">0</span>{/if}
                        </span>
                    </div>
                    <div class="base64-result-scroll">
                        {#each $base64Results as result, index}
                            <button type="button" class="base64-result-row" class:is-selected={$base64SelectedIdx === index} aria-pressed={$base64SelectedIdx === index} onclick={() => base64SelectedIdx.set(index)}>
                                <div class="truncate" title={result.source_path}>{result.file_name || fileName(result.source_path)}</div>
                                {#if result.success}
                                    <div class="text-[10px] text-muted">{fmtBytes(result.original_size)} -&gt; {fmtBytes(result.base64_len)}</div>
                                {:else}
                                    <div class="text-[10px] text-error truncate" title={result.error}>{result.error}</div>
                                {/if}
                            </button>
                        {/each}
                    </div>
                </div>

                <div class="base64-result-canvas">
                    <div class="base64-output-toolbar">
                        <div class="base64-copy-mode" role="group" aria-label="Copy format">
                            <button type="button" class="base64-copy-mode-button" class:is-active={$base64CopyMode === 'data_uri'} aria-pressed={$base64CopyMode === 'data_uri'} onclick={() => base64CopyMode.set('data_uri')}>Data URI</button>
                            <button type="button" class="base64-copy-mode-button" class:is-active={$base64CopyMode === 'raw'} aria-pressed={$base64CopyMode === 'raw'} onclick={() => base64CopyMode.set('raw')}>Raw Base64</button>
                        </div>
                        <div class="base64-copy-actions">
                            <button onclick={copyBase64Selected} disabled={!selected} class="inline-flex items-center gap-1 text-xs text-muted hover:text-text disabled:opacity-40"><Copy class="w-3 h-3" /> Copy selected</button>
                            <button onclick={copyBase64All} disabled={successCount === 0} class="inline-flex items-center gap-1 text-xs text-muted hover:text-text disabled:opacity-40"><Copy class="w-3 h-3" /> Copy all</button>
                        </div>
                    </div>

                    {#if selected}
                        <div class="base64-preview-card">
                            <img src={selected.data_uri} alt={selected.file_name} class="w-14 h-14 object-contain rounded" />
                            <div class="text-xs text-text-secondary min-w-0">
                                <div class="truncate" title={selected.source_path}>{selected.file_name}</div>
                                <div class="text-muted">{selected.mime}</div>
                                <div class="text-muted">{fmtBytes(selected.original_size)} -&gt; {fmtBytes(selected.base64_len)} Base64</div>
                            </div>
                        </div>
                        <textarea value={$base64CopyMode === 'data_uri' ? selected.data_uri : selected.raw_base64} readonly spellcheck="false" class="base64-output"></textarea>
                    {:else if $base64Results[$base64SelectedIdx] && !$base64Results[$base64SelectedIdx].success}
                        <div class="p-3 bg-error-soft border border-error-strong rounded-xl text-error text-sm">{$base64Results[$base64SelectedIdx].error}</div>
                    {/if}
                </div>
            </div>
        </div>
    {/if}
</ToolPage>
<style>
    .base64-toolbar {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 8px;
        padding: 0 0 12px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .base64-toolbar > :global(.flex-1) { flex: 1; }
    .base64-action, .base64-primary, .base64-browse {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 7px;
        min-height: 34px;
        padding: 0 11px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12px;
        cursor: pointer;
    }
    .base64-action:hover:not(:disabled), .base64-browse:hover:not(:disabled) {
        border-color: color-mix(in srgb, var(--color-text) 24%, var(--color-border));
        color: var(--color-text);
    }
    .base64-clear:hover:not(:disabled) {
        border-color: color-mix(in srgb, var(--color-error) 45%, var(--color-border));
        color: var(--color-error);
    }
    .base64-primary {
        border-color: var(--color-accent);
        background: var(--color-accent);
        color: var(--color-accent-contrast);
    }
    .base64-primary:hover:not(:disabled) { background: var(--color-accent-hover, var(--color-accent)); }
    .base64-action:disabled, .base64-primary:disabled, .base64-browse:disabled { cursor: default; opacity: .5; }
    .base64-action:focus-visible, .base64-primary:focus-visible, .base64-browse:focus-visible,
    .base64-result-row:focus-visible, .base64-copy-mode-button:focus-visible, .base64-copy-actions button:focus-visible,
    .base64-output:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }

    .base64-queue {
        min-width: 0;
        padding: 16px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
    }
    .base64-browse { color: var(--color-text); }
    .base64-file-list {
        display: flex;
        flex-direction: column;
        max-height: 18rem;
        overflow: auto;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    .base64-queue-row {
        padding: 9px 10px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 66%, transparent);
        color: var(--color-text);
        font-size: 12px;
    }
    .base64-queue-row:first-child { border-top: 0; }
    .base64-queue-row:hover { background: color-mix(in srgb, var(--color-text) 3%, transparent); }
    .base64-queue-row button {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 26px;
        height: 26px;
        border: 1px solid transparent;
        border-radius: var(--radius-control, 8px);
        background: transparent;
        color: var(--color-muted);
        cursor: pointer;
    }
    .base64-queue-row button:hover { border-color: color-mix(in srgb, var(--color-error) 38%, var(--color-border)); color: var(--color-error); }

    .base64-progress {
        display: grid;
        gap: 8px;
        padding: 11px 12px;
        border-left: 2px solid var(--color-accent);
        border-top: 1px solid var(--color-divider, var(--color-border));
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }

    .base64-results {
        padding-top: 4px;
        border-top: 1px solid var(--color-divider, var(--color-border));
    }
    .base64-results-grid {
        display: grid;
        grid-template-columns: minmax(220px, 280px) minmax(0, 1fr);
        gap: 12px;
    }
    .base64-result-list, .base64-result-canvas {
        min-width: 0;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
    }
    .base64-result-list { overflow: hidden; }
    .base64-result-list-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        padding: 10px 12px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .base64-result-scroll { max-height: 30rem; overflow: auto; }
    .base64-result-row {
        position: relative;
        display: block;
        width: 100%;
        padding: 10px 12px 10px 16px;
        border: 0;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 66%, transparent);
        background: transparent;
        color: var(--color-text);
        font: inherit;
        text-align: left;
        cursor: pointer;
    }
    .base64-result-row:first-child { border-top: 0; }
    .base64-result-row:hover { background: color-mix(in srgb, var(--color-text) 3%, transparent); }
    .base64-result-row.is-selected { background: var(--color-panel-2); color: var(--color-text); }
    .base64-result-row.is-selected::before {
        position: absolute;
        top: 8px;
        bottom: 8px;
        left: 5px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }

    .base64-result-canvas { display: flex; flex-direction: column; gap: 12px; padding: 12px; }
    .base64-output-toolbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        flex-wrap: wrap;
        gap: 8px;
    }
    .base64-copy-mode {
        display: inline-flex;
        gap: 3px;
        padding: 3px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel);
    }
    .base64-copy-mode-button {
        position: relative;
        min-height: 28px;
        padding: 0 10px 0 14px;
        border: 0;
        border-radius: calc(var(--radius-control, 8px) - 2px);
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 11px;
        cursor: pointer;
    }
    .base64-copy-mode-button:hover { color: var(--color-text); }
    .base64-copy-mode-button.is-active { background: var(--color-panel-2); color: var(--color-text); }
    .base64-copy-mode-button.is-active::before {
        position: absolute;
        top: 6px;
        bottom: 6px;
        left: 4px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }
    .base64-copy-actions { display: inline-flex; flex-wrap: wrap; gap: 5px; }
    .base64-copy-actions button {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        min-height: 28px;
        padding: 0 6px;
        border: 0;
        background: transparent;
        color: var(--color-muted);
        font-size: 11px;
        cursor: pointer;
    }
    .base64-copy-actions button:hover:not(:disabled) { color: var(--color-text); }
    .base64-copy-actions button:disabled { cursor: default; opacity: .45; }
    .base64-preview-card {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    .base64-preview-card img {
        width: 56px;
        height: 56px;
        flex: none;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel);
        object-fit: contain;
    }
    .base64-output {
        width: 100%;
        min-height: 16rem;
        box-sizing: border-box;
        resize: vertical;
        padding: 11px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
        font-size: 12px;
        line-height: 1.55;
    }
    @media (max-width: 760px) {
        .base64-toolbar > :global(.flex-1) { display: none; }
        .base64-results-grid { grid-template-columns: 1fr; }
        .base64-result-scroll { max-height: 14rem; }
    }
</style>
