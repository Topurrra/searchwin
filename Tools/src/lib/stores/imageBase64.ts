import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { t } from '$lib/i18n';
import { toast } from './toasts';
import { reportBusy } from './globalBusy';
import { reportOperationOutcome } from './operationOutcome';
import { addUnique, cancelImageOperation, initialProgress, newOperationId, setBusy, type Base64ImageResult, type ImageProgress } from './imageBatchCommon';

export type ImageBase64CopyMode = 'data_uri' | 'raw';

export const base64Files = writable<string[]>([]);
export const base64CopyMode = writable<ImageBase64CopyMode>('data_uri');
export const base64SelectedIdx = writable(0);
export const base64Processing = writable(false);
export const base64Cancelling = writable(false);
export const base64Results = writable<Base64ImageResult[]>([]);
export const base64OperationId = writable('');
export const base64ProgressByPath = writable<Record<string, ImageProgress>>({});

let initialized = false;
let unlisten: UnlistenFn | null = null;

export async function initImageBase64Store() {
    if (initialized) return;
    initialized = true;
    unlisten = await listen<ImageProgress>('image-progress', (event) => {
        const progress = event.payload;
        if (progress.tool !== 'base64' || progress.operation_id !== get(base64OperationId)) return;
        base64ProgressByPath.update((current) => ({ ...current, [progress.source_path]: progress }));
    });
}

export function destroyImageBase64StoreListener() {
    unlisten?.();
    unlisten = null;
    initialized = false;
}

export function addBase64Files(paths: string[]) {
    if (get(base64Processing)) return;
    base64Files.update((current) => addUnique(current, paths));
}

export function removeBase64File(path: string) {
    base64Files.update((current) => current.filter((item) => item !== path));
    base64ProgressByPath.update((current) => {
        const next = { ...current };
        delete next[path];
        return next;
    });
}

export function clearBase64State() {
    if (get(base64Processing)) return;
    base64Files.set([]);
    base64Results.set([]);
    base64ProgressByPath.set({});
    base64OperationId.set('');
    base64SelectedIdx.set(0);
    base64Cancelling.set(false);
}

export async function runImageBase64() {
    const paths = get(base64Files);
    if (!paths.length || get(base64Processing)) return;

    const operationId = newOperationId('img-base64');
    const started = Date.now();
    base64OperationId.set(operationId);
    base64Results.set([]);
    base64SelectedIdx.set(0);
    base64ProgressByPath.set(initialProgress(paths, operationId, 'base64'));
    base64Processing.set(true);
    base64Cancelling.set(false);
    await setBusy(true);
    const stopBusy = reportBusy('img-base64', t('store.imageBase64.busy', { values: { count: paths.length } }));

    try {
        const results = await invoke<Base64ImageResult[]>('images_to_base64', {
            options: {
                paths,
                operation_id: operationId,
            },
        });
        base64Results.set(results);

        const success = results.filter((r) => r.success).length;
        const failed = results.filter((r) => !r.success && r.error !== 'Cancelled').length;
        const elapsed = Math.max(1, Math.round((Date.now() - started) / 1000));

        if (get(base64Cancelling) || results.some((r) => r.error === 'Cancelled')) {
            reportOperationOutcome({
                toolId: 'img-base64',
                kind: 'cancelled',
                notifyTitle: t('store.imageBase64.cancelledTitle'),
                notifyMessage: t('store.imageBase64.cancelledMessage', { values: { done: success, total: paths.length } }),
                toastMessage: t('store.imageBase64.cancelledToast', { values: { done: success, total: paths.length } }),
                activitySummary: t('store.imageBase64.cancelledActivity'),
                activityDetails: t('store.imageBase64.cancelledDetails', { values: { done: success, total: paths.length } }),
            });
        } else if (failed === 0) {
            reportOperationOutcome({
                toolId: 'img-base64',
                kind: 'success',
                notifyTitle: t('store.imageBase64.doneTitle', { values: { count: success } }),
                notifyMessage: t('store.imageBase64.doneMessage', { values: { elapsed } }),
                toastMessage: t('store.imageBase64.doneToast', { values: { count: success } }),
                activitySummary: t('store.imageBase64.doneActivity', { values: { count: success } }),
            });
        } else if (success === 0) {
            reportOperationOutcome({
                toolId: 'img-base64',
                kind: 'failed',
                notifyTitle: t('store.imageBase64.failedTitle'),
                notifyMessage: t('store.imageBase64.failedMessage', { values: { count: failed } }),
                toastMessage: t('store.imageBase64.failedToast'),
                activitySummary: t('store.imageBase64.failedActivity'),
                activityDetails: t('store.imageBase64.failedDetails', { values: { count: failed } }),
            });
        } else {
            reportOperationOutcome({
                toolId: 'img-base64',
                kind: 'partial',
                notifyTitle: t('store.imageBase64.partialTitle', { values: { success, failed } }),
                notifyMessage: t('store.imageBase64.partialMessage', { values: { elapsed } }),
                toastMessage: t('store.imageBase64.partialToast', { values: { success, failed } }),
                activitySummary: t('store.imageBase64.partialActivity'),
                activityDetails: t('store.imageBase64.partialDetails', { values: { success, failed } }),
            });
        }
    } catch (error) {
        const msg = String(error);
        const wasCancel = get(base64Cancelling);
        reportOperationOutcome({
            toolId: 'img-base64',
            kind: wasCancel ? 'cancelled' : 'failed',
            notifyTitle: wasCancel ? t('store.imageBase64.cancelledTitle') : t('store.imageBase64.failedTitle'),
            notifyMessage: msg,
            toastMessage: msg,
            activitySummary: wasCancel ? t('store.imageBase64.cancelledActivity') : t('store.imageBase64.failedActivity'),
            activityDetails: msg.slice(0, 200),
        });
    } finally {
        base64Processing.set(false);
        base64Cancelling.set(false);
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelImageBase64() {
    if (!get(base64Processing) || get(base64Cancelling)) return;
    base64Cancelling.set(true);
    await cancelImageOperation(get(base64OperationId));
}

export async function copyBase64Selected() {
    const results = get(base64Results);
    const selected = results[get(base64SelectedIdx)];
    if (!selected?.success) return;
    const mode = get(base64CopyMode);
    await navigator.clipboard.writeText(mode === 'data_uri' ? selected.data_uri : selected.raw_base64);
    toast(mode === 'data_uri' ? t('store.imageBase64.copiedDataUri') : t('store.imageBase64.copiedRaw'), 'success');
}

export async function copyBase64All() {
    const mode = get(base64CopyMode);
    const texts = get(base64Results).filter((r) => r.success).map((r) => (mode === 'data_uri' ? r.data_uri : r.raw_base64));
    if (!texts.length) return;
    await navigator.clipboard.writeText(texts.join('\n\n'));
    toast(t('store.imageBase64.copiedAll', { values: { count: texts.length } }), 'success');
}
