// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

// The folder's listing comes from the engine, through the browser; a file
// WebView2 can't play is made playable by the browser (host:play.prepare).
const host = vi.hoisted(() => ({
    calls: [] as { cmd: string; args: Record<string, unknown> }[],
    prepared: null as unknown | ((args: any) => unknown),
}));
vi.mock('@tauri-apps/api/core', () => ({
    invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
        host.calls.push({ cmd, args });
        if (cmd === 'list_folder_children')
            return ['a.mp3', 'b.mp3', 'c.mp3'].map((name) => ({ name, isDir: false, size: 1, modifiedMs: 0 }));
        if (cmd === 'host:play.prepare')
            return typeof host.prepared === 'function' ? host.prepared(args) : host.prepared;
        return null;
    },
    convertFileSrc: (path: string) => `https://files.search/local?path=${encodeURIComponent(path)}`,
}));

import Page from './+page.svelte';

async function open(path: string) {
    location.hash = `#/play?path=${encodeURIComponent(path)}`;
    const page = mount(Page, { target: document.body });
    // The playlist (and whatever the browser prepares) arrives a turn later.
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
        host.calls = [];
        host.prepared = null;
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

    it('plays an MKV from the file the browser made playable, with its subtitles', async () => {
        host.prepared = {
            media: 'C:\\Cache\\k\\media.mp4',
            subtitles: [{ path: 'C:\\Cache\\k\\sub-0.vtt', label: 'English', language: 'en' }],
        };
        page = await open('C:\\Films\\film.mkv');

        expect(host.calls).toContainEqual({ cmd: 'host:play.prepare', args: { path: 'C:\\Films\\film.mkv', how: 'remux' } });
        const video = document.querySelector('video')!;
        expect(video.getAttribute('src')).toBe('https://files.search/local?path=C%3A%5CCache%5Ck%5Cmedia.mp4');
        const track = video.querySelector('track')!;
        expect(track.getAttribute('src')).toBe('https://files.search/local?path=C%3A%5CCache%5Ck%5Csub-0.vtt');
        expect(track.getAttribute('label')).toBe('English');
    });

    it('starts a direct WebM while the codec and subtitle check is still pending', async () => {
        host.prepared = () => new Promise(() => {});
        page = await open('C:\\Films\\portrait.webm');

        expect(host.calls).toContainEqual({ cmd: 'host:play.prepare', args: { path: 'C:\\Films\\portrait.webm', how: 'check' } });
        expect(document.querySelector('video')!.getAttribute('src')).toBe(
            'https://files.search/local?path=C%3A%5CFilms%5Cportrait.webm',
        );
    });

    it('remuxes an MP4 whose audio WebView2 would play silent (AC-3) before playing it', async () => {
        host.prepared = (args: { how: string }) =>
            args.how === 'check' ? { playable: false, subtitles: [] } : { media: 'C:\\Cache\\m\\media.mp4', subtitles: [] };
        page = await open('C:\\Films\\ac3.mp4');
        await new Promise((resolve) => setTimeout(resolve));
        flushSync();

        expect(host.calls.filter((c) => c.cmd === 'host:play.prepare').map((c) => c.args.how)).toEqual(['check', 'remux']);
        expect(document.querySelector('video')!.getAttribute('src')).toBe('https://files.search/local?path=C%3A%5CCache%5Cm%5Cmedia.mp4');
    });

    it('without the FFmpeg pack, offers it instead of failing', async () => {
        host.prepared = { needsPack: true };
        page = await open('C:\\Films\\film.avi');

        expect(document.querySelector('video')!.getAttribute('src') ?? '').toBe('');
        const button = [...document.querySelectorAll('button')].find((b) => b.textContent?.includes('FFmpeg pack'))!;
        button.click();
        await tick();
        expect(host.calls.map((c) => c.cmd)).toContain('host:packs.open');
    });
});
