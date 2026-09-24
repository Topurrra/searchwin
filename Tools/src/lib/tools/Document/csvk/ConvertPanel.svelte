<script lang="ts">
    /*
      ConvertPanel — the "Convert" mode of CSV Toolkit. This folds in the
      former standalone "Excel ↔ CSV" tool (2026-06-05): round-trip
      spreadsheets in both directions. The old inner tab pair is now an
      elegant segmented direction toggle. Logic preserved verbatim from
      ExcelCsv.svelte (the `spreadsheet` store).
    */
    import { open, save } from '@tauri-apps/plugin-dialog';
    import { onMount } from 'svelte';
    import DropZone from '$lib/DropZone.svelte';
    import { FileSpreadsheet, Plus, FolderOpen, Play, Trash2, FileOutput, Ban } from '@lucide/svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import { Button, ToolPanel, Checkbox } from '$lib/ui';
    import {
        addCsvFiles,
        addXlsxFiles,
        cancelCsvToExcel,
        cancelExcelToCsv,
        clearCsvState,
        clearExcelToCsvState,
        csvAutoFilter,
        csvAutoWidth,
        csvBoldHeader,
        csvCancelling,
        csvDelimiter,
        csvDetectTypes,
        csvError,
        csvFiles,
        csvFreezeHeader,
        csvHasHeader,
        csvOutputPath,
        csvProcessing,
        csvProgress,
        csvResult,
        initSpreadsheetListeners,
        loadSpreadsheet,
        removeCsvFile,
        removeXlsxFile,
        runCsvToExcel,
        runExcelToCsv,
        spreadsheetTab,
        xlsxDelimiter,
        xlsxError,
        xlsxIncludeSheetCol,
        xlsxInfo,
        xlsxOutputDir,
        xlsxPath,
        xlsxPaths,
        xlsxProcessing,
        xlsxProgress,
        xlsxQuoteAll,
        xlsxResult,
        xlsxSpecificSheet,
        xlsxStrategy,
        xlsxCancelling,
    } from '$lib/stores/spreadsheet';

    onMount(() => { initSpreadsheetListeners(); });

    async function pickXlsx() {
        const sel = await open({
            multiple: true,
            directory: false,
            filters: [{ name: 'Spreadsheets', extensions: ['xlsx', 'xls', 'xlsb', 'xlsm', 'ods'] }],
        });
        if (Array.isArray(sel)) {
            await addXlsxFiles(sel);
        } else if (typeof sel === 'string') {
            await addXlsxFiles([sel]);
        }
    }

    async function pickXlsxFolder() {
        const sel = await open({ multiple: true, directory: true });
        if (!sel) return;
        if (Array.isArray(sel)) await addXlsxFiles(sel);
        else await addXlsxFiles([sel]);
    }

    async function handleXlsxDrop(paths: string[]) {
        if (paths.length > 0) await addXlsxFiles(paths);
    }

    async function selectXlsxPath(path: string) {
        if (path === $xlsxPath) return;
        xlsxPath.set(path);
        await loadSpreadsheet(path);
    }

    async function pickXlsxOutputDir() {
        const sel = await open({ multiple: false, directory: true });
        if (typeof sel === 'string') xlsxOutputDir.set(sel);
    }

    async function pickCsvFiles() {
        const sel = await open({
            multiple: true,
            directory: false,
            filters: [{ name: 'CSV/TSV', extensions: ['csv', 'tsv', 'txt'] }],
        });
        if (Array.isArray(sel)) addCsvFiles(sel);
        else if (typeof sel === 'string') addCsvFiles([sel]);
    }

    async function pickCsvOutput() {
        const baseName = $csvFiles.length > 0 ? ($csvFiles[0].split(/[\\/]/).pop()?.replace(/\.[^.]+$/, '') || 'workbook') : 'workbook';
        const out = await save({
            defaultPath: `${baseName}.xlsx`,
            filters: [{ name: 'Excel', extensions: ['xlsx'] }],
        });
        if (out) csvOutputPath.set(out);
    }

    function fileName(path: string) {
        return path.split(/[\\/]/).pop() || path;
    }

    const xlsxProgressPercent = $derived(
        $xlsxProgress && $xlsxProgress.items_total > 0
            ? Math.round(($xlsxProgress.items_done / $xlsxProgress.items_total) * 100)
            : 0,
    );
    const csvProgressPercent = $derived(
        $csvProgress && $csvProgress.items_total > 0
            ? Math.round(($csvProgress.items_done / $csvProgress.items_total) * 100)
            : 0,
    );
</script>

<div class="csvk-panel">
    <div class="csvk-head">
        <div class="csvk-head-text">
            <h2 class="csvk-title">Convert — round-trip spreadsheets in both directions</h2>
            <p class="csvk-desc">
                Drop in XLSX/XLS/XLSB/ODS to export per-sheet CSVs, or stack multiple CSVs into a single workbook with
                formatting, auto-width, and frozen headers. Conversion runs locally — no service required.
            </p>
        </div>
        <div class="csvk-seg" role="group" aria-label="Conversion direction">
            <button
                type="button"
                class="csvk-seg-btn"
                class:is-active={$spreadsheetTab === 'excel-to-csv'}
                aria-pressed={$spreadsheetTab === 'excel-to-csv'}
                onclick={() => spreadsheetTab.set('excel-to-csv')}
            >Excel → CSV</button>
            <button
                type="button"
                class="csvk-seg-btn"
                class:is-active={$spreadsheetTab === 'csv-to-excel'}
                aria-pressed={$spreadsheetTab === 'csv-to-excel'}
                onclick={() => spreadsheetTab.set('csv-to-excel')}
            >CSV → Excel</button>
        </div>
    </div>

    {#if $spreadsheetTab === 'excel-to-csv'}
        <!-- ═══ Excel → CSV ═══ -->
        <div class="csvk-toolbar">
            <Button variant="secondary" icon={Plus} onclick={pickXlsx} disabled={$xlsxProcessing}>Add spreadsheets</Button>
            <Button variant="secondary" icon={FolderOpen} onclick={pickXlsxFolder} disabled={$xlsxProcessing}>Add folder</Button>
            <Button
                variant="secondary"
                icon={FolderOpen}
                onclick={pickXlsxOutputDir}
                disabled={$xlsxProcessing}
                title={$xlsxOutputDir || 'Same folder as input'}
            >
                {$xlsxOutputDir ? `Output: ${fileName($xlsxOutputDir)}` : 'Output: same folder'}
            </Button>
            {#if $xlsxPaths.length > 0 && !$xlsxProcessing}
                <Button variant="ghost" icon={Trash2} onclick={clearExcelToCsvState}>Clear ({$xlsxPaths.length})</Button>
            {/if}
            <div class="csvk-toolbar-end">
                {#if $xlsxProcessing}
                    <Button variant="danger" icon={Ban} onclick={cancelExcelToCsv} loading={$xlsxCancelling} disabled={$xlsxCancelling}>
                        {$xlsxCancelling ? 'Cancelling' : 'Cancel'}
                    </Button>
                {/if}
                <Button
                    variant="primary"
                    icon={Play}
                    onclick={runExcelToCsv}
                    loading={$xlsxProcessing}
                    disabled={$xlsxProcessing || $xlsxPaths.length === 0}
                >
                    {$xlsxProcessing ? 'Exporting…' : 'Export CSV'}
                </Button>
            </div>
        </div>

        {#if $xlsxPaths.length === 0}
            <ToolPanel padding="md">
                <DropZone onFiles={handleXlsxDrop} accept={['xlsx', 'xls', 'xlsb', 'xlsm', 'ods']}>
                    {#snippet children()}
                        <EmptyState
                            icon={FileSpreadsheet}
                            title="Drop spreadsheets here"
                            description="XLSX · XLS · XLSB · XLSM · ODS — each can be exported as one or many CSVs."
                            variant="dashed"
                        />
                    {/snippet}
                </DropZone>
            </ToolPanel>
        {:else}
            <div class="csvk-grid">
                <div class="csvk-col">
                    <!-- Source list -->
                    <ToolPanel padding="md">
                        <div class="csvk-section-label" style="margin-bottom: 8px;">Source files ({$xlsxPaths.length})</div>
                        <div class="csvk-list">
                            {#each $xlsxPaths as sourcePath (sourcePath)}
                                <div class="csvk-row" class:is-selected={$xlsxPath === sourcePath}>
                                    <button type="button" class="csvk-row-main" style="background: transparent; border: none; cursor: pointer; text-align: left;" onclick={() => selectXlsxPath(sourcePath)}>
                                        <span class="csvk-row-name" title={sourcePath}>{fileName(sourcePath)}</span>
                                        <span class="csvk-row-path" title={sourcePath}>{sourcePath}</span>
                                    </button>
                                    {#if !$xlsxProcessing}
                                        <button type="button" class="csvk-link-btn" onclick={() => removeXlsxFile(sourcePath)} aria-label="Remove {fileName(sourcePath)}">Remove</button>
                                    {/if}
                                </div>
                            {/each}
                        </div>
                    </ToolPanel>

                    <!-- Preview -->
                    {#if $xlsxPath && $xlsxInfo}
                        <ToolPanel padding="md">
                            <div class="csvk-head" style="margin-bottom: 8px;">
                                <div class="csvk-head-text">
                                    <div class="csvk-row-name" style="font-size: 13px; font-weight: 500;" title={$xlsxPath}>{fileName($xlsxPath)}</div>
                                    <div class="csvk-row-path" title={$xlsxPath}>{$xlsxPath}</div>
                                </div>
                            </div>
                            <div class="csvk-stats cols-3" style="margin-bottom: 10px;">
                                <div class="csvk-stat">
                                    <div class="csvk-stat-label">Format</div>
                                    <div class="csvk-stat-val" style="font-size: 13px; text-transform: uppercase;">{$xlsxInfo.format}</div>
                                </div>
                                <div class="csvk-stat">
                                    <div class="csvk-stat-label">Sheets</div>
                                    <div class="csvk-stat-val">{$xlsxInfo.sheets.length}</div>
                                </div>
                                <div class="csvk-stat">
                                    <div class="csvk-stat-label">Preview</div>
                                    <div class="csvk-stat-val">{$xlsxInfo.preview_rows.length}</div>
                                </div>
                            </div>
                            <div class="csvk-table-wrap" style="max-height: 13rem;">
                                <table class="csvk-table">
                                    <tbody>
                                        {#each $xlsxInfo.preview_rows.slice(0, 12) as row}
                                            <tr>
                                                {#each row.slice(0, 8) as cell}
                                                    <td title={cell}>{cell}</td>
                                                {/each}
                                            </tr>
                                        {/each}
                                    </tbody>
                                </table>
                            </div>
                        </ToolPanel>
                    {/if}

                    <!-- Progress -->
                    {#if $xlsxProcessing || $xlsxProgress}
                        <ToolPanel padding="md">
                            <div class="csvk-prog">
                                <div class="csvk-prog-head">
                                    <div class="csvk-prog-main">
                                        <span>{$xlsxProcessing ? ($xlsxCancelling ? 'Cancelling…' : 'Exporting sheets…') : 'Last export'}</span>
                                        {#if $xlsxProgress?.current_item}<span class="csvk-prog-item" title={$xlsxProgress.current_item}>{$xlsxProgress.current_item}</span>{/if}
                                    </div>
                                    <span class="csvk-prog-meta">{$xlsxProgress?.items_done ?? 0}/{$xlsxProgress?.items_total ?? 0} · {$xlsxProgress?.rows_done ?? 0} rows</span>
                                </div>
                                <div class="csvk-bar" role="progressbar" aria-label="Export progress">
                                    <div class="csvk-bar-fill" style={`width: ${xlsxProgressPercent}%`}></div>
                                </div>
                            </div>
                        </ToolPanel>
                    {/if}

                    <!-- Results -->
                    {#if $xlsxResult.length > 0}
                        <ToolPanel padding="md">
                            <div class="csvk-section-label" style="margin-bottom: 8px;">Export complete · {$xlsxResult.length} file{$xlsxResult.length === 1 ? '' : 's'}</div>
                            <div class="csvk-results">
                                {#each $xlsxResult as result}
                                    <div class="csvk-result" class:is-fail={!result.success}>
                                        <div class="csvk-result-row">
                                            <span class="csvk-result-name" title={result.source_path}>{fileName(result.source_path)}</span>
                                            <span class="csvk-badge {result.success ? 'ok' : 'fail'}">{result.success ? 'OK' : 'Fail'}</span>
                                        </div>
                                        {#if result.success}
                                            <div class="csvk-result-detail">{result.total_sheets} sheet{result.total_sheets === 1 ? '' : 's'} · {result.total_rows} rows</div>
                                            {#each result.outputs as outPath}
                                                <div class="csvk-result-path" title={outPath}>{outPath}</div>
                                            {/each}
                                        {:else}
                                            <div class="csvk-result-error">{result.error}</div>
                                        {/if}
                                    </div>
                                {/each}
                            </div>
                        </ToolPanel>
                    {/if}

                    {#if $xlsxError}
                        <div class="csvk-error">{$xlsxError}</div>
                    {/if}
                </div>

                <!-- Options -->
                <ToolPanel padding="md">
                    <div class="csvk-section-label" style="margin-bottom: 10px;">Options</div>
                    <div class="csvk-fields">
                        <label class="csvk-field">
                            <span class="csvk-label">Export mode</span>
                            <select class="csvk-select" bind:value={$xlsxStrategy} disabled={$xlsxProcessing} aria-label="Export mode">
                                <option value="all_separate">All sheets separately</option>
                                <option value="first_only">First sheet only</option>
                                <option value="specific">Specific sheet</option>
                                <option value="merge">Merge sheets</option>
                            </select>
                        </label>
                        {#if $xlsxStrategy === 'specific' && $xlsxInfo}
                            <label class="csvk-field">
                                <span class="csvk-label">Sheet</span>
                                <select class="csvk-select" bind:value={$xlsxSpecificSheet} disabled={$xlsxProcessing} aria-label="Sheet">
                                    {#each $xlsxInfo.sheets as s}<option value={s.name}>{s.name}</option>{/each}
                                </select>
                            </label>
                        {/if}
                        <label class="csvk-field">
                            <span class="csvk-label">Delimiter</span>
                            <select class="csvk-select" bind:value={$xlsxDelimiter} disabled={$xlsxProcessing} aria-label="Delimiter">
                                <option value="comma">Comma</option>
                                <option value="tab">Tab</option>
                                <option value="semicolon">Semicolon</option>
                                <option value="pipe">Pipe</option>
                            </select>
                        </label>
                        <div class="csvk-checks" style="margin-top: 2px;">
                            <Checkbox bind:checked={$xlsxQuoteAll} disabled={$xlsxProcessing} label="Quote all fields" />
                            {#if $xlsxStrategy === 'merge'}
                                <Checkbox bind:checked={$xlsxIncludeSheetCol} disabled={$xlsxProcessing} label="Include sheet column" />
                            {/if}
                        </div>
                    </div>
                </ToolPanel>
            </div>
        {/if}
    {:else}
        <!-- ═══ CSV → Excel ═══ -->
        <div class="csvk-toolbar">
            <Button variant="secondary" icon={Plus} onclick={pickCsvFiles} disabled={$csvProcessing}>Add CSV/TSV</Button>
            <Button
                variant="secondary"
                icon={FileOutput}
                onclick={pickCsvOutput}
                disabled={$csvProcessing}
                title={$csvOutputPath || 'Choose output .xlsx'}
            >
                {$csvOutputPath ? `Output: ${fileName($csvOutputPath)}` : 'Choose output .xlsx'}
            </Button>
            {#if $csvFiles.length > 0 && !$csvProcessing}
                <Button variant="ghost" icon={Trash2} onclick={clearCsvState}>Clear ({$csvFiles.length})</Button>
            {/if}
            <div class="csvk-toolbar-end">
                {#if $csvProcessing}
                    <Button variant="danger" icon={Ban} onclick={cancelCsvToExcel} loading={$csvCancelling} disabled={$csvCancelling}>
                        {$csvCancelling ? 'Cancelling' : 'Cancel'}
                    </Button>
                {/if}
                <Button
                    variant="primary"
                    icon={Play}
                    onclick={runCsvToExcel}
                    loading={$csvProcessing}
                    disabled={$csvProcessing || $csvFiles.length === 0 || !$csvOutputPath}
                >
                    {$csvProcessing ? 'Creating…' : 'Create workbook'}
                </Button>
            </div>
        </div>

        <div class="csvk-grid">
            <div class="csvk-col">
                <ToolPanel padding="md">
                    <DropZone onFiles={addCsvFiles} accept={['csv', 'tsv', 'txt']}>
                        {#snippet children()}
                            {#if $csvFiles.length === 0}
                                <EmptyState
                                    icon={FileSpreadsheet}
                                    title="Drop CSV/TSV files here"
                                    description="Each file becomes one Excel sheet."
                                    variant="dashed"
                                />
                            {:else}
                                <div class="csvk-list">
                                    {#each $csvFiles as path (path)}
                                        <div class="csvk-row">
                                            <div class="csvk-row-main">
                                                <span class="csvk-row-name" title={path}>{fileName(path)}</span>
                                                <span class="csvk-row-path" title={path}>{path}</span>
                                            </div>
                                            {#if !$csvProcessing}
                                                <button type="button" class="csvk-link-btn" onclick={() => removeCsvFile(path)} aria-label="Remove {fileName(path)}">Remove</button>
                                            {/if}
                                        </div>
                                    {/each}
                                </div>
                            {/if}
                        {/snippet}
                    </DropZone>
                </ToolPanel>

                {#if $csvProcessing || $csvProgress}
                    <ToolPanel padding="md">
                        <div class="csvk-prog">
                            <div class="csvk-prog-head">
                                <div class="csvk-prog-main">
                                    <span>{$csvProcessing ? ($csvCancelling ? 'Cancelling…' : 'Creating workbook…') : 'Last workbook'}</span>
                                    {#if $csvProgress?.current_item}<span class="csvk-prog-item" title={$csvProgress.current_item}>{fileName($csvProgress.current_item)}</span>{/if}
                                </div>
                                <span class="csvk-prog-meta">{$csvProgress?.items_done ?? 0}/{$csvProgress?.items_total ?? 0} · {$csvProgress?.rows_done ?? 0} rows</span>
                            </div>
                            <div class="csvk-bar" role="progressbar" aria-label="Workbook progress">
                                <div class="csvk-bar-fill" style={`width: ${csvProgressPercent}%`}></div>
                            </div>
                        </div>
                    </ToolPanel>
                {/if}

                {#if $csvResult}
                    <ToolPanel padding="md">
                        <div class="csvk-section-label" style="margin-bottom: 6px; color: var(--color-success);">Excel created</div>
                        <div class="csvk-result-detail">{$csvResult.total_sheets} sheet{$csvResult.total_sheets === 1 ? '' : 's'} · {$csvResult.total_rows} rows</div>
                        <div class="csvk-result-path" title={$csvResult.output_path} style="margin-top: 4px;">{$csvResult.output_path}</div>
                    </ToolPanel>
                {/if}
                {#if $csvError}
                    <div class="csvk-error">{$csvError}</div>
                {/if}
            </div>

            <ToolPanel padding="md">
                <div class="csvk-section-label" style="margin-bottom: 10px;">Workbook options</div>
                <div class="csvk-fields">
                    <label class="csvk-field">
                        <span class="csvk-label">Delimiter</span>
                        <select class="csvk-select" bind:value={$csvDelimiter} disabled={$csvProcessing} aria-label="Delimiter">
                            <option value="auto">Auto</option>
                            <option value="comma">Comma</option>
                            <option value="tab">Tab</option>
                            <option value="semicolon">Semicolon</option>
                            <option value="pipe">Pipe</option>
                        </select>
                    </label>
                    <div class="csvk-checks" style="margin-top: 2px;">
                        <Checkbox bind:checked={$csvHasHeader} disabled={$csvProcessing} label="First row is header" />
                        <Checkbox bind:checked={$csvBoldHeader} disabled={$csvProcessing} label="Bold header" />
                        <Checkbox bind:checked={$csvAutoFilter} disabled={$csvProcessing} label="Auto filter" />
                        <Checkbox bind:checked={$csvAutoWidth} disabled={$csvProcessing} label="Auto width" />
                        <Checkbox bind:checked={$csvFreezeHeader} disabled={$csvProcessing} label="Freeze header" />
                        <Checkbox bind:checked={$csvDetectTypes} disabled={$csvProcessing} label="Detect numbers/booleans" />
                    </div>
                </div>
            </ToolPanel>
        </div>
    {/if}
</div>
