<script lang="ts">
    /*
      CleanPanel — the "Clean" mode of CSV Toolkit. Body-only panel;
      logic preserved verbatim from the former CsvCleaner.svelte.
    */
    import { invoke } from '@tauri-apps/api/core';
    import { open } from '@tauri-apps/plugin-dialog';
    import { onMount } from 'svelte';
    import { get } from 'svelte/store';
    import DropZone from '$lib/DropZone.svelte';
    import { FileSpreadsheet, FolderOpen, Wand2, Plus, Trash2, Play, Eye, Ban } from '@lucide/svelte';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import { notify } from '$lib/stores/notifications';
    import { recordActivity } from '$lib/stores/activityLog';
    import { reportBusy } from '$lib/stores/globalBusy';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import { Button, ToolPanel, Checkbox } from '$lib/ui';
    import {
        cancelCsvCleanerRun,
        csvCleanerCancelling,
        csvCleanerProcessing,
        csvCleanerProgress,
        csvCleanerPreviewing,
        initCsvCleanerListeners,
        startCsvCleanerRun,
        setCsvCleanerStopped,
        csvCleanerSources,
        csvCleanerSelectedSource,
        csvCleanerOutputDir,
        csvCleanerInputDelimiter,
        csvCleanerOutputDelimiter,
        csvCleanerHasHeader,
        csvCleanerTrimCells,
        csvCleanerRemoveEmptyRows,
        csvCleanerRemoveEmptyColumns,
        csvCleanerNormalizeHeaders,
        csvCleanerPreview,
        csvCleanerPreviewAll,
        csvCleanerResults,
        csvCleanerError,
        type CsvCleanerDelimiter,
        type CsvCleanupPreviewResult,
        type CleanCsvBatchResult,
    } from '$lib/stores/csvCleaner';

    type Delimiter = CsvCleanerDelimiter;

    type CsvCleanupOptions = {
        source_path: string;
        input_delimiter: Delimiter;
        has_header: boolean;
        trim_cells: boolean;
        remove_empty_rows: boolean;
        remove_empty_columns: boolean;
        normalize_headers: boolean;
        output_delimiter: Exclude<Delimiter, 'auto'>;
    };

    type CleanCsvResult = {
        output_path: string;
        original_rows: number;
        cleaned_rows: number;
        original_columns: number;
        cleaned_columns: number;
        removed_empty_rows: number;
        removed_empty_columns: number;
    };

    type CsvSourcesResult = {
        paths: string[];
        warnings: string[];
    };

    // Preview state joins the rest of the cleaner state in its module store,
    // so a large preview stays visibly in flight after this panel remounts.
    const previewLoading = $derived($csvCleanerPreviewing);

    const successCount = $derived($csvCleanerResults.filter((r) => r.success).length);
    const failCount = $derived($csvCleanerResults.filter((r) => !r.success).length);
    const warningCount = $derived($csvCleanerPreviewAll.reduce((sum, item) => sum + item.removed_empty_columns + item.removed_empty_rows, 0));
    const cleanProgressPercent = $derived(
        $csvCleanerProgress && $csvCleanerProgress.items_total > 0
            ? Math.round(($csvCleanerProgress.items_done / $csvCleanerProgress.items_total) * 100)
            : 0,
    );

    function fileName(path: string) {
        return path.split(/[\\/]/).pop() ?? path;
    }

    function fileDirectory(path: string) {
        const idx = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
        if (idx < 0) return '';
        return path.slice(0, idx);
    }

    function normalizePath(path: string) {
        return path.toLowerCase();
    }

    function toPathJoin(dir: string, name: string) {
        if (!dir) return name;
        const sep = dir.includes('\\') ? '\\' : '/';
        return dir.endsWith(sep) ? `${dir}${name}` : `${dir}${sep}${name}`;
    }

    async function setBusy(busy: boolean) {
        try {
            await invoke('set_busy', { busy });
        } catch {
            // best effort
        }
    }

    function makeFailedResult(sourcePath: string, message: string): CleanCsvBatchResult {
        return {
            source_path: sourcePath,
            success: false,
            error: message,
            output_path: '',
            original_rows: 0,
            cleaned_rows: 0,
            original_columns: 0,
            cleaned_columns: 0,
            removed_empty_rows: 0,
            removed_empty_columns: 0,
        };
    }

    async function collectCsvSources(raw: string[]) {
        if (!raw.length) return [] as string[];

        const response = await invoke<CsvSourcesResult>('collect_csv_sources', {
            options: {
                paths: raw,
                recursive: true,
            },
        });

        if (response.warnings.length > 0 && response.paths.length === 0) {
            csvCleanerError.set(response.warnings.join('\n'));
        }

        return response.paths ?? [];
    }

    async function addCsvSources(raw: string[]) {
        const discovered = await collectCsvSources(raw);
        if (!discovered.length) return;

        const seen = new Set($csvCleanerSources.map(normalizePath));
        const next = discovered.filter((path) => !seen.has(normalizePath(path)));

        if (next.length > 0) {
            csvCleanerSources.set([...$csvCleanerSources, ...next]);
            if (!$csvCleanerSelectedSource) csvCleanerSelectedSource.set(next[0]);
            csvCleanerResults.set([]);
            csvCleanerPreview.set(null);
            csvCleanerPreviewAll.set([]);
            csvCleanerError.set(null);
        }
    }

    function removeSourcePath(path: string) {
        const next = $csvCleanerSources.filter((entry) => entry !== path);
        csvCleanerSources.set(next);

        if ($csvCleanerSelectedSource === path) {
            csvCleanerSelectedSource.set(next[0] ?? null);
        }

        if (next.length === 0) {
            csvCleanerPreview.set(null);
            csvCleanerPreviewAll.set([]);
            csvCleanerResults.set([]);
        }
    }

    function clearSources() {
        if (previewLoading) return;
        csvCleanerSources.set([]);
        csvCleanerSelectedSource.set(null);
        csvCleanerOutputDir.set(null);
        csvCleanerPreview.set(null);
        csvCleanerPreviewAll.set([]);
        csvCleanerResults.set([]);
        csvCleanerError.set(null);
    }

    async function pickCsvFiles() {
        const selected = await open({
            multiple: true,
            directory: false,
            filters: [{ name: 'CSV files', extensions: ['csv', 'tsv', 'txt'] }],
        });
        if (!selected) return;

        if (Array.isArray(selected)) {
            await addCsvSources(selected);
        } else {
            await addCsvSources([selected]);
        }
    }

    async function pickCsvFolders() {
        const selected = await open({
            multiple: true,
            directory: true,
        });
        if (!selected) return;

        if (Array.isArray(selected)) {
            await addCsvSources(selected);
        } else {
            await addCsvSources([selected]);
        }
    }

    async function chooseOutputDirectory() {
        const selected = await open({ multiple: false, directory: true });
        if (typeof selected === 'string') {
            csvCleanerOutputDir.set(selected);
        }
    }

    function buildOptions(sourcePath: string): CsvCleanupOptions {
        return {
            source_path: sourcePath,
            input_delimiter: $csvCleanerInputDelimiter,
            has_header: $csvCleanerHasHeader,
            trim_cells: $csvCleanerTrimCells,
            remove_empty_rows: $csvCleanerRemoveEmptyRows,
            remove_empty_columns: $csvCleanerRemoveEmptyColumns,
            normalize_headers: $csvCleanerNormalizeHeaders,
            output_delimiter: $csvCleanerOutputDelimiter,
        };
    }

    function makeOutputPath(sourcePath: string, usedOutputs: Set<string>) {
        const base = fileName(sourcePath).replace(/\.[^.]+$/, '');
        const ext = $csvCleanerOutputDelimiter === 'tab' ? 'tsv' : 'csv';
        const outputDir = $csvCleanerOutputDir ?? fileDirectory(sourcePath);
        const stem = base || 'cleaned';

        let suffix = '';
        let index = 0;
        let outputPath = toPathJoin(outputDir, `${stem}_cleaned${suffix}.${ext}`);
        while (usedOutputs.has(normalizePath(outputPath))) {
            index += 1;
            suffix = `_${index}`;
            outputPath = toPathJoin(outputDir, `${stem}_cleaned${suffix}.${ext}`);
        }

        usedOutputs.add(normalizePath(outputPath));
        return outputPath;
    }

    async function previewCleanupSingle() {
        const selectedSourcePath = $csvCleanerSelectedSource;
        if (!selectedSourcePath || previewLoading || $csvCleanerProcessing) return;

        csvCleanerError.set(null);
        csvCleanerPreviewing.set(true);
        const stopBusy = reportBusy('csv-cleaner', 'Previewing CSV cleanup');

        try {
            const data = await invoke<CsvCleanupPreviewResult>('preview_csv_cleanup', {
                options: { options: buildOptions(selectedSourcePath) },
            });
            csvCleanerPreview.set(data);
            csvCleanerPreviewAll.set([
                ...$csvCleanerPreviewAll.filter((item) => item.source_path !== data.source_path),
                data,
            ]);
            notify({
                level: 'success',
                title: 'CSV cleanup preview ready',
                message: `${data.cleaned_rows} rows / ${data.cleaned_columns} columns after cleanup`,
                toolId: 'csv-cleaner',
            });
            toast('CSV cleanup preview ready', 'success');
        } catch (e) {
            const message = String(e);
            csvCleanerError.set(message);
            csvCleanerPreview.set(null);
            notify({ level: 'error', title: 'CSV preview failed', message, toolId: 'csv-cleaner' });
            toast('CSV preview failed', 'error');
        } finally {
            csvCleanerPreviewing.set(false);
            stopBusy();
        }
    }

    async function previewCleanupAll() {
        const sourcePaths = $csvCleanerSources;
        if (!sourcePaths.length || previewLoading || $csvCleanerProcessing) return;

        csvCleanerError.set(null);
        csvCleanerPreviewing.set(true);
        csvCleanerPreviewAll.set([]);
        csvCleanerPreview.set(null);
        const stopBusy = reportBusy('csv-cleaner', 'Previewing CSV cleanup');

        try {
            const next: CsvCleanupPreviewResult[] = [];
            let firstPreview: CsvCleanupPreviewResult | null = null;
            for (const sourcePath of sourcePaths) {
                try {
                    const data = await invoke<CsvCleanupPreviewResult>('preview_csv_cleanup', {
                        options: { options: buildOptions(sourcePath) },
                    });
                    if (!firstPreview) {
                        firstPreview = data;
                        next.push(data);
                    } else {
                        next.push({
                            ...data,
                            preview_rows: [],
                        });
                    }
                } catch (e) {
                    const prev = $csvCleanerError;
                    csvCleanerError.set(`${prev ? `${prev}\n` : ''}${fileName(sourcePath)}: ${String(e)}`);
                }
            }

            csvCleanerPreviewAll.set(next);
            csvCleanerPreview.set(firstPreview ?? null);

            if (next.length > 0) {
                notify({
                    level: 'success',
                    title: 'CSV cleanup preview complete',
                    message: `${next.length} file${next.length === 1 ? '' : 's'} previewed`,
                    toolId: 'csv-cleaner',
                });
                toast('CSV cleanup preview complete', 'success');
            }
        } finally {
            csvCleanerPreviewing.set(false);
            stopBusy();
        }
    }

    async function cleanSelectedSource() {
        const source = $csvCleanerSelectedSource;
        if (!source || previewLoading || $csvCleanerProcessing) return;

        const usedOutputs = new Set<string>();
        const operationId = startCsvCleanerRun(source);

        csvCleanerResults.set([]);
        csvCleanerError.set(null);
        await setBusy(true);
        const stopBusy = reportBusy('csv-cleaner', 'Cleaning CSV');

        try {
            const out = makeOutputPath(source, usedOutputs);
            const result = await invoke<CleanCsvResult>('clean_csv_file', {
                options: {
                    options: buildOptions(source),
                    output_path: out,
                    operation_id: operationId,
                },
            });

            const entry: CleanCsvBatchResult = {
                ...result,
                source_path: source,
                success: true,
                error: null,
            };
            csvCleanerResults.set([entry]);

            const currentPreview = $csvCleanerPreview;
            if (currentPreview?.source_path === source) {
                csvCleanerPreview.set({
                    ...currentPreview,
                    source_path: source,
                });
            }

            notify({
                level: 'success',
                title: 'Cleaned CSV saved',
                message: `${entry.cleaned_rows} rows / ${entry.cleaned_columns} columns written`,
                toolId: 'csv-cleaner',
            });
            toast('CSV cleaned', 'success');
            void recordActivity({
                toolId: 'csv-cleaner',
                summary: `Cleaned 1 CSV file`,
                details: `${entry.cleaned_rows} rows · ${entry.cleaned_columns} columns · Output: ${out}`,
                outcome: 'success',
            });
        } catch (e) {
            const message = String(e);
            csvCleanerError.set(message);
            const isCancel = message.includes('Cancelled');
            const title = isCancel ? 'CSV cleaning cancelled' : 'Saving cleaned CSV failed';
            const notifyLevel = isCancel ? 'warning' : 'error';
            const toastLevel = isCancel ? 'info' : 'error';
            csvCleanerResults.set([makeFailedResult(source, message)]);
            notify({ level: notifyLevel, title, message, toolId: 'csv-cleaner' });
            toast(isCancel ? 'CSV cleaning cancelled' : 'Saving cleaned CSV failed', toastLevel);
            void recordActivity({
                toolId: 'csv-cleaner',
                summary: isCancel ? `CSV cleaning cancelled` : `CSV cleaning failed`,
                details: message.slice(0, 200),
                outcome: isCancel ? 'cancelled' : 'failed',
            });
        } finally {
            setCsvCleanerStopped();
            await setBusy(false);
            stopBusy();
        }
    }

    async function cleanAllSources() {
        const sourcePaths = $csvCleanerSources;
        if (!sourcePaths.length || previewLoading || $csvCleanerProcessing) return;

        const usedOutputs = new Set<string>();
        const operationId = startCsvCleanerRun(sourcePaths[0], sourcePaths.length);

        csvCleanerResults.set([]);
        csvCleanerError.set(null);
        await setBusy(true);
        const stopBusy = reportBusy('csv-cleaner', `Cleaning ${sourcePaths.length} CSV file${sourcePaths.length === 1 ? '' : 's'}`);

        try {
            const next: CleanCsvBatchResult[] = [];
            let cancelled = false;
            for (const [index, sourcePath] of sourcePaths.entries()) {
                if (get(csvCleanerCancelling)) {
                    cancelled = true;
                    break;
                }

                try {
                    const out = makeOutputPath(sourcePath, usedOutputs);
                    const result = await invoke<CleanCsvResult>('clean_csv_file', {
                        options: {
                            options: buildOptions(sourcePath),
                            output_path: out,
                            operation_id: operationId,
                            item_index: index,
                            items_total: sourcePaths.length,
                        },
                    });
                    next.push({
                        ...result,
                        source_path: sourcePath,
                        success: true,
                        error: null,
                    });
                } catch (e) {
                    const message = String(e);
                    next.push(makeFailedResult(sourcePath, message));
                    if (message.includes('Cancelled')) {
                        cancelled = true;
                        break;
                    }
                }
            }

            if (get(csvCleanerCancelling)) {
                cancelled = true;
            }

            csvCleanerResults.set(next);
            const ok = next.filter((item) => item.success).length;
            const failed = next.length - ok;

            if (cancelled) {
                notify({
                    level: 'warning',
                    title: 'CSV cleaning cancelled',
                    message: `${ok} file${ok === 1 ? '' : 's'} completed before cancellation`,
                    toolId: 'csv-cleaner',
                });
                toast(`Cleaning cancelled after ${ok} file${ok === 1 ? '' : 's'}`, 'info');
                void recordActivity({
                    toolId: 'csv-cleaner',
                    summary: `CSV cleaning cancelled`,
                    details: `${ok} cleaned before cancel · ${sourcePaths.length} requested`,
                    outcome: 'cancelled',
                });
            } else if (failed === 0) {
                notify({
                    level: 'success',
                    title: 'CSV cleaning finished',
                    message: `Saved ${ok} file${ok === 1 ? '' : 's'}`,
                    toolId: 'csv-cleaner',
                });
                toast(`Saved ${ok} file${ok === 1 ? '' : 's'}`, 'success');
                void recordActivity({
                    toolId: 'csv-cleaner',
                    summary: `Cleaned ${ok} CSV file${ok === 1 ? '' : 's'}`,
                    outcome: 'success',
                });
            } else {
                notify({
                    level: 'warning',
                    title: 'CSV cleaning partially failed',
                    message: `${ok} done, ${failed} failed`,
                    toolId: 'csv-cleaner',
                });
                // UxAudit UX-F-01: was `toast(\`${ok} done, ${failed}
                // failed\`, 'error')` — no failure list, no path to
                // it. The Activity tab keeps the per-file details, so
                // point users there instead of dropping the count.
                errorToast(
                    `${ok} file${ok === 1 ? '' : 's'} cleaned, ${failed} failed`,
                    null,
                    {
                        hint: 'Open the Activity tab in the top bar to see which files failed and why.',
                        durationMs: 6500,
                    },
                );
                void recordActivity({
                    toolId: 'csv-cleaner',
                    summary: `CSV cleaning finished with errors`,
                    details: `${ok} ok · ${failed} failed`,
                    outcome: 'success',
                });
            }
        } finally {
            setCsvCleanerStopped();
            await setBusy(false);
            stopBusy();
        }
    }

    async function cancelCsvClean() {
        try {
            await cancelCsvCleanerRun();
        } catch (e) {
            csvCleanerError.set(String(e));
        }
    }

    onMount(() => {
        initCsvCleanerListeners();
    });
</script>

<div class="csvk-panel">
    <div class="csvk-head">
        <div class="csvk-head-text">
            <h2 class="csvk-title">Clean — tame messy exports before anyone else sees them</h2>
            <p class="csvk-desc">
                Trim cells, drop fully-empty rows and columns, normalize headers to safe machine names — all with a live
                preview so you know exactly what's about to be written. One file or a whole batch.
            </p>
        </div>
    </div>

    <!-- Toolbar -->
    <div class="csvk-toolbar">
        <Button variant="secondary" icon={Plus} onclick={pickCsvFiles} disabled={previewLoading || $csvCleanerProcessing}>Add files</Button>
        <Button variant="secondary" icon={FolderOpen} onclick={pickCsvFolders} disabled={previewLoading || $csvCleanerProcessing}>Add folders</Button>
        <Button
            variant="secondary"
            icon={FolderOpen}
            onclick={chooseOutputDirectory}
            disabled={previewLoading || $csvCleanerProcessing}
            title={$csvCleanerOutputDir || 'Same folder as source'}
        >
            {$csvCleanerOutputDir ? `Output: ${fileName($csvCleanerOutputDir)}` : 'Output: alongside source'}
        </Button>
        {#if $csvCleanerSources.length > 0 && !$csvCleanerProcessing && !previewLoading}
            <Button variant="ghost" icon={Trash2} onclick={clearSources}>Clear ({$csvCleanerSources.length})</Button>
        {/if}
        <div class="csvk-toolbar-end">
            <Button
                variant="secondary"
                icon={Eye}
                onclick={previewCleanupSingle}
                loading={previewLoading}
                disabled={!$csvCleanerSelectedSource || previewLoading || $csvCleanerProcessing}
            >
                {previewLoading ? 'Previewing…' : 'Preview'}
            </Button>
            <Button
                variant="secondary"
                icon={Wand2}
                onclick={previewCleanupAll}
                disabled={$csvCleanerSources.length === 0 || previewLoading || $csvCleanerProcessing}
            >Preview all</Button>
            {#if $csvCleanerProcessing}
                <Button variant="danger" icon={Ban} onclick={cancelCsvClean} loading={$csvCleanerCancelling} disabled={$csvCleanerCancelling}>
                    {$csvCleanerCancelling ? 'Cancelling' : 'Cancel'}
                </Button>
            {/if}
            <Button
                variant="primary"
                icon={Play}
                onclick={$csvCleanerSources.length > 1 ? cleanAllSources : cleanSelectedSource}
                loading={$csvCleanerProcessing}
                disabled={$csvCleanerSources.length === 0 || previewLoading || $csvCleanerProcessing}
            >
                {$csvCleanerProcessing ? 'Cleaning…' : `Clean ${$csvCleanerSources.length > 1 ? 'all' : 'selected'}`}
            </Button>
        </div>
    </div>

    <!-- Progress -->
    {#if $csvCleanerProcessing && $csvCleanerProgress}
        <ToolPanel padding="md">
            <div class="csvk-prog">
                <div class="csvk-prog-head">
                    <div class="csvk-prog-main">
                        <span>{$csvCleanerProgress.cancelled ? 'Cancelling…' : 'Cleaning CSV files…'}</span>
                        <span class="csvk-prog-item" title={$csvCleanerProgress.current_item}>{fileName($csvCleanerProgress.current_item || $csvCleanerSelectedSource || '')}</span>
                    </div>
                    <span class="csvk-prog-meta">{$csvCleanerProgress.rows_done} rows</span>
                </div>
                <div class="csvk-bar" role="progressbar" aria-label="Clean progress">
                    <div class="csvk-bar-fill" style={`width: ${cleanProgressPercent}%`}></div>
                </div>
            </div>
        </ToolPanel>
    {/if}

    <div class="csvk-grid">
        <div class="csvk-col">
            <!-- Sources -->
            <ToolPanel padding="md">
                <div class="csvk-head" style="margin-bottom: 8px;">
                    <span class="csvk-section-label">Sources ({$csvCleanerSources.length})</span>
                    {#if $csvCleanerSources.length > 0}
                        <button
                            type="button"
                            class="csvk-link-btn"
                            onclick={() => removeSourcePath($csvCleanerSelectedSource ?? '')}
                            disabled={!$csvCleanerSelectedSource || previewLoading || $csvCleanerProcessing}
                        >Remove selected</button>
                    {/if}
                </div>
                <DropZone onFiles={addCsvSources} accept={[]}>
                    {#snippet children()}
                        {#if $csvCleanerSources.length === 0}
                            <EmptyState
                                icon={FileSpreadsheet}
                                title="Drop CSV/TSV/TXT files here"
                                description="Or use Add files / Add folders above. The first file becomes the preview target — switch by clicking another row."
                                variant="dashed"
                            />
                        {:else}
                            <div class="csvk-list">
                                {#each $csvCleanerSources as source (source)}
                                    <button
                                        type="button"
                                        class="csvk-row"
                                        class:is-selected={$csvCleanerSelectedSource === source}
                                        onclick={() => { csvCleanerSelectedSource.set(source); }}
                                    >
                                        <div class="csvk-row-main">
                                            <span class="csvk-row-name" title={source}>{fileName(source)}</span>
                                            <span class="csvk-row-path" title={source}>{source}</span>
                                        </div>
                                    </button>
                                {/each}
                            </div>
                        {/if}
                    {/snippet}
                </DropZone>
            </ToolPanel>

            <!-- Stat cards -->
            <div class="csvk-stats cols-4">
                <div class="csvk-stat">
                    <div class="csvk-stat-label">Rows</div>
                    <div class="csvk-stat-val">{$csvCleanerPreview?.cleaned_rows ?? 0}</div>
                </div>
                <div class="csvk-stat">
                    <div class="csvk-stat-label">Columns</div>
                    <div class="csvk-stat-val">{$csvCleanerPreview?.cleaned_columns ?? 0}</div>
                </div>
                <div class="csvk-stat">
                    <div class="csvk-stat-label">Empty rows removed</div>
                    <div class="csvk-stat-val is-accent">{$csvCleanerPreview?.removed_empty_rows ?? 0}</div>
                </div>
                <div class="csvk-stat">
                    <div class="csvk-stat-label">Empty cols removed</div>
                    <div class="csvk-stat-val is-accent">{$csvCleanerPreview?.removed_empty_columns ?? 0}</div>
                </div>
            </div>

            <!-- Preview -->
            <ToolPanel padding="md">
                {#if $csvCleanerPreview}
                    <div class="csvk-head" style="margin-bottom: 8px;">
                        <div class="csvk-head-text">
                            <div class="csvk-section-label">Preview</div>
                            <div class="csvk-hint" title={$csvCleanerPreview.source_path}>
                                Detected input delimiter: {$csvCleanerPreview.detected_delimiter} · {fileName($csvCleanerPreview.source_path)}
                            </div>
                        </div>
                        {#if $csvCleanerPreview.header_changed}
                            <span class="csvk-pill">Headers normalized</span>
                        {/if}
                    </div>
                    <div class="csvk-table-wrap">
                        <table class="csvk-table">
                            <tbody>
                                {#each $csvCleanerPreview.preview_rows as row, rowIndex}
                                    <tr>
                                        {#each row as cell}
                                            <td class:is-header={rowIndex === 0 && $csvCleanerHasHeader} title={cell}>{cell || ' '}</td>
                                        {/each}
                                    </tr>
                                {/each}
                            </tbody>
                        </table>
                    </div>
                {:else}
                    <EmptyState
                        icon={FileSpreadsheet}
                        title="No preview yet"
                        description="Select one or more CSV exports and run a preview to inspect cleaned output before saving."
                        variant="dashed"
                    />
                {/if}
            </ToolPanel>

            <!-- Preview-all summary -->
            {#if $csvCleanerPreviewAll.length > 0}
                <ToolPanel padding="md">
                    <div class="csvk-section-label" style="margin-bottom: 8px;">Preview summary</div>
                    <div class="csvk-list">
                        {#each $csvCleanerPreviewAll as item}
                            <div class="csvk-row">
                                <div class="csvk-row-main">
                                    <span class="csvk-row-name" title={item.source_path}>{fileName(item.source_path)}</span>
                                    <span class="csvk-row-path">{item.cleaned_rows}/{item.original_rows} rows · {item.cleaned_columns}/{item.original_columns} columns</span>
                                </div>
                            </div>
                        {/each}
                    </div>
                </ToolPanel>
            {/if}

            <!-- Results -->
            {#if $csvCleanerResults.length > 0}
                <ToolPanel padding="md">
                    <div class="csvk-head" style="margin-bottom: 8px;">
                        <span class="csvk-section-label">Cleaned CSV results</span>
                        <span class="csvk-hint">
                            <span style="color: var(--color-success);">{successCount} done</span>
                            {#if failCount > 0} · <span style="color: var(--color-error);">{failCount} failed</span>{/if}
                            {#if warningCount > 0} · {warningCount} warnings{/if}
                        </span>
                    </div>
                    <div class="csvk-results">
                        {#each $csvCleanerResults as result}
                            <div class="csvk-result" class:is-fail={!result.success}>
                                <div class="csvk-result-row">
                                    <span class="csvk-result-name" title={result.source_path}>{fileName(result.source_path)}</span>
                                    <span class="csvk-badge {result.success ? 'ok' : 'fail'}">{result.success ? 'OK' : 'Fail'}</span>
                                </div>
                                {#if result.success}
                                    <div class="csvk-result-path" title={result.output_path}>{result.output_path}</div>
                                    <div class="csvk-result-detail">
                                        {result.cleaned_rows} rows · {result.cleaned_columns} columns · removed {result.removed_empty_rows} empty rows and {result.removed_empty_columns} empty columns
                                    </div>
                                {:else}
                                    <div class="csvk-result-error">{result.error}</div>
                                {/if}
                            </div>
                        {/each}
                    </div>
                </ToolPanel>
            {/if}

            {#if $csvCleanerError}
                <div class="csvk-error">{$csvCleanerError}</div>
            {/if}
        </div>

        <!-- Cleanup rules -->
        <ToolPanel padding="md">
            <div class="csvk-section-label" style="margin-bottom: 10px;">Cleanup rules</div>
            <div class="csvk-fields">
                <label class="csvk-field">
                    <span class="csvk-label">Input delimiter</span>
                    <select class="csvk-select" bind:value={$csvCleanerInputDelimiter} aria-label="Input delimiter">
                        <option value="auto">Auto detect</option>
                        <option value="comma">Comma</option>
                        <option value="tab">Tab</option>
                        <option value="semicolon">Semicolon</option>
                        <option value="pipe">Pipe</option>
                    </select>
                </label>
                <label class="csvk-field">
                    <span class="csvk-label">Output delimiter</span>
                    <select class="csvk-select" bind:value={$csvCleanerOutputDelimiter} aria-label="Output delimiter">
                        <option value="comma">Comma</option>
                        <option value="tab">Tab</option>
                        <option value="semicolon">Semicolon</option>
                        <option value="pipe">Pipe</option>
                    </select>
                </label>
                <div class="csvk-checks" style="margin-top: 4px; border-top: 1px solid var(--color-border); padding-top: 12px;">
                    <Checkbox bind:checked={$csvCleanerHasHeader} label="First row is header" />
                    <Checkbox bind:checked={$csvCleanerTrimCells} label="Trim spaces in cells" />
                    <Checkbox bind:checked={$csvCleanerRemoveEmptyRows} label="Remove fully empty rows" />
                    <Checkbox bind:checked={$csvCleanerRemoveEmptyColumns} label="Remove fully empty columns" />
                    <Checkbox bind:checked={$csvCleanerNormalizeHeaders} disabled={!$csvCleanerHasHeader} label="Normalize headers to machine names" />
                </div>
            </div>
        </ToolPanel>
    </div>
</div>
