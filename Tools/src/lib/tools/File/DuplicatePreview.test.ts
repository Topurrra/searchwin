// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, expect, it, vi } from 'vitest';

const host = vi.hoisted(() => ({ calls: [] as { cmd: string; args: unknown }[] }));
vi.mock('@tauri-apps/api/core', () => ({
    invoke: async (cmd: string, args: unknown) => {
        host.calls.push({ cmd, args });
        if (cmd === 'preview_duplicate_file') return {
            kind: 'binary', meta: { path: (args as { path: string }).path, fileName: 'setup.exe', extension: 'exe', sizeBytes: 10, modifiedMs: null }, hexPreview: '',
        };
        return null;
    },
    convertFileSrc: (path: string) => path,
}));
vi.mock('@tauri-apps/plugin-opener', () => ({
    revealItemInDir: async (path: string) => { host.calls.push({ cmd: 'revealItemInDir', args: { path } }); },
}));

import DuplicatePreview from './DuplicatePreview.svelte';

let page: ReturnType<typeof mount> | undefined;
afterEach(() => {
    if (page) unmount(page);
    page = undefined;
    document.body.innerHTML = '';
    host.calls = [];
});

it('opens through the browser and reveals through Explorer', async () => {
    const path = 'C:\\Users\\me\\Downloads\\setup.exe';
    page = mount(DuplicatePreview, { target: document.body, props: { path } });
    await tick();
    await new Promise((resolve) => setTimeout(resolve));
    flushSync();
    host.calls = [];

    document.querySelector<HTMLButtonElement>('button[title="Open file"]')!.click();
    document.querySelector<HTMLButtonElement>('button[title="Reveal in folder"]')!.click();
    await tick();
    await vi.waitFor(() => expect(host.calls).toHaveLength(2));
    expect(host.calls).toEqual([
        { cmd: 'host:open.file', args: { path } },
        { cmd: 'revealItemInDir', args: { path } },
    ]);
});
