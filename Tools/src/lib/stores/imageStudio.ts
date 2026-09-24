/*
  Image Studio store — the unified image pipeline (resize + convert + compress
  in one pass). Built on the former Image Resizer's batch/single engine and
  extended with an output-format axis (Keep original or convert to JPEG/PNG/
  WebP/…) and a "Keep size" mode for pure convert/compress.

  Backend: reuses the `resize_images` Tauri command (which now also honours
  `output_format`), so progress events still arrive tagged `tool: 'resizer'`.
*/
import { writable, get } from 'svelte/store';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { t } from '$lib/i18n';
import { reportBusy } from './globalBusy';
import { reportOperationOutcome } from './operationOutcome';
import { addUnique, cancelImageOperation, initialProgress, newOperationId, setBusy, type ImageProgress, type ImageResult } from './imageBatchCommon';

export type ImageStudioTab = 'single' | 'batch';
/** 'none' = keep current dimensions (pure convert/compress). */
export type ImageStudioMode = 'none' | 'px' | 'percent' | 'longest_side' | 'shortest_side';
export type ImageStudioFilter = 'lanczos' | 'nearest' | 'linear' | 'cubic';
/** 'keep' = preserve the source's own format/extension. */
export type ImageStudioFormat = 'keep' | 'jpg' | 'png' | 'webp' | 'avif' | 'bmp' | 'tiff' | 'ico' | 'gif';
export type ImageStudioOperation = 'resize' | 'crop' | 'background' | null;
export type ImageStudioCrop = { x: number; y: number; width: number; height: number };
export type ImageStudioBackgroundQuality = 'fast' | 'high';

export const studioActiveTab = writable<ImageStudioTab>('single');
export const studioOutputDir = writable<string | null>(null);
// Non-empty default so a keep-format + keep-size pass never silently overwrites the original.
export const studioSuffix = writable('_studio');
export const studioOutputFormat = writable<ImageStudioFormat>('keep');
export const studioFilter = writable<ImageStudioFilter>('lanczos');
export const studioQuality = writable(90);
export const studioBackgroundQuality = writable<ImageStudioBackgroundQuality>('fast');
export const studioProcessing = writable(false);
export const studioCancelling = writable(false);
export const studioOperationId = writable('');
export const studioOperationKind = writable<ImageStudioOperation>(null);
export const studioResults = writable<ImageResult[]>([]);
export const studioResultLabel = writable<string | null>(null);
export const studioProgressByPath = writable<Record<string, ImageProgress>>({});

export const studioSinglePath = writable<string | null>(null);
export const studioSinglePreview = writable('');
export const studioSingleOriginalW = writable(0);
export const studioSingleOriginalH = writable(0);
export const studioSingleWidth = writable(0);
export const studioSingleHeight = writable(0);
export const studioSinglePreserveAspect = writable(true);
export const studioSingleCropEnabled = writable(false);
export const studioSingleCrop = writable<ImageStudioCrop | null>(null);

export const studioBatchFiles = writable<string[]>([]);
export const studioMode = writable<ImageStudioMode>('px');
export const studioWidth = writable<number | null>(1920);
export const studioHeight = writable<number | null>(1080);
export const studioPercent = writable(50);
export const studioLongestSide = writable(1920);
export const studioShortestSide = writable(720);
export const studioPreserveAspect = writable(true);

let initialized = false;
let unlisten: UnlistenFn | null = null;

export async function initImageStudioStore() {
    if (initialized) return;
    initialized = true;
    unlisten = await listen<ImageProgress>('image-progress', (event) => {
        const progress = event.payload;
        const operationId = get(studioOperationId);
        if (!operationId || progress.operation_id !== operationId) return;
        studioProgressByPath.update((current) => ({ ...current, [progress.source_path]: progress }));
    });
}

export function destroyImageStudioStoreListener() {
    unlisten?.();
    unlisten = null;
    initialized = false;
}

export function clampDimension(value: number) {
    return Math.max(1, Math.min(50000, Math.round(value || 1)));
}

export function setStudioSinglePath(path: string) {
    studioSinglePath.set(path);
    studioSinglePreview.set(convertFileSrc(path));
    studioSingleOriginalW.set(0);
    studioSingleOriginalH.set(0);
    studioSingleWidth.set(0);
    studioSingleHeight.set(0);
    studioSingleCropEnabled.set(false);
    studioSingleCrop.set(null);
    studioResults.set([]);
    studioResultLabel.set(null);
    studioProgressByPath.set({});
}

export function clearStudioSingle() {
    if (get(studioProcessing)) return;
    studioSinglePath.set(null);
    studioSinglePreview.set('');
    studioSingleOriginalW.set(0);
    studioSingleOriginalH.set(0);
    studioSingleWidth.set(0);
    studioSingleHeight.set(0);
    studioSingleCropEnabled.set(false);
    studioSingleCrop.set(null);
    studioResults.set([]);
    studioResultLabel.set(null);
    studioProgressByPath.set({});
}

export function setStudioSingleOriginalDimensions(width: number, height: number) {
    if (!width || !height) return;
    const currentW = get(studioSingleWidth);
    const currentH = get(studioSingleHeight);
    studioSingleOriginalW.set(width);
    studioSingleOriginalH.set(height);
    if (!currentW || !currentH) {
        studioSingleWidth.set(width);
        studioSingleHeight.set(height);
    }
    if (!get(studioSingleCrop)) {
        studioSingleCrop.set({ x: 0, y: 0, width, height });
    }
}

export function setStudioSingleWidth(value: number) {
    const nextW = clampDimension(value);
    studioSingleWidth.set(nextW);
    if (get(studioSinglePreserveAspect)) {
        const ow = get(studioSingleOriginalW);
        const oh = get(studioSingleOriginalH);
        if (ow && oh) studioSingleHeight.set(clampDimension(nextW / (ow / oh)));
    }
}

export function setStudioSingleHeight(value: number) {
    const nextH = clampDimension(value);
    studioSingleHeight.set(nextH);
    if (get(studioSinglePreserveAspect)) {
        const ow = get(studioSingleOriginalW);
        const oh = get(studioSingleOriginalH);
        if (ow && oh) studioSingleWidth.set(clampDimension(nextH * (ow / oh)));
    }
}

export function applyStudioSinglePercent(percent: number) {
    const ow = get(studioSingleOriginalW);
    const oh = get(studioSingleOriginalH);
    if (!ow || !oh) return;
    studioSingleWidth.set(clampDimension(ow * (percent / 100)));
    studioSingleHeight.set(clampDimension(oh * (percent / 100)));
}

export function resetStudioSingleSize() {
    const ow = get(studioSingleOriginalW);
    const oh = get(studioSingleOriginalH);
    if (!ow || !oh) return;
    studioSingleWidth.set(ow);
    studioSingleHeight.set(oh);
}

export function setStudioSingleCrop(crop: ImageStudioCrop) {
    const originalW = get(studioSingleOriginalW);
    const originalH = get(studioSingleOriginalH);
    if (!originalW || !originalH) return;

    const x = Math.max(0, Math.min(originalW - 1, Math.round(crop.x || 0)));
    const y = Math.max(0, Math.min(originalH - 1, Math.round(crop.y || 0)));
    const width = Math.max(1, Math.min(originalW - x, Math.round(crop.width || 1)));
    const height = Math.max(1, Math.min(originalH - y, Math.round(crop.height || 1)));
    studioSingleCrop.set({ x, y, width, height });
}

export function resetStudioSingleCrop() {
    const width = get(studioSingleOriginalW);
    const height = get(studioSingleOriginalH);
    if (!width || !height) return;
    studioSingleCrop.set({ x: 0, y: 0, width, height });
}

export function addStudioBatchFiles(paths: string[]) {
    studioBatchFiles.update((current) => addUnique(current, paths));
}

export function removeStudioBatchFile(path: string) {
    studioBatchFiles.update((current) => current.filter((item) => item !== path));
    studioProgressByPath.update((current) => {
        const next = { ...current };
        delete next[path];
        return next;
    });
}

export function clearStudioBatch() {
    if (get(studioProcessing)) return;
    studioBatchFiles.set([]);
    studioResults.set([]);
    studioResultLabel.set(null);
    studioProgressByPath.set({});
    studioOperationId.set('');
    studioCancelling.set(false);
}

function progressFor(paths: string[], operationId: string, tool = 'resizer') {
    studioProgressByPath.set(initialProgress(paths, operationId, tool));
}

async function runStudio(paths: string[], options: Record<string, unknown>, modeLabel: 'single' | 'batch') {
    if (!paths.length || get(studioProcessing)) return;

    const operationId = newOperationId('img-studio');
    const started = Date.now();
    studioOperationId.set(operationId);
    studioOperationKind.set('resize');
    studioResults.set([]);
    studioResultLabel.set(null);
    progressFor(paths, operationId);
    studioProcessing.set(true);
    studioCancelling.set(false);
    await setBusy(true);
    const stopBusy = reportBusy('image-studio', t('store.imageResizer.busy', { values: { count: paths.length } }));

    try {
        const results = await invoke<ImageResult[]>('resize_images', {
            options: {
                paths,
                output_dir: get(studioOutputDir),
                suffix: get(studioSuffix),
                output_format: get(studioOutputFormat),
                filter: get(studioFilter),
                quality: get(studioQuality),
                operation_id: operationId,
                ...options,
            },
        });
        studioResults.set(results);

        const success = results.filter((r) => r.success).length;
        const failed = results.filter((r) => !r.success && r.error !== 'Cancelled').length;
        const elapsed = Math.max(1, Math.round((Date.now() - started) / 1000));
        const cancelled = results.some((r) => r.error === 'Cancelled');

        if (cancelled) {
            reportOperationOutcome({
                toolId: 'image-studio',
                kind: 'cancelled',
                notifyTitle: t('store.imageResizer.cancelledTitle'),
                notifyMessage: t('store.imageResizer.cancelledMessage', { values: { done: success, total: paths.length } }),
                toastMessage: t('store.imageResizer.cancelledToast', { values: { done: success, total: paths.length } }),
                activitySummary: t('store.imageResizer.cancelledActivity'),
                activityDetails: t('store.imageResizer.cancelledDetails', { values: { done: success, total: paths.length, mode: modeLabel } }),
            });
        } else if (failed === 0) {
            reportOperationOutcome({
                toolId: 'image-studio',
                kind: 'success',
                notifyTitle: modeLabel === 'single' ? t('store.imageResizer.doneTitleSingle') : t('store.imageResizer.doneTitleBatch', { values: { count: success } }),
                notifyMessage: t('store.imageResizer.doneMessage', { values: { elapsed } }),
                toastMessage: modeLabel === 'single' ? t('store.imageResizer.doneToastSingle') : t('store.imageResizer.doneToastBatch', { values: { count: success } }),
                activitySummary: modeLabel === 'single' ? t('store.imageResizer.doneActivitySingle') : t('store.imageResizer.doneActivityBatch', { values: { count: success } }),
            });
        } else if (success === 0) {
            reportOperationOutcome({
                toolId: 'image-studio',
                kind: 'failed',
                notifyTitle: t('store.imageResizer.failedTitle'),
                notifyMessage: t('store.imageResizer.failedMessage', { values: { count: failed } }),
                toastMessage: t('store.imageResizer.failedToast'),
                activitySummary: t('store.imageResizer.failedActivity'),
                activityDetails: t('store.imageResizer.failedDetails', { values: { count: failed, mode: modeLabel } }),
            });
        } else {
            reportOperationOutcome({
                toolId: 'image-studio',
                kind: 'partial',
                notifyTitle: t('store.imageResizer.partialTitle', { values: { success, failed } }),
                notifyMessage: t('store.imageResizer.partialMessage', { values: { elapsed } }),
                toastMessage: t('store.imageResizer.partialToast', { values: { success, failed } }),
                activitySummary: t('store.imageResizer.partialActivity'),
                activityDetails: t('store.imageResizer.partialDetails', { values: { success, failed, mode: modeLabel } }),
            });
        }
    } catch (error) {
        const msg = String(error);
        reportOperationOutcome({
            toolId: 'image-studio',
            kind: 'failed',
            notifyTitle: t('store.imageResizer.failedTitle'),
            notifyMessage: msg,
            toastMessage: msg,
            activitySummary: t('store.imageResizer.failedActivity'),
            activityDetails: msg.slice(0, 200),
        });
    } finally {
        studioProcessing.set(false);
        studioCancelling.set(false);
        studioOperationKind.set(null);
        await setBusy(false);
        stopBusy();
    }
}

export async function runStudioSingle() {
    const path = get(studioSinglePath);
    const width = get(studioSingleWidth);
    const height = get(studioSingleHeight);
    if (!path || !width || !height) return;
    await runStudio([path], {
        mode: 'px',
        width,
        height,
        percent: 100,
        longest_side: width,
        shortest_side: Math.min(width, height),
        preserve_aspect: get(studioSinglePreserveAspect),
    }, 'single');
}

type StudioSingleAction = Exclude<ImageStudioOperation, 'resize' | null>;

async function runStudioSingleAction(
    kind: StudioSingleAction,
    command: 'crop_images' | 'remove_image_background',
    extraOptions: Record<string, unknown>,
) {
    const path = get(studioSinglePath);
    if (!path || get(studioProcessing)) return;

    const background = kind === 'background';
    const actionName = background ? 'Background removal' : 'Crop';
    const operationId = newOperationId(background ? 'img-background' : 'img-crop');
    studioOperationId.set(operationId);
    studioOperationKind.set(kind);
    studioResults.set([]);
    studioResultLabel.set(null);
    progressFor([path], operationId, kind);
    studioProcessing.set(true);
    studioCancelling.set(false);
    await setBusy(true);
    const stopBusy = reportBusy('image-studio', background ? 'Removing image background…' : 'Cropping image…');

    try {
        const results = await invoke<ImageResult[]>(command, {
            options: {
                paths: [path],
                output_dir: get(studioOutputDir),
                suffix: get(studioSuffix),
                operation_id: operationId,
                ...extraOptions,
            },
        });
        studioResults.set(results);

        const failed = results.filter((result) => !result.success && result.error !== 'Cancelled').length;
        const cancelled = results.some((result) => result.error === 'Cancelled');
        if (cancelled) {
            reportOperationOutcome({
                toolId: 'image-studio',
                kind: 'cancelled',
                notifyTitle: `${actionName} cancelled`,
                notifyMessage: 'The original image was left untouched.',
                toastMessage: `${actionName} cancelled.`,
                activitySummary: `${actionName} cancelled`,
            });
        } else if (failed === 0) {
            studioResultLabel.set(background ? 'Background removed · PNG' : 'Crop applied');
            reportOperationOutcome({
                toolId: 'image-studio',
                kind: 'success',
                notifyTitle: background ? 'Background removed' : 'Crop complete',
                notifyMessage: background ? 'Saved a PNG with a transparent background.' : 'Saved the cropped image.',
                toastMessage: background ? 'Background removed.' : 'Crop complete.',
                activitySummary: background ? 'Background removed' : 'Crop complete',
            });
        } else {
            reportOperationOutcome({
                toolId: 'image-studio',
                kind: 'failed',
                notifyTitle: `${actionName} failed`,
                notifyMessage: results.find((result) => !result.success)?.error ?? `Could not complete ${actionName.toLowerCase()}.`,
                toastMessage: `${actionName} failed.`,
                activitySummary: `${actionName} failed`,
            });
        }
    } catch (error) {
        const message = String(error);
        reportOperationOutcome({
            toolId: 'image-studio',
            kind: 'failed',
            notifyTitle: `${actionName} failed`,
            notifyMessage: message,
            toastMessage: message,
            activitySummary: `${actionName} failed`,
            activityDetails: message.slice(0, 200),
        });
    } finally {
        studioProcessing.set(false);
        studioCancelling.set(false);
        studioOperationKind.set(null);
        await setBusy(false);
        stopBusy();
    }
}

export async function runStudioSingleCrop() {
    const crop = get(studioSingleCrop);
    if (!crop || !get(studioSingleCropEnabled)) return;
    await runStudioSingleAction('crop', 'crop_images', {
        output_format: get(studioOutputFormat),
        x: crop.x,
        y: crop.y,
        width: crop.width,
        height: crop.height,
        quality: get(studioQuality),
    });
}

export async function removeStudioSingleBackground() {
    await runStudioSingleAction('background', 'remove_image_background', {
        quality: get(studioBackgroundQuality),
    });
}

export async function runStudioBatch() {
    const paths = get(studioBatchFiles);
    await runStudio(paths, {
        mode: get(studioMode),
        width: get(studioWidth),
        height: get(studioHeight),
        percent: get(studioPercent),
        longest_side: get(studioLongestSide),
        shortest_side: get(studioShortestSide),
        preserve_aspect: get(studioPreserveAspect),
    }, 'batch');
}

export async function cancelImageStudio() {
    if (!get(studioProcessing) || get(studioCancelling)) return;
    studioCancelling.set(true);
    await cancelImageOperation(get(studioOperationId));
}

export function currentStudioProgressForActivePath() {
    const path = get(studioSinglePath);
    if (!path) return null;
    return get(studioProgressByPath)[path] ?? null;
}
