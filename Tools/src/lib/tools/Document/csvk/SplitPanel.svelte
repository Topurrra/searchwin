<script lang="ts">
    /*
      SplitPanel — the "Split" mode of CSV Toolkit. Body-only panel;
      logic preserved verbatim from the former CsvSplit.svelte.
    */
    import { onMount } from 'svelte';
    import { open } from '@tauri-apps/plugin-dialog';
    import DropZone from '$lib/DropZone.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import { Button, ToolPanel, Checkbox } from '$lib/ui';
    import { FileSpreadsheet, FolderOpen, Plus, Play, Trash2, Ban } from '@lucide/svelte';
    import {
        addCsvSplitSources,
        cancelCsvSplit,
        clearCsvSplitState,
        csvSplitCancelling,
        csvSplitDelimiter,
        csvSplitError,
        csvSplitIncludeHeader,
        csvSplitOutputDir,
        csvSplitProgress,
        csvSplitProcessing,
        csvSplitResults,
        csvSplitRowsPerFile,
        csvSplitSources,
        initCsvSplitListeners,
        removeCsvSplitSource,
        runCsvSplit,
    } from '$lib/stores/csvSplit';

    const splitProgressPercent = $derived(
        $csvSplitProgress && $csvSplitProgress.items_total > 0
            ? Math.round(($csvSplitProgress.items_done / $csvSplitProgress.items_total) * 100)
            : 0,
    );

    const totalSplitFiles = $derived($csvSplitResults.reduce((sum, item) => sum + item.split_count, 0));
    const totalRows = $derived($csvSplitResults.reduce((sum, item) => sum + item.total_rows, 0));
    const splitSuccessCount = $derived($csvSplitResults.filter((item) => item.success).length);
    const splitFailCount = $derived($csvSplitResults.filter((item) => !item.success).length);

    onMount(() => {
        initCsvSplitListeners();
    });

    function fileName(path: string) {
        return path.split(/[\\/]/).pop() ?? path;
    }

    function safeRowsPerFile(value: number) {
        if (!Number.isFinite(value) || value <= 0) return 1000;
        return Math.round(value);
    }

    async function pickCsvFiles() {
        const selected = await open({
            multiple: true,
            directory: false,
            filters: [{ name: 'CSV files', extensions: ['csv', 'tsv', 'txt'] }],
        });
        if (Array.isArray(selected)) {
            await addCsvSplitSources(selected);
        } else if (typeof selected === 'string') {
            await addCsvSplitSources([selected]);
        }
    }

    async function pickCsvFolders() {
        const selected = await open({ multiple: true, directory: true });
        if (!selected) return;
        if (Array.isArray(selected)) {
            await addCsvSplitSources(selected);
        } else {
            await addCsvSplitSources([selected]);
        }
    }

    async function pickOutputDir() {
        const selected = await open({ directory: true, multiple: false });
        if (typeof selected === 'string') {
            csvSplitOutputDir.set(selected);
        }
    }

    function onRowsPerFileChange(event: Event) {
        const target = event.currentTarget as HTMLInputElement;
        csvSplitRowsPerFile.set(safeRowsPerFile(Number(target.value)));
    }

    async function run() {
        if (!$csvSplitSources.length || !$csvSplitOutputDir) return;
        if (!$csvSplitRowsPerFile || $csvSplitRowsPerFile <= 0) {
            csvSplitRowsPerFile.set(1000);
            return;
        }
        await runCsvSplit();
    }
</script>

<div class="csvk-panel">
    <div class="csvk-head">
        <div class="csvk-head-text">
            <h2 class="csvk-title">Split — cut huge CSVs into safe-to-handle chunks</h2>
            <p class="csvk-desc">
                Pick the chunk size, drop in your CSV/TSV files, and KeepItLocal slices each into numbered output files —
                optionally repeating the header row in every chunk for easy downstream processing.
            </p>
        </div>
    </div>

    <!-- Toolbar -->
    <div class="csvk-toolbar">
        <Button variant="secondary" icon={Plus} onclick={pickCsvFiles} disabled={$csvSplitProcessing}>Add files</Button>
        <Button variant="secondary" icon={FolderOpen} onclick={pickCsvFolders} disabled={$csvSplitProcessing}>Add folders</Button>
        <Button
            variant="secondary"
            icon={FolderOpen}
            onclick={pickOutputDir}
            disabled={$csvSplitProcessing}
            title={$csvSplitOutputDir ?? ''}
        >
            {$csvSplitOutputDir ? `Output: ${fileName($csvSplitOutputDir)}` : 'Choose output folder'}
        </Button>
        {#if $csvSplitSources.length > 0 && !$csvSplitProcessing}
            <Button variant="ghost" icon={Trash2} onclick={clearCsvSplitState}>Clear ({$csvSplitSources.length})</Button>
        {/if}
        <div class="csvk-toolbar-end">
            {#if $csvSplitProcessing}
                <Button variant="danger" icon={Ban} onclick={cancelCsvSplit} loading={$csvSplitCancelling} disabled={$csvSplitCancelling}>
                    {$csvSplitCancelling ? 'Cancelling' : 'Cancel'}
                </Button>
            {/if}
            <Button
                variant="primary"
                icon={Play}
                onclick={run}
                loading={$csvSplitProcessing}
                disabled={$csvSplitProcessing || $csvSplitSources.length === 0 || !$csvSplitOutputDir || !$csvSplitRowsPerFile}
            >
                {$csvSplitProcessing ? 'Splitting…' : 'Split files'}
            </Button>
        </div>
    </div>

    <div class="csvk-grid">
        <div class="csvk-col">
            <!-- Source files -->
            <ToolPanel padding="md">
                <DropZone onFiles={addCsvSplitSources} accept={['csv', 'tsv', 'txt']}>
                    {#snippet children()}
                        {#if $csvSplitSources.length === 0}
                            <EmptyState
                                icon={FileSpreadsheet}
                                title="Drop CSV/TSV files here"
                                description="Or use Add files / Add folders above. Output filenames get a _split_001 suffix."
                                variant="dashed"
                            />
                        {:else}
                            <div class="csvk-list">
                                {#each $csvSplitSources as path (path)}
                                    <div class="csvk-row">
                                        <div class="csvk-row-main">
                                            <span class="csvk-row-name" title={fileName(path)}>{fileName(path)}</span>
                                            <span class="csvk-row-path" title={path}>{path}</span>
                                        </div>
                                        {#if !$csvSplitProcessing}
                                            <button
                                                type="button"
                                                class="csvk-link-btn"
                                                onclick={() => removeCsvSplitSource(path)}
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
            {#if $csvSplitProcessing || $csvSplitProgress}
                <ToolPanel padding="md">
                    <div class="csvk-prog">
                        <div class="csvk-prog-head">
                            <div class="csvk-prog-main">
                                <span>{$csvSplitProcessing ? ($csvSplitCancelling ? 'Cancelling…' : 'Splitting files…') : 'Last split'}</span>
                                {#if $csvSplitProgress?.current_item}
                                    <span class="csvk-prog-item" title={$csvSplitProgress.current_item}>{fileName($csvSplitProgress.current_item)}</span>
                                {/if}
                            </div>
                            <span class="csvk-prog-meta">
                                {$csvSplitProgress?.items_done ?? 0}/{$csvSplitProgress?.items_total ?? 0} files · {$csvSplitProgress?.rows_done ?? 0} rows
                            </span>
                        </div>
                        <div class="csvk-bar" role="progressbar" aria-label="Split progress">
                            <div class="csvk-bar-fill" style={`width: ${splitProgressPercent}%`}></div>
                        </div>
                    </div>
                </ToolPanel>
            {/if}

            <!-- Error -->
            {#if $csvSplitError}
                <div class="csvk-error">{$csvSplitError}</div>
            {/if}

            <!-- Results -->
            {#if $csvSplitResults.length > 0}
                <ToolPanel padding="md">
                    <div class="csvk-head" style="margin-bottom: 8px;">
                        <span class="csvk-section-label">Results</span>
                        <span class="csvk-hint">
                            <span style="color: var(--color-success);">{splitSuccessCount} succeeded</span>
                            {#if splitFailCount > 0} · <span style="color: var(--color-error);">{splitFailCount} failed</span>{/if}
                            · {totalSplitFiles} output files · {totalRows.toLocaleString()} rows
                        </span>
                    </div>
                    <div class="csvk-results">
                        {#each $csvSplitResults as item}
                            <div class="csvk-result" class:is-fail={!item.success}>
                                <div class="csvk-result-row">
                                    <span class="csvk-result-name" title={item.source_path}>{fileName(item.source_path)}</span>
                                    <span class="csvk-badge {item.success ? 'ok' : 'fail'}">{item.success ? 'OK' : 'Fail'}</span>
                                </div>
                                <div class="csvk-result-detail">
                                    {item.rows_per_file} rows/chunk · {item.split_count} output file{item.split_count === 1 ? '' : 's'}
                                </div>
                                {#if item.output_paths.length > 0}
                                    <div class="csvk-result-path" title={item.output_paths[0]}>
                                        {item.output_paths[0]}{#if item.output_paths.length > 1} (+{item.output_paths.length - 1} more){/if}
                                    </div>
                                {/if}
                                {#if !item.success && item.error}
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
                    <span class="csvk-label">Rows per file</span>
                    <input
                        class="csvk-input"
                        type="number"
                        min="1"
                        bind:value={$csvSplitRowsPerFile}
                        oninput={onRowsPerFileChange}
                        disabled={$csvSplitProcessing}
                        aria-label="Rows per file"
                    />
                </label>
                <label class="csvk-field">
                    <span class="csvk-label">Delimiter</span>
                    <select class="csvk-select" bind:value={$csvSplitDelimiter} disabled={$csvSplitProcessing} aria-label="Delimiter">
                        <option value="auto">Auto</option>
                        <option value="comma">Comma</option>
                        <option value="tab">Tab</option>
                        <option value="semicolon">Semicolon</option>
                        <option value="pipe">Pipe</option>
                    </select>
                </label>
                <div class="csvk-checks" style="margin-top: 2px;">
                    <Checkbox bind:checked={$csvSplitIncludeHeader} disabled={$csvSplitProcessing} label="Repeat header in each chunk" />
                </div>
                <div class="csvk-hint">
                    <button
                        type="button"
                        class="csvk-link-btn"
                        style="padding: 0; color: var(--color-muted);"
                        onclick={() => csvSplitRowsPerFile.set(1000)}
                        disabled={$csvSplitProcessing}
                    >Reset to 1000 rows/file</button>
                    · {totalSplitFiles} chunks created so far
                </div>
            </div>
        </ToolPanel>
    </div>
</div>
