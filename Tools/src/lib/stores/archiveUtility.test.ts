import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const { invokeMock, toastMock } = vi.hoisted(() => ({ invokeMock: vi.fn(), toastMock: vi.fn() }));

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('$lib/stores/toasts', () => ({ toast: toastMock }));

import {
    archiveCreateFormat,
    archiveCreateError,
    archiveCreateOutputPath,
    archiveCreatePaths,
    archiveCreateProcessing,
    archiveCreateResult,
    archiveExtractInfo,
    archiveExtractError,
    archiveExtractOutputDir,
    archiveExtractPath,
    inspectArchive,
    runArchiveCreate,
    runArchiveExtract,
} from './archiveUtility';

describe('Archive Utility runtime state', () => {
    beforeEach(() => {
        invokeMock.mockReset();
        toastMock.mockReset();
        archiveCreatePaths.set(['C:\\work']);
        archiveCreateOutputPath.set('C:\\work.7z');
        archiveCreateFormat.set('7z');
        archiveCreateProcessing.set(false);
        archiveCreateResult.set(null);
        archiveExtractPath.set('');
        archiveExtractInfo.set(null);
        archiveExtractOutputDir.set('');
        archiveExtractError.set(null);
    });

    it('keeps an active create operation in shared state until it completes', async () => {
        let resolveRun!: (result: { output_path: string; total_files: number; original_size: number; archive_size: number; elapsed_ms: number }) => void;
        invokeMock.mockReturnValueOnce(new Promise((resolve) => { resolveRun = resolve; }));

        const run = runArchiveCreate(null);
        expect(get(archiveCreateProcessing)).toBe(true);
        expect(invokeMock).toHaveBeenCalledWith('create_archive', {
            options: expect.objectContaining({
                paths: ['C:\\work'],
                output_path: 'C:\\work.7z',
                format: '7z',
            }),
        });

        resolveRun({ output_path: 'C:\\work.7z', total_files: 1, original_size: 2, archive_size: 1, elapsed_ms: 1 });
        await expect(run).resolves.toMatchObject({ output_path: 'C:\\work.7z' });
        expect(get(archiveCreateProcessing)).toBe(false);
        expect(get(archiveCreateResult)).toMatchObject({ total_files: 1 });
    });

    it('sends an optional password when inspecting an archive', async () => {
        invokeMock.mockResolvedValueOnce({
            format: '7z',
            entries: [],
            total_size: 0,
            has_encryption: false,
        });

        await inspectArchive('C:\\protected.7z', 'secret');

        expect(invokeMock).toHaveBeenCalledWith('inspect_archive', {
            options: { path: 'C:\\protected.7z', password: 'secret' },
        });
    });

    it('reports a cancelled archive creation as information instead of an error', async () => {
        invokeMock.mockRejectedValueOnce(new Error('Cancelled by user'));

        await expect(runArchiveCreate(null)).resolves.toBeNull();

        expect(get(archiveCreateError)).toBeNull();
        expect(toastMock).toHaveBeenCalledWith('Archive creation cancelled.', 'info');
    });

    it('tells users that a cancelled extraction leaves already extracted files', async () => {
        archiveExtractPath.set('C:\\archive.zip');
        archiveExtractOutputDir.set('C:\\output');
        invokeMock.mockRejectedValueOnce(new Error('Cancelled by user'));

        await expect(runArchiveExtract(null)).resolves.toBeNull();

        expect(get(archiveExtractError)).toBeNull();
        expect(toastMock).toHaveBeenCalledWith(
            'Extraction cancelled. Files already extracted remain in the destination.',
            'info',
        );
    });
});
