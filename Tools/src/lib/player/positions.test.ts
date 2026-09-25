import { describe, expect, it, vi } from 'vitest';
import { after, folderOf, inFolder, load, save } from './positions';

function memory(): Storage {
    const map = new Map<string, string>();
    return {
        get length() {
            return map.size;
        },
        key: (i: number) => [...map.keys()][i] ?? null,
        getItem: (k: string) => map.get(k) ?? null,
        setItem: (k: string, v: string) => void map.set(k, String(v)),
        removeItem: (k: string) => void map.delete(k),
        clear: () => map.clear(),
    };
}

describe('remembered positions', () => {
    it('saves and loads by path, whatever the slashes and case', () => {
        const store = memory();
        save(store, 'C:\\Music\\Song.wav', 6.7, 12);
        expect(load(store, 'c:/music/song.wav')).toBe(6);
    });

    it('reads a value saved the old way (plain seconds)', () => {
        const store = memory();
        store.setItem('search-player:position:c:\\a.mp3', '42');
        expect(load(store, 'C:\\a.mp3')).toBe(42);
    });

    it('forgets a finished file (at its start or in its last 5 seconds) rather than keeping it at 0', () => {
        const store = memory();
        save(store, 'C:\\a.mp3', 30, 100);
        save(store, 'C:\\a.mp3', 0, 100);
        save(store, 'C:\\b.mp3', 30, 100);
        save(store, 'C:\\b.mp3', 97, 100);
        expect(store.length).toBe(0);
        // Its length not known yet: kept.
        save(store, 'C:\\c.mp3', 6, NaN);
        expect(load(store, 'C:\\c.mp3')).toBe(6);
    });

    it('keeps the 200 most recently played files', () => {
        vi.useFakeTimers();
        try {
            const store = memory();
            store.setItem('something-else', 'kept');
            for (let i = 0; i < 220; i++) {
                vi.setSystemTime(1000 + i);
                save(store, `C:\\v\\${i}.mp4`, 10, 100);
            }
            expect(store.length).toBe(201);
            expect(load(store, 'C:\\v\\19.mp4')).toBe(0);
            expect(load(store, 'C:\\v\\20.mp4')).toBe(10);
            expect(store.getItem('something-else')).toBe('kept');
        } finally {
            vi.useRealTimers();
        }
    });
});

describe('the playlist', () => {
    it("a drive's root keeps its backslash", () => {
        expect(folderOf('E:\\song.mp3')).toBe('E:\\');
        expect(folderOf('E:/song.mp3')).toBe('E:/');
        expect(folderOf('C:\\Music\\a.mp3')).toBe('C:\\Music');
        expect(folderOf('song.mp3')).toBe('');
        expect(inFolder('E:\\', 'b.mp3')).toBe('E:\\b.mp3');
        expect(inFolder('C:\\Music', 'b.mp3')).toBe('C:\\Music\\b.mp3');
    });

    it('stops after the last track', () => {
        expect(after(0, 3)).toBe(1);
        expect(after(2, 3)).toBe(-1);
        expect(after(-1, 3)).toBe(-1);
        expect(after(0, 1)).toBe(-1);
    });
});
