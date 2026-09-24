import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const { invokeMock, listenMock } = vi.hoisted(() => ({ invokeMock: vi.fn(), listenMock: vi.fn() }));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }));
vi.mock('$lib/stores/toasts', () => ({ toast: vi.fn() }));

import {
    cancelMediaOperation,
    mediaCompletedPath,
    mediaCancelling,
    mediaError,
    mediaInputPath,
    mediaMode,
    mediaOperationId,
    mediaOutputPath,
    mediaProcessing,
    mediaProgress,
    mediaTargetMegabytes,
    initMediaUtility,
    runMediaOperation,
} from './mediaUtility';

describe('Media Utility runtime state', () => {
    let progressHandler: ((event: { payload: { operation_id: string; stage: 'processing'; progress: number; processed_seconds: number; duration_seconds: number } }) => void) | null = null;

    beforeAll(async () => {
        listenMock.mockImplementation(async (_event: string, handler: typeof progressHandler) => {
            progressHandler = handler;
            return vi.fn();
        });
        await initMediaUtility();
    });

    beforeEach(() => {
        invokeMock.mockReset();
        mediaInputPath.set('C:\\input.mp4');
        mediaOutputPath.set('C:\\output.mp4');
        mediaMode.set('compress');
        mediaTargetMegabytes.set(1024);
        mediaProcessing.set(false);
        mediaCancelling.set(false);
        mediaOperationId.set(null);
        mediaProgress.set(null);
        mediaError.set(null);
        mediaCompletedPath.set(null);
    });

    it('keeps an active compression and real progress visible across navigation', async () => {
        let resolveRun!: (result: { outputPath: string; originalBytes: number; outputBytes: number }) => void;
        invokeMock.mockImplementation((command: string) => {
            if (command === 'media_compress_video') {
                return new Promise((resolve) => { resolveRun = resolve; });
            }
            return Promise.resolve();
        });

        const run = runMediaOperation();
        const operationId = get(mediaOperationId);
        expect(get(mediaProcessing)).toBe(true);
        expect(invokeMock).toHaveBeenCalledWith('media_compress_video', {
            operationId,
            inputPath: 'C:\\input.mp4',
            outputPath: 'C:\\output.mp4',
            targetMegabytes: 1024,
        });
        progressHandler?.({
            payload: {
                operation_id: operationId!,
                stage: 'processing',
                progress: 42,
                processed_seconds: 25,
                duration_seconds: 60,
            },
        });
        expect(get(mediaProgress)).toMatchObject({ progress: 42, processed_seconds: 25 });

        resolveRun({ outputPath: 'C:\\output.mp4', originalBytes: 2, outputBytes: 1 });
        await expect(run).resolves.toMatchObject({ outputPath: 'C:\\output.mp4' });
        expect(get(mediaProcessing)).toBe(false);
        expect(get(mediaCompletedPath)).toBe('C:\\output.mp4');
    });

    it('sends the active operation id when cancelling', async () => {
        let rejectRun!: (cause: string) => void;
        invokeMock.mockImplementation((command: string) => {
            if (command === 'media_compress_video') {
                return new Promise((_resolve, reject) => { rejectRun = reject; });
            }
            return Promise.resolve();
        });

        const run = runMediaOperation();
        const operationId = get(mediaOperationId);
        await cancelMediaOperation();
        expect(get(mediaCancelling)).toBe(true);
        expect(invokeMock).toHaveBeenCalledWith('cancel_media_operation', { operationId });

        rejectRun('Cancelled by user');
        await expect(run).resolves.toBeNull();
        expect(get(mediaError)).toBeNull();
    });
});
