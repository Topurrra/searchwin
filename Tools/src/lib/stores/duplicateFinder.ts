import { derived, get, writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { t } from '$lib/i18n';
import { notify } from './notifications';
import { toast } from './toasts';
import { recordActivity } from './activityLog';
import { reportBusy } from './globalBusy';
import { reportOperationOutcome } from './operationOutcome';

export type DuplicateFileEntry = {
    path: string;
    file_name: string;
    size: number;
    modified_ms: number | null;
    extension: string;
};

export type DuplicateGroup = {
    hash: string;
    size: number;
    count: number;
    wasted_bytes: number;
    files: DuplicateFileEntry[];
};

export type DuplicateScanResult = {
    success: boolean;
    canceled: boolean;
    error: string | null;
    scanned_files: number;
    hashed_files: number;
    duplicate_groups: DuplicateGroup[];
    duplicate_files: number;
    reclaimable_bytes: number;
};

export type MoveFileResult = {
    source_path: string;
    output_path: string;
    success: boolean;
    error: string | null;
};

export type RecoveryTransactionSummary = {
    id: string;
    kind: string;
    created_at: number;
    entry_count: number;
    description: string;
};

type FileRecoveryState = {
    duplicate_move: RecoveryTransactionSummary | null;
};

type MoveDuplicatesResponse = {
    results: MoveFileResult[];
    recovery: RecoveryTransactionSummary | null;
};

export const duplicateRoots = writable<string[]>([]);
export const duplicateRecursive = writable(true);
export const duplicateIncludeHidden = writable(false);
export const duplicateFollowSymlinks = writable(false);
export const duplicateMinSizeKb = writable(1);
// Quality Pass Wave 1 / DF-3 (2026-05-28): file-type filter. Comma- or
// space-separated list of extensions (without leading dot). Empty = all
// files. Matches backend's DuplicateScanOptions.extension_filter.
export const duplicateExtensionFilter = writable('');
export const duplicateHashAlgorithm = writable<'blake3' | 'sha256'>('blake3');
// Quality Pass Wave 1 / DF-1 (2026-05-29): scan mode + sensitivity.
// "exact" runs the byte-identical pipeline (BLAKE3 / SHA-256 of full
// file bytes — catches only files with identical content). "perceptual"
// fingerprints images with dHash 8×8 and groups by Hamming distance —
// catches resized, recompressed, lightly cropped, and slightly recolored
// visual duplicates that byte-hashing misses. Backend defaults match.
//
// `duplicateSimilarityThreshold` is only consulted in perceptual mode.
// 0–18 bits is the sensible range out of 64 — 4 is "near-identical
// re-encode", 10 is "obvious resize / quality swap" (default), 18 is
// "loose visual match" (more false positives).
export type DuplicateHashStrategy = 'exact' | 'perceptual';
export const duplicateHashStrategy = writable<DuplicateHashStrategy>('exact');
export const duplicateSimilarityThreshold = writable<number>(10);
export const duplicatePerformance = writable<'balanced' | 'fast'>('balanced');
export const duplicateScanning = writable(false);
export const duplicateMoving = writable(false);
export const duplicateResult = writable<DuplicateScanResult | null>(null);
export const duplicateMoveResults = writable<MoveFileResult[]>([]);
export const duplicateRecovery = writable<RecoveryTransactionSummary | null>(null);
export const duplicateSelectedPaths = writable<string[]>([]);
export const duplicateExpandedGroups = writable<string[]>([]);
export const duplicatePreviewExpandedPaths = writable<string[]>([]);
export const duplicateDestinationDir = writable<string | null>(null);
export const duplicateLastJob = writable<string | null>(null);
export const duplicateCancelRequested = writable(false);
// Quality Pass Wave 1 user feedback (2026-05-29): live progress card
// during the scan. Backend emits the `duplicate-scan-progress` event
// every ~500 entries during the walk and every ~50 hashes during the
// hash phase; the listener (wired in scanDuplicateFiles) updates this
// store, and the UI binds to it so the user sees real numbers tick up
// instead of a static green spinner.
export type DuplicateProgress = {
    scanned: number;
    hashed: number;
    // DF-1 added the "perceptual" phase for the dHash pass in image
    // mode. The frontend treats "hashing" / "perceptual" the same for
    // the bar — both indicate the per-file CPU pass after the walk.
    phase: 'walking' | 'hashing' | 'perceptual' | 'idle';
};
export const duplicateProgress = writable<DuplicateProgress>({
    scanned: 0,
    hashed: 0,
    phase: 'idle',
});

// Quality Pass Wave 1 / DF-2 (2026-05-28): auto-pick "keep" heuristic.
// Re-sorts each group's files at read time so the chosen "keep" file
// lands first — the existing "first file kept by default" UI then
// surfaces the smart choice without any backend change.
//   first          = backend's original order (scan order, basically random)
//   newest         = most-recently-modified file stays
//   oldest         = least-recently-modified (often the "original")
//   shortest_path  = the file in the shallowest folder (often the canonical copy)
//   longest_name   = the file with the most descriptive filename
export type DuplicateKeepHeuristic =
    | 'first'
    | 'newest'
    | 'oldest'
    | 'shortest_path'
    | 'longest_name';
export const duplicateKeepHeuristic = writable<DuplicateKeepHeuristic>('first');

let recoveryInitialized = false;

/**
 * Quality Pass Wave 1 / DF-3 (2026-05-28): bulk-select all duplicate
 * files (non-"kept" ones) whose path contains the given substring.
 * Case-insensitive. The first file in each group is the "kept" file
 * per the auto-keep heuristic and is never auto-selected.
 *
 * Idiomatic competition: AllDup / dupeGuru let you select-all-in-folder
 * with a click. This is the same UX as one helper call.
 */
export function selectDuplicatesByLocation(
    needle: string,
    groups: { files: { path: string }[] }[],
): void {
    const lower = needle.trim().toLowerCase();
    if (!lower) return;
    const matched = new Set<string>();
    for (const group of groups) {
        // First file is the "kept" one — never auto-select it.
        for (let i = 1; i < group.files.length; i++) {
            const file = group.files[i];
            if (file.path.toLowerCase().includes(lower)) {
                matched.add(file.path);
            }
        }
    }
    duplicateSelectedPaths.update((current) => {
        const set = new Set(current);
        for (const path of matched) set.add(path);
        return Array.from(set);
    });
}

function applyKeepHeuristic<T extends { path: string; modified_ms?: number | null; file_name?: string }>(
    files: T[],
    heuristic: DuplicateKeepHeuristic,
): T[] {
    if (heuristic === 'first' || files.length <= 1) return files;
    const sorted = [...files];
    sorted.sort((a, b) => {
        switch (heuristic) {
            case 'newest':
                return (b.modified_ms ?? 0) - (a.modified_ms ?? 0);
            case 'oldest':
                return (a.modified_ms ?? 0) - (b.modified_ms ?? 0);
            case 'shortest_path':
                return a.path.length - b.path.length;
            case 'longest_name':
                return (b.file_name?.length ?? 0) - (a.file_name?.length ?? 0);
        }
    });
    return sorted;
}

export const duplicateGroups = derived(
    [duplicateResult, duplicateKeepHeuristic],
    ([$result, $heuristic]) => {
        const groups = $result?.duplicate_groups ?? [];
        if ($heuristic === 'first') return groups;
        return groups.map((group) => ({
            ...group,
            files: applyKeepHeuristic(group.files, $heuristic),
        }));
    },
);
export const duplicateSelectedBytes = derived(
    [duplicateGroups, duplicateSelectedPaths],
    ([$groups, $selectedPaths]) =>
        $groups
            .flatMap((group) => group.files)
            .filter((file) => $selectedPaths.includes(file.path))
            .reduce((sum, file) => sum + file.size, 0),
);

async function setBusy(busy: boolean) {
    try {
        await invoke('set_busy', { busy });
    } catch {
        // set_busy is best-effort; older builds/tools may not expose it.
    }
}

export function duplicateRootGuardMessage(path: string): string | null {
    const normalized = path.replaceAll('\\', '/').toLowerCase().replace(/\/+$/, '');

    if (!normalized || normalized === '/') {
        return t('store.duplicateFinder.rootBlockedRoot');
    }

    if (/^[a-z]:$/.test(normalized)) {
        return t('store.duplicateFinder.rootBlockedDrive');
    }

    const isSafeUserLocation =
        normalized.startsWith('c:/users/') ||
        normalized.startsWith('d:/users/') ||
        normalized.startsWith('e:/users/') ||
        normalized.startsWith('/users/') ||
        normalized.startsWith('/home/');

    // Desktop/Downloads/Documents/Pictures/project folders must be allowed.
    // This explicit allow prevents the guard from becoming too paranoid.
    if (isSafeUserLocation) return null;

    const blocked = new Set([
        'c:/windows',
        'c:/program files',
        'c:/program files (x86)',
        'c:/programdata',
        'c:/system volume information',
        '/system',
        '/library',
        '/applications',
        '/bin',
        '/sbin',
        '/usr',
        '/var',
        '/etc',
        '/dev',
        '/proc',
        '/sys',
        '/run',
        '/boot',
    ]);

    for (const blockedPath of blocked) {
        if (normalized === blockedPath || normalized.startsWith(`${blockedPath}/`)) {
            return t('store.duplicateFinder.rootBlockedSystem');
        }
    }

    return null;
}

export function addDuplicateRoots(paths: string[]) {
    const current = get(duplicateRoots);
    const known = new Set(current);
    const accepted: string[] = [];
    const blocked: string[] = [];

    for (const path of paths) {
        if (known.has(path)) continue;
        const warning = duplicateRootGuardMessage(path);
        if (warning) blocked.push(path);
        else accepted.push(path);
    }

    if (blocked.length) {
        toast(t('store.duplicateFinder.sourcesBlockedToast', { values: { count: blocked.length } }), 'error');
        notify({
            level: 'warning',
            title: t('store.duplicateFinder.sourceBlockedTitle'),
            message: t('store.duplicateFinder.sourceBlockedMessage'),
            toolId: 'duplicate-finder',
        });
    }

    if (!accepted.length) return;

    duplicateRoots.set([...current, ...accepted]);
    duplicateResult.set(null);
    duplicateMoveResults.set([]);
    duplicateSelectedPaths.set([]);
    duplicateExpandedGroups.set([]);
    duplicatePreviewExpandedPaths.set([]);
}

export function removeDuplicateRoot(path: string) {
    duplicateRoots.update((roots) => roots.filter((root) => root !== path));
    duplicateResult.set(null);
    duplicateMoveResults.set([]);
    duplicateSelectedPaths.set([]);
    duplicateExpandedGroups.set([]);
    duplicatePreviewExpandedPaths.set([]);
}

export function clearDuplicateInputs() {
    duplicateRoots.set([]);
    duplicateResult.set(null);
    duplicateMoveResults.set([]);
    duplicateSelectedPaths.set([]);
    duplicateExpandedGroups.set([]);
    duplicatePreviewExpandedPaths.set([]);
}

export async function initDuplicateRecovery(): Promise<void> {
    if (recoveryInitialized) return;
    recoveryInitialized = true;
    await refreshDuplicateRecovery();
}

export async function refreshDuplicateRecovery(): Promise<void> {
    try {
        const state = await invoke<FileRecoveryState>('get_file_recovery_state');
        duplicateRecovery.set(state.duplicate_move ?? null);
    } catch (error) {
        console.warn('Could not load duplicate recovery state:', error);
    }
}

export function toggleDuplicateSelection(path: string) {
    duplicateSelectedPaths.update((selected) =>
        selected.includes(path) ? selected.filter((item) => item !== path) : [...selected, path],
    );
}

export function setDuplicateGroupSelection(group: DuplicateGroup, checked: boolean) {
    const paths = group.files.slice(1).map((file) => file.path);
    duplicateSelectedPaths.update((selected) => {
        const set = new Set(selected);
        for (const path of paths) {
            if (checked) set.add(path);
            else set.delete(path);
        }
        return [...set];
    });
}

export function toggleDuplicateGroup(hash: string) {
    duplicateExpandedGroups.update((groups) =>
        groups.includes(hash) ? groups.filter((item) => item !== hash) : [...groups, hash],
    );
}

export async function cancelDuplicateScan(): Promise<void> {
    if (!get(duplicateScanning)) return;

    duplicateCancelRequested.set(true);
    duplicateLastJob.set(t('store.duplicateFinder.cancellingJob'));

    try {
        await invoke('cancel_duplicate_scan');
        toast(t('store.duplicateFinder.cancellingToast'), 'info');
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        notify({ level: 'error', title: t('store.duplicateFinder.cancelFailedTitle'), message, toolId: 'duplicate-finder' });
        toast(message, 'error');
    }
}

export async function scanDuplicateFiles(): Promise<void> {
    if (get(duplicateScanning)) return toast(t('store.duplicateFinder.scanRunning'), 'info');
    if (get(duplicateMoving)) return toast(t('store.duplicateFinder.moveWaitFinish'), 'info');

    const roots = get(duplicateRoots);
    if (!roots.length) return toast(t('store.duplicateFinder.needRoots'), 'error');

    duplicateScanning.set(true);
    duplicateCancelRequested.set(false);
    duplicateLastJob.set(t('store.duplicateFinder.scanningJob'));
    duplicateResult.set(null);
    duplicateMoveResults.set([]);
    duplicateSelectedPaths.set([]);
    duplicateExpandedGroups.set([]);
    duplicatePreviewExpandedPaths.set([]);
    duplicateProgress.set({ scanned: 0, hashed: 0, phase: 'walking' });
    await setBusy(true);
    const stopBusy = reportBusy('duplicate-finder', t('store.duplicateFinder.scanBusy'));

    // Quality Pass Wave 1 user feedback (2026-05-29): subscribe to
    // backend progress events for the duration of the scan. The
    // listener fires every ~500 walked entries / ~50 hashes, so the
    // UI can show counters ticking instead of a static spinner.
    let unlistenProgress: UnlistenFn | null = null;
    try {
        unlistenProgress = await listen<DuplicateProgress>(
            'duplicate-scan-progress',
            (event) => {
                duplicateProgress.set({
                    scanned: event.payload.scanned ?? 0,
                    hashed: event.payload.hashed ?? 0,
                    phase: event.payload.phase ?? 'walking',
                });
            },
        );
    } catch {
        // Live progress is best-effort; the scan still works without it.
    }

    const startedAt = Date.now();

    try {
        const response = await invoke<DuplicateScanResult>('find_duplicate_files', {
            options: {
                roots,
                recursive: get(duplicateRecursive),
                include_hidden: get(duplicateIncludeHidden),
                follow_symlinks: get(duplicateFollowSymlinks),
                min_size: Math.max(0, Math.round(get(duplicateMinSizeKb) * 1024)),
                hash_algorithm: get(duplicateHashAlgorithm),
                max_threads: get(duplicatePerformance) === 'fast' ? 0 : 4,
                extension_filter: get(duplicateExtensionFilter),
                // DF-1: perceptual mode + Hamming-distance threshold.
                hash_strategy: get(duplicateHashStrategy),
                similarity_threshold: Math.max(0, Math.min(64, Math.round(get(duplicateSimilarityThreshold)))),
            },
        });

        duplicateResult.set(response);
        const elapsed = Math.max(1, Math.round((Date.now() - startedAt) / 1000));

        if (response.canceled) {
            notify({
                level: 'info',
                title: t('store.duplicateFinder.scanCancelledTitle'),
                message: t('store.duplicateFinder.scanCancelledMessage', { values: { count: response.scanned_files } }),
                toolId: 'duplicate-finder',
            });
            toast(t('store.duplicateFinder.scanCancelledToast'), 'info');
            void recordActivity({
                toolId: 'duplicate-finder',
                summary: t('store.duplicateFinder.scanCancelledActivity'),
                details: t('store.duplicateFinder.scanCancelledDetails', { values: { count: response.scanned_files } }),
                outcome: 'cancelled',
            });
            return;
        }

        if (!response.success) {
            const message = response.error ?? t('store.duplicateFinder.scanFailedFallback');
            reportOperationOutcome({
                toolId: 'duplicate-finder',
                kind: 'failed',
                notifyTitle: t('store.duplicateFinder.scanFailedTitle'),
                notifyMessage: message,
                toastMessage: message,
                activitySummary: t('store.duplicateFinder.scanFailedActivity'),
                activityDetails: message.slice(0, 200),
            });
            return;
        }

        duplicateExpandedGroups.set(response.duplicate_groups.slice(0, 4).map((group) => group.hash));
        duplicateSelectedPaths.set(response.duplicate_groups.flatMap((group) => group.files.slice(1).map((file) => file.path)));

        if (!response.duplicate_groups.length) {
            notify({
                level: 'success',
                title: t('store.duplicateFinder.noDuplicatesTitle'),
                message: t('store.duplicateFinder.noDuplicatesMessage', { values: { count: response.scanned_files, elapsed } }),
                toolId: 'duplicate-finder',
            });
            toast(t('store.duplicateFinder.noDuplicatesToast'), 'info');
            void recordActivity({
                toolId: 'duplicate-finder',
                summary: t('store.duplicateFinder.noDuplicatesActivity', { values: { count: response.scanned_files } }),
                outcome: 'success',
            });
        } else {
            reportOperationOutcome({
                toolId: 'duplicate-finder',
                kind: 'success',
                notifyTitle: t('store.duplicateFinder.foundTitle', { values: { count: response.duplicate_files } }),
                notifyMessage: t('store.duplicateFinder.foundMessage', { values: { groups: response.duplicate_groups.length, bytes: formatBytes(response.reclaimable_bytes), elapsed } }),
                toastMessage: t('store.duplicateFinder.foundToast', { values: { count: response.duplicate_files } }),
                activitySummary: t('store.duplicateFinder.foundActivity', { values: { count: response.duplicate_files, groups: response.duplicate_groups.length } }),
                activityDetails: t('store.duplicateFinder.foundDetails', { values: { bytes: formatBytes(response.reclaimable_bytes), scanned: response.scanned_files } }),
            });
        }
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        reportOperationOutcome({
            toolId: 'duplicate-finder',
            kind: 'failed',
            notifyTitle: t('store.duplicateFinder.scanFailedTitle'),
            notifyMessage: message,
            toastMessage: message,
            activitySummary: t('store.duplicateFinder.scanFailedActivity'),
            activityDetails: message.slice(0, 200),
        });
    } finally {
        duplicateScanning.set(false);
        duplicateCancelRequested.set(false);
        duplicateLastJob.set(null);
        duplicateProgress.set({ scanned: 0, hashed: 0, phase: 'idle' });
        if (unlistenProgress) {
            try { unlistenProgress(); } catch { /* ignore */ }
        }
        await setBusy(false);
        stopBusy();
    }
}

export async function moveSelectedDuplicates(): Promise<void> {
    if (get(duplicateMoving)) return toast(t('store.duplicateFinder.moveRunning'), 'info');
    if (get(duplicateScanning)) return toast(t('store.duplicateFinder.scanWaitFinish'), 'info');

    const selectedPaths = get(duplicateSelectedPaths);
    const destinationDir = get(duplicateDestinationDir);

    if (!selectedPaths.length) return toast(t('store.duplicateFinder.needSelection'), 'error');
    if (!destinationDir) return toast(t('store.duplicateFinder.needDestination'), 'error');

    duplicateMoving.set(true);
    duplicateLastJob.set(t('store.duplicateFinder.movingJob'));
    duplicateMoveResults.set([]);
    await setBusy(true);
    const stopBusy = reportBusy('duplicate-finder', t('store.duplicateFinder.moveBusy'));

    try {
        const response = await invoke<MoveDuplicatesResponse>('move_duplicate_files', {
            options: {
                paths: selectedPaths,
                destination_dir: destinationDir,
            },
        });

        duplicateMoveResults.set(response.results);
        duplicateRecovery.set(response.recovery ?? null);
        const ok = response.results.filter((result) => result.success).length;
        const failed = response.results.length - ok;

        // Quality Pass Wave 1 user feedback (2026-05-29): on a successful
        // move, drop the moved files from the in-memory result in place
        // instead of kicking off a fresh scan. Re-walking a 902 k-file
        // tree after every move click is exactly the "felt buggy" feedback
        // the user just gave — the work was already done at scan time, so
        // we surgically remove what the user just acted on, recompute the
        // group counters, and drop groups that collapsed below 2 files.
        // The frontend renders the updated state immediately.
        if (ok > 0) {
            const movedSuccessfully = new Set(
                response.results.filter((r) => r.success).map((r) => r.source_path),
            );
            duplicateResult.update((current) => {
                if (!current) return current;
                const next_groups = current.duplicate_groups
                    .map((group) => {
                        const files = group.files.filter((f) => !movedSuccessfully.has(f.path));
                        // Recompute wasted_bytes using sum-minus-max so it
                        // stays correct in BOTH exact mode (every file's
                        // size is identical so it reduces to size * (count-1))
                        // AND perceptual mode (sizes vary; we keep the
                        // largest copy as the "keep").
                        const totalSize = files.reduce((s, f) => s + f.size, 0);
                        const maxSize = files.reduce((m, f) => Math.max(m, f.size), 0);
                        const wasted_bytes = Math.max(0, totalSize - maxSize);
                        return {
                            ...group,
                            files,
                            count: files.length,
                            wasted_bytes,
                        };
                    })
                    .filter((group) => group.files.length >= 2);
                const duplicate_files = next_groups.reduce(
                    (sum, g) => sum + Math.max(0, g.count - 1),
                    0,
                );
                const reclaimable_bytes = next_groups.reduce((sum, g) => sum + g.wasted_bytes, 0);
                return {
                    ...current,
                    duplicate_groups: next_groups,
                    duplicate_files,
                    reclaimable_bytes,
                };
            });
            // Drop the moved paths from the selection set so the "Move
            // selected" button stops counting them and the next click
            // can act on a fresh selection.
            duplicateSelectedPaths.update((selected) =>
                selected.filter((p) => !movedSuccessfully.has(p)),
            );
            // Collapse any expanded preview pane for a moved file — its
            duplicatePreviewExpandedPaths.update((paths) => paths.filter((path) => !movedSuccessfully.has(path)));
            // bytes are at the destination now, the asset:// URL on the
            // old path would 404. (No-op when DuplicatePreview already
            // unmounts, but keeps the panel sets in sync.)
            duplicateExpandedGroups.update((expanded) => {
                // Drop hashes for groups that no longer exist in the result.
                const surviving = new Set(
                    get(duplicateResult)?.duplicate_groups.map((g) => g.hash) ?? [],
                );
                return expanded.filter((hash) => surviving.has(hash));
            });
        }

        if (ok) {
            notify({
                level: failed ? 'warning' : 'success',
                title: failed ? t('store.duplicateFinder.movedTitlePartial', { values: { ok, failed } }) : t('store.duplicateFinder.movedTitleOk', { values: { count: ok } }),
                message: t('store.duplicateFinder.movedMessage', { values: { dir: destinationDir } }),
                toolId: 'duplicate-finder',
            });
            toast(t('store.duplicateFinder.movedToast', { values: { count: ok } }), 'success');
        }

        if (failed) {
            notify({
                level: 'error',
                title: t('store.duplicateFinder.moveFailedSomeTitle'),
                message: t('store.duplicateFinder.moveFailedSomeMessage', { values: { count: failed } }),
                toolId: 'duplicate-finder',
            });
            toast(t('store.duplicateFinder.moveFailedSomeToast', { values: { count: failed } }), 'error');
        }

        void recordActivity({
            toolId: 'duplicate-finder',
            summary: failed
                ? t('store.duplicateFinder.movedActivityPartial', { values: { ok, failed } })
                : t('store.duplicateFinder.movedActivityOk', { values: { count: ok } }),
            details: t('store.duplicateFinder.movedActivityDetails', { values: { dir: destinationDir } }),
            outcome: ok > 0 ? 'success' : 'failed',
        });

    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        reportOperationOutcome({
            toolId: 'duplicate-finder',
            kind: 'failed',
            notifyTitle: t('store.duplicateFinder.moveFailedTitle'),
            notifyMessage: message,
            toastMessage: message,
            activitySummary: t('store.duplicateFinder.moveFailedActivity'),
            activityDetails: message.slice(0, 200),
        });
    } finally {
        duplicateMoving.set(false);
        duplicateLastJob.set(null);
        await setBusy(false);
        stopBusy();
        // No auto-rescan — the in-memory cleanup above already reflects
        // what the user just did. Users who want a fresh walk (e.g. they
        // moved files INTO the scan tree and want to see what new
        // duplicates that revealed) can hit Scan again manually.
    }
}

export async function undoDuplicateMove(): Promise<void> {
    const recovery = get(duplicateRecovery);
    if (!recovery) return toast('No duplicate move recovery is available', 'info');
    if (get(duplicateMoving)) return toast('A duplicate move is already running', 'info');
    if (get(duplicateScanning)) return toast('Wait until the duplicate scan finishes', 'info');

    duplicateMoving.set(true);
    duplicateLastJob.set('Restoring moved duplicates...');
    await setBusy(true);
    const stopBusy = reportBusy('duplicate-finder', 'Restoring duplicates');

    try {
        const response = await invoke<MoveDuplicatesResponse>('undo_duplicate_move', {
            transactionId: recovery.id,
        });

        duplicateMoveResults.set(response.results);
        duplicateRecovery.set(response.recovery ?? null);

        const restored = response.results.filter((result) => result.success).length;
        const failed = response.results.length - restored;

        if (restored) {
            notify({
                level: failed ? 'warning' : 'success',
                title: failed ? `Restored ${restored}, failed ${failed}` : `Restored ${restored} duplicate${restored === 1 ? '' : 's'}`,
                message: response.recovery
                    ? 'Some files still need manual attention before the restore can finish.'
                    : 'Moved duplicates were restored successfully.',
                toolId: 'duplicate-finder',
            });
            toast(`Restored ${restored} duplicate${restored === 1 ? '' : 's'}`, failed ? 'info' : 'success');
        }

        if (failed) {
            notify({
                level: 'error',
                title: 'Some duplicates could not be restored',
                message: `${failed} item${failed === 1 ? '' : 's'} still need manual attention`,
                toolId: 'duplicate-finder',
            });
            toast(`${failed} restore action${failed === 1 ? '' : 's'} failed`, 'error');
        }

        void recordActivity({
            toolId: 'duplicate-finder',
            summary: failed
                ? `Restored ${restored} duplicate${restored === 1 ? '' : 's'} · ${failed} failed`
                : `Restored ${restored} moved duplicate${restored === 1 ? '' : 's'}`,
            outcome: restored > 0 ? 'success' : 'failed',
        });
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        reportOperationOutcome({
            toolId: 'duplicate-finder',
            kind: 'failed',
            notifyTitle: 'Restore duplicates failed',
            notifyMessage: message,
            toastMessage: message,
            activitySummary: `Restore moved duplicates failed`,
            activityDetails: message.slice(0, 200),
        });
    } finally {
        duplicateMoving.set(false);
        duplicateLastJob.set(null);
        await setBusy(false);
        stopBusy();
    }
}

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
