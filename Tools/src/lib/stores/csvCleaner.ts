import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type CsvCleanerProgress = {
    operation_id: string;
    mode: string;
    current_item: string;
    items_done: number;
    items_total: number;
    rows_done: number;
    cancelled: boolean;
};

export type CsvCleanerDelimiter = 'auto' | 'comma' | 'tab' | 'semicolon' | 'pipe';
export type CsvCleanerOutputDelimiter = Exclude<CsvCleanerDelimiter, 'auto'>;

export type CsvCleanupPreviewResult = {
    source_path: string;
    detected_delimiter: string;
    original_rows: number;
    cleaned_rows: number;
    original_columns: number;
    cleaned_columns: number;
    removed_empty_rows: number;
    removed_empty_columns: number;
    header_changed: boolean;
    preview_rows: string[][];
};

export type CleanCsvBatchResult = {
    source_path: string;
    output_path: string;
    original_rows: number;
    cleaned_rows: number;
    original_columns: number;
    cleaned_columns: number;
    removed_empty_rows: number;
    removed_empty_columns: number;
    success: boolean;
    error: string | null;
};
/** Preview calls can read large CSVs too; keep their in-flight state across panel remounts. */
export const csvCleanerPreviewing = writable(false);

export const csvCleanerProcessing = writable(false);
export const csvCleanerCancelling = writable(false);
export const csvCleanerProgress = writable<CsvCleanerProgress | null>(null);

// Persistent user-produced state — lifted out of CleanPanel.svelte so a
// completed preview/clean run plus the chosen sources/options survive
// leaving and returning to the tool (the workspace remounts the panel
// fresh on navigation, wiping any component-local `$state`).
export const csvCleanerSources = writable<string[]>([]);
export const csvCleanerSelectedSource = writable<string | null>(null);
export const csvCleanerOutputDir = writable<string | null>(null);

export const csvCleanerInputDelimiter = writable<CsvCleanerDelimiter>('auto');
export const csvCleanerOutputDelimiter = writable<CsvCleanerOutputDelimiter>('comma');
export const csvCleanerHasHeader = writable(true);
export const csvCleanerTrimCells = writable(true);
export const csvCleanerRemoveEmptyRows = writable(true);
export const csvCleanerRemoveEmptyColumns = writable(false);
export const csvCleanerNormalizeHeaders = writable(true);

export const csvCleanerPreview = writable<CsvCleanupPreviewResult | null>(null);
export const csvCleanerPreviewAll = writable<CsvCleanupPreviewResult[]>([]);
export const csvCleanerResults = writable<CleanCsvBatchResult[]>([]);
export const csvCleanerError = writable<string | null>(null);

let activeCsvCleanerOperationId: string | null = null;
let initialized = false;
let unlisten: UnlistenFn | null = null;

function makeOperationId() {
    return `csv-cleaner-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export async function initCsvCleanerListeners() {
    if (initialized) return;
    initialized = true;
    unlisten = await listen<CsvCleanerProgress>('spreadsheet-progress', (event) => {
        const progress = event.payload;
        if (progress.mode !== 'csv_cleanup') return;
        if (progress.operation_id !== activeCsvCleanerOperationId) return;
        csvCleanerProgress.set(progress);
        if (progress.cancelled) {
            csvCleanerCancelling.set(false);
        }
    });
}

export function startCsvCleanerRun(initialItem: string, itemsTotal = 1) {
    const operationId = makeOperationId();
    activeCsvCleanerOperationId = operationId;
    csvCleanerProcessing.set(true);
    csvCleanerCancelling.set(false);
    csvCleanerProgress.set({
        operation_id: operationId,
        mode: 'csv_cleanup',
        current_item: initialItem,
        items_done: 0,
        items_total: itemsTotal,
        rows_done: 0,
        cancelled: false,
    });
    return operationId;
}

export async function cancelCsvCleanerRun() {
    if (!activeCsvCleanerOperationId) return;
    csvCleanerCancelling.set(true);
    await invoke('cancel_spreadsheet_operation', { operationId: activeCsvCleanerOperationId });
}

export function clearCsvCleanerState() {
    csvCleanerProcessing.set(false);
    csvCleanerCancelling.set(false);
    activeCsvCleanerOperationId = null;
}

export function setCsvCleanerStopped() {
    clearCsvCleanerState();
}
