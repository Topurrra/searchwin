import {writable, derived} from 'svelte/store';
import {invoke} from '@tauri-apps/api/core';
import {listen, type UnlistenFn} from '@tauri-apps/api/event';
import { notify } from './notifications';
import { recordActivity } from './activityLog';
import { reportBusy } from './globalBusy';
import { reportOperationOutcome } from './operationOutcome';

export type ShredResult = {
    path: string;
    size: number;
    passes_completed: number;
    success: boolean;
    error: string | null;
    kind: 'file' | 'directory' | 'other' | 'unknown';
};

export type ProgressEvent = {
    current_file: string;
    current_pass: number;
    total_passes: number;
    bytes_done: number;
    bytes_total: number;
    files_done: number;
    files_total: number;
};

export type WipeProgressEvent = {
    bytes_written: number;
    estimated_total: number;
};

export type Method = 'quick' | 'dod3' | 'dod7';

// --- Shred state ---
export const shredFiles = writable<string[]>([]);
export type ShredderTab = 'shred' | 'wipe';
export const shredderTab = writable<ShredderTab>('shred');
export const shredMethod = writable<Method>('quick');
export const shredRandomize = writable<boolean>(true);
export const shredRecursive = writable<boolean>(true);
export const shredResults = writable<ShredResult[]>([]);
export const shredProcessing = writable<boolean>(false);
export const shredProgress = writable<ProgressEvent | null>(null);

// --- Wipe state ---
export const wipeDir = writable<string | null>(null);
export const wipeProcessing = writable<boolean>(false);
export const wipeProgress = writable<WipeProgressEvent | null>(null);
export const wipeResult = writable<{ bytes: number; error: string | null } | null>(null);

// --- Listeners (set up once, never torn down) ---
let listenersInitialized = false;
let unlistenShred: UnlistenFn | null = null;
let unlistenWipe: UnlistenFn | null = null;

async function setBusy(busy: boolean) {
    try {
        await invoke('set_busy', { busy });
    } catch {}
}

export async function initShredderListeners() {
    if (listenersInitialized) return;
    listenersInitialized = true;

    unlistenShred = await listen<ProgressEvent>('shred-progress', (e) => {
        shredProgress.set(e.payload);
    });
    unlistenWipe = await listen<WipeProgressEvent>('wipe-progress', (e) => {
        wipeProgress.set(e.payload);
    });
}

// --- Actions ---
export async function runShred(opts: {
    paths: string[];
    method: Method;
    randomize_names: boolean;
    recursive: boolean;
}): Promise<void> {
    shredProcessing.set(true);
    shredResults.set([]);
    shredProgress.set(null);
    await setBusy(true);
    const stopBusy = reportBusy('file-shredder', `Shredding ${opts.paths.length} file${opts.paths.length === 1 ? '' : 's'}`);

    const startTime = Date.now();

    try {
        const results = await invoke<ShredResult[]>('shred_files', { options: opts });
        shredResults.set(results);

        const failed = new Set(results.filter(r => !r.success).map(r => r.path));
        shredFiles.update(files => files.filter(f => failed.has(f)));

        const success = results.filter(r => r.success).length;
        const fail = results.filter(r => !r.success).length;
        const elapsed = Math.round((Date.now() - startTime) / 1000);

        if (fail === 0) {
            reportOperationOutcome({
                toolId: 'file-shredder',
                kind: 'success',
                notifyTitle: `Shredded ${success} item${success === 1 ? '' : 's'}`,
                notifyMessage: `Completed in ${elapsed}s`,
                toastMessage: `Shredded ${success} item${success === 1 ? '' : 's'}`,
                activitySummary: `Shredded ${success} item${success === 1 ? '' : 's'}`,
                activityDetails: `Method: ${opts.method}`,
            });
        } else if (success === 0) {
            reportOperationOutcome({
                toolId: 'file-shredder',
                kind: 'failed',
                notifyTitle: `Shred failed`,
                notifyMessage: `${fail} item${fail === 1 ? '' : 's'} could not be shredded`,
                toastMessage: `Shred failed`,
                activitySummary: `Shred failed`,
                activityDetails: `All ${fail} item${fail === 1 ? '' : 's'} failed · method: ${opts.method}`,
            });
        } else {
            reportOperationOutcome({
                toolId: 'file-shredder',
                kind: 'partial',
                notifyTitle: `Shredded ${success}, failed ${fail}`,
                notifyMessage: `Completed in ${elapsed}s — check results`,
                toastMessage: `${success} done, ${fail} failed`,
                activitySummary: `Shred finished with errors`,
                activityDetails: `${success} ok · ${fail} failed · method: ${opts.method}`,
            });
        }
    } catch (e) {
        const msg = String(e);
        shredResults.set([{
            path: '(operation)',
            size: 0,
            passes_completed: 0,
            success: false,
            error: msg,
            kind: 'unknown',
        }]);
        const cancelled = msg.toLowerCase().includes('cancel');
        notify({
            level: cancelled ? 'warning' : 'error',
            title: cancelled ? 'Shred cancelled' : 'Shred operation failed',
            message: msg,
            toolId: 'file-shredder',
        });
        void recordActivity({
            toolId: 'file-shredder',
            summary: cancelled ? `Shred cancelled` : `Shred operation failed`,
            details: msg.slice(0, 200),
            outcome: cancelled ? 'cancelled' : 'failed',
        });
    } finally {
        shredProcessing.set(false);
        shredProgress.set(null);
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelOperation(): Promise<void> {
    await invoke('cancel_operation');
}

export type WipePattern = 'zeros' | 'fast' | 'random' | 'ones';

export const wipePattern = writable<WipePattern>('fast');

export async function runWipe(targetDir: string, pattern: WipePattern): Promise<void> {
    wipeProcessing.set(true);
    wipeResult.set(null);
    wipeProgress.set(null);
    await setBusy(true);
    const stopBusy = reportBusy('file-shredder', 'Wiping free space');

    const startTime = Date.now();

    try {
        const bytes = await invoke<number>('wipe_free_space', { targetDir, pattern });
        wipeResult.set({ bytes, error: null });

        const elapsed = Math.round((Date.now() - startTime) / 1000);
        const gb = (bytes / (1024 ** 3)).toFixed(1);

        reportOperationOutcome({
            toolId: 'file-shredder',
            kind: 'success',
            notifyTitle: `Free space wiped`,
            notifyMessage: `${gb} GB written in ${elapsed}s on ${targetDir}`,
            toastMessage: `Wipe complete: ${gb} GB`,
            activitySummary: `Wiped ${gb} GB of free space`,
            activityDetails: `Target: ${targetDir} · pattern: ${pattern}`,
        });
    } catch (e) {
        const msg = String(e);
        wipeResult.set({ bytes: 0, error: msg });
        const isCancel = msg.includes('Cancelled');
        notify({
            level: isCancel ? 'warning' : 'error',
            title: isCancel ? 'Wipe cancelled' : 'Wipe failed',
            message: msg,
            toolId: 'file-shredder',
        });
        void recordActivity({
            toolId: 'file-shredder',
            summary: isCancel ? `Free space wipe cancelled` : `Free space wipe failed`,
            details: `Target: ${targetDir} · ${msg.slice(0, 150)}`,
            outcome: isCancel ? 'cancelled' : 'failed',
        });
    } finally {
        wipeProcessing.set(false);
        wipeProgress.set(null);
        await setBusy(false);
        stopBusy();
    }
}

export function clearShredHistory() {
    shredResults.set([]);
}

export function clearWipeHistory() {
    wipeResult.set(null);
}

export function clearShredFiles() {
    shredFiles.set([]);
}