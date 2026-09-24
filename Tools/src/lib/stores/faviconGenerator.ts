import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { t } from '$lib/i18n';
import { toast } from './toasts';
import { reportBusy } from './globalBusy';
import { reportOperationOutcome } from './operationOutcome';
import { cancelImageExtraOperation, fileStem, newOperationId, setBusy, type ImageExtraProgress } from './imageExtraCommon';

export type FaviconOutput = { path: string; size: number; kind: string };
export type FaviconResult = { source_path: string; output_dir: string; outputs: FaviconOutput[]; success: boolean; error: string | null };

export const faviconAllSizes = [16, 24, 32, 48, 64, 128, 180, 192, 256, 512];

export const faviconSourcePath = writable<string | null>(null);
export const faviconPreview = writable('');
export const faviconOutputDir = writable<string | null>(null);
export const faviconAppName = writable('');
export const faviconBackground = writable('#ffffff');
export const faviconTransparent = writable(true);
export const faviconPaddingPercent = writable(12);
export const faviconMakePng = writable(true);
export const faviconMakeIco = writable(true);
export const faviconSelectedSizes = writable<Record<number, boolean>>({ 16: true, 32: true, 48: true, 180: true, 192: true, 256: true, 512: true });
export const faviconGenerating = writable(false);
export const faviconCancelling = writable(false);
export const faviconResult = writable<FaviconResult | null>(null);
export const faviconOperationId = writable('');
export const faviconProgress = writable<ImageExtraProgress | null>(null);

let initialized = false;
let unlisten: UnlistenFn | null = null;

export async function initFaviconGeneratorStore() {
    if (initialized) return;
    initialized = true;
    unlisten = await listen<ImageExtraProgress>('image-extra-progress', (event) => {
        const progress = event.payload;
        if (progress.tool !== 'favicon' || progress.operation_id !== get(faviconOperationId)) return;
        faviconProgress.set(progress);
    });
}

export function setFaviconSource(path: string, preview: string) {
    faviconSourcePath.set(path);
    faviconPreview.set(preview);
    if (!get(faviconAppName)) faviconAppName.set(fileStem(path));
    faviconResult.set(null);
    faviconProgress.set(null);
}

export function toggleFaviconSize(size: number) {
    faviconSelectedSizes.update((current) => ({ ...current, [size]: !current[size] }));
}

export function activeFaviconSizes() {
    const selected = get(faviconSelectedSizes);
    return faviconAllSizes.filter((size) => selected[size]);
}

export function clearFaviconState() {
    if (get(faviconGenerating)) return;
    faviconSourcePath.set(null);
    faviconPreview.set('');
    faviconOutputDir.set(null);
    faviconAppName.set('');
    faviconBackground.set('#ffffff');
    faviconTransparent.set(true);
    faviconPaddingPercent.set(12);
    faviconMakePng.set(true);
    faviconMakeIco.set(true);
    faviconSelectedSizes.set({ 16: true, 32: true, 48: true, 180: true, 192: true, 256: true, 512: true });
    faviconResult.set(null);
    faviconOperationId.set('');
    faviconProgress.set(null);
    faviconCancelling.set(false);
}

export async function runFaviconGeneration() {
    const sourcePath = get(faviconSourcePath);
    if (!sourcePath || get(faviconGenerating)) return;
    if (!get(faviconMakePng) && !get(faviconMakeIco)) return toast(t('store.faviconGenerator.needFormat'), 'error');
    const sizes = activeFaviconSizes();
    if (!sizes.length) return toast(t('store.faviconGenerator.needSize'), 'error');

    const operationId = newOperationId('favicon');
    const started = Date.now();
    faviconOperationId.set(operationId);
    faviconProgress.set({ operation_id: operationId, tool: 'favicon', source_path: sourcePath, file_name: sourcePath.split(/[\\/]/).pop() ?? sourcePath, index: 0, total: 1, stage: 'queued', progress: 0, message: 'Queued' });
    faviconResult.set(null);
    faviconGenerating.set(true);
    faviconCancelling.set(false);
    await setBusy(true);
    const stopBusy = reportBusy('img-favicon', t('store.faviconGenerator.busy'));

    try {
        const result = await invoke<FaviconResult>('generate_favicons', {
            options: {
                source_path: sourcePath,
                output_dir: get(faviconOutputDir),
                app_name: get(faviconAppName) || null,
                background: get(faviconBackground),
                transparent: get(faviconTransparent),
                padding_percent: get(faviconPaddingPercent),
                sizes,
                make_ico: get(faviconMakeIco),
                make_png: get(faviconMakePng),
                operation_id: operationId,
            },
        });
        faviconResult.set(result);
        const elapsed = Math.max(1, Math.round((Date.now() - started) / 1000));

        if (get(faviconCancelling) || result.error === 'Cancelled') {
            reportOperationOutcome({
                toolId: 'img-favicon',
                kind: 'cancelled',
                notifyTitle: t('store.faviconGenerator.cancelledTitle'),
                notifyMessage: t('store.faviconGenerator.cancelledMessage', { values: { count: result.outputs.length } }),
                toastMessage: t('store.faviconGenerator.cancelledToast'),
                activitySummary: t('store.faviconGenerator.cancelledActivity'),
                activityDetails: t('store.faviconGenerator.cancelledDetails', { values: { count: result.outputs.length } }),
            });
        } else if (result.success) {
            reportOperationOutcome({
                toolId: 'img-favicon',
                kind: 'success',
                notifyTitle: t('store.faviconGenerator.doneTitle', { values: { count: result.outputs.length } }),
                notifyMessage: t('store.faviconGenerator.doneMessage', { values: { elapsed } }),
                toastMessage: t('store.faviconGenerator.doneToast', { values: { count: result.outputs.length } }),
                activitySummary: t('store.faviconGenerator.doneActivity', { values: { count: result.outputs.length } }),
                activityDetails: result.output_dir ? t('store.faviconGenerator.doneDetails', { values: { dir: result.output_dir } }) : null,
            });
        } else {
            const failMessage = result.error ?? t('store.faviconGenerator.failedFallback');
            reportOperationOutcome({
                toolId: 'img-favicon',
                kind: 'failed',
                notifyTitle: t('store.faviconGenerator.failedTitle'),
                notifyMessage: failMessage,
                toastMessage: failMessage,
                activitySummary: t('store.faviconGenerator.failedActivity'),
                activityDetails: failMessage.slice(0, 200),
            });
        }
    } catch (error) {
        const message = String(error);
        const wasCancel = get(faviconCancelling);
        reportOperationOutcome({
            toolId: 'img-favicon',
            kind: wasCancel ? 'cancelled' : 'failed',
            notifyTitle: wasCancel ? t('store.faviconGenerator.cancelledTitle') : t('store.faviconGenerator.failedTitle'),
            notifyMessage: message,
            toastMessage: message,
            activitySummary: wasCancel ? t('store.faviconGenerator.cancelledActivity') : t('store.faviconGenerator.failedActivity'),
            activityDetails: message.slice(0, 200),
        });
    } finally {
        faviconGenerating.set(false);
        faviconCancelling.set(false);
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelFaviconGeneration() {
    if (!get(faviconGenerating) || get(faviconCancelling)) return;
    faviconCancelling.set(true);
    await cancelImageExtraOperation(get(faviconOperationId));
}
