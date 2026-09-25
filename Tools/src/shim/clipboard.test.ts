import { afterEach, describe, expect, it, vi } from 'vitest';

const bridge = vi.hoisted(() => ({ inSearch: true, call: vi.fn(async () => null) }));
vi.mock('./bridge', () => bridge);

import { writeSecret } from './clipboard';

describe('writeSecret', () => {
    afterEach(() => {
        bridge.call.mockClear();
        vi.unstubAllGlobals();
    });

    it('in Search, hands the secret to the browser, never to navigator.clipboard', async () => {
        const writeText = vi.fn(async () => {});
        vi.stubGlobal('navigator', { clipboard: { writeText } });
        await writeSecret('correct horse battery staple');
        expect(bridge.call).toHaveBeenCalledWith('host:clipboard.secret', { text: 'correct horse battery staple' });
        expect(writeText).not.toHaveBeenCalled();
    });

    it('a refusal from the browser is the caller\'s to report', async () => {
        bridge.call.mockRejectedValueOnce('the clipboard was busy');
        await expect(writeSecret('x')).rejects.toBe('the clipboard was busy');
    });

    it('outside Search (pnpm dev), uses the page\'s own clipboard', async () => {
        bridge.inSearch = false;
        try {
            const writeText = vi.fn(async () => {});
            vi.stubGlobal('navigator', { clipboard: { writeText } });
            await writeSecret('dev only');
            expect(writeText).toHaveBeenCalledWith('dev only');
            expect(bridge.call).not.toHaveBeenCalled();
        } finally {
            bridge.inSearch = true;
        }
    });
});
