import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { reportBusy } from './globalBusy';
import { reportOperationOutcome } from './operationOutcome';

export type SpreadsheetTab = 'excel-to-csv' | 'csv-to-excel';
export type Delimiter = 'comma' | 'tab' | 'semicolon' | 'pipe';
export type CsvDelimiter = Delimiter | 'auto';
export type SheetStrategy = 'all_separate' | 'first_only' | 'specific' | 'merge';

export type SheetInfo = { name: string; rows: number; cols: number };
export type SpreadsheetInfo = { format: string; sheets: SheetInfo[]; preview_rows: string[][]; preview_sheet: string };
export type ExcelToCsvResult = { source_path: string; outputs: string[]; total_rows: number; total_sheets: number; success: boolean; error: string | null };
export type CsvToExcelResult = { output_path: string; total_sheets: number; total_rows: number };
export type SpreadsheetProgress = { operation_id: string; mode: string; current_item: string; items_done: number; items_total: number; rows_done: number; cancelled: boolean };

export const spreadsheetTab = writable<SpreadsheetTab>('excel-to-csv');

export const xlsxPaths = writable<string[]>([]);
export const xlsxPath = writable<string | null>(null);
export const xlsxInfo = writable<SpreadsheetInfo | null>(null);
export const xlsxOutputDir = writable<string | null>(null);
export const xlsxStrategy = writable<SheetStrategy>('all_separate');
export const xlsxSpecificSheet = writable('');
export const xlsxDelimiter = writable<Delimiter>('comma');
export const xlsxQuoteAll = writable(false);
export const xlsxIncludeSheetCol = writable(true);
export const xlsxProcessing = writable(false);
export const xlsxCancelling = writable(false);
export const xlsxProgress = writable<SpreadsheetProgress | null>(null);
export const xlsxResult = writable<ExcelToCsvResult[]>([]);
export const xlsxError = writable<string | null>(null);

export const csvFiles = writable<string[]>([]);
export const csvOutputPath = writable<string | null>(null);
export const csvDelimiter = writable<CsvDelimiter>('auto');
export const csvHasHeader = writable(true);
export const csvBoldHeader = writable(true);
export const csvAutoFilter = writable(true);
export const csvAutoWidth = writable(true);
export const csvFreezeHeader = writable(true);
export const csvDetectTypes = writable(true);
export const csvProcessing = writable(false);
export const csvCancelling = writable(false);
export const csvProgress = writable<SpreadsheetProgress | null>(null);
export const csvResult = writable<CsvToExcelResult | null>(null);
export const csvError = writable<string | null>(null);

let activeExcelOperationId: string | null = null;
let activeCsvOperationId: string | null = null;
let initialized = false;
let unlisten: UnlistenFn | null = null;

async function setBusy(busy: boolean) { try { await invoke('set_busy', { busy }); } catch {} }
function makeOperationId(prefix: string) { return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`; }
function normalizePath(path: string) { return path.toLowerCase(); }

export async function initSpreadsheetListeners() {
    if (initialized) return;
    initialized = true;
    unlisten = await listen<SpreadsheetProgress>('spreadsheet-progress', (event) => {
        const p = event.payload;
        if (activeExcelOperationId && p.operation_id === activeExcelOperationId) xlsxProgress.set(p);
        if (activeCsvOperationId && p.operation_id === activeCsvOperationId) csvProgress.set(p);
    });
}

export async function loadSpreadsheet(path: string) {
    xlsxPath.set(path);
    xlsxInfo.set(null);
    xlsxResult.set([]);
    xlsxError.set(null);
    try {
        const info = await invoke<SpreadsheetInfo>('inspect_spreadsheet', { options: { path } });
        xlsxInfo.set(info);
        if (info.sheets.length > 0) xlsxSpecificSheet.set(info.sheets[0].name);
    } catch (e) {
        xlsxError.set(String(e));
    }
}

async function collectExcelSources(raw: string[]) {
    if (!raw.length) return [];

    const response = await invoke<{
        paths: string[];
        warnings: string[];
    }>('collect_excel_sources', {
        options: {
            paths: raw,
            recursive: true,
        },
    });

    if (response.paths.length === 0 && response.warnings.length > 0) {
        xlsxError.set(response.warnings.join('\n'));
    }

    return response.paths ?? [];
}

export async function addXlsxFiles(paths: string[]) {
    const discovered = await collectExcelSources(paths);
    if (!discovered.length) return;

    const existing = get(xlsxPaths);
    const seen = new Set(existing.map(normalizePath));
    const next = discovered.filter((path) => !seen.has(normalizePath(path)));
    if (next.length === 0) return;

    xlsxPaths.update((items) => [...items, ...next]);
    if (!get(xlsxPath) && next.length > 0) {
        xlsxPath.set(next[0]);
        await loadSpreadsheet(next[0]);
    }
}

export function removeXlsxFile(path: string) {
    xlsxPaths.update((files) => files.filter((item) => item !== path));
    const remaining = get(xlsxPaths);
    if (get(xlsxPath) !== path) return;
    if (remaining.length === 0) {
        xlsxPath.set(null);
        xlsxInfo.set(null);
        return;
    }

    xlsxPath.set(remaining[0]);
    void loadSpreadsheet(remaining[0]);
}

export function clearExcelToCsvState() {
    if (get(xlsxProcessing)) return;
    xlsxPaths.set([]);
    xlsxPath.set(null);
    xlsxInfo.set(null);
    xlsxResult.set([]);
    xlsxError.set(null);
    xlsxProgress.set(null);
    xlsxSpecificSheet.set('');
}

export async function runExcelToCsv() {
    const sources = get(xlsxPaths);
    if (!sources.length || get(xlsxProcessing)) return;

    const operationId = makeOperationId('csv-toolkit');
    activeExcelOperationId = operationId;
    xlsxProcessing.set(true);
    xlsxCancelling.set(false);
    xlsxResult.set([]);
    xlsxError.set(null);
    xlsxProgress.set({ operation_id: operationId, mode: 'excel_to_csv', current_item: '', items_done: 0, items_total: sources.length, rows_done: 0, cancelled: false });
    await setBusy(true);
    const stopBusy = reportBusy('csv-toolkit', 'Converting spreadsheet');

    try {
        const results: ExcelToCsvResult[] = [];
        for (let index = 0; index < sources.length; index += 1) {
            const source = sources[index];
            if (get(xlsxCancelling)) break;
            xlsxProgress.set({ operation_id: operationId, mode: 'excel_to_csv', current_item: source, items_done: index, items_total: sources.length, rows_done: 0, cancelled: false });

            try {
                const result = await invoke<ExcelToCsvResult>('excel_to_csv', {
                    options: {
                        source_path: source,
                        output_dir: get(xlsxOutputDir),
                        sheet_strategy: get(xlsxStrategy),
                        specific_sheet: get(xlsxStrategy) === 'specific' ? get(xlsxSpecificSheet) : null,
                        delimiter: get(xlsxDelimiter),
                        quote_all: get(xlsxQuoteAll),
                        include_sheet_name_column: get(xlsxIncludeSheetCol),
                        operation_id: operationId,
                    },
                });
                results.push(result);
            } catch (e) {
                const msg = String(e);
                results.push({
                    source_path: source,
                    outputs: [],
                    total_rows: 0,
                    total_sheets: 0,
                    success: false,
                    error: msg,
                });
                if (msg.includes('Cancelled')) break;
            }
        }

        const successCount = results.filter((r) => r.success).length;
        const failureCount = results.length - successCount;
        xlsxResult.set(results);

        if (get(xlsxCancelling)) {
            reportOperationOutcome({
                toolId: 'csv-toolkit',
                kind: 'cancelled',
                notifyTitle: 'Excel export cancelled',
                notifyMessage: `${successCount} converted before cancellation`,
                toastMessage: 'Excel export cancelled',
                activitySummary: `Excel → CSV cancelled`,
                activityDetails: `${successCount} converted before cancel`,
            });
            return;
        }

        if (results.length === 0) {
            const msg = 'No spreadsheet file was processed';
            xlsxError.set(msg);
            reportOperationOutcome({
                toolId: 'csv-toolkit',
                kind: 'failed',
                notifyTitle: 'Excel export failed',
                notifyMessage: msg,
                toastMessage: msg,
                activitySummary: `Excel → CSV failed`,
                activityDetails: msg,
            });
            return;
        }

        if (failureCount === 0) {
            reportOperationOutcome({
                toolId: 'csv-toolkit',
                kind: 'success',
                notifyTitle: `Exported ${successCount} file${successCount === 1 ? '' : 's'}`,
                notifyMessage: `Completed ${successCount} file${successCount === 1 ? '' : 's'}`,
                toastMessage: `Exported ${successCount} file${successCount === 1 ? '' : 's'}`,
                activitySummary: `Exported ${successCount} spreadsheet${successCount === 1 ? '' : 's'} → CSV`,
            });
        } else if (successCount === 0) {
            const msg = `${failureCount} file${failureCount === 1 ? '' : 's'} failed`;
            xlsxError.set(msg);
            reportOperationOutcome({
                toolId: 'csv-toolkit',
                kind: 'failed',
                notifyTitle: 'Excel export failed',
                notifyMessage: msg,
                toastMessage: msg,
                activitySummary: `Excel → CSV failed`,
                activityDetails: msg,
            });
        } else {
            reportOperationOutcome({
                toolId: 'csv-toolkit',
                kind: 'partial',
                notifyTitle: 'Excel export finished with errors',
                notifyMessage: `${successCount} succeeded, ${failureCount} failed`,
                toastMessage: `${successCount} succeeded, ${failureCount} failed`,
                activitySummary: `Excel → CSV finished with errors`,
                activityDetails: `${successCount} ok · ${failureCount} failed`,
            });
        }
    } finally {
        xlsxProcessing.set(false);
        xlsxCancelling.set(false);
        activeExcelOperationId = null;
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelExcelToCsv() {
    if (!activeExcelOperationId || get(xlsxCancelling)) return;
    xlsxCancelling.set(true);
    try { await invoke('cancel_spreadsheet_operation', { operationId: activeExcelOperationId }); } catch (e) { xlsxError.set(String(e)); }
}

export function addCsvFiles(paths: string[]) {
    const current = get(csvFiles);
    const seen = new Set(current);
    csvFiles.set([...current, ...paths.filter((p) => !seen.has(p))]);
}

export function removeCsvFile(path: string) { csvFiles.update((files) => files.filter((f) => f !== path)); }
export function clearCsvState() { if (!get(csvProcessing)) { csvFiles.set([]); csvResult.set(null); csvError.set(null); csvProgress.set(null); } }

export async function runCsvToExcel() {
    const sources = get(csvFiles);
    const outputPath = get(csvOutputPath);
    if (!sources.length || !outputPath || get(csvProcessing)) return;

    const operationId = makeOperationId('csv-excel');
    activeCsvOperationId = operationId;
    csvProcessing.set(true);
    csvCancelling.set(false);
    csvResult.set(null);
    csvError.set(null);
    csvProgress.set({ operation_id: operationId, mode: 'csv_to_excel', current_item: '', items_done: 0, items_total: sources.length, rows_done: 0, cancelled: false });
    await setBusy(true);
    const stopBusy = reportBusy('csv-toolkit', 'Converting spreadsheet');

    try {
        const result = await invoke<CsvToExcelResult>('csv_to_excel', {
            options: {
                sources,
                output_path: outputPath,
                delimiter: get(csvDelimiter),
                has_header: get(csvHasHeader),
                bold_header: get(csvBoldHeader),
                auto_filter: get(csvAutoFilter),
                auto_width: get(csvAutoWidth),
                freeze_header: get(csvFreezeHeader),
                detect_types: get(csvDetectTypes),
                operation_id: operationId,
            },
        });
        csvResult.set(result);
        reportOperationOutcome({
            toolId: 'csv-toolkit',
            kind: 'success',
            notifyTitle: 'Excel created',
            notifyMessage: `${result.total_sheets} sheet${result.total_sheets === 1 ? '' : 's'}, ${result.total_rows} rows`,
            toastMessage: `Excel created (${result.total_sheets} sheets)`,
            activitySummary: `CSV → Excel: ${result.total_sheets} sheet${result.total_sheets === 1 ? '' : 's'}, ${result.total_rows} rows`,
            activityDetails: `Output: ${result.output_path}`,
        });
    } catch (e) {
        const msg = String(e);
        csvError.set(msg);
        const isCancel = msg.includes('Cancelled');
        reportOperationOutcome({
            toolId: 'csv-toolkit',
            kind: isCancel ? 'cancelled' : 'failed',
            notifyTitle: isCancel ? 'CSV export cancelled' : 'CSV export failed',
            notifyMessage: msg,
            toastMessage: isCancel ? 'Export cancelled' : 'Export failed',
            activitySummary: isCancel ? `CSV → Excel cancelled` : `CSV → Excel failed`,
            activityDetails: msg.slice(0, 200),
        });
    } finally {
        csvProcessing.set(false);
        csvCancelling.set(false);
        activeCsvOperationId = null;
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelCsvToExcel() {
    if (!activeCsvOperationId || get(csvCancelling)) return;
    csvCancelling.set(true);
    try { await invoke('cancel_spreadsheet_operation', { operationId: activeCsvOperationId }); } catch (e) { csvError.set(String(e)); }
}
