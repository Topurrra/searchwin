/**
 * Background-task continuity (Wave 3.2, 2026-05-26) — Folder Diff &
 * 3-Way Merge.
 *
 * Same pattern as `cryptoState.ts`: the Tauri `folder_diff` command
 * runs on a `spawn_blocking` thread and continues regardless of
 * frontend navigation, but the component owned its state locally so
 * navigating away and back showed a blank page even while the diff
 * was still computing in the background. Lifting these stores to
 * module scope makes the in-flight UI survive remounts.
 *
 * **What's NOT lifted** (deliberate):
 * - `message` (flash toast) — short-lived self-clearing UI element,
 *   pointless to persist.
 */

import { writable } from 'svelte/store';
import type { FolderDiff, MergeResult, SyncFoldersResult } from './diffMerge';

/** Folder Diff vs File Diff (Quality Pass Wave 1 / DM-3, 2026-05-28)
 *  vs 3-Way Merge tab. The 'file' value is new — older persisted
 *  values fall back to 'folder' via the type union narrowing in the
 *  component. */
export const diffTab = writable<'folder' | 'file' | 'merge'>('folder');
/** True while folder diff is running. Reads Cancel button visibility. */
export const diffBusy = writable(false);
/** True while the file-vs-file Rust diff is active. */
export const diffFileLoading = writable(false);
/** True while a folder sync operation is active. */
export const diffSyncing = writable(false);

// ── Folder diff tab ──
export const diffLeftDir = writable('');
export const diffRightDir = writable('');
export const diffResult = writable<FolderDiff | null>(null);
/** The currently-expanded entry's path (the one whose inline diff is
 *  shown). Null when the list is collapsed. */
export const diffExpanded = writable<string | null>(null);
/** The expanded entry's pre-computed unified-diff text. */
export const diffText = writable('');
/** True while the per-entry diff is being fetched. */
export const diffLoading = writable(false);
/** Result-list filter (added / removed / modified). */
export const diffFilter = writable<'all' | 'added' | 'removed' | 'modified'>('all');
/** Operation ID of the running diff — needed for the Cancel button. */
export const diffCurrentOpId = writable<string | null>(null);

// ── File-vs-file tab (Quality Pass Wave 1 / DM-3) ──
// The folder tab already persisted via this store; the file tab kept its
// state component-local, so a computed file-vs-file diff vanished on nav.
// Lifted here (2026-06-12) so it survives remounts like the folder tab.
export const diffFileLeft = writable('');
export const diffFileRight = writable('');
export const diffFileText = writable('');
/** 'unified' = classic patch view; 'split' = WinMerge two-column. Shared
 *  by the folder and file tabs — persisted so the user's preference and
 *  the rendered file diff survive navigation. */
export const diffViewMode = writable<'unified' | 'split'>('unified');
/** The last folder sync's summary, kept across a remount. */
export const diffSyncResult = writable<SyncFoldersResult | null>(null);

// ── 3-way merge tab ──
export const mergeBase = writable('');
export const mergeOurs = writable('');
export const mergeTheirs = writable('');
export const mergeResultState = writable<MergeResult | null>(null);
