// The player's remembered positions (localStorage on tools.search), and the
// path arithmetic its playlist needs. Kept apart from the page so it can be
// tested without a <video>.

export const PREFIX = 'search-player:position:';

/** How many files' positions are kept: the most recently played. */
export const KEEP = 200;

/** A file this close to its end is finished: it starts over next time. */
export const END_SLACK = 5;

type Store = Pick<Storage, 'getItem' | 'setItem' | 'removeItem' | 'key' | 'length'>;

export function keyOf(path: string): string {
    return PREFIX + path.replace(/\//g, '\\').toLowerCase();
}

/** `seconds|savedAtMs`; an older plain `seconds` still reads. */
function parse(raw: string | null): { seconds: number; at: number } {
    if (!raw) return { seconds: 0, at: 0 };
    const [s, at] = raw.split('|');
    const seconds = Number(s);
    return { seconds: Number.isFinite(seconds) && seconds > 0 ? seconds : 0, at: Number(at) || 0 };
}

/** Where to start `path`: its remembered position, or 0. */
export function load(store: Store, path: string): number {
    try {
        return parse(store.getItem(keyOf(path))).seconds;
    } catch {
        return 0;
    }
}

/** Whether `seconds` into a file `duration` long is worth resuming at. */
export function resumable(seconds: number, duration: number): boolean {
    return seconds > 0 && (!Number.isFinite(duration) || duration <= 0 || seconds < duration - END_SLACK);
}

/**
 * Remembers `seconds` into `path`. A finished file (at its start or within
 * its last seconds) is forgotten rather than kept at 0; past KEEP files the
 * least recently played are let go.
 */
export function save(store: Store, path: string, seconds: number, duration = NaN, now = Date.now()): void {
    try {
        if (!resumable(seconds, duration)) {
            store.removeItem(keyOf(path));
            return;
        }
        store.setItem(keyOf(path), `${Math.floor(seconds)}|${now}`);
        prune(store);
    } catch {
        /* private mode, quota, or storage disabled — just don't remember */
    }
}

/** Only the KEEP most recently saved positions stay. */
export function prune(store: Store, keep = KEEP): void {
    const mine: { key: string; at: number }[] = [];
    for (let i = 0; i < store.length; i++) {
        const key = store.key(i);
        if (key?.startsWith(PREFIX)) mine.push({ key, at: parse(store.getItem(key)).at });
    }
    if (mine.length <= keep) return;
    mine.sort((a, b) => b.at - a.at);
    for (const { key } of mine.slice(keep)) store.removeItem(key);
}

/** The folder a file is in: `E:\song.mp3` → `E:\` (a drive's root keeps its backslash). */
export function folderOf(path: string): string {
    const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
    if (i < 0) return '';
    if (i === 2 && /^[A-Za-z]:$/.test(path.slice(0, 2))) return path.slice(0, 3);
    if (i === 0) return path.slice(0, 1);
    return path.slice(0, i);
}

/** A file in `folder`, without doubling a root's backslash. */
export function inFolder(folder: string, name: string): string {
    return /[\\/]$/.test(folder) ? folder + name : `${folder}\\${name}`;
}

/** The track after `index`, or -1 at the end: the last one doesn't wrap around by itself. */
export function after(index: number, count: number): number {
    return index >= 0 && index < count - 1 ? index + 1 : -1;
}
