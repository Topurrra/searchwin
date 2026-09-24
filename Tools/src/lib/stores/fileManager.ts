/*
  fileManager.ts — module-level store for the dual-pane File Manager.

  Why module-level (not component-local $state): the Category Workspace
  wraps each tool in a {#key} block that UNMOUNTS the component on every
  nav change (see project_nav_state_and_freeze_pass.md). Component-local
  state would be wiped each time the user tabs away and back. By keeping
  every pane's path / entries / selection / sort here as Svelte stores
  (the same pattern duplicateFinder.ts uses), the two panes survive the
  unmount and the user returns to exactly where they left off.

  Only EPHEMERAL UI (drag state, splitter drag, open dialogs) stays as
  $state inside FileManager.svelte — that's fine to lose on nav.
*/
import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import {
    Folder,
    File as FileIcon,
    FileText,
    FileCode,
    FileImage,
    FileVideo,
    FileMusic,
    FileArchive,
    FileSpreadsheet,
    FileLock,
    type Icon as LucideIcon,
} from '@lucide/svelte';

// ─── Shared command-contract types (mirror file_manager.rs, camelCase) ───

export interface FmEntry {
    name: string;
    path: string;
    isDir: boolean;
    size: number;
    modifiedMs: number;
    hidden: boolean;
}

export interface FmListing {
    entries: FmEntry[];
    truncated: boolean;
    total: number;
    parent: string | null;
}

export interface FmOpResult {
    copied: number;
    skipped: number;
    failed: number;
    errors: string[];
}

export interface FmDeleteResult {
    deleted: number;
    errors: string[];
}

export interface FmProgress {
    operationId: string;
    done: number;
    total: number;
    currentPath: string;
}

export type SortBy = 'name' | 'size' | 'modified';
export type PaneId = 'left' | 'right';

/** Everything that defines one pane. Kept as a plain object inside a
 *  writable so a single .update() can mutate any field. */
export interface PaneState {
    path: string;
    entries: FmEntry[];
    /** Set of selected entry full paths (selection survives nav). */
    selected: string[];
    /** The "focused" row index for keyboard nav (-1 = none). */
    focusIndex: number;
    sortBy: SortBy;
    showHidden: boolean;
    truncated: boolean;
    total: number;
    parent: string | null;
    loading: boolean;
    error: string | null;
}

function emptyPane(path: string): PaneState {
    return {
        path,
        entries: [],
        selected: [],
        focusIndex: -1,
        sortBy: 'name',
        showHidden: false,
        truncated: false,
        total: 0,
        parent: null,
        loading: false,
        error: null,
    };
}

/** First-run seed path. On first init we replace this with the first
 *  logical drive root (usually C:\) — a directory fm_list_dir can always
 *  read. Re-mounts keep wherever the user navigated to. */
const SEED = 'C:\\';

export const leftPane = writable<PaneState>(emptyPane(SEED));
export const rightPane = writable<PaneState>(emptyPane(SEED));

/** Which pane has focus — drives where Copy/Move sends FROM, and which
 *  pane the keyboard drives. */
export const activePane = writable<PaneId>('left');

/** The drive list for the switcher. Loaded once; shared by both panes. */
export const driveList = writable<string[]>([]);

// ─── Folder sizes (recursive, computed in the background) ────────────────
//
// Explorer never shows folder sizes; computing one means walking the whole
// subtree, so we must NEVER block the listing on it. After each load we fill
// folder sizes in asynchronously (bounded concurrency) and cancel the whole
// batch the moment the pane navigates away. Keyed by full path.

export interface FolderSizeState {
    status: 'pending' | 'done' | 'error';
    bytes: number;
}
export const folderSizes = writable<Record<string, FolderSizeState>>({});

/** Toggle (persisted, default ON): compute + show recursive folder sizes. */
function readShowSizes(): boolean {
    if (typeof localStorage === 'undefined') return true;
    return localStorage.getItem('kil.fm.folderSizes') !== '0';
}
export const showFolderSizes = writable<boolean>(readShowSizes());
showFolderSizes.subscribe((v) => {
    if (typeof localStorage === 'undefined') return;
    try {
        localStorage.setItem('kil.fm.folderSizes', v ? '1' : '0');
    } catch {
        /* storage unavailable — non-fatal */
    }
});

/** Bounded number of folders sized at once (disk-friendly). */
const SIZE_CONCURRENCY = 5;
/** Monotonic per-pane batch id: a new listing supersedes the old batch. */
const sizeSeq: Record<PaneId, number> = { left: 0, right: 0 };
/** The cancel token handed to the backend for the current batch, per pane. */
const sizeToken: Record<PaneId, string> = { left: '', right: '' };

async function cancelSizeBatch(pane: PaneId): Promise<void> {
    const tok = sizeToken[pane];
    if (!tok) return;
    try {
        await invoke('fm_cancel', { operationId: tok });
    } catch {
        /* best-effort — the walk also polls the flag */
    }
}

/** Compute recursive sizes for the pane's folders in the background. Cancels any
 *  in-flight batch for the pane first, then mints a fresh token so stale walks
 *  (from the previous folder) bail and never write into the new listing. */
export async function computeFolderSizes(pane: PaneId): Promise<void> {
    await cancelSizeBatch(pane);
    sizeSeq[pane] += 1;
    const seq = sizeSeq[pane];
    const token = `fm-size-${pane}-${seq}`;
    sizeToken[pane] = token;

    if (!get(showFolderSizes)) return;

    const folders = get(paneStore(pane)).entries.filter((e) => e.isDir);
    if (!folders.length) return;

    // Show spinners immediately for every folder in this listing.
    folderSizes.update((m) => {
        const next = { ...m };
        for (const f of folders) next[f.path] = { status: 'pending', bytes: 0 };
        return next;
    });

    let i = 0;
    const worker = async (): Promise<void> => {
        while (i < folders.length) {
            if (sizeSeq[pane] !== seq) return; // superseded by a newer listing
            const folder = folders[i++];
            try {
                const bytes = await invoke<number>('fm_dir_size', {
                    path: folder.path,
                    operationId: token,
                });
                if (sizeSeq[pane] !== seq) return;
                folderSizes.update((m) => ({ ...m, [folder.path]: { status: 'done', bytes } }));
            } catch {
                if (sizeSeq[pane] !== seq) return;
                folderSizes.update((m) => ({ ...m, [folder.path]: { status: 'error', bytes: 0 } }));
            }
        }
    };
    await Promise.all(
        Array.from({ length: Math.min(SIZE_CONCURRENCY, folders.length) }, worker),
    );

    // Once the batch is in, re-order by size if the pane is sorted that way (folder
    // sizes are client-side + async, so the backend couldn't sort by them).
    if (sizeSeq[pane] === seq && get(paneStore(pane)).sortBy === 'size') {
        resortPaneBySize(pane);
    }
}

/** Re-order a pane's entries by effective size — folders by their computed
 *  recursive size, files by their own size — biggest first. Only meaningful when
 *  `sortBy === 'size'`. Keeps everything index-consistent (selection is by path,
 *  focus is re-pointed to the same row) so clicks/keyboard stay correct. */
function resortPaneBySize(pane: PaneId): void {
    const sizes = get(folderSizes);
    const effective = (e: FmEntry): number => (e.isDir ? (sizes[e.path]?.bytes ?? 0) : e.size);
    paneStore(pane).update((p) => {
        if (p.sortBy !== 'size') return p;
        const focusedPath = p.entries[p.focusIndex]?.path ?? null;
        const entries = [...p.entries].sort((a, b) => {
            const diff = effective(b) - effective(a); // descending: biggest first
            return diff !== 0 ? diff : a.name.localeCompare(b.name);
        });
        const focusIndex = focusedPath
            ? entries.findIndex((e) => e.path === focusedPath)
            : p.focusIndex;
        return { ...p, entries, focusIndex };
    });
}

/** Flip the folder-size feature on/off. On → recompute both panes; off → stop
 *  any in-flight walks (computed values stay cached, just hidden). */
export async function setShowFolderSizes(value: boolean): Promise<void> {
    showFolderSizes.set(value);
    if (value) {
        await Promise.all([computeFolderSizes('left'), computeFolderSizes('right')]);
    } else {
        sizeSeq.left += 1;
        sizeSeq.right += 1;
        await Promise.all([cancelSizeBatch('left'), cancelSizeBatch('right')]);
    }
}

/** Live op progress (copy/move/backup). null = no op running. */
export const fmOpProgress = writable<FmProgress | null>(null);

/** True while any File Manager backend mutation is active. This stays
 * module-owned so a remount cannot re-enable destructive actions mid-run. */
export const fmBusy = writable(false);

/** The operation id of a running long op (copy/move/backup), or null.
 *  Component reads this to show a cancel button + progress bar. */
export const fmRunningOp = writable<string | null>(null);

/** Marks whether the very first init has run, so re-mounts (nav back to
 *  the tool) keep the user's navigated-to paths instead of re-seeding. */
let seeded = false;

function paneStore(pane: PaneId) {
    return pane === 'left' ? leftPane : rightPane;
}

/** Load drives, seed both panes at the first drive root on the very
 *  first mount only, then list both panes. On re-mounts this only
 *  refreshes the drive list and re-lists the (preserved) paths. */
export async function initFileManager(): Promise<void> {
    await loadDrives();
    if (!seeded) {
        seeded = true;
        const first = get(driveList)[0] ?? SEED;
        leftPane.update((p) => ({ ...p, path: first }));
        rightPane.update((p) => ({ ...p, path: first }));
    }
    await Promise.all([loadDir('left'), loadDir('right')]);
}

export async function loadDrives(): Promise<void> {
    try {
        const drives = await invoke<string[]>('list_logical_drives');
        driveList.set(drives);
    } catch {
        // Best-effort; the switcher just won't show buttons.
    }
}

/** (Re)read a pane's current directory from disk and store the listing.
 *  Preserves selection entries that still exist after the refresh. */
export async function loadDir(pane: PaneId): Promise<void> {
    const store = paneStore(pane);
    const snapshot = get(store);
    store.update((p) => ({ ...p, loading: true, error: null }));
    try {
        const listing = await invoke<FmListing>('fm_list_dir', {
            path: snapshot.path,
            showHidden: snapshot.showHidden,
            sortBy: snapshot.sortBy,
        });
        const validPaths = new Set(listing.entries.map((e) => e.path));
        store.update((p) => ({
            ...p,
            entries: listing.entries,
            truncated: listing.truncated,
            total: listing.total,
            parent: listing.parent,
            selected: p.selected.filter((s) => validPaths.has(s)),
            focusIndex: listing.entries.length ? Math.min(Math.max(p.focusIndex, 0), listing.entries.length - 1) : -1,
            loading: false,
        }));
        // Fill folder sizes in the background — never block the listing on it.
        void computeFolderSizes(pane);
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        store.update((p) => ({ ...p, loading: false, error: message }));
    }
}

/** Navigate a pane to a new directory (and load it). Clears selection. */
export async function navigateTo(pane: PaneId, path: string): Promise<void> {
    paneStore(pane).update((p) => ({ ...p, path, selected: [], focusIndex: -1 }));
    await loadDir(pane);
}

/** Go up to the parent directory if there is one. */
export async function goUp(pane: PaneId): Promise<void> {
    const parent = get(paneStore(pane)).parent;
    if (parent) await navigateTo(pane, parent);
}

export async function setSort(pane: PaneId, sortBy: SortBy): Promise<void> {
    paneStore(pane).update((p) => ({ ...p, sortBy }));
    await loadDir(pane);
}

export async function setShowHidden(pane: PaneId, showHidden: boolean): Promise<void> {
    paneStore(pane).update((p) => ({ ...p, showHidden }));
    await loadDir(pane);
}

export function setFocusIndex(pane: PaneId, index: number): void {
    paneStore(pane).update((p) => ({ ...p, focusIndex: index }));
}

// ─── Selection helpers (multi-select with Ctrl / Shift / plain click) ───

export function selectSingle(pane: PaneId, index: number): void {
    paneStore(pane).update((p) => {
        const entry = p.entries[index];
        return { ...p, selected: entry ? [entry.path] : [], focusIndex: index };
    });
}

export function toggleSelect(pane: PaneId, index: number): void {
    paneStore(pane).update((p) => {
        const entry = p.entries[index];
        if (!entry) return p;
        const has = p.selected.includes(entry.path);
        return {
            ...p,
            selected: has ? p.selected.filter((s) => s !== entry.path) : [...p.selected, entry.path],
            focusIndex: index,
        };
    });
}

/** Range-select from the current focus (anchor) to `index`, inclusive. */
export function selectRange(pane: PaneId, index: number): void {
    paneStore(pane).update((p) => {
        const anchor = p.focusIndex < 0 ? index : p.focusIndex;
        const lo = Math.min(anchor, index);
        const hi = Math.max(anchor, index);
        const range = p.entries.slice(lo, hi + 1).map((e) => e.path);
        return { ...p, selected: range, focusIndex: index };
    });
}

export function clearSelection(pane: PaneId): void {
    paneStore(pane).update((p) => ({ ...p, selected: [] }));
}

/** Resolve the selected entries (full FmEntry objects) for a pane. */
export function selectedEntries(pane: PaneId): FmEntry[] {
    const p = get(paneStore(pane));
    const set = new Set(p.selected);
    return p.entries.filter((e) => set.has(e.path));
}

export function otherPane(pane: PaneId): PaneId {
    return pane === 'left' ? 'right' : 'left';
}

// ─── Extension → icon map ───────────────────────────────────────────────

const EXT_ICONS: Record<string, typeof LucideIcon> = {
    // Text / docs
    txt: FileText, md: FileText, rtf: FileText, log: FileText,
    pdf: FileText, doc: FileText, docx: FileText, odt: FileText,
    // Code
    js: FileCode, ts: FileCode, jsx: FileCode, tsx: FileCode,
    json: FileCode, html: FileCode, htm: FileCode, css: FileCode,
    scss: FileCode, rs: FileCode, py: FileCode, go: FileCode,
    java: FileCode, c: FileCode, h: FileCode, cpp: FileCode,
    cs: FileCode, rb: FileCode, php: FileCode, sh: FileCode,
    ps1: FileCode, yml: FileCode, yaml: FileCode, toml: FileCode,
    xml: FileCode, svelte: FileCode, vue: FileCode, sql: FileCode,
    // Images
    png: FileImage, jpg: FileImage, jpeg: FileImage, gif: FileImage,
    bmp: FileImage, webp: FileImage, svg: FileImage, ico: FileImage,
    tif: FileImage, tiff: FileImage, heic: FileImage, avif: FileImage,
    // Video
    mp4: FileVideo, mkv: FileVideo, mov: FileVideo, avi: FileVideo,
    webm: FileVideo, wmv: FileVideo, flv: FileVideo, m4v: FileVideo,
    // Audio
    mp3: FileMusic, wav: FileMusic, flac: FileMusic, aac: FileMusic,
    ogg: FileMusic, m4a: FileMusic, wma: FileMusic,
    // Archives
    zip: FileArchive, rar: FileArchive, '7z': FileArchive, tar: FileArchive,
    gz: FileArchive, bz2: FileArchive, xz: FileArchive,
    // Spreadsheets
    xls: FileSpreadsheet, xlsx: FileSpreadsheet, csv: FileSpreadsheet, ods: FileSpreadsheet,
    // Locked / keys
    key: FileLock, pem: FileLock, p12: FileLock, pfx: FileLock,
    gpg: FileLock, asc: FileLock,
};

/** Pick a lucide icon for an entry: folders get Folder, files map by
 *  extension, unknown extensions get the generic File icon. */
export function iconForEntry(entry: FmEntry): typeof LucideIcon {
    if (entry.isDir) return Folder;
    const dot = entry.name.lastIndexOf('.');
    if (dot <= 0) return FileIcon;
    const ext = entry.name.slice(dot + 1).toLowerCase();
    return EXT_ICONS[ext] ?? FileIcon;
}

/** Human-readable byte size — same algorithm as duplicateFinder.formatBytes. */
export function formatBytes(bytes: number): string {
    if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let size = bytes;
    let index = 0;
    while (size >= 1024 && index < units.length - 1) {
        size /= 1024;
        index += 1;
    }
    return `${size.toFixed(size >= 10 || index === 0 ? 0 : 1)} ${units[index]}`;
}

/** Short relative-time / date string for the modified column. */
export function formatModified(ms: number): string {
    if (!Number.isFinite(ms) || ms <= 0) return '—';
    const d = new Date(ms);
    if (Number.isNaN(d.getTime())) return '—';
    return d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}
