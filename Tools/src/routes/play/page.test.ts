// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

// The folder's listing comes from the engine, through the browser.
vi.mock('@tauri-apps/api/core', () => ({
    invoke: async (cmd: string) =>
        cmd === 'list_folder_children'
            ? ['a.mp3', 'b.mp3', 'c.mp3'].map((name) => ({ name, isDir: false, size: 1, modifiedMs: 0 }))
            : null,
    convertFileSrc: (path: string) => `https://files.search/local?path=${encodeURIComponent(path)}`,
}));

import Page from './+page.svelte';

async function open(path: string) {
    location.hash = `#/play?path=${encodeURIComponent(path)}`;
    const page = mount(Page, { target: document.body });
    // The playlist arrives a turn later.
    await tick();
    await new Promise((resolve) => setTimeout(resolve));
    flushSync();
    return page;
}

function press(key: string) {
    window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    flushSync();
}

describe('the play page', () => {
    let page: ReturnType<typeof mount> | undefined;
    afterEach(() => {
        if (page) unmount(page);
        page = undefined;
    });

    it('moves to the next and previous track in place: the address follows, the history does not grow', async () => {
        page = await open('C:\\Music\\a.mp3');
        const entries = history.length;
        press('n');
        expect(location.hash).toBe('#/play?path=C%3A%5CMusic%5Cb.mp3');
        press('n');
        expect(location.hash).toBe('#/play?path=C%3A%5CMusic%5Cc.mp3');
        press('p');
        expect(location.hash).toBe('#/play?path=C%3A%5CMusic%5Cb.mp3');
        expect(history.length).toBe(entries);
    });
});
