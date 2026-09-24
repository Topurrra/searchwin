import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { notify } from './notifications';
import { toast } from './toasts';
import { recordActivity } from './activityLog';
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

export type CsvSplitFileResult = {
    source_path: string;
    output_dir: string;
    output_paths: string[];
    total_rows: number;
    split_count: number;
    rows_per_file: number;
    success: boolean;
    error: string | null;
};

export type CsvSourcesResult = {
    paths: string[];
    warnings: string[];
};

export const csvSplitSources = writable<string[]>([]);
export const csvSplitOutputDir = writable<string | null>(null);
export const csvSplitRowsPerFile = writable<number>(1000);
export const csvSplitDelimiter = writable<Delimiter>('auto');
export const csvSplitIncludeHeader = writable(true);
export const csvSplitProcessing = writable(false);
export const csvSplitCancelling = writable(false);
export const csvSplitProgress = writable<SpreadsheetProgress | null>(null);
export const csvSplitResults = writable<CsvSplitFileResult[]>([]);
export const csvSplitError = writable<string | null>(null);

let activeCsvSplitOperationId: string | null = null;
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
        csvSplitError.set(response.warnings.join('\n'));
    }

    return response.paths ?? [];
}

export async function initCsvSplitListeners() {
    if (unlisten) return;
    unlisten = await listen<SpreadsheetProgress>('spreadsheet-progress', (event) => {
        const progress = event.payload;
        if (progress.mode !== 'csv_split') return;
        if (!activeCsvSplitOperationId || progress.operation_id !== activeCsvSplitOperationId) return;
        csvSplitProgress.set(progress);
        if (progress.cancelled) {
            csvSplitCancelling.set(false);
        }
    });
}

export async function addCsvSplitSources(paths: string[]) {
    const discovered = await collectCsvSources(paths);
    if (!discovered.length) return;

    const current = get(csvSplitSources);
    const seen = new Set(current.map(normalizePath));
    const next = discovered.filter((path) => !seen.has(normalizePath(path)));
    if (next.length === 0) return;

    csvSplitSources.set([...current, ...next]);
    csvSplitError.set(null);
}

export function removeCsvSplitSource(path: string) {
    csvSplitSources.update((paths) => paths.filter((item) => item !== path));
}

export function clearCsvSplitState() {
    if (get(csvSplitProcessing)) return;
    csvSplitSources.set([]);
    csvSplitOutputDir.set(null);
    csvSplitResults.set([]);
    csvSplitProgress.set(null);
    csvSplitError.set(null);
    csvSplitCancelling.set(false);
}

export async function runCsvSplit() {
    const sources = get(csvSplitSources);
    const outputDir = get(csvSplitOutputDir);
    const rowsPerFile = get(csvSplitRowsPerFile);
    if (!sources.length || !outputDir || !rowsPerFile || get(csvSplitProcessing)) return;

    const operationId = makeOperationId('csv-split');
    activeCsvSplitOperationId = operationId;
    csvSplitProcessing.set(true);
    csvSplitCancelling.set(false);
    csvSplitResults.set([]);
    csvSplitError.set(null);
    csvSplitProgress.set({
        operation_id: operationId,
        mode: 'csv_split',
        current_item: sources[0] ?? '',
        items_done: 0,
        items_total: sources.length,
        rows_done: 0,
        cancelled: false,
    });
    await setBusy(true);
    const stopBusy = reportBusy('csv-split', 'Splitting CSV');

    try {
        const response = await invoke<CsvSplitFileResult[]>('split_csv_files', {
            options: {
                sources,
                output_dir: outputDir,
                rows_per_file: rowsPerFile,
                include_header: get(csvSplitIncludeHeader),
                delimiter: get(csvSplitDelimiter),
                operation_id: operationId,
            },
        });

        csvSplitResults.set(response);
        const successCount = response.filter((item) => item.success).length;
        const failureCount = response.length - successCount;

        if (response.length === 0) {
            const message = 'No CSV source was processed';
            csvSplitError.set(message);
            reportOperationOutcome({
                toolId: 'csv-split',
                kind: 'failed',
                notifyTitle: 'CSV split failed',
                notifyMessage: message,
                toastMessage: message,
                activitySummary: `CSV split failed`,
                activityDetails: message,
            });
            return;
        }

        if (failureCount === 0) {
            const totalChunks = response.reduce((sum, r) => sum + (r.split_count ?? 0), 0);
            reportOperationOutcome({
                toolId: 'csv-split',
                kind: 'success',
                notifyTitle: 'CSV split completed',
                notifyMessage: `${successCount} file${successCount === 1 ? '' : 's'} split`,
                toastMessage: `CSV split complete: ${successCount} file${successCount === 1 ? '' : 's'}`,
                activitySummary: `Split ${successCount} CSV file${successCount === 1 ? '' : 's'} into ${totalChunks} chunk${totalChunks === 1 ? '' : 's'}`,
            });
        } else if (successCount === 0) {
            const message = `${failureCount} file${failureCount === 1 ? '' : 's'} failed`;
            csvSplitError.set(message);
            reportOperationOutcome({
                toolId: 'csv-split',
                kind: 'failed',
                notifyTitle: 'CSV split failed',
                notifyMessage: message,
                toastMessage: 'CSV split failed',
                activitySummary: `CSV split failed`,
                activityDetails: message,
            });
        } else {
            notify({
                level: 'info',
                title: 'CSV split partially completed',
                message: `${successCount} succeeded, ${failureCount} failed`,
                toolId: 'csv-split',
            });
            toast(`${successCount} succeeded, ${failureCount} failed`, 'info');
            void recordActivity({
                toolId: 'csv-split',
                summary: `CSV split finished with errors`,
                details: `${successCount} ok · ${failureCount} failed`,
                outcome: 'success',
            });
        }
    } catch (e) {
        const message = String(e);
        csvSplitError.set(message);
        const isCancel = message.includes('Cancelled');
        notify({
            level: isCancel ? 'info' : 'error',
            title: isCancel ? 'CSV split cancelled' : 'CSV split failed',
            message,
            toolId: 'csv-split',
        });
        toast(isCancel ? 'CSV split cancelled' : 'CSV split failed', isCancel ? 'info' : 'error');
        void recordActivity({
            toolId: 'csv-split',
            summary: isCancel ? `CSV split cancelled` : `CSV split failed`,
            details: message.slice(0, 200),
            outcome: isCancel ? 'cancelled' : 'failed',
        });
    } finally {
        csvSplitProcessing.set(false);
        csvSplitCancelling.set(false);
        activeCsvSplitOperationId = null;
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelCsvSplit() {
    if (!activeCsvSplitOperationId || get(csvSplitCancelling)) return;
    csvSplitCancelling.set(true);
    try {
        await invoke('cancel_spreadsheet_operation', { operationId: activeCsvSplitOperationId });
    } catch (e) {
        csvSplitError.set(String(e));
    }
}
