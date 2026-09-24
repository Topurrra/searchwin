import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { get, writable } from 'svelte/store';
import { reportBusy } from './globalBusy';
import { reportOperationOutcome } from './operationOutcome';

type CsvDelimiter = 'auto' | 'comma' | 'tab' | 'semicolon' | 'pipe';

export type CsvToJsonOutputFormat = 'jsonl' | 'json';

export type CsvToJsonProgress = {
    operation_id: string;
    mode: string;
    current_item: string;
    items_done: number;
    items_total: number;
    rows_done: number;
    cancelled: boolean;
};

export type CsvToJsonFileResult = {
    source_path: string;
    output_path: string;
    rows_written: number;
    columns_written: number;
    success: boolean;
    error: string | null;
};

export const csvToJsonSources = writable<string[]>([]);
export const csvToJsonOutputDir = writable<string | null>(null);
export const csvToJsonDelimiter = writable<CsvDelimiter>('auto');
export const csvToJsonHasHeader = writable<boolean>(true);
export const csvToJsonOutputFormat = writable<CsvToJsonOutputFormat>('jsonl');
export const csvToJsonProcessing = writable(false);
export const csvToJsonCancelling = writable(false);
export const csvToJsonProgress = writable<CsvToJsonProgress | null>(null);
export const csvToJsonResults = writable<CsvToJsonFileResult[]>([]);
export const csvToJsonError = writable<string | null>(null);

let activeCsvToJsonOperationId: string | null = null;
let initialized = false;
let unlisten: UnlistenFn | null = null;

function makeOperationId() {
    return `csv-to-json-${Date.now()}-${Math.random().toString(16).slice(2)}`;
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

    const response = await invoke<{ paths: string[]; warnings: string[] }>('collect_csv_sources', {
        options: {
            paths: raw,
            recursive: true,
        },
    });

    const next = response.paths ?? [];
    if (next.length === 0 && response.warnings.length > 0) {
        csvToJsonError.set(response.warnings.join('\n'));
    }

    return next;
}

export async function initCsvToJsonListeners() {
    if (initialized) return;
    initialized = true;

    unlisten = await listen<CsvToJsonProgress>('spreadsheet-progress', (event) => {
        const progress = event.payload;
        if (progress.mode !== 'csv_to_json') return;
        if (!activeCsvToJsonOperationId || progress.operation_id !== activeCsvToJsonOperationId) return;
        csvToJsonProgress.set(progress);
        if (progress.cancelled) {
            csvToJsonCancelling.set(false);
        }
    });
}

export async function addCsvToJsonSources(paths: string[]) {
    const discovered = await collectCsvSources(paths);
    if (!discovered.length) return;

    const current = get(csvToJsonSources);
    const seen = new Set(current.map(normalizePath));
    const next = discovered.filter((path) => !seen.has(normalizePath(path)));
    csvToJsonSources.set([...current, ...next]);
}

export function removeCsvToJsonSource(path: string) {
    csvToJsonSources.update((paths) => paths.filter((p) => p !== path));
}

export function clearCsvToJsonState() {
    if (get(csvToJsonProcessing)) return;
    csvToJsonSources.set([]);
    csvToJsonResults.set([]);
    csvToJsonProgress.set(null);
    csvToJsonError.set(null);
    csvToJsonCancelling.set(false);
}

export function clearCsvToJsonResults() {
    csvToJsonResults.set([]);
    csvToJsonError.set(null);
    csvToJsonProgress.set(null);
}

export async function runCsvToJson() {
    const sources = get(csvToJsonSources);
    if (!sources.length || get(csvToJsonProcessing)) return;

    const operationId = makeOperationId();
    activeCsvToJsonOperationId = operationId;
    csvToJsonProcessing.set(true);
    csvToJsonCancelling.set(false);
    csvToJsonResults.set([]);
    csvToJsonError.set(null);
    csvToJsonProgress.set({
        operation_id: operationId,
        mode: 'csv_to_json',
        current_item: sources[0] ?? '',
        items_done: 0,
        items_total: sources.length,
        rows_done: 0,
        cancelled: false,
    });
    await setBusy(true);
    const stopBusy = reportBusy('csv-json', 'Converting CSV to JSON');

    try {
        const results = await invoke<CsvToJsonFileResult[]>('csv_to_json', {
            options: {
                sources,
                output_dir: get(csvToJsonOutputDir),
                delimiter: get(csvToJsonDelimiter),
                has_header: get(csvToJsonHasHeader),
                output_format: get(csvToJsonOutputFormat),
                operation_id: operationId,
            },
        });

        csvToJsonResults.set(results);

        const success = results.filter((r) => r.success).length;
        const fail = results.length - success;
        const cancelled = get(csvToJsonCancelling) || success + fail < sources.length;

        const fmt = get(csvToJsonOutputFormat);
        if (cancelled) {
            reportOperationOutcome({
                toolId: 'csv-json',
                kind: 'cancelled',
                notifyTitle: 'CSV to JSON cancelled',
                notifyMessage: `${success} file${success === 1 ? '' : 's'} converted before cancellation`,
                toastMessage: 'CSV to JSON cancelled',
                activitySummary: `CSV → JSON cancelled`,
                activityDetails: `${success} converted before cancel · target ${fmt.toUpperCase()}`,
            });
        } else if (fail === 0) {
            reportOperationOutcome({
                toolId: 'csv-json',
                kind: 'success',
                notifyTitle: `Converted ${success} file${success === 1 ? '' : 's'} to JSON`,
                notifyMessage: 'Done.',
                toastMessage: `Converted ${success} file${success === 1 ? '' : 's'} to JSON`,
                activitySummary: `Converted ${success} CSV file${success === 1 ? '' : 's'} → ${fmt.toUpperCase()}`,
            });
        } else if (success === 0) {
            const message = `${fail} file${fail === 1 ? '' : 's'} failed`;
            csvToJsonError.set(message);
            reportOperationOutcome({
                toolId: 'csv-json',
                kind: 'failed',
                notifyTitle: 'CSV to JSON failed',
                notifyMessage: message,
                toastMessage: 'CSV to JSON failed',
                activitySummary: `CSV → JSON failed`,
                activityDetails: message,
            });
        } else {
            reportOperationOutcome({
                toolId: 'csv-json',
                kind: 'partial',
                notifyTitle: 'CSV to JSON finished with errors',
                notifyMessage: `${success} succeeded, ${fail} failed`,
                toastMessage: `${success} succeeded, ${fail} failed`,
                activitySummary: `CSV → JSON finished with errors`,
                activityDetails: `${success} ok · ${fail} failed`,
            });
        }
    } catch (error) {
        const message = String(error);
        csvToJsonError.set(message);
        const isCancel = message.includes('Cancelled');
        reportOperationOutcome({
            toolId: 'csv-json',
            kind: isCancel ? 'cancelled' : 'failed',
            notifyTitle: isCancel ? 'CSV to JSON cancelled' : 'CSV to JSON failed',
            notifyMessage: message,
            toastMessage: isCancel ? 'Conversion cancelled' : 'Conversion failed',
            activitySummary: isCancel ? `CSV → JSON cancelled` : `CSV → JSON failed`,
            activityDetails: message.slice(0, 200),
        });
    } finally {
        csvToJsonProcessing.set(false);
        csvToJsonCancelling.set(false);
        activeCsvToJsonOperationId = null;
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelCsvToJson() {
    if (!activeCsvToJsonOperationId || get(csvToJsonCancelling)) return;
    csvToJsonCancelling.set(true);
    try {
        await invoke('cancel_spreadsheet_operation', {
            // camelCase, matching the other five callers of this command. Rust takes
            // `operation_id: String` (NOT Option), so the snake_case key didn't just
            // arrive empty — the invoke rejected as a missing required argument, and
            // cancel failed outright.
            operationId: activeCsvToJsonOperationId,
        });
    } catch (error) {
        csvToJsonError.set(String(error));
    }
}

