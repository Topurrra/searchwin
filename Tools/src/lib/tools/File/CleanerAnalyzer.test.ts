// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, expect, it, vi } from 'vitest';

const host = vi.hoisted(() => ({ calls: [] as { cmd: string; args: unknown }[] }));
vi.mock('@tauri-apps/api/core', () => ({
    invoke: async (cmd: string, args: unknown) => { host.calls.push({ cmd, args }); return null; },
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
vi.mock('@tauri-apps/plugin-opener', () => ({
    revealItemInDir: async (path: string) => { host.calls.push({ cmd: 'revealItemInDir', args: { path } }); },
}));

import CleanerAnalyzer from './CleanerAnalyzer.svelte';
import { cleanerDetailTab, cleanerReport } from '$lib/stores/cleanerAnalyzer';

let page: ReturnType<typeof mount> | undefined;
afterEach(() => {
    if (page) unmount(page);
    page = undefined;
    document.body.innerHTML = '';
    cleanerReport.set(null);
    cleanerDetailTab.set('caches');
    host.calls = [];
});

it('reveals an old file location without opening the file', async () => {
    const path = 'C:\\Users\\me\\Downloads\\setup.exe';
    cleanerDetailTab.set('old');
    cleanerReport.set({
        generatedAtMs: 0,
        cleanupTargets: [],
        oldLargeFiles: [{ path, fileName: 'setup.exe', bytes: 200, modifiedMs: 0, accessedMs: 0, daysSinceModified: 400, recommendation: 'Review' }],
        startupItems: [],
        diskSummaries: [],
        memorySummary: { totalBytes: null, availableBytes: null, usedPercent: null, source: '' },
        topUserDirs: [],
        fileTypeBreakdown: [],
        totalCleanableBytes: 0,
        tips: [],
    });
    page = mount(CleanerAnalyzer, { target: document.body });
    flushSync();
    document.querySelector<HTMLButtonElement>('button[aria-label="Open file location"]')!.click();
    await tick();
    expect(host.calls).toEqual([{ cmd: 'revealItemInDir', args: { path } }]);
});
