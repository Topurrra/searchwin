<script lang="ts">
    /*
      JsonPanel — the "→ JSON" mode of CSV Toolkit. Body-only panel;
      logic preserved verbatim from the former CsvToJson.svelte.
    */
    import { onMount } from 'svelte';
    import { open } from '@tauri-apps/plugin-dialog';
    import DropZone from '$lib/DropZone.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import { Button, ToolPanel, Checkbox } from '$lib/ui';
    import { FileJson, FolderOpen, Plus, Play, Trash2, Ban } from '@lucide/svelte';
    import {
        addCsvToJsonSources,
        cancelCsvToJson,
        clearCsvToJsonResults,
        clearCsvToJsonState,
        csvToJsonCancelling,
        csvToJsonDelimiter,
        csvToJsonError,
        csvToJsonHasHeader,
        csvToJsonOutputDir,
        csvToJsonOutputFormat,
        csvToJsonProcessing,
        csvToJsonProgress,
        csvToJsonResults,
        csvToJsonSources,
        initCsvToJsonListeners,
        removeCsvToJsonSource,
        runCsvToJson,
    } from '$lib/stores/csvToJson';

    onMount(() => {
        initCsvToJsonListeners();
    });

    const progressPercent = $derived(
        $csvToJsonProgress && $csvToJsonProgress.items_total > 0
            ? Math.round(($csvToJsonProgress.items_done / $csvToJsonProgress.items_total) * 100)
            : 0,
    );

    const totalFiles = $derived($csvToJsonResults.length);
    const successCount = $derived($csvToJsonResults.filter((r) => r.success).length);
    const failCount = $derived($csvToJsonResults.filter((r) => !r.success).length);
    const totalRows = $derived($csvToJsonResults.reduce((sum, r) => sum + r.rows_written, 0));
    const totalColumns = $derived($csvToJsonResults.reduce((sum, r) => sum + r.columns_written, 0));
    const totalColumnsText = $derived(totalColumns > 0 ? totalColumns.toLocaleString() : '0');

    async function pickFiles() {
        const selected = await open({
            multiple: true,
            directory: false,
            filters: [{ name: 'CSV files', extensions: ['csv', 'tsv', 'txt'] }],
        });
        if (Array.isArray(selected)) await addCsvToJsonSources(selected);
        else if (typeof selected === 'string') await addCsvToJsonSources([selected]);
    }

    async function pickFolders() {
        const selected = await open({
            multiple: true,
            directory: true,
        });
        if (!selected) return;
        if (Array.isArray(selected)) await addCsvToJsonSources(selected);
        else await addCsvToJsonSources([selected]);
    }

    async function pickOutputDir() {
        const selected = await open({
            multiple: false,
            directory: true,
        });
        if (typeof selected === 'string') {
            csvToJsonOutputDir.set(selected);
        }
    }

    function fileName(path: string) {
        return path.split(/[\\/]/).pop() ?? path;
    }

    function run() {
        if (!$csvToJsonSources.length) return;
        runCsvToJson();
    }
</script>

<div class="csvk-panel">
    <div class="csvk-head">
        <div class="csvk-head-text">
            <h2 class="csvk-title">→ JSON — stream tabular data into JSON or JSONL</h2>
            <p class="csvk-desc">
                Drop in CSV/TSV files or whole folders. KeepItLocal converts each to a JSON Lines stream or a single JSON
                array. Headers become keys; rows write incrementally so even very large files convert without spiking memory.
            </p>
        </div>
    </div>

    <!-- Toolbar -->
    <div class="csvk-toolbar">
        <Button variant="secondary" icon={Plus} onclick={pickFiles} disabled={$csvToJsonProcessing}>Add files</Button>
        <Button variant="secondary" icon={FolderOpen} onclick={pickFolders} disabled={$csvToJsonProcessing}>Add folders</Button>
        <Button
            variant="secondary"
            icon={FolderOpen}
            onclick={pickOutputDir}
            disabled={$csvToJsonProcessing}
            title={$csvToJsonOutputDir || 'Same folder as input'}
        >
            {$csvToJsonOutputDir ? `Output: ${fileName($csvToJsonOutputDir)}` : 'Output: alongside input'}
        </Button>
        {#if $csvToJsonSources.length > 0 && !$csvToJsonProcessing}
            <Button variant="ghost" icon={Trash2} onclick={clearCsvToJsonState}>Clear ({$csvToJsonSources.length})</Button>
        {/if}
        <div class="csvk-toolbar-end">
            {#if $csvToJsonProcessing}
                <Button variant="danger" icon={Ban} onclick={cancelCsvToJson} loading={$csvToJsonCancelling} disabled={$csvToJsonCancelling}>
                    {$csvToJsonCancelling ? 'Cancelling' : 'Cancel'}
                </Button>
            {/if}
            <Button
                variant="primary"
                icon={Play}
                onclick={run}
                loading={$csvToJsonProcessing}
                disabled={$csvToJsonProcessing || $csvToJsonSources.length === 0}
            >
                {$csvToJsonProcessing ? 'Converting…' : `Convert ${$csvToJsonSources.length || ''}`.trim()}
            </Button>
        </div>
    </div>

    <div class="csvk-grid">
        <div class="csvk-col">
            <!-- Source files -->
            <ToolPanel padding="md">
                <DropZone onFiles={addCsvToJsonSources} accept={['csv', 'tsv', 'txt']}>
                    {#snippet children()}
                        {#if $csvToJsonSources.length === 0}
                            <EmptyState
                                icon={FileJson}
                                title="Drop CSV/TSV files here"
                                description="Or use Add files / Add folders above. Each input file becomes one JSON output file."
                                variant="dashed"
                            />
                        {:else}
                            <div class="csvk-list">
                                {#each $csvToJsonSources as path (path)}
                                    <div class="csvk-row">
                                        <div class="csvk-row-main">
                                            <span class="csvk-row-name" title={fileName(path)}>{fileName(path)}</span>
                                            <span class="csvk-row-path" title={path}>{path}</span>
                                        </div>
                                        {#if !$csvToJsonProcessing}
                                            <button
                                                type="button"
                                                class="csvk-link-btn"
                                                onclick={() => removeCsvToJsonSource(path)}
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
            {#if $csvToJsonProcessing || $csvToJsonProgress}
                <ToolPanel padding="md">
                    <div class="csvk-prog">
                        <div class="csvk-prog-head">
                            <div class="csvk-prog-main">
                                <span>{$csvToJsonProcessing ? ($csvToJsonCancelling ? 'Cancelling…' : 'Converting files…') : 'Last conversion'}</span>
                                {#if $csvToJsonProgress?.current_item}
                                    <span class="csvk-prog-item" title={$csvToJsonProgress.current_item}>{fileName($csvToJsonProgress.current_item)}</span>
                                {/if}
                            </div>
                            <span class="csvk-prog-meta">
                                {$csvToJsonProgress?.items_done ?? 0}/{$csvToJsonProgress?.items_total ?? 0} files · {$csvToJsonProgress?.rows_done ?? 0} rows
                            </span>
                        </div>
                        <div class="csvk-bar" role="progressbar" aria-label="Conversion progress">
                            <div class="csvk-bar-fill" style={`width: ${progressPercent}%`}></div>
                        </div>
                    </div>
                </ToolPanel>
            {/if}

            <!-- Error -->
            {#if $csvToJsonError}
                <div class="csvk-error">{$csvToJsonError}</div>
            {/if}

            <!-- Results -->
            {#if $csvToJsonResults.length > 0}
                <ToolPanel padding="md">
                    <div class="csvk-head" style="margin-bottom: 8px;">
                        <span class="csvk-section-label">Results</span>
                        <span class="csvk-hint">
                            <span style="color: var(--color-success);">{successCount} succeeded</span>
                            {#if failCount > 0} · <span style="color: var(--color-error);">{failCount} failed</span>{/if}
                            {#if totalFiles > 0} · {totalRows.toLocaleString()} rows · {totalColumnsText} cols{/if}
                            {#if !$csvToJsonProcessing}
                                · <button type="button" class="csvk-link-btn" style="padding: 0;" onclick={clearCsvToJsonResults}>Clear</button>
                            {/if}
                        </span>
                    </div>
                    <div class="csvk-results">
                        {#each $csvToJsonResults as item}
                            <div class="csvk-result" class:is-fail={!item.success}>
                                <div class="csvk-result-row">
                                    <span class="csvk-result-name" title={item.source_path}>{fileName(item.source_path)}</span>
                                    <span class="csvk-badge {item.success ? 'ok' : 'fail'}">{item.success ? 'OK' : 'Fail'}</span>
                                </div>
                                <div class="csvk-result-detail">{item.rows_written.toLocaleString()} rows · {item.columns_written} columns</div>
                                {#if item.output_path}
                                    <div class="csvk-result-path" title={item.output_path}>{item.output_path}</div>
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
                    <span class="csvk-label">Input delimiter</span>
                    <select class="csvk-select" bind:value={$csvToJsonDelimiter} disabled={$csvToJsonProcessing} aria-label="Input delimiter">
                        <option value="auto">Auto</option>
                        <option value="comma">Comma</option>
                        <option value="tab">Tab</option>
                        <option value="semicolon">Semicolon</option>
                        <option value="pipe">Pipe</option>
                    </select>
                </label>
                <label class="csvk-field">
                    <span class="csvk-label">Output format</span>
                    <select class="csvk-select" bind:value={$csvToJsonOutputFormat} disabled={$csvToJsonProcessing} aria-label="Output format">
                        <option value="jsonl">JSONL (.jsonl)</option>
                        <option value="json">JSON array (.json)</option>
                    </select>
                </label>
                <div class="csvk-checks" style="margin-top: 2px;">
                    <Checkbox bind:checked={$csvToJsonHasHeader} disabled={$csvToJsonProcessing} label="First row is header" />
                </div>
                <div class="csvk-hint">
                    JSONL streams one row per line — best for huge datasets and pipelines. JSON array packs everything
                    into a single document — convenient for small files.
                </div>
            </div>
        </ToolPanel>
    </div>
</div>
