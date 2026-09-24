import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { t } from '$lib/i18n';
import { toast } from './toasts';
import { reportBusy } from './globalBusy';
import { reportOperationOutcome } from './operationOutcome';
import { cancelImageExtraOperation, fileName, fmtBytes, newOperationId, setBusy, type ImageExtraProgress } from './imageExtraCommon';

export type WatermarkMode = 'text' | 'image';
export type WatermarkPosition = 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right' | 'center';

export type WatermarkResult = {
    source_path: string;
    output_path: string;
    original_size: number;
    new_size: number;
    dimensions: [number, number];
    success: boolean;
    error: string | null;
};

export type WatermarkConfig = {
    suffix: string;
    mode: WatermarkMode;
    text: string;
    watermarkPath: string | null;
    watermarkPreview: string;
    position: WatermarkPosition;
    tiled: boolean;
    opacity: number;
    scalePercent: number;
    margin: number;
    color: string;
    quality: number;
};

export type WatermarkItem = {
    path: string;
    preview: string;
    config: WatermarkConfig;
};

export const watermarkItems = writable<WatermarkItem[]>([]);
export const watermarkActiveIndex = writable(0);
export const watermarkOutputDir = writable<string | null>(null);
export const watermarkProcessing = writable(false);
export const watermarkCancelling = writable(false);
export const watermarkCurrentJob = writable('');
export const watermarkResults = writable<WatermarkResult[]>([]);
export const watermarkOperationId = writable('');
export const watermarkProgressByPath = writable<Record<string, ImageExtraProgress>>({});

let initialized = false;
let unlisten: UnlistenFn | null = null;

export function defaultWatermarkConfig(): WatermarkConfig {
    return {
        suffix: '_watermarked',
        mode: 'text',
        text: 'KeepItLocal',
        watermarkPath: null,
        watermarkPreview: '',
        position: 'bottom-right',
        tiled: false,
        opacity: 35,
        scalePercent: 18,
        margin: 32,
        color: '#ffffff',
        quality: 90,
    };
}

export async function initImageWatermarkStore() {
    if (initialized) return;
    initialized = true;
    unlisten = await listen<ImageExtraProgress>('image-extra-progress', (event) => {
        const progress = event.payload;
        if (progress.tool !== 'watermark' || progress.operation_id !== get(watermarkOperationId)) return;
        watermarkProgressByPath.update((current) => ({ ...current, [progress.source_path]: progress }));
        if (progress.file_name) watermarkCurrentJob.set(progress.file_name);
    });
}

export function addWatermarkItems(paths: string[], previewFor: (path: string) => string) {
    const existing = new Set(get(watermarkItems).map((item) => item.path));
    const baseConfig = getActiveWatermarkConfig();
    const additions = paths
        .filter((path) => !existing.has(path))
        .map((path) => ({ path, preview: previewFor(path), config: { ...baseConfig } }));
    if (!additions.length) return;
    watermarkItems.update((current) => [...current, ...additions]);
    if (get(watermarkItems).length === additions.length) watermarkActiveIndex.set(0);
    watermarkResults.set([]);
}

export function removeWatermarkItem(path: string) {
    const current = get(watermarkItems);
    const next = current.filter((item) => item.path !== path);
    watermarkItems.set(next);
    watermarkResults.update((results) => results.filter((r) => r.source_path !== path));
    watermarkProgressByPath.update((progress) => {
        const copy = { ...progress };
        delete copy[path];
        return copy;
    });
    watermarkActiveIndex.update((index) => Math.min(index, Math.max(0, next.length - 1)));
}

export function setWatermarkActiveIndex(index: number) {
    const count = get(watermarkItems).length;
    watermarkActiveIndex.set(Math.max(0, Math.min(index, Math.max(0, count - 1))));
}

export function getActiveWatermarkItem() {
    return get(watermarkItems)[get(watermarkActiveIndex)] ?? null;
}

export function getActiveWatermarkConfig() {
    return getActiveWatermarkItem()?.config ?? defaultWatermarkConfig();
}

export function updateActiveWatermarkConfig(patch: Partial<WatermarkConfig>) {
    const index = get(watermarkActiveIndex);
    watermarkItems.update((items) => items.map((item, itemIndex) => itemIndex === index ? { ...item, config: { ...item.config, ...patch } } : item));
}

export function resetActiveWatermarkConfig() {
    updateActiveWatermarkConfig(defaultWatermarkConfig());
}

export function applyActiveWatermarkToAll() {
    const cfg = getActiveWatermarkConfig();
    watermarkItems.update((items) => items.map((item) => ({ ...item, config: { ...cfg } })));
    toast(t('store.imageWatermark.copiedSettings', { values: { count: get(watermarkItems).length } }), 'success');
}

export function clearWatermarkState() {
    if (get(watermarkProcessing)) return;
    watermarkItems.set([]);
    watermarkActiveIndex.set(0);
    watermarkOutputDir.set(null);
    watermarkResults.set([]);
    watermarkProgressByPath.set({});
    watermarkOperationId.set('');
    watermarkCurrentJob.set('');
    watermarkCancelling.set(false);
}

export async function runImageWatermark() {
    const items = get(watermarkItems);
    if (!items.length || get(watermarkProcessing)) return;

    const operationId = newOperationId('watermark');
    const started = Date.now();
    watermarkOperationId.set(operationId);
    watermarkResults.set([]);
    watermarkProgressByPath.set(Object.fromEntries(items.map((item, index) => [item.path, { operation_id: operationId, tool: 'watermark', source_path: item.path, file_name: fileName(item.path), index, total: items.length, stage: 'queued', progress: 0, message: 'Queued' }])));
    watermarkProcessing.set(true);
    watermarkCancelling.set(false);
    watermarkCurrentJob.set('');
    await setBusy(true);
    const stopBusy = reportBusy('img-watermark', t('store.imageWatermark.busy', { values: { count: items.length } }));

    const nextResults: WatermarkResult[] = [];

    try {
        for (let index = 0; index < items.length; index += 1) {
            if (get(watermarkCancelling)) break;
            const item = items[index];
            const cfg = item.config;
            watermarkCurrentJob.set(fileName(item.path));

            if (cfg.mode === 'text' && !cfg.text.trim()) {
                nextResults.push({ source_path: item.path, output_path: '', original_size: 0, new_size: 0, dimensions: [0, 0], success: false, error: t('store.imageWatermark.needText') });
                watermarkResults.set([...nextResults]);
                continue;
            }
            if (cfg.mode === 'image' && !cfg.watermarkPath) {
                nextResults.push({ source_path: item.path, output_path: '', original_size: 0, new_size: 0, dimensions: [0, 0], success: false, error: t('store.imageWatermark.needImage') });
                watermarkResults.set([...nextResults]);
                continue;
            }

            const response = await invoke<WatermarkResult[]>('watermark_images', {
                options: {
                    paths: [item.path],
                    output_dir: get(watermarkOutputDir),
                    suffix: cfg.suffix,
                    mode: cfg.mode,
                    text: cfg.mode === 'text' ? cfg.text : null,
                    watermark_path: cfg.mode === 'image' ? cfg.watermarkPath : null,
                    position: cfg.position,
                    tiled: cfg.tiled,
                    opacity: cfg.opacity,
                    scale_percent: cfg.scalePercent,
                    margin: cfg.margin,
                    color: cfg.color,
                    quality: cfg.quality,
                    operation_id: operationId,
                },
            });
            const result = response[0];
            nextResults.push(result);
            watermarkResults.set([...nextResults]);
            if (result?.error === 'Cancelled') {
                watermarkCancelling.set(true);
                break;
            }
        }

        const success = nextResults.filter((r) => r.success).length;
        const failed = nextResults.filter((r) => !r.success && r.error !== 'Cancelled').length;
        const elapsed = Math.max(1, Math.round((Date.now() - started) / 1000));

        if (get(watermarkCancelling) || nextResults.some((r) => r.error === 'Cancelled')) {
            reportOperationOutcome({
                toolId: 'img-watermark',
                kind: 'cancelled',
                notifyTitle: t('store.imageWatermark.cancelledTitle'),
                notifyMessage: t('store.imageWatermark.cancelledMessage', { values: { done: success, total: items.length } }),
                toastMessage: t('store.imageWatermark.cancelledToast', { values: { done: success, total: items.length } }),
                activitySummary: t('store.imageWatermark.cancelledActivity'),
                activityDetails: t('store.imageWatermark.cancelledDetails', { values: { done: success, total: items.length } }),
            });
        } else if (failed === 0) {
            reportOperationOutcome({
                toolId: 'img-watermark',
                kind: 'success',
                notifyTitle: t('store.imageWatermark.doneTitle', { values: { count: success } }),
                notifyMessage: t('store.imageWatermark.doneMessage', { values: { elapsed } }),
                toastMessage: t('store.imageWatermark.doneToast', { values: { count: success } }),
                activitySummary: t('store.imageWatermark.doneActivity', { values: { count: success } }),
            });
        } else if (success === 0) {
            reportOperationOutcome({
                toolId: 'img-watermark',
                kind: 'failed',
                notifyTitle: t('store.imageWatermark.failedTitle'),
                notifyMessage: t('store.imageWatermark.failedMessage', { values: { count: failed } }),
                toastMessage: t('store.imageWatermark.failedToast'),
                activitySummary: t('store.imageWatermark.failedActivity'),
                activityDetails: t('store.imageWatermark.failedDetails', { values: { count: failed } }),
            });
        } else {
            reportOperationOutcome({
                toolId: 'img-watermark',
                kind: 'partial',
                notifyTitle: t('store.imageWatermark.partialTitle', { values: { success, failed } }),
                notifyMessage: t('store.imageWatermark.partialMessage', { values: { elapsed } }),
                toastMessage: t('store.imageWatermark.partialToast', { values: { success, failed } }),
                activitySummary: t('store.imageWatermark.partialActivity'),
                activityDetails: t('store.imageWatermark.partialDetails', { values: { success, failed } }),
            });
        }
    } catch (error) {
        const message = String(error);
        const wasCancel = get(watermarkCancelling);
        reportOperationOutcome({
            toolId: 'img-watermark',
            kind: wasCancel ? 'cancelled' : 'failed',
            notifyTitle: wasCancel ? t('store.imageWatermark.cancelledTitle') : t('store.imageWatermark.failedTitle'),
            notifyMessage: message,
            toastMessage: message,
            activitySummary: wasCancel ? t('store.imageWatermark.cancelledActivity') : t('store.imageWatermark.failedActivity'),
            activityDetails: message.slice(0, 200),
        });
    } finally {
        watermarkProcessing.set(false);
        watermarkCancelling.set(false);
        watermarkCurrentJob.set('');
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelImageWatermark() {
    if (!get(watermarkProcessing) || get(watermarkCancelling)) return;
    watermarkCancelling.set(true);
    await cancelImageExtraOperation(get(watermarkOperationId));
}

export { fileName, fmtBytes };
