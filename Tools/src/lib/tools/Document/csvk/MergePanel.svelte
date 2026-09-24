<script lang="ts">
    /*
      MergePanel — the "Merge" mode of CSV Toolkit. Body-only panel
      (no ToolPage); composed from the foundation kit + shared
      `.csvk-*` widgets defined in CsvToolkit.svelte. Logic is the
      former CsvMerge.svelte, preserved verbatim.
    */
    import { onMount } from 'svelte';
    import { open, save } from '@tauri-apps/plugin-dialog';
    import DropZone from '$lib/DropZone.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import { Button, ToolPanel, Checkbox } from '$lib/ui';
    import { FileSpreadsheet, FolderOpen, Plus, Play, Trash2, FileOutput, Ban } from '@lucide/svelte';
    import {
        addCsvMergeSources,
        cancelCsvMerge,
        clearCsvMergeState,
        csvMergeCancelling,
        csvMergeError,
        csvMergeIncludeHeaders,
        csvMergeIncludeSource,
        csvMergeInputDelimiter,
        csvMergeOutputDelimiter,
        csvMergeOutputPath,
        csvMergeProcessing,
        csvMergeProgress,
        csvMergeResults,
        csvMergeSources,
        initCsvMergeListeners,
        removeCsvMergeSource,
        runCsvMerge,
    } from '$lib/stores/csvMerge';

    const successCount = $derived($csvMergeResults.filter((r) => r.success).length);
    const failCount = $derived($csvMergeResults.filter((r) => !r.success).length);
    const totalRows = $derived($csvMergeResults.reduce((sum, item) => sum + item.rows_written, 0));
    const mergeProgressPercent = $derived(
        $csvMergeProgress && $csvMergeProgress.items_total > 0
            ? Math.round(($csvMergeProgress.items_done / $csvMergeProgress.items_total) * 100)
            : 0,
    );

    onMount(() => {
        initCsvMergeListeners();
    });

    function fileName(path: string) {
        return path.split(/[\\/]/).pop() || path;
    }

    function outputExt() {
        if ($csvMergeOutputDelimiter === 'tab') return 'tsv';
        return 'csv';
    }

    function outputDefaultPath() {
        if (!$csvMergeSources.length) return `merged.${outputExt()}`;
        const base = fileName($csvMergeSources[0]).replace(/\.[^.]+$/, '') || 'merged';
        const idx = Math.max($csvMergeSources[0].lastIndexOf('/'), $csvMergeSources[0].lastIndexOf('\\'));
        const folder = idx >= 0 ? $csvMergeSources[0].slice(0, idx) : '';
        const sep = folder.includes('\\') ? '\\' : '/';
        return folder ? `${folder}${folder.endsWith(sep) ? '' : sep}${base}_merged.${outputExt()}` : `${base}_merged.${outputExt()}`;
    }

    async function pickCsvFiles() {
        const selected = await open({
            multiple: true,
            directory: false,
            filters: [{ name: 'CSV files', extensions: ['csv', 'tsv', 'txt'] }],
        });
        if (Array.isArray(selected)) {
            await addCsvMergeSources(selected);
        } else if (typeof selected === 'string') {
            await addCsvMergeSources([selected]);
        }
    }

    async function pickCsvFolders() {
        const selected = await open({
            multiple: true,
            directory: true,
        });
        if (!selected) return;
        if (Array.isArray(selected)) {
            await addCsvMergeSources(selected);
        } else {
            await addCsvMergeSources([selected]);
        }
    }

    async function pickOutput() {
        const selected = await save({
            defaultPath: outputDefaultPath(),
            filters: [{ name: 'CSV', extensions: ['csv', 'tsv'] }],
        });
        if (typeof selected === 'string') {
            csvMergeOutputPath.set(selected);
        }
    }
</script>

<div class="csvk-panel">
    <div class="csvk-head">
        <div class="csvk-head-text">
            <h2 class="csvk-title">Merge — stitch many CSVs into one clean file</h2>
            <p class="csvk-desc">
                Append multiple CSV/TSV files into a single output. Optionally tag each row with its source filename and
                de-duplicate header rows. Mismatched delimiters get normalized on the way out.
            </p>
        </div>
    </div>

    <!-- Toolbar -->
    <div class="csvk-toolbar">
        <Button variant="secondary" icon={Plus} onclick={pickCsvFiles} disabled={$csvMergeProcessing}>Add files</Button>
        <Button variant="secondary" icon={FolderOpen} onclick={pickCsvFolders} disabled={$csvMergeProcessing}>Add folders</Button>
        <Button
            variant="secondary"
            icon={FileOutput}
            onclick={pickOutput}
            disabled={$csvMergeProcessing}
            title={$csvMergeOutputPath || 'Choose output file'}
        >
            {$csvMergeOutputPath ? `Output: ${fileName($csvMergeOutputPath)}` : 'Choose output file'}
        </Button>
        {#if $csvMergeSources.length > 0 && !$csvMergeProcessing}
            <Button variant="ghost" icon={Trash2} onclick={clearCsvMergeState}>Clear ({$csvMergeSources.length})</Button>
        {/if}
        <div class="csvk-toolbar-end">
            {#if $csvMergeProcessing}
                <Button variant="danger" icon={Ban} onclick={cancelCsvMerge} loading={$csvMergeCancelling} disabled={$csvMergeCancelling}>
                    {$csvMergeCancelling ? 'Cancelling' : 'Cancel'}
                </Button>
            {/if}
            <Button
                variant="primary"
                icon={Play}
                onclick={runCsvMerge}
                loading={$csvMergeProcessing}
                disabled={$csvMergeProcessing || $csvMergeSources.length === 0 || !$csvMergeOutputPath}
            >
                {$csvMergeProcessing ? 'Merging…' : `Merge ${$csvMergeSources.length || ''}`.trim()}
            </Button>
        </div>
    </div>

    <div class="csvk-grid">
        <div class="csvk-col">
            <!-- Source files -->
            <ToolPanel padding="md">
                <div class="csvk-section-label" style="margin-bottom: 8px;">Source files ({$csvMergeSources.length})</div>
                <DropZone onFiles={addCsvMergeSources} accept={[]}>
                    {#snippet children()}
                        {#if $csvMergeSources.length === 0}
                            <EmptyState
                                icon={FileSpreadsheet}
                                title="Drop CSV/TSV files here"
                                description="Or use Add files / Add folders above. Files merge in the order shown."
                                variant="dashed"
                            />
                        {:else}
                            <div class="csvk-list">
                                {#each $csvMergeSources as path (path)}
                                    <div class="csvk-row">
                                        <div class="csvk-row-main">
                                            <span class="csvk-row-name" title={fileName(path)}>{fileName(path)}</span>
                                            <span class="csvk-row-path" title={path}>{path}</span>
                                        </div>
                                        {#if !$csvMergeProcessing}
                                            <button
                                                type="button"
                                                class="csvk-link-btn"
                                                onclick={() => removeCsvMergeSource(path)}
                                                aria-label="Remove {fileName(path)}"
                                            >Remove</button>
                                        {/if}
                                    </div>
                                {/each}
                            </div>
                        {/if}
                    {/snippet}
                </DropZone>
            </ToolPanel>

            <!-- Progress -->
            {#if $csvMergeProcessing || $csvMergeProgress}
                <ToolPanel padding="md">
                    <div class="csvk-prog">
                        <div class="csvk-prog-head">
                            <div class="csvk-prog-main">
                                <span>{$csvMergeProcessing ? ($csvMergeCancelling ? 'Cancelling…' : 'Merging files…') : 'Last merge'}</span>
                                {#if $csvMergeProgress?.current_item}
                                    <span class="csvk-prog-item" title={$csvMergeProgress.current_item}>{fileName($csvMergeProgress.current_item)}</span>
                                {/if}
                            </div>
                            <span class="csvk-prog-meta">
                                {$csvMergeProgress?.items_done ?? 0}/{$csvMergeProgress?.items_total ?? 0} files · {$csvMergeProgress?.rows_done ?? 0} rows
                            </span>
                        </div>
                        <div class="csvk-bar" role="progressbar" aria-label="Merge progress">
                            <div class="csvk-bar-fill" style={`width: ${mergeProgressPercent}%`}></div>
                        </div>
                    </div>
                </ToolPanel>
            {/if}

            <!-- Error -->
            {#if $csvMergeError}
                <div class="csvk-error">{$csvMergeError}</div>
            {/if}

            <!-- Results -->
            {#if $csvMergeResults.length > 0}
                <ToolPanel padding="md">
                    <div class="csvk-head" style="margin-bottom: 8px;">
                        <span class="csvk-section-label">Results</span>
                        <span class="csvk-hint">
                            <span style="color: var(--color-success);">{successCount} succeeded</span>
                            {#if failCount > 0} · <span style="color: var(--color-error);">{failCount} failed</span>{/if}
                            · {totalRows.toLocaleString()} rows written
                        </span>
                    </div>
                    <div class="csvk-results">
                        {#each $csvMergeResults as item}
                            <div class="csvk-result" class:is-fail={!item.success}>
                                <div class="csvk-result-row">
                                    <span class="csvk-result-name" title={item.source_path}>{fileName(item.source_path)}</span>
                                    <span class="csvk-badge {item.success ? 'ok' : 'fail'}">{item.success ? 'OK' : 'Fail'}</span>
                                </div>
                                {#if item.success}
                                    <div class="csvk-result-detail">{item.rows_written.toLocaleString()} rows written</div>
                                {:else if item.error}
                                    <div class="csvk-result-error">{item.error}</div>
                                {/if}
                            </div>
                        {/each}
                    </div>
                </ToolPanel>
            {/if}
        </div>

        <!-- Options -->
        <ToolPanel padding="md">
            <div class="csvk-section-label" style="margin-bottom: 10px;">Options</div>
            <div class="csvk-fields">
                <label class="csvk-field">
                    <span class="csvk-label">Source delimiter</span>
                    <select class="csvk-select" bind:value={$csvMergeInputDelimiter} disabled={$csvMergeProcessing} aria-label="Source delimiter">
                        <option value="auto">Auto</option>
                        <option value="comma">Comma</option>
                        <option value="tab">Tab</option>
                        <option value="semicolon">Semicolon</option>
                        <option value="pipe">Pipe</option>
                    </select>
                </label>
                <label class="csvk-field">
                    <span class="csvk-label">Output delimiter</span>
                    <select class="csvk-select" bind:value={$csvMergeOutputDelimiter} disabled={$csvMergeProcessing} aria-label="Output delimiter">
                        <option value="comma">Comma</option>
                        <option value="tab">Tab</option>
                        <option value="semicolon">Semicolon</option>
                        <option value="pipe">Pipe</option>
                    </select>
                </label>
                <div class="csvk-checks" style="margin-top: 2px;">
                    <Checkbox bind:checked={$csvMergeIncludeSource} disabled={$csvMergeProcessing} label="Include source column" />
                    <Checkbox bind:checked={$csvMergeIncludeHeaders} disabled={$csvMergeProcessing} label="Keep only first header row" />
                </div>
            </div>
        </ToolPanel>
    </div>
</div>
