import { describe, expect, it } from 'vitest';
import { KEEP, PREFIX, after, folderOf, inFolder, keyOf, load, prune, resumable, save } from './positions';

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
        store.setItem(keyOf('C:\\a.mp3'), '42');
        expect(load(store, 'C:\\a.mp3')).toBe(42);
    });

    it('forgets a finished file rather than keeping it at 0', () => {
        const store = memory();
        save(store, 'C:\\a.mp3', 30, 100);
        save(store, 'C:\\a.mp3', 0, 100);
        expect(store.getItem(keyOf('C:\\a.mp3'))).toBeNull();
        save(store, 'C:\\b.mp3', 30, 100);
        save(store, 'C:\\b.mp3', 97, 100);
        expect(store.getItem(keyOf('C:\\b.mp3'))).toBeNull();
    });

    it('resumes only short of the end', () => {
        expect(resumable(6, 12)).toBe(true);
        expect(resumable(8, 12)).toBe(false);
        expect(resumable(0, 12)).toBe(false);
        expect(resumable(6, NaN)).toBe(true);
    });

    it('keeps the last KEEP files, dropping the least recently played', () => {
        const store = memory();
        store.setItem('something-else', 'kept');
        for (let i = 0; i < KEEP + 20; i++) save(store, `C:\\v\\${i}.mp4`, 10, 100, 1000 + i);
        const mine = [...Array(store.length).keys()].map((i) => store.key(i)!).filter((k) => k.startsWith(PREFIX));
        expect(mine.length).toBe(KEEP);
        expect(load(store, 'C:\\v\\0.mp4')).toBe(0);
        expect(load(store, `C:\\v\\${KEEP + 19}.mp4`)).toBe(10);
        expect(store.getItem('something-else')).toBe('kept');
        prune(store, 5);
        expect([...Array(store.length).keys()].filter((i) => store.key(i)!.startsWith(PREFIX)).length).toBe(5);
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
