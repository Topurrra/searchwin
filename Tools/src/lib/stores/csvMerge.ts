import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { reportBusy } from './globalBusy';
import { reportOperationOutcome } from './operationOutcome';
import { writable, get } from 'svelte/store';

type Delimiter = 'auto' | 'comma' | 'tab' | 'semicolon' | 'pipe';

export type SpreadsheetProgress = {
    operation_id: string;
    mode: string;
    current_item: string;
    items_done: number;
    items_total: number;
    rows_done: number;
    cancelled: boolean;
};

export type CsvMergeFileResult = {
    source_path: string;
    output_path: string;
    rows_read: number;
    rows_written: number;
    success: boolean;
    error: string | null;
};

export type CsvSourcesResult = {
    paths: string[];
    warnings: string[];
};

export const csvMergeSources = writable<string[]>([]);
export const csvMergeOutputPath = writable<string | null>(null);
export const csvMergeInputDelimiter = writable<Delimiter>('auto');
export const csvMergeOutputDelimiter = writable<'comma' | 'tab' | 'semicolon' | 'pipe'>('comma');
export const csvMergeIncludeSource = writable(true);
export const csvMergeIncludeHeaders = writable(true);
export const csvMergeProcessing = writable(false);
export const csvMergeCancelling = writable(false);
export const csvMergeProgress = writable<SpreadsheetProgress | null>(null);
export const csvMergeResults = writable<CsvMergeFileResult[]>([]);
export const csvMergeError = writable<string | null>(null);

let activeCsvMergeOperationId: string | null = null;
let unlisten: UnlistenFn | null = null;

function makeOperationId(prefix: string) {
    return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function normalizePath(path: string) {
    return path.toLowerCase();
}

async function setBusy(busy: boolean) {
    try {
        await invoke('set_busy', { busy });
    } catch {}
}

async function collectCsvSources(raw: string[]) {
    if (!raw.length) return [] as string[];

    const response = await invoke<CsvSourcesResult>('collect_csv_sources', {
        options: {
            paths: raw,
            recursive: true,
        },
    });

    if (response.paths.length === 0 && response.warnings.length > 0) {
        csvMergeError.set(response.warnings.join('\n'));
    }

    return response.paths ?? [];
}

export async function initCsvMergeListeners() {
    if (unlisten) return;
    unlisten = await listen<SpreadsheetProgress>('spreadsheet-progress', (event) => {
        const p = event.payload;
        if (p.mode !== 'csv_merge') return;
        if (!activeCsvMergeOperationId || p.operation_id !== activeCsvMergeOperationId) return;
        csvMergeProgress.set(p);
        if (p.cancelled) {
            csvMergeCancelling.set(false);
        }
    });
}

export async function addCsvMergeSources(paths: string[]) {
    const discovered = await collectCsvSources(paths);
    if (!discovered.length) return;

    const current = get(csvMergeSources);
    const seen = new Set(current.map(normalizePath));
    const next = discovered.filter((path) => !seen.has(normalizePath(path)));
    if (next.length === 0) return;

    csvMergeSources.set([...current, ...next]);
    csvMergeError.set(null);
}

export function removeCsvMergeSource(path: string) {
    csvMergeSources.update((paths) => paths.filter((item) => item !== path));
}

export function clearCsvMergeState() {
    if (get(csvMergeProcessing)) return;
    csvMergeSources.set([]);
    csvMergeOutputPath.set(null);
    csvMergeResults.set([]);
    csvMergeProgress.set(null);
    csvMergeError.set(null);
    csvMergeCancelling.set(false);
}

export async function runCsvMerge() {
    const sources = get(csvMergeSources);
    const outputPath = get(csvMergeOutputPath);
    if (!sources.length || !outputPath || get(csvMergeProcessing)) return;

    const operationId = makeOperationId('csv-merge');
    activeCsvMergeOperationId = operationId;
    csvMergeProcessing.set(true);
    csvMergeCancelling.set(false);
    csvMergeResults.set([]);
    csvMergeError.set(null);
    csvMergeProgress.set({
        operation_id: operationId,
        mode: 'csv_merge',
        current_item: sources[0],
        items_done: 0,
        items_total: sources.length,
        rows_done: 0,
        cancelled: false,
    });
    await setBusy(true);
    const stopBusy = reportBusy('csv-merge', 'Merging CSVs');

    try {
        const response = await invoke<CsvMergeFileResult[]>('merge_csv_files', {
            options: {
                sources,
                output_path: outputPath,
                source_delimiter: get(csvMergeInputDelimiter),
                output_delimiter: get(csvMergeOutputDelimiter),
                include_source: get(csvMergeIncludeSource),
                include_headers: get(csvMergeIncludeHeaders),
                operation_id: operationId,
            },
        });

        csvMergeResults.set(response);
        const successCount = response.filter((item) => item.success).length;
        const failureCount = response.length - successCount;

        if (failureCount === 0 && response.length === sources.length) {
            reportOperationOutcome({
                toolId: 'csv-merge',
                kind: 'success',
                notifyTitle: 'CSV merge completed',
                notifyMessage: `${response.length} file${response.length === 1 ? '' : 's'} merged`,
                toastMessage: `CSV merge complete: ${successCount} file${successCount === 1 ? '' : 's'}`,
                activitySummary: `Merged ${response.length} CSV file${response.length === 1 ? '' : 's'}`,
                activityDetails: `Output: ${outputPath}`,
            });
        } else if (successCount === 0) {
            const message = response.length ? `No files written (${failureCount} failed)` : 'No files merged';
            csvMergeError.set(message);
            reportOperationOutcome({
                toolId: 'csv-merge',
                kind: 'failed',
                notifyTitle: 'CSV merge failed',
                notifyMessage: message,
                toastMessage: 'CSV merge failed',
                activitySummary: `CSV merge failed`,
                activityDetails: message,
            });
        } else {
            reportOperationOutcome({
                toolId: 'csv-merge',
                kind: 'partial',
                notifyTitle: 'CSV merge finished with errors',
                notifyMessage: `${successCount} succeeded, ${failureCount} failed`,
                toastMessage: `${successCount} succeeded, ${failureCount} failed`,
                activitySummary: `CSV merge finished with errors`,
                activityDetails: `${successCount} ok · ${failureCount} failed`,
            });
        }
    } catch (e) {
        const message = String(e);
        csvMergeError.set(message);
        const isCancel = message.includes('Cancelled');
        reportOperationOutcome({
            toolId: 'csv-merge',
            kind: isCancel ? 'cancelled' : 'failed',
            notifyTitle: isCancel ? 'CSV merge cancelled' : 'CSV merge failed',
            notifyMessage: message,
            toastMessage: isCancel ? 'CSV merge cancelled' : 'CSV merge failed',
            activitySummary: isCancel ? `CSV merge cancelled` : `CSV merge failed`,
            activityDetails: message.slice(0, 200),
        });
    } finally {
        csvMergeProcessing.set(false);
        csvMergeCancelling.set(false);
        activeCsvMergeOperationId = null;
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelCsvMerge() {
    if (!activeCsvMergeOperationId || get(csvMergeCancelling)) return;
    csvMergeCancelling.set(true);
    try {
        await invoke('cancel_spreadsheet_operation', { operationId: activeCsvMergeOperationId });
    } catch (e) {
        csvMergeError.set(String(e));
    }
}
