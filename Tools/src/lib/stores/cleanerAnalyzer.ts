/**
 * Cleaner / Analyzer — navigation-surviving result state (2026-06-12).
 *
 * Same pattern as `diffMergeState.ts` / `duplicateFinder.ts`: the app
 * wraps the active tool in `{#key}` blocks, so leaving the Cleaner and
 * coming back UNMOUNTS the component and wipes its component-local
 * `$state`. A completed analysis (the `report`), the user's cleanup
 * selection, and the last clean result therefore vanished on nav. Lifting
 * them to module scope makes them survive remounts.
 *
 * **What stays component-local** (deliberate, transient):
 * - `analyzing` / `cancelling` / `cleaning` flags — in-flight UI; the scan
 *   itself does not survive a remount the way a background task would, so
 *   the in-progress state is not worth persisting.
 * - `progress` + the `cleaner-analyze-progress` listener — live, throttled
 *   per-op events that only matter while a scan is running.
 * - `confirmText` — the CONFIRM safety gate; intentionally NOT persisted so
 *   navigating away and back never leaves a primed "CONFIRM" in the box.
 * - `errorMessage` — recoverable inline error tied to a just-failed scan.
 * - scan options (`includeCacheScan`, `oldFileDays`, …) — cheap inputs the
 *   user re-confirms per run; left local to keep this store focused on the
 *   produced RESULT, matching the task's scope.
 */

import { writable } from 'svelte/store';

// ─── Backend data shapes (serde camelCase) — mirror of the component ───
export type CleanupTargetSummary = {
    id: string;
    label: string;
    category: string;
    path: string;
    description: string;
    bytes: number;
    fileCount: number;
    directoryCount: number;
    safeToClean: boolean;
};
export type OldLargeFileItem = {
    path: string;
    fileName: string;
    bytes: number;
    modifiedMs: number | null;
    accessedMs: number | null;
    daysSinceModified: number | null;
    recommendation: string;
};
export type StartupItem = {
    name: string;
    source: string;
    commandOrPath: string;
    recommendation: string;
};
export type DiskSummary = {
    path: string;
    totalBytes: number | null;
    availableBytes: number | null;
    usedPercent: number | null;
};
export type MemorySummary = {
    totalBytes: number | null;
    availableBytes: number | null;
    usedPercent: number | null;
    source: string;
};
export type TopUserDirEntry = {
    path: string;
    label: string;
    parentLabel: string;
    bytes: number;
    fileCount: number;
};
export type FileTypeBreakdownEntry = { category: string; bytes: number; fileCount: number };
export type CleanerAnalyzeReport = {
    generatedAtMs: number;
    cleanupTargets: CleanupTargetSummary[];
    oldLargeFiles: OldLargeFileItem[];
    startupItems: StartupItem[];
    diskSummaries: DiskSummary[];
    memorySummary: MemorySummary;
    topUserDirs: TopUserDirEntry[];
    fileTypeBreakdown: FileTypeBreakdownEntry[];
    totalCleanableBytes: number;
    tips: string[];
};
export type CleanerCleanResultItem = {
    id: string;
    label: string;
    path: string;
    deletedBytes: number;
    deletedEntries: number;
    success: boolean;
    error: string | null;
};
export type CleanerCleanResult = {
    success: boolean;
    deletedBytes: number;
    deletedEntries: number;
    results: CleanerCleanResultItem[];
};

export type CleanerProgress = {
    operationId: string;
    stage: string;
    stageIndex: number;
    stageTotal: number;
    entriesScanned: number;
    currentPath: string;
};

export type CleanerRuntimeState = {
    analyzing: boolean;
    cancelling: boolean;
    cleaning: boolean;
    operationId: string | null;
    progress: CleanerProgress | null;
    error: string | null;
};

// ─── Persisted result/selection state ──────────────────────────────────
/** The completed analysis report. Null until the first scan finishes. */
export const cleanerReport = writable<CleanerAnalyzeReport | null>(null);
/** IDs of cleanup targets the user has checked for the CONFIRM-gated
 *  cleanup. Seeded from the report (safe, non-empty targets) on each scan. */
export const cleanerSelectedTargetIds = writable<string[]>([]);
/** Result of the most recent cleanup run. Null until the user cleans. */
export const cleanerLastCleanResult = writable<CleanerCleanResult | null>(null);
/** Active detail tab (caches / types / old / startup / tips). Persisted so
 *  the user returns to the same view they left. */
export const cleanerDetailTab = writable<string>('caches');
/** Live run state. Kept separately from the durable report and selection. */
export const cleanerRuntime = writable<CleanerRuntimeState>({
    analyzing: false,
    cancelling: false,
    cleaning: false,
    operationId: null,
    progress: null,
    error: null,
});
