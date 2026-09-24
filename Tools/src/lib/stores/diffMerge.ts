// Folder Diff + 3-Way Merge — typed bindings over the Rust commands.

import { invoke } from '@tauri-apps/api/core';

export interface DiffEntry {
    path: string;
    status: 'added' | 'removed' | 'modified';
    leftSize: number | null;
    rightSize: number | null;
    isText: boolean;
}

export interface FolderDiff {
    entries: DiffEntry[];
    added: number;
    removed: number;
    modified: number;
    identical: number;
}

export interface MergeResult {
    merged: string;
    conflicts: boolean;
}

export const folderDiff = (left: string, right: string, operationId?: string) =>
    invoke<FolderDiff>('folder_diff', { left, right, operationId });

/** Signal a running folder-diff to abort. Wave 2.5 — the backend checks
 *  the flag at each WalkDir entry and before each byte-compare, so cancel
 *  latency is ~one-file's-worth on a slow disk. */
export const cancelDiffOperation = (operationId: string) =>
    invoke<void>('cancel_diff_operation', { operationId });

export const fileUnifiedDiff = (left: string, right: string) =>
    invoke<string>('file_unified_diff', { left, right });

export const threeWayMerge = (base: string, ours: string, theirs: string) =>
    invoke<MergeResult>('three_way_merge', { base, ours, theirs });

export const diffReadText = (path: string) => invoke<string>('diff_read_text', { path });

// Quality Pass Wave 1 / DM-4 (2026-05-29): folder sync.
export type SyncFileResult = {
    path: string;
    action: 'copied' | 'deleted' | 'skipped' | 'failed';
    error: string | null;
};
export type SyncFoldersResult = {
    copiedCount: number;
    deletedCount: number;
    failedCount: number;
    items: SyncFileResult[];
};
export type SyncMode =
    | 'copy_left_to_right'
    | 'copy_right_to_left'
    | 'mirror_left_to_right';
export const syncFolders = (
    left: string,
    right: string,
    mode: SyncMode,
    operationId?: string,
) =>
    invoke<SyncFoldersResult>('sync_folders', { left, right, mode, operationId });

export function formatSize(bytes: number | null): string {
    if (bytes === null || bytes === undefined) return '—';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
