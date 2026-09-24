import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const { encryptFileMock, decryptFileMock } = vi.hoisted(() => ({
    encryptFileMock: vi.fn(),
    decryptFileMock: vi.fn(),
}));

vi.mock('./crypto', () => ({
    encryptFile: encryptFileMock,
    decryptFile: decryptFileMock,
}));

import {
    cryptoBusy,
    cryptoCancelRequested,
    cryptoCurrentOpId,
    cryptoFiles,
    cryptoMode,
    cryptoResults,
    cryptoTab,
    runCryptoFiles,
} from './cryptoState';

describe('runCryptoFiles', () => {
    beforeEach(() => {
        encryptFileMock.mockReset();
        decryptFileMock.mockReset();
        cryptoTab.set('files');
        cryptoMode.set('encrypt');
        cryptoBusy.set(false);
        cryptoFiles.set([]);
        cryptoResults.set([]);
        cryptoCurrentOpId.set(null);
        cryptoCancelRequested.set(false);
    });

    it('keeps a snapshot-backed batch visible in stores while it runs', async () => {
        let resolveFirst!: (result: { outputPath: string }) => void;
        const first = new Promise<{ outputPath: string }>((resolve) => {
            resolveFirst = resolve;
        });
        encryptFileMock.mockReturnValueOnce(first).mockResolvedValueOnce({ outputPath: 'b.txt.kenc' });

        const files = ['a.txt', 'b.txt'];
        const run = runCryptoFiles({ mode: 'encrypt', files, password: 'secret' });
        files.splice(1);

        expect(get(cryptoBusy)).toBe(true);
        expect(get(cryptoCurrentOpId)).toMatch(/^crypto-/);

        resolveFirst({ outputPath: 'a.txt.kenc' });

        await expect(run).resolves.toEqual({ cancelled: false, ok: 2, fail: 0 });
        expect(encryptFileMock).toHaveBeenNthCalledWith(1, expect.objectContaining({ inputPath: 'a.txt', password: 'secret' }));
        expect(encryptFileMock).toHaveBeenNthCalledWith(2, expect.objectContaining({ inputPath: 'b.txt', password: 'secret' }));
        expect(get(cryptoResults)).toEqual([
            { input: 'a.txt', output: 'a.txt.kenc' },
            { input: 'b.txt', output: 'b.txt.kenc' },
        ]);
        expect(get(cryptoBusy)).toBe(false);
        expect(get(cryptoCurrentOpId)).toBeNull();
    });
});
