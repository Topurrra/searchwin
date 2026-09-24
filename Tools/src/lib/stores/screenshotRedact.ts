/**
 * Screenshot Redact — navigation-surviving editor state (2026-06-12).
 *
 * Same pattern as `diffMergeState.ts` / `duplicateFinder.ts`: the app
 * remounts the active tool via `{#key}` blocks, so leaving the tool and
 * returning wipes component-local `$state`. That meant a loaded image and
 * every hand-drawn redaction box (plus the undo/redo history) vanished on
 * nav — exactly the work a user would not want to lose. Lifting that state
 * to module scope makes it survive remounts.
 *
 * **What stays component-local** (deliberate, transient):
 * - in-progress drawing (`drawing`, `drawStart`, `currentRect`) — a live
 *   mouse gesture; meaningless across a remount.
 * - DOM refs (`imgEl`, `containerEl`) — recreated on mount.
 * - `imageNaturalWidth/Height` — re-derived from the <img> `onload` when the
 *   persisted `imageUrl` reloads.
 * - `saving` / `error` — transient per-export UI.
 */

import { writable } from 'svelte/store';

export type Mode = 'black' | 'white' | 'blur' | 'pixelate';

export type Region = {
    id: number;
    x: number;
    y: number;
    width: number;
    height: number;
    mode: Mode;
    strength: number;
};

/** Absolute path of the loaded source image, or null when none is loaded. */
export const redactSourcePath = writable<string | null>(null);
/** asset:// URL for the <img> src (from convertFileSrc), or null. */
export const redactImageUrl = writable<string | null>(null);
/** All drawn redaction regions, in natural-pixel coordinates. */
export const redactRegions = writable<Region[]>([]);
/** Monotonic id counter for new regions. Persisted so ids stay unique
 *  across navigation (a fresh component instance must not restart at 1
 *  and collide with regions already in the array). */
export const redactNextId = writable<number>(1);
/** Undo/redo snapshots of the regions array. Past = states we can undo
 *  INTO, future = states we can redo INTO. */
export const redactUndoStack = writable<Region[][]>([]);
export const redactRedoStack = writable<Region[][]>([]);
/** Active redaction mode + blur/pixelate strength for the NEXT drawn box. */
export const redactMode = writable<Mode>('black');
export const redactStrength = writable<number>(15);
/** Output path of the last successful export (the success banner). */
export const redactLastSaved = writable<string | null>(null);
/** True while the Rust export is active, so remounting cannot start it twice. */
export const redactSaving = writable(false);
/** Last export error, kept with the result surface across a remount. */
export const redactError = writable<string | null>(null);
