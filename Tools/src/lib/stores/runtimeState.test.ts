import { beforeEach, describe, expect, it } from 'vitest';
import { get } from 'svelte/store';

import {
    cleanerRuntime,
    type CleanerRuntimeState,
} from './cleanerAnalyzer';
import {
    ocrCancelRequested,
    ocrCurrentOperationId,
    ocrError,
    ocrResultText,
    ocrRunning,
} from './ocrTool';
import {
    camBusy,
    camOcrCancelRequested,
    camOcrOperationId,
    camOcrText,
    camPhase,
} from './camScanner';

const idleCleanerRuntime: CleanerRuntimeState = {
    analyzing: false,
    cancelling: false,
    cleaning: false,
    operationId: null,
    progress: null,
    error: null,
};

describe('navigation-surviving operation state', () => {
    beforeEach(() => {
        cleanerRuntime.set({ ...idleCleanerRuntime });
        ocrRunning.set(false);
        ocrCancelRequested.set(false);
        ocrCurrentOperationId.set(null);
        ocrError.set(null);
        ocrResultText.set('');
        camBusy.set(false);
        camPhase.set(null);
        camOcrOperationId.set(null);
        camOcrCancelRequested.set(false);
        camOcrText.set('');
    });

    it('keeps Cleaner progress available to a remounted view', () => {
        cleanerRuntime.set({
            analyzing: true,
            cancelling: false,
            cleaning: false,
            operationId: 'cleaner-123',
            progress: {
                operationId: 'cleaner-123',
                stage: 'Scanning downloads',
                stageIndex: 2,
                stageTotal: 5,
                entriesScanned: 128,
                currentPath: 'C:\\Users\\neo\\Downloads',
            },
            error: null,
        });

        expect(get(cleanerRuntime)).toMatchObject({
            analyzing: true,
            operationId: 'cleaner-123',
            progress: expect.objectContaining({ entriesScanned: 128 }),
        });
    });

    it('keeps OCR cancellation and output state available to a remounted view', () => {
        ocrRunning.set(true);
        ocrCancelRequested.set(true);
        ocrCurrentOperationId.set('ocr-123');
        ocrResultText.set('Recognized text');

        expect({
            running: get(ocrRunning),
            cancelling: get(ocrCancelRequested),
            operationId: get(ocrCurrentOperationId),
            result: get(ocrResultText),
        }).toEqual({
            running: true,
            cancelling: true,
            operationId: 'ocr-123',
            result: 'Recognized text',
        });
    });

    it('keeps CamScanner OCR progress and extracted text available to a remounted view', () => {
        camBusy.set(true);
        camPhase.set('ocr');
        camOcrOperationId.set('camscan-123');
        camOcrCancelRequested.set(true);
        camOcrText.set('Scanned receipt');

        expect({
            busy: get(camBusy),
            phase: get(camPhase),
            operationId: get(camOcrOperationId),
            cancelling: get(camOcrCancelRequested),
            text: get(camOcrText),
        }).toEqual({
            busy: true,
            phase: 'ocr',
            operationId: 'camscan-123',
            cancelling: true,
            text: 'Scanned receipt',
        });
    });
});
