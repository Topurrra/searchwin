// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { deliver } from '../../../shim/bridge';

// The browser answers `host:` calls and the engine the rest; here, one
// engine whose FFmpeg comes and goes.
const host = vi.hoisted(() => ({
    calls: [] as string[],
    ffmpeg: false,
}));
vi.mock('@tauri-apps/api/core', () => ({
    invoke: async (cmd: string) => {
        host.calls.push(cmd);
        if (cmd === 'ffmpeg_status')
            return { available: host.ffmpeg, recorderAvailable: host.ffmpeg, ffprobeAvailable: host.ffmpeg, version: null };
        return null;
    },
}));

import MediaUtility from './MediaUtility.svelte';

async function settle() {
    await tick();
    await new Promise((resolve) => setTimeout(resolve));
    flushSync();
}

const packButton = () => [...document.querySelectorAll('button')].find((b) => b.textContent?.includes('Get the FFmpeg pack'));

describe('Media Utility without FFmpeg', () => {
    let page: ReturnType<typeof mount> | undefined;
    afterEach(() => {
        if (page) unmount(page);
        page = undefined;
        host.calls = [];
        host.ffmpeg = false;
    });

    it('offers the FFmpeg pack, and is ready once the pack arrives', async () => {
        page = mount(MediaUtility, { target: document.body });
        await settle();

        packButton()!.click();
        await settle();
        expect(host.calls).toContain('host:packs.open');

        host.ffmpeg = true;
        deliver('packs-changed', { id: 'ffmpeg', installed: '8.1.2' });
        await settle();
        expect(packButton()).toBeUndefined();
    });
});
