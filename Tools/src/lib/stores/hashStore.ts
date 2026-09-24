import { derived, get, writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { t } from '$lib/i18n';
import { notify } from './notifications';
import { toast } from './toasts';
import { recordActivity } from './activityLog';
import { reportBusy } from './globalBusy';
import { reportOperationOutcome } from './operationOutcome';

export type HashAlgorithm = 'md5' | 'sha1' | 'sha256' | 'blake3';

export type HashSet = {
    md5: string;
    sha1: string;
    sha256: string;
    blake3: string;
};

export type HashRowStatus = 'idle' | 'running' | 'done' | 'pass' | 'fail' | 'error' | 'cancelled';

export type HashRow = {
    id: number;
    path: string;
    name: string;
    expected: string;
    status: HashRowStatus;
    hashes: HashSet | null;
    elapsed: number;
    error: string | null;
    startedAt: number | null;
};

const INITIAL_ROW: Omit<HashRow, 'id' | 'path' | 'name'> = {
    expected: '',
    status: 'idle',
    hashes: null,
    elapsed: 0,
    error: null,
    startedAt: null,
};

export const hashRows = writable<HashRow[]>([]);
export const hashVerifyAlgorithm = writable<HashAlgorithm>('sha256');
export const hashProcessing = writable(false);
export const hashCancelling = writable(false);
export const hashOperationId = writable('');
export const hashError = writable<string | null>(null);

export const hashSummary = derived(hashRows, ($rows) => {
    let done = 0;
    let pass = 0;
    let fail = 0;
    let error = 0;
    let cancelled = 0;
    let verified = 0;

    for (const row of $rows) {
        if (row.status === 'done' || row.status === 'pass' || row.status === 'fail' || row.status === 'error' || row.status === 'cancelled') {
            done += 1;
        }
        if (row.status === 'pass') pass += 1;
        if (row.status === 'fail') fail += 1;
        if (row.status === 'error') error += 1;
        if (row.status === 'cancelled') cancelled += 1;
        if (row.status === 'pass' || row.status === 'fail') verified += 1;
    }

    return {
        total: $rows.length,
        done,
        pass,
        fail,
        error,
        cancelled,
        verified,
    };
});

export function fileName(path: string) {
    return path.split(/[\\/]/).pop() || path;
}

function newOperationId(prefix: string) {
    return `${prefix}-${globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`}`;
}

function normalizePath(path: string) {
    return path.trim().toLowerCase();
}

function normalizeHash(value: string): string {
    return value.trim().toLowerCase().replace(/[^0-9a-f]/g, '');
}

function parseAlgorithm(value: string): HashAlgorithm | null {
    const lower = value.toLowerCase().trim();
    if (lower === 'md5' || lower === 'sha1' || lower === 'sha256' || lower === 'blake3') return lower;
    return null;
}

function expectedLength(algo: HashAlgorithm): number {
    if (algo === 'md5') return 32;
    if (algo === 'sha1') return 40;
    return 64;
}

function extractExpectedHash(value: string): string {
    const trimmed = value.trim();
    const match = trimmed.match(/^(?:md5|sha1|sha256|blake3)\s*[:=]\s*([a-fA-F0-9]+)$/i);
    return match?.[1] ?? trimmed;
}

function findExpectedAlgorithm(value: string): HashAlgorithm | null {
    const trimmed = value.trim();
    const match = trimmed.match(/^(md5|sha1|sha256|blake3)\s*[:=]\s*([a-fA-F0-9]+)$/i);
    return parseAlgorithm(match?.[1] ?? '');
}

function evaluateRow(expected: string, hashes: HashSet, defaultAlgorithm: HashAlgorithm): { status: HashRowStatus; error: string | null } {
    const trimmed = expected.trim();
    if (!trimmed) return { status: 'done', error: null };

    const algorithm = findExpectedAlgorithm(expected) ?? defaultAlgorithm;
    const target = normalizeHash(extractExpectedHash(expected));

    if (target.length !== expectedLength(algorithm)) {
        return {
            status: 'error',
            error: `Expected ${algorithm.toUpperCase()} hash should be ${expectedLength(algorithm)} hex chars`,
        };
    }

    return hashes[algorithm] === target ? { status: 'pass', error: null } : { status: 'fail', error: 'Digest mismatch' };
}

function isCancelledError(message: string): boolean {
    return message.toLowerCase().includes('cancelled');
}

function isHashResult(status: HashRowStatus): boolean {
    return status === 'done' || status === 'pass' || status === 'fail';
}

function setRows(updater: (rows: HashRow[]) => HashRow[]) {
    hashRows.update(updater);
}

function updateRow(id: number, patch: Partial<HashRow>) {
    hashRows.update((rows) => rows.map((row) => (row.id === id ? { ...row, ...patch } : row)));
}

async function setBusy(busy: boolean) {
    try {
        await invoke('set_busy', { busy });
    } catch {
        // best-effort: not all releases wire this command
    }
}

export function addHashFiles(paths: string[]) {
    if (!paths.length) return;
    const incoming = paths.filter((entry): entry is string => typeof entry === 'string' && entry.trim().length > 0);
    if (!incoming.length) return;

    hashRows.update((current) => {
        const known = new Set(current.map((item) => normalizePath(item.path)));
        const now = Date.now();
        const items = [...current];
        let index = 0;

        for (const path of incoming) {
            const normalized = normalizePath(path);
            if (known.has(normalized)) continue;
            const id = now + index;
            known.add(normalized);
            index += 1;
            items.push({ id, path, name: fileName(path), ...INITIAL_ROW });
        }

        return items;
    });
}

export function removeHashFile(id: number) {
    hashRows.update((rows) => rows.filter((row) => row.id !== id));
    hashError.set(null);
}

export function setHashExpected(id: number, expected: string) {
    updateRow(id, { expected });
}

export function clearHashState() {
    if (get(hashProcessing)) return;
    hashRows.set([]);
    hashError.set(null);
    hashCancelling.set(false);
    hashOperationId.set('');
}

export function resetHashRows() {
    setRows((rows) =>
        rows.map((row) => ({
            ...row,
            status: 'idle',
            hashes: null,
            error: null,
            elapsed: 0,
            startedAt: null,
        })),
    );
    hashError.set(null);
}

export async function runHashCheck() {
    if (get(hashProcessing)) return;
    const snapshot = get(hashRows);
    if (!snapshot.length) {
        toast(t('store.hashStore.needFile'), 'error');
        return;
    }

    const operationId = newOperationId('hash-check');
    const startedAt = Date.now();
    hashOperationId.set(operationId);
    hashProcessing.set(true);
    hashCancelling.set(false);
    hashError.set(null);

    const rowsToProcess = snapshot.map<HashRow>((row) => ({ ...row, status: 'idle', error: null, hashes: null, elapsed: 0, startedAt: null }));
    hashRows.set(rowsToProcess);
    await setBusy(true);
    const stopBusy = reportBusy('hash-check', t('store.hashStore.busy', { values: { count: rowsToProcess.length } }));

    try {
    for (const row of rowsToProcess) {
        if (get(hashCancelling) || get(hashOperationId) !== operationId) {
            updateRow(row.id, { status: 'cancelled', error: 'Cancelled', startedAt: null });
            continue;
        }

        const rowStart = performance.now();
        updateRow(row.id, { status: 'running', startedAt: rowStart, error: null, hashes: null });

        try {
            const hashes = await invoke<HashSet>('compute_hashes', {
                path: row.path,
                // camelCase: `#[tauri::command]` defaults to ArgumentCase::Camel, so
                // Rust's `operation_id` param binds to the JS key `operationId`. As
                // snake_case this silently arrived as `None` (the param is
                // `Option<String>`), so mid-file cancel never reached the hasher —
                // cancellation only took effect between rows, via the JS loop check.
                operationId,
            });
            const verdict = evaluateRow(row.expected, hashes, get(hashVerifyAlgorithm));
            updateRow(row.id, {
                status: verdict.status,
                hashes,
                elapsed: Math.round(performance.now() - rowStart),
                startedAt: null,
                error: verdict.error,
            });
        } catch (error) {
            const message = String(error);
            const cancelled = isCancelledError(message) || get(hashCancelling);
            const status = cancelled ? 'cancelled' : 'error';
            updateRow(row.id, {
                status,
                hashes: null,
                elapsed: Math.round(performance.now() - rowStart),
                startedAt: null,
                error: cancelled ? 'Cancelled' : message,
            });

            if (cancelled) {
                break;
            }
        }
    }

    const remaining = get(hashRows).filter((row) => row.status === 'idle');
    if (remaining.length && get(hashCancelling)) {
        for (const row of remaining) {
            updateRow(row.id, { status: 'cancelled', error: 'Cancelled' });
        }
    }

    const total = get(hashRows).length;
    const completedRows = get(hashRows).filter((row) => isHashResult(row.status));
    const done = completedRows.length;
    const mismatches = get(hashRows).filter((row) => row.status === 'fail').length;
    const errors = get(hashRows).filter((row) => row.status === 'error').length;
    const elapsed = Math.max(1, Math.round((Date.now() - startedAt) / 1000));

    const cancelled = get(hashCancelling);
    if (cancelled) {
        const finalCompleted = done + get(hashRows).filter((row) => row.status === 'cancelled').length;
        reportOperationOutcome({
            toolId: 'hash-check',
            kind: 'cancelled',
            notifyTitle: t('store.hashStore.cancelledTitle'),
            notifyMessage: t('store.hashStore.cancelledMessage', { values: { done: finalCompleted, total } }),
            toastMessage: t('store.hashStore.cancelledToast', { values: { done: finalCompleted, total } }),
            activitySummary: t('store.hashStore.cancelledActivity'),
            activityDetails: t('store.hashStore.cancelledDetails', { values: { done: finalCompleted, total } }),
        });
    } else if (errors > 0) {
        notify({
            level: 'error',
            title: t('store.hashStore.errorTitle'),
            message: t('store.hashStore.errorMessage', { values: { count: errors } }),
            toolId: 'hash-check',
        });
        toast(t('store.hashStore.errorToast', { values: { count: errors } }), 'error');
        void recordActivity({
            toolId: 'hash-check',
            summary: done === 0 ? t('store.hashStore.errorActivityFailed') : t('store.hashStore.errorActivityPartial'),
            details: t('store.hashStore.errorDetails', { values: { done, failed: errors } }),
            outcome: done === 0 ? 'failed' : 'success',
        });
    } else if (mismatches > 0) {
        reportOperationOutcome({
            toolId: 'hash-check',
            kind: 'partial',
            notifyTitle: t('store.hashStore.mismatchTitle'),
            notifyMessage: t('store.hashStore.mismatchMessage', { values: { count: mismatches } }),
            toastMessage: t('store.hashStore.mismatchToast', { values: { count: mismatches } }),
            activitySummary: t('store.hashStore.mismatchActivity', { values: { done, mismatches } }),
        });
    } else {
        reportOperationOutcome({
            toolId: 'hash-check',
            kind: 'success',
            notifyTitle: t('store.hashStore.doneTitle'),
            notifyMessage: t('store.hashStore.doneMessage', { values: { count: done, elapsed } }),
            toastMessage: t('store.hashStore.doneToast', { values: { count: done } }),
            activitySummary: t('store.hashStore.doneActivity', { values: { count: done } }),
        });
    }

    } finally {
        hashProcessing.set(false);
        hashCancelling.set(false);
        hashOperationId.set('');
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelHashRun() {
    if (!get(hashProcessing) || get(hashCancelling)) return;

    hashCancelling.set(true);
    const operationId = get(hashOperationId);
    try {
        await invoke('cancel_hash_operation', { operationId });
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        notify({ level: 'error', title: t('store.hashStore.cancelFailedTitle'), message, toolId: 'hash-check' });
        toast(message, 'error');
    }
}

export async function copyText(value: string) {
    await navigator.clipboard.writeText(value);
}
