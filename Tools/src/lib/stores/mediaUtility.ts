import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { get, writable } from 'svelte/store';
import { toast } from '$lib/stores/toasts';

export type MediaUtilityMode = 'extract' | 'compress';

type MediaUtilityResult = {
    outputPath: string;
    originalBytes: number;
    outputBytes: number;
};

export type MediaUtilityProgress = {
    operation_id: string;
    stage: 'starting' | 'processing' | 'completed' | 'cancelled' | 'error';
    progress: number | null;
    processed_seconds: number;
    duration_seconds: number | null;
};

/** Runtime-only state so an FFmpeg job remains visible after tool navigation. */
export const mediaInputPath = writable('');
export const mediaOutputPath = writable('');
export const mediaMode = writable<MediaUtilityMode>('extract');
export const mediaTargetMegabytes = writable(25);
export const mediaProcessing = writable(false);
export const mediaCancelling = writable(false);
export const mediaOperationId = writable<string | null>(null);
export const mediaProgress = writable<MediaUtilityProgress | null>(null);
export const mediaError = writable<string | null>(null);
export const mediaCompletedPath = writable<string | null>(null);

let progressUnlisten: UnlistenFn | null = null;
let progressListenerStarting = false;

export async function initMediaUtility(): Promise<void> {
    if (progressUnlisten || progressListenerStarting) return;
    progressListenerStarting = true;
    try {
        progressUnlisten = await listen<MediaUtilityProgress>('media-progress', (event) => {
            if (event.payload.operation_id === get(mediaOperationId)) {
                mediaProgress.set(event.payload);
            }
        });
    } finally {
        progressListenerStarting = false;
    }
}

export function setMediaInput(path: string | null): void {
    if (!path || get(mediaProcessing)) return;
    mediaInputPath.set(path);
    mediaOutputPath.set('');
    mediaCompletedPath.set(null);
    mediaError.set(null);
}

export function setMediaMode(mode: MediaUtilityMode): void {
    if (get(mediaProcessing)) return;
    mediaMode.set(mode);
    mediaOutputPath.set('');
    mediaCompletedPath.set(null);
    mediaError.set(null);
}

export function setMediaOutput(path: string): void {
    if (get(mediaProcessing)) return;
    mediaOutputPath.set(path);
    mediaCompletedPath.set(null);
    mediaError.set(null);
}

export async function runMediaOperation(): Promise<MediaUtilityResult | null> {
    if (get(mediaProcessing)) return null;

    const inputPath = get(mediaInputPath);
    const outputPath = get(mediaOutputPath);
    const mode = get(mediaMode);
    const targetMegabytes = get(mediaTargetMegabytes);
    if (!inputPath || !outputPath) return null;

    const operationId = crypto.randomUUID();
    mediaProcessing.set(true);
    mediaCancelling.set(false);
    mediaOperationId.set(operationId);
    mediaProgress.set({
        operation_id: operationId,
        stage: 'starting',
        progress: null,
        processed_seconds: 0,
        duration_seconds: null,
    });
    mediaError.set(null);
    mediaCompletedPath.set(null);
    try {
        const result = mode === 'extract'
            ? await invoke<MediaUtilityResult>('media_extract_audio', { operationId, inputPath, outputPath })
            : await invoke<MediaUtilityResult>('media_compress_video', { operationId, inputPath, outputPath, targetMegabytes });
        mediaCompletedPath.set(result.outputPath || outputPath);
        toast(mode === 'extract' ? 'MP3 extracted.' : 'Video compressed.', 'success');
        return result;
    } catch (cause) {
        const message = String(cause);
        if (message.toLowerCase().includes('cancel')) {
            toast('Media operation cancelled.', 'info');
        } else {
            mediaError.set(message);
        }
        return null;
    } finally {
        mediaProcessing.set(false);
        mediaCancelling.set(false);
        mediaOperationId.set(null);
    }
}

export async function cancelMediaOperation(): Promise<void> {
    const operationId = get(mediaOperationId);
    if (!operationId || !get(mediaProcessing)) return;

    mediaCancelling.set(true);
    try {
        await invoke('cancel_media_operation', { operationId });
        toast('Cancelling media operation…', 'info');
    } catch (cause) {
        mediaCancelling.set(false);
        mediaError.set(String(cause));
    }
}
