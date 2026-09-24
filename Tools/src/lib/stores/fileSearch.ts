import { get, writable, type Writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { revealItemInDir } from '@tauri-apps/plugin-opener';
// Wave 6 (2026-05-28): switched the content-search first-run default
// from [Desktop, Documents, Downloads] to [homeDir] — captures the same
// three folders plus Pictures / Music / Videos / OneDrive / projects /
// AppData/Roaming configs. AppData/Local (huge app caches) is excluded
// in DEFAULT_EXCLUDE_FOLDERS so the index stays sensible-sized.
import { homeDir } from '@tauri-apps/api/path';
import { notify } from './notifications';
import { toast } from './toasts';
import { recordActivity } from './activityLog';
import { isFirstDiskIndexSeeded, markFirstDiskIndexSeeded } from './onboarding';

export type FileSearchStatus = {
    initialized: boolean;
    indexing: boolean;
    watching: boolean;
    roots: string[];
    filenameRoots: string[];
    includeHidden: boolean;
    // Wave 7.7 (2026-05-28): `followSymlinks` removed along with the
    // backend field — see the comment in `search.rs` for the rationale.
    // Walker is permanently non-following.
    indexContent: boolean;
    maxContentKb: number;
    commitEvery: number;
    watcherEnabled: boolean;
    watcherPaused: boolean;
    excludeFolders: string[];
    excludeExtensions: string[];
    // Wave 6 (2026-05-28): removed `mftFastIndex`, `allowRootDriveWatcher`,
    // and `elevated` along with the MFT engine. System paths are now
    // hard-blocked from both indexing and watching.
    filenameIndexMessage: string | null;
    /** Filename-index document count + last-built time, reported the same way
     *  `indexedFiles` / `lastIndexedAtMs` are for the content index. */
    filenameIndexedFiles: number;
    filenameLastIndexedAtMs: number | null;
    /** On-disk size, in bytes, of the active filename index directory. */
    filenameIndexBytes: number;
    /** A filename-index build is in progress — distinct from `indexing`,
     *  which is the content build. */
    filenameIndexing: boolean;
    /** The filename index is being live-watched right now (walker notify-watch
     *  loop) — distinct from `watching` (content watcher). */
    filenameWatching: boolean;
    rebuildSchedule: FileSearchRebuildSchedule;
    indexedFiles: number;
    /** Live "entries scanned" / "files skipped" counters for the in-flight
     *  build, mirrored from the backend so the progress panel reads real numbers
     *  even when it mounts mid-build and missed the transient progress events. */
    scannedEntries: number;
    skippedFiles: number;
    updatedFiles: number;
    deletedFiles: number;
    lastIndexedAtMs: number | null;
    /** On-disk size, in bytes, of the active content index directory. */
    contentIndexBytes: number;
    lastError: string | null;
    diagnostics: FileSearchDiagnostics;
};

export type FileSearchDiagnostics = {
    indexWorker: string;
    watcherWorker: string;
    watcherStrategy: string;
    performanceMode: FileSearchPerformanceMode | string;
    lastWorkerMessage: string | null;
    lastStatusAtMs: number | null;
};

export type FileSearchRebuildSchedule = {
    enabled: boolean;
    intervalHours: number;
    lastScheduledRebuildAtMs: number | null;
};

export type FileSearchProgress = {
    stage: string;
    /** Which index this event reports — 'filename' or 'content'. */
    indexKind: string;
    indexedFiles: number;
    scannedEntries: number;
    skippedFiles: number;
    currentPath: string;
    finished: boolean;
    canceled: boolean;
    /** Whether the build succeeded — meaningful only on a `finished` event. */
    success: boolean;
    /** Phase 2 / Task 4.1: a promoted *partial* index — a low-memory pause or a
     *  cancel committed what was indexed so far. Resumable, not complete; shown
     *  distinctly from a full success. Meaningful only on a `finished` content event. */
    partial?: boolean;
    message: string;
};

export type FileSearchResultItem = {
    path: string;
    fileName: string;
    entryType: 'file' | 'folder' | string;
    extension: string;
    size: number;
    modifiedMs: number;
    score: number;
    matchedKeywords: string[];
    matchReason: string;
    /** Sensitive-content kinds detected at index time (e.g. "aws_access_key"). */
    sensitiveKinds: string[];
};

export type FileSearchQueryResult = {
    query: string;
    totalHits: number;
    returned: number;
    tookMs: number;
    results: FileSearchResultItem[];
};

export type ContentSearchResultItem = {
    path: string;
    fileName: string;
    extension: string;
    size: number;
    modifiedMs: number;
    score: number;
    snippet: string;
    snippets: string[];
    matchCount: number;
    matchedKeywords: string[];
    /** Sensitive-content kinds detected at index time. */
    sensitiveKinds: string[];
    /** Semantic (embedding cosine) similarity 0..1; > 0 = a meaning match. */
    semanticScore: number;
    /** True when surfaced ONLY by semantic search (no literal keyword match). */
    semanticOnly: boolean;
};

export type ContentSearchQueryResult = {
    query: string;
    totalHits: number;
    returned: number;
    tookMs: number;
    results: ContentSearchResultItem[];
    /** True when the semantic (meaning) pass actually ran for this query. */
    semanticActive: boolean;
};

export type LaunchTargetItem = {
    id: string;
    name: string;
    path: string;
    kind: string;
    source: string;
    score: number;
};

export type LaunchTargetSearchResult = {
    query: string;
    totalHits: number;
    returned: number;
    tookMs: number;
    cacheBuiltAtMs: number | null;
    results: LaunchTargetItem[];
};

export type FileSearchPerformanceMode = 'balanced' | 'fast' | 'quiet';

export const fileSearchRoots = writable<string[]>([]);
/**
 * Sources for the filename/path index. Kept separate from `fileSearchRoots`
 * (which now means *content* sources) so the two indexes can be scoped
 * independently from their respective settings tabs.
 */
export const fileSearchFilenameRoots = writable<string[]>([]);
export const fileSearchIncludeHidden = writable(false);
// Wave 7.7 (2026-05-28): `fileSearchFollowSymlinks` removed — see the
// backend `search.rs` comment for the rationale. Walker is permanently
// non-following.
export const fileSearchMaxContentKb = writable(1024);
export const fileSearchCommitEvery = writable(25_000);
// The performance-mode picker is USER-owned: it represents the mode the user
// wants their *manual* builds to use. It is NOT driven by backend status — the
// status only ever reports the LAST build's mode (e.g. the auto-index's quiet),
// which would snap the picker back. We persist the choice in localStorage so it
// survives restarts, and `saveFileSearchIndexOptions` writes it into the index
// config for the actual builds.
const PERF_MODE_KEY = 'kil:fileSearchPerformanceMode';
function loadPerfMode(): FileSearchPerformanceMode {
    if (typeof localStorage === 'undefined') return 'balanced';
    const v = localStorage.getItem(PERF_MODE_KEY);
    return v === 'fast' || v === 'quiet' || v === 'balanced' ? v : 'balanced';
}
export const fileSearchPerformanceMode = writable<FileSearchPerformanceMode>(loadPerfMode());
fileSearchPerformanceMode.subscribe((mode) => {
    if (typeof localStorage !== 'undefined') localStorage.setItem(PERF_MODE_KEY, mode);
});
export const fileSearchWatcherEnabled = writable(true);
// Wave 6 (2026-05-28): `fileSearchMftFastIndex`, `fileSearchAllowRootDriveWatcher`,
// and `fileSearchElevated` were removed with the MFT engine. The walker is
// the only path now, and system paths (`C:\Windows`, `Program Files`,
// `Program Files (x86)`, `ProgramData`) are unconditionally hard-blocked
// in the backend — no frontend toggle.
export const fileSearchFilenameIndexMessage = writable<string | null>(null);
export const DEFAULT_FILE_SEARCH_EXCLUDE_FOLDERS = [
    '.git',
    '.hg',
    '.svn',
    '.cache',
    '.gradle',
    '.idea',
    '.mypy_cache',
    '.next',
    '.pytest_cache',
    '.ruff_cache',
    '.svelte-kit',
    '.venv',
    '.vscode',
    '$recycle.bin',
    '$windows.~bt',
    '$windows.~ws',
    '__pycache__',
    'appdata/local/crashdumps',
    'appdata/local/temp',
    'bin',
    'build',
    'com.keepitlocal.app',
    'config.msi',
    'dist',
    'file-search-index',
    'inetpub/logs',
    'microsoft/windows/wer',
    'node_modules',
    'obj',
    'programdata/microsoft/windows/wer',
    'programdata/package cache',
    'recovery',
    'keepitlocal-cache',
    'system volume information',
    'target',
    'temp',
    'tmp',
    'users/*/appdata/local/temp',
    'venv',
    'windows/debug',
    'windows/logs',
    'windows/panther',
    'windows/prefetch',
    'windows/softwaredistribution/download',
    'windows/temp',
    'windows/winsxs/temp',
];
export const DEFAULT_FILE_SEARCH_EXCLUDE_EXTENSIONS = [
    'a',
    'bin',
    'cache',
    'class',
    'crdownload',
    'dll',
    'dmp',
    'ds_store',
    'dylib',
    'gitignore',
    'ilk',
    'lib',
    'o',
    'obj',
    'part',
    'pdb',
    'pyc',
    'pyd',
    'pyo',
    'so',
    'sys',
    'temp',
    'tmp',
];
export const fileSearchExcludeFolders = writable<string[]>([...DEFAULT_FILE_SEARCH_EXCLUDE_FOLDERS]);

/* Wave 8 / task #88 (2026-05-28): OCR-on-Index for scan-only PDFs.
 *
 * Off by default — Tesseract is slow (typically 2-5 s/page) and most
 * users index folders that contain real-text PDFs (Word exports, web
 * downloads). When enabled, the user picks specific folders (e.g.
 * `~/Documents/Receipts` or `~/Documents/Scanned IDs`) and the
 * extractor subprocess falls back to OCR ONLY for PDFs in those
 * folders whose pdfium-render + lopdf passes both yield no text.
 *
 * Languages locked at eng+rus+kat (all already bundled under
 * `tessdata-runtime/` per the Tauri resources block).
 * Page cap is per-file to bound the worst case on a giant scan.
 *
 * Persisted in localStorage following the proven pattern from
 * `fileSearchPerformanceMode` — backend stays the source of truth
 * for what actually runs during indexing, but the UI hydrates from
 * localStorage so the toggle / folder list survive restarts without
 * waiting for the backend round-trip on every cold start. The
 * `saveFileSearchIndexOptions` call (triggered by every setter
 * below) keeps both sides in sync.  */
const OCR_ENABLED_KEY = 'keepitlocal.fileSearch.ocrEnabled';
const OCR_FOLDERS_KEY = 'keepitlocal.fileSearch.ocrFolders';
const OCR_MAX_PAGES_KEY = 'keepitlocal.fileSearch.ocrMaxPages';
const OCR_LANGS_KEY = 'keepitlocal.fileSearch.ocrLangs';
// Wave 8.2 (2026-05-28): smart-skip heuristic knobs.
const OCR_MIN_DIM_KEY = 'keepitlocal.fileSearch.ocrMinImageDim';
const OCR_MAX_ASPECT_KEY = 'keepitlocal.fileSearch.ocrMaxAspectRatio';
const OCR_TIMEOUT_KEY = 'keepitlocal.fileSearch.ocrTimeoutSecs';

function loadOcrEnabled(): boolean {
    if (typeof localStorage === 'undefined') return false;
    return localStorage.getItem(OCR_ENABLED_KEY) === '1';
}
function loadOcrFolders(): string[] {
    if (typeof localStorage === 'undefined') return [];
    try {
        const raw = localStorage.getItem(OCR_FOLDERS_KEY);
        if (!raw) return [];
        const parsed = JSON.parse(raw);
        return Array.isArray(parsed)
            ? parsed.filter((v): v is string => typeof v === 'string')
            : [];
    } catch {
        return [];
    }
}
function loadOcrMaxPages(): number {
    if (typeof localStorage === 'undefined') return 20;
    const v = Number(localStorage.getItem(OCR_MAX_PAGES_KEY) ?? '');
    return Number.isFinite(v) && v >= 1 && v <= 200 ? v : 20;
}
function loadOcrLangs(): string {
    if (typeof localStorage === 'undefined') return 'eng+rus+kat';
    const v = localStorage.getItem(OCR_LANGS_KEY)?.trim();
    return v && v.length > 0 ? v : 'eng+rus+kat';
}
/** Wave 8.2 helpers — clamp to safe ranges on load so a corrupt
 *  localStorage value can't get past the backend defaults. */
function loadOcrMinDim(): number {
    if (typeof localStorage === 'undefined') return 400;
    const v = Number(localStorage.getItem(OCR_MIN_DIM_KEY) ?? '');
    return Number.isFinite(v) && v >= 0 && v <= 5000 ? v : 400;
}
function loadOcrMaxAspect(): number {
    if (typeof localStorage === 'undefined') return 5;
    const v = Number(localStorage.getItem(OCR_MAX_ASPECT_KEY) ?? '');
    return Number.isFinite(v) && v >= 0 && v <= 50 ? v : 5;
}
function loadOcrTimeout(): number {
    if (typeof localStorage === 'undefined') return 30;
    const v = Number(localStorage.getItem(OCR_TIMEOUT_KEY) ?? '');
    return Number.isFinite(v) && v >= 0 && v <= 600 ? v : 30;
}

export const fileSearchOcrEnabled = writable(loadOcrEnabled());
export const fileSearchOcrFolders = writable<string[]>(loadOcrFolders());
export const fileSearchOcrMaxPagesPerFile = writable(loadOcrMaxPages());
export const fileSearchOcrLangs = writable(loadOcrLangs());
export const fileSearchOcrMinImageDim = writable(loadOcrMinDim());
export const fileSearchOcrMaxAspectRatio = writable(loadOcrMaxAspect());
export const fileSearchOcrPerFileTimeoutSecs = writable(loadOcrTimeout());

// Mirror every change into localStorage. Save-to-backend is wired
// via dedicated setters below (`setFileSearchOcrEnabled`, etc.)
// rather than blanket subscribers, so a transient `set()` doesn't
// fire a Tauri invoke per keystroke in the langs editor.
fileSearchOcrEnabled.subscribe((v) => {
    if (typeof localStorage !== 'undefined') {
        localStorage.setItem(OCR_ENABLED_KEY, v ? '1' : '0');
    }
});
fileSearchOcrFolders.subscribe((v) => {
    if (typeof localStorage !== 'undefined') {
        localStorage.setItem(OCR_FOLDERS_KEY, JSON.stringify(v));
    }
});
fileSearchOcrMaxPagesPerFile.subscribe((v) => {
    if (typeof localStorage !== 'undefined') {
        localStorage.setItem(OCR_MAX_PAGES_KEY, String(v));
    }
});
fileSearchOcrLangs.subscribe((v) => {
    if (typeof localStorage !== 'undefined') {
        localStorage.setItem(OCR_LANGS_KEY, v);
    }
});
fileSearchOcrMinImageDim.subscribe((v) => {
    if (typeof localStorage !== 'undefined') {
        localStorage.setItem(OCR_MIN_DIM_KEY, String(v));
    }
});
fileSearchOcrMaxAspectRatio.subscribe((v) => {
    if (typeof localStorage !== 'undefined') {
        localStorage.setItem(OCR_MAX_ASPECT_KEY, String(v));
    }
});
fileSearchOcrPerFileTimeoutSecs.subscribe((v) => {
    if (typeof localStorage !== 'undefined') {
        localStorage.setItem(OCR_TIMEOUT_KEY, String(v));
    }
});

// Indexing Phase 2 / Task 4.3 (2026-06-18): "Search inside files" opt-in.
// localStorage-owned like the OCR knobs and performance mode — the backend
// honors it per build via the index options, but the UI hydrates from
// localStorage so the toggle survives restarts. Default ON; users on low-RAM
// machines can turn it off to keep just the (always-built) filename index.
const CONTENT_INDEXING_ENABLED_KEY = 'keepitlocal.fileSearch.contentIndexingEnabled';
function loadContentIndexingEnabled(): boolean {
    if (typeof localStorage === 'undefined') return true;
    // Absent (never set) → default ON; '0' → off.
    return localStorage.getItem(CONTENT_INDEXING_ENABLED_KEY) !== '0';
}
export const fileSearchContentIndexingEnabled = writable(loadContentIndexingEnabled());
fileSearchContentIndexingEnabled.subscribe((v) => {
    if (typeof localStorage !== 'undefined') {
        localStorage.setItem(CONTENT_INDEXING_ENABLED_KEY, v ? '1' : '0');
    }
});

export function setFileSearchContentIndexingEnabled(enabled: boolean) {
    fileSearchContentIndexingEnabled.set(enabled);
    void saveFileSearchIndexOptions();
}

// Semantic search (beta, 2026-07-01): opt-in embedding-based retrieval on top
// of keyword search. localStorage-owned like the other index knobs. Default
// OFF — it needs the bundled model, embeds every doc at index time, and does a
// cosine scan per query; the backend also no-ops it unless the app was built
// with the `semantic` feature. Requires content indexing to be on.
const SEMANTIC_SEARCH_ENABLED_KEY = 'keepitlocal.fileSearch.semanticSearchEnabled';
function loadSemanticSearchEnabled(): boolean {
    if (typeof localStorage === 'undefined') return false;
    return localStorage.getItem(SEMANTIC_SEARCH_ENABLED_KEY) === '1';
}
export const fileSearchSemanticEnabled = writable(loadSemanticSearchEnabled());
fileSearchSemanticEnabled.subscribe((v) => {
    if (typeof localStorage !== 'undefined') {
        localStorage.setItem(SEMANTIC_SEARCH_ENABLED_KEY, v ? '1' : '0');
    }
});

export function setFileSearchSemanticEnabled(enabled: boolean) {
    fileSearchSemanticEnabled.set(enabled);
    void saveFileSearchIndexOptions();
}

export function setFileSearchOcrEnabled(enabled: boolean) {
    fileSearchOcrEnabled.set(enabled);
    void saveFileSearchIndexOptions();
}
export function addFileSearchOcrFolder(path: string) {
    const next = path.trim();
    if (!next) return;
    fileSearchOcrFolders.update((folders) =>
        folders.includes(next) ? folders : [...folders, next],
    );
    void saveFileSearchIndexOptions();
}
export function removeFileSearchOcrFolder(path: string) {
    fileSearchOcrFolders.update((folders) => folders.filter((p) => p !== path));
    void saveFileSearchIndexOptions();
}
export function setFileSearchOcrMaxPages(pages: number) {
    const clamped = Math.max(1, Math.min(200, Math.round(pages)));
    fileSearchOcrMaxPagesPerFile.set(clamped);
    void saveFileSearchIndexOptions();
}
export function setFileSearchOcrMinImageDim(px: number) {
    const clamped = Math.max(0, Math.min(5000, Math.round(px)));
    fileSearchOcrMinImageDim.set(clamped);
    void saveFileSearchIndexOptions();
}
export function setFileSearchOcrMaxAspectRatio(ratio: number) {
    // Allow 0 (disabled) or 1.0+; clamp tight upper to avoid float weirdness.
    const v = Number.isFinite(ratio) && ratio >= 0 ? ratio : 5;
    const clamped = v === 0 ? 0 : Math.max(1, Math.min(50, v));
    fileSearchOcrMaxAspectRatio.set(clamped);
    void saveFileSearchIndexOptions();
}
export function setFileSearchOcrTimeoutSecs(secs: number) {
    const clamped = Math.max(0, Math.min(600, Math.round(secs)));
    fileSearchOcrPerFileTimeoutSecs.set(clamped);
    void saveFileSearchIndexOptions();
}

export const fileSearchExcludeExtensions = writable<string[]>([
    ...DEFAULT_FILE_SEARCH_EXCLUDE_EXTENSIONS,
]);
export const fileSearchRebuildScheduleEnabled = writable(false);
export const fileSearchRebuildIntervalHours = writable(24);
export const fileSearchLastScheduledRebuildAtMs = writable<number | null>(null);

export const fileSearchStatus = writable<FileSearchStatus | null>(null);
export const fileSearchProgress = writable<FileSearchProgress | null>(null);
export const fileSearchIndexing = writable(false);
export const fileSearchCancelling = writable(false);
export const fileSearchQuery = writable('');
export const fileSearchMode = writable<'files' | 'content'>('files');
export const fileSearchNaturalLanguage = writable(true);
export const fileSearchExtensionFilter = writable('');
export const fileSearchPathFilter = writable('');
/**
 * Size and modified-date quick filters for the in-app Search page. Each holds a
 * natural-language token (e.g. `larger than 100mb`, `this week`) that
 * `runFileSearch` / `runContentSearch` append to the query so the backend's
 * existing NL parser turns it into a structured filter — no backend change.
 * `''` means the filter is off.
 */
export const fileSearchSizeFilter = writable('');
export const fileSearchDateFilter = writable('');

/** Size buckets for the Search-page filter widget; `token` is appended verbatim. */
export const FILE_SEARCH_SIZE_OPTIONS: ReadonlyArray<{ label: string; token: string }> = [
    { label: 'Any size', token: '' },
    { label: 'Empty', token: 'empty' },
    { label: 'Under 1 MB', token: 'smaller than 1mb' },
    { label: 'Under 10 MB', token: 'smaller than 10mb' },
    { label: 'Under 100 MB', token: 'smaller than 100mb' },
    { label: 'Over 100 MB', token: 'larger than 100mb' },
    { label: 'Over 1 GB', token: 'larger than 1gb' },
];

/** Modified-date windows for the Search-page filter widget. */
export const FILE_SEARCH_DATE_OPTIONS: ReadonlyArray<{ label: string; token: string }> = [
    { label: 'Any time', token: '' },
    { label: 'Today', token: 'today' },
    { label: 'Yesterday', token: 'yesterday' },
    { label: 'Past 7 days', token: 'this week' },
    { label: 'Past 30 days', token: 'recently' },
];

export const fileSearchResult = writable<FileSearchQueryResult | null>(null);
export const fileSearchSearching = writable(false);
export const contentSearchResult = writable<ContentSearchQueryResult | null>(null);
export const contentSearchSearching = writable(false);
export const fileSearchLaunchResult = writable<LaunchTargetSearchResult | null>(null);
export const fileSearchLaunchSearching = writable(false);
export const fileSearchLastMessage = writable<string | null>(null);

let initialized = false;
let unlistenProgress: UnlistenFn | null = null;
let activeFileSearchRequestId = 0;
let activeContentSearchRequestId = 0;
let activeLaunchSearchRequestId = 0;

/**
 * Per-index timestamp of the last build-finished notification. The content
 * worker monitor re-emits an identical `finished` progress event on every poll
 * between the worker writing its final status and the process exiting, so a
 * short window collapses those duplicates without dropping genuine builds
 * (which finish far more than a few seconds apart).
 */
const lastBuildNotifyAtMs: Record<string, number> = {};

/**
 * Notify (and activity-log) the outcome of an index build. Driven by the
 * `file-search-index-progress` `finished` event — the single completion signal
 * for both indexes, since every build runs in an isolated background worker and
 * the foreground result branches in `start*SearchIndex` are never taken.
 */
function notifyIndexBuildFinished(progress: FileSearchProgress) {
    const isFilename = progress.indexKind === 'filename';
    const indexLabel = isFilename ? 'Filename index' : 'Content index';

    const now = Date.now();
    if (now - (lastBuildNotifyAtMs[progress.indexKind] ?? 0) < 5_000) {
        return;
    }
    lastBuildNotifyAtMs[progress.indexKind] = now;

    const count = progress.indexedFiles.toLocaleString();
    const entryWord = isFilename ? 'filenames' : 'entries';

    // Phase 2 / Task 4.1: a promoted partial index (low-memory pause or cancel).
    // It is a valid, resumable index — never report it as a full success (which
    // would imply content search is complete) or as a failure.
    if (progress.partial) {
        notify({
            level: 'warning',
            title: `${indexLabel} paused`,
            message:
                progress.message ||
                `Indexed ${count} ${entryWord} so far — re-run to index the rest.`,
            toolId: 'file-search',
        });
        void recordActivity({
            toolId: 'file-search',
            // No dedicated 'partial' outcome exists; 'cancelled' signals
            // "interrupted, not complete" (the summary carries the nuance).
            summary: `${indexLabel} paused — ${count} ${entryWord} indexed`,
            details: progress.message ? progress.message.slice(0, 200) : null,
            outcome: 'cancelled',
        });
        return;
    }

    if (progress.canceled) {
        notify({
            level: 'warning',
            title: `${indexLabel} build cancelled`,
            message: `Indexed ${count} ${entryWord} before cancellation.`,
            toolId: 'file-search',
        });
        void recordActivity({
            toolId: 'file-search',
            summary: `${indexLabel} build cancelled`,
            details: `Indexed ${count} ${entryWord} before cancel`,
            outcome: 'cancelled',
        });
        return;
    }

    if (progress.success) {
        notify({
            level: 'success',
            title: `${indexLabel} ready`,
            message: `Indexed ${count} ${entryWord}.`,
            toolId: 'file-search',
        });
        void recordActivity({
            toolId: 'file-search',
            summary: `${indexLabel} ready — ${count} ${entryWord}`,
            details: null,
            outcome: 'success',
        });
        return;
    }

    notify({
        level: 'error',
        title: `${indexLabel} build failed`,
        message: progress.message || 'The index build did not finish successfully.',
        toolId: 'file-search',
    });
    void recordActivity({
        toolId: 'file-search',
        summary: `${indexLabel} build failed`,
        details: progress.message ? progress.message.slice(0, 200) : null,
        outcome: 'failed',
    });
}

export async function initFileSearchStore() {
    if (initialized) return;
    initialized = true;

    unlistenProgress = await listen<FileSearchProgress>('file-search-index-progress', (event) => {
        const progress = event.payload;
        lastProgressEventAtMs = Date.now();
        fileSearchProgress.set(progress);
        fileSearchLastMessage.set(progress.message);

        // Watch-stage progress events already carry indexed/deleted counts.
        // Don't trigger a full status invoke on every event — that was firing
        // every 1.5s and causing constant UI re-renders even when nothing changed.
        if (progress.stage === 'watch') {
            return;
        }

        if (progress.finished) {
            fileSearchIndexing.set(false);
            fileSearchCancelling.set(false);
            void refreshFileSearchStatus();
            notifyIndexBuildFinished(progress);
        }
    });

    await refreshFileSearchStatus();
}

// ── Live-build status poll ──────────────────────────────────────────────
// During a build the authoritative counts live in `get_file_search_status`
// (the worker monitor mirrors them there). Progress events normally drive the
// UI, but the first-run auto-index starts before any progress listener is
// mounted, so its events are lost. This 1s poll keeps the status — and, when
// events aren't flowing, the progress panel — live regardless, then self-stops
// when the build ends. `lastProgressEventAtMs` lets the poll defer to live
// events when they ARE flowing (so it never fights a fresher event).
let lastProgressEventAtMs = 0;
let buildStatusPoll: ReturnType<typeof setInterval> | null = null;
function startBuildStatusPoll() {
    if (buildStatusPoll) return;
    buildStatusPoll = setInterval(() => {
        if (!get(fileSearchIndexing)) {
            if (buildStatusPoll) {
                clearInterval(buildStatusPoll);
                buildStatusPoll = null;
            }
            return;
        }
        void refreshFileSearchStatus();
    }, 1000);
}

export async function refreshFileSearchStatus() {
    try {
        const status = await invoke<FileSearchStatus>('get_file_search_status');
        fileSearchStatus.set(status);
        // A build is "in progress" if either index is building — the content
        // build sets `indexing`, the filename build sets `filenameIndexing`.
        const building = Boolean(status.indexing) || Boolean(status.filenameIndexing);
        fileSearchIndexing.set(building);
        if (building) {
            // Persist-status fallback for the live progress panel. A view that
            // mounts mid-build — most importantly the first-run auto-index, which
            // starts before any progress listener exists — has no transient
            // `file-search-index-progress` event to read. When events aren't
            // flowing (none in the last 1.5s), mirror the status's live scan
            // counts into the progress store so the panel shows real numbers
            // instead of 0/0/0. When events ARE flowing they own the panel (they
            // also carry the current path), so we don't overwrite them.
            const eventsFlowing = Date.now() - lastProgressEventAtMs < 1500;
            if (!eventsFlowing) {
                const kind = status.indexing ? 'content' : 'filename';
                const indexed = status.indexing
                    ? (status.indexedFiles ?? 0)
                    : (status.filenameIndexedFiles ?? 0);
                fileSearchProgress.set({
                    stage: 'indexing',
                    indexKind: kind,
                    indexedFiles: indexed,
                    scannedEntries: status.scannedEntries ?? 0,
                    skippedFiles: status.skippedFiles ?? 0,
                    currentPath: '',
                    finished: false,
                    canceled: false,
                    success: false,
                    message: status.diagnostics?.lastWorkerMessage ?? 'Building index…',
                });
            }
            startBuildStatusPoll();
        }

        if (Array.isArray(status.roots)) {
            fileSearchRoots.set(status.roots);
        }
        if (Array.isArray(status.filenameRoots)) {
            fileSearchFilenameRoots.set(status.filenameRoots);
        }
        fileSearchIncludeHidden.set(Boolean(status.includeHidden));
        if (Number.isFinite(status.maxContentKb) && status.maxContentKb >= 8) {
            fileSearchMaxContentKb.set(status.maxContentKb);
        }
        if (Number.isFinite(status.commitEvery) && status.commitEvery >= 5_000) {
            fileSearchCommitEvery.set(status.commitEvery);
        }
        fileSearchWatcherEnabled.set(status.watcherEnabled !== false);
        // Wave 6 (2026-05-28): status no longer carries mftFastIndex /
        // allowRootDriveWatcher / elevated — those fields are gone with the
        // MFT engine. The walker is unconditional now.
        fileSearchFilenameIndexMessage.set(
            typeof status.filenameIndexMessage === 'string' ? status.filenameIndexMessage : null,
        );
        if (Array.isArray(status.excludeFolders)) {
            fileSearchExcludeFolders.set(status.excludeFolders);
        }
        if (Array.isArray(status.excludeExtensions)) {
            fileSearchExcludeExtensions.set(status.excludeExtensions);
        }
        if (status.rebuildSchedule) {
            fileSearchRebuildScheduleEnabled.set(status.rebuildSchedule.enabled);
            fileSearchRebuildIntervalHours.set(status.rebuildSchedule.intervalHours || 24);
            fileSearchLastScheduledRebuildAtMs.set(
                status.rebuildSchedule.lastScheduledRebuildAtMs ?? null,
            );
        }
        // NOTE: we intentionally do NOT set `fileSearchPerformanceMode` from the
        // status here. `status.diagnostics.performanceMode` only reports the mode
        // of the LAST build (e.g. the auto-index's quiet/fast) — not the user's
        // chosen default. Driving the picker from it snapped the user's selection
        // back on every refresh (and `saveFileSearchIndexOptions` calls this right
        // after saving, so it reverted instantly). The picker is user-owned +
        // localStorage-backed; builds read the saved config.
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        fileSearchLastMessage.set(message);
    }
}

export function addFileSearchRoots(paths: string[]) {
    const current = new Set(get(fileSearchRoots));
    const next = [...current];
    for (const path of paths) {
        if (!current.has(path)) {
            current.add(path);
            next.push(path);
        }
    }
    fileSearchRoots.set(next);
    void saveFileSearchIndexOptions();
}

export function removeFileSearchRoot(path: string) {
    fileSearchRoots.update((roots) => roots.filter((root) => root !== path));
    void saveFileSearchIndexOptions();
}

export function clearFileSearchRoots() {
    fileSearchRoots.set([]);
    void saveFileSearchIndexOptions();
}

export function addFileSearchFilenameRoots(paths: string[]) {
    const current = new Set(get(fileSearchFilenameRoots));
    const next = [...current];
    for (const path of paths) {
        if (!current.has(path)) {
            current.add(path);
            next.push(path);
        }
    }
    fileSearchFilenameRoots.set(next);
    void saveFileSearchIndexOptions();
}

export function removeFileSearchFilenameRoot(path: string) {
    fileSearchFilenameRoots.update((roots) => roots.filter((root) => root !== path));
    void saveFileSearchIndexOptions();
}

export function clearFileSearchFilenameRoots() {
    fileSearchFilenameRoots.set([]);
    void saveFileSearchIndexOptions();
}

function addNormalizedValues(
    store: Writable<string[]>,
    values: string[],
    normalize: (value: string) => string,
) {
    const current = new Set(get(store));
    const next = [...current];
    for (const rawValue of values) {
        for (const part of rawValue.split(/[,;\n]/)) {
            const value = normalize(part);
            if (!value || current.has(value)) continue;
            current.add(value);
            next.push(value);
        }
    }
    store.set(next.sort((a, b) => a.localeCompare(b)));
}

function normalizeExcludeFolder(value: string) {
    return value.trim().replace(/\\/g, '/').replace(/^\/+|\/+$/g, '').toLowerCase();
}

function normalizeExcludeExtension(value: string) {
    return value.trim().replace(/^\.+/, '').toLowerCase();
}

export function addFileSearchExcludeFolders(values: string[]) {
    addNormalizedValues(fileSearchExcludeFolders, values, normalizeExcludeFolder);
    void saveFileSearchIndexOptions();
}

export function removeFileSearchExcludeFolder(value: string) {
    const normalized = normalizeExcludeFolder(value);
    fileSearchExcludeFolders.update((folders) => folders.filter((folder) => folder !== normalized));
    void saveFileSearchIndexOptions();
}

export function addFileSearchExcludeExtensions(values: string[]) {
    addNormalizedValues(fileSearchExcludeExtensions, values, normalizeExcludeExtension);
    void saveFileSearchIndexOptions();
}

export function removeFileSearchExcludeExtension(value: string) {
    const normalized = normalizeExcludeExtension(value);
    fileSearchExcludeExtensions.update((extensions) =>
        extensions.filter((extension) => extension !== normalized),
    );
    void saveFileSearchIndexOptions();
}

export function resetFileSearchExclusions() {
    fileSearchExcludeFolders.set([...DEFAULT_FILE_SEARCH_EXCLUDE_FOLDERS]);
    fileSearchExcludeExtensions.set([...DEFAULT_FILE_SEARCH_EXCLUDE_EXTENSIONS]);
    void saveFileSearchIndexOptions();
}

export async function saveFileSearchIndexOptions() {
    try {
        await invoke('save_file_search_index_options', {
            options: currentFileSearchIndexOptions(),
        });
        await refreshFileSearchStatus();
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        fileSearchLastMessage.set(message);
        toast(message, 'error');
    }
}

export async function saveFileSearchRebuildSchedule() {
    try {
        await invoke('save_file_search_index_options', {
            options: currentFileSearchIndexOptions(),
        });
        const schedule = await invoke<FileSearchRebuildSchedule>(
            'save_file_search_rebuild_schedule',
            {
                schedule: currentFileSearchRebuildSchedule(),
            },
        );
        fileSearchRebuildScheduleEnabled.set(schedule.enabled);
        fileSearchRebuildIntervalHours.set(schedule.intervalHours);
        fileSearchLastScheduledRebuildAtMs.set(schedule.lastScheduledRebuildAtMs ?? null);
        await refreshFileSearchStatus();
        toast(schedule.enabled ? 'Scheduled rebuild saved' : 'Scheduled rebuild disabled', 'success');
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        fileSearchLastMessage.set(message);
        toast(message, 'error');
    }
}

function currentFileSearchRebuildSchedule(): FileSearchRebuildSchedule {
    return {
        enabled: get(fileSearchRebuildScheduleEnabled),
        intervalHours: Math.max(1, Math.min(720, Math.round(get(fileSearchRebuildIntervalHours)))),
        lastScheduledRebuildAtMs: get(fileSearchLastScheduledRebuildAtMs),
    };
}

/**
 * First-run / post-wipe disk indexing (silent, background). On the first launch
 * (or the first launch after a data wipe) it seeds sensible defaults and kicks
 * off a build with no UI — so Search "just works" without the user configuring
 * anything:
 *   • FILENAME index  → every drive the user has (walker engine, not MFT)
 *   • CONTENT index   → Desktop / Downloads / Documents only
 *   • "Watch root drives" ON (the user verified it works well)
 * The chosen drives + folders are persisted, so File Search Index settings
 * shows them pre-picked.
 *
 * "Have we already auto-run?" is tracked by a persisted flag in the onboarding
 * file (`firstDiskIndexSeeded`), read FRESH from the backend here. That file
 * survives normal restarts but is removed by `reset_all_data`, so the auto-run
 * fires exactly once — and again only after a wipe. (An earlier version gated
 * on "are roots empty?", but the status query doesn't reliably report saved
 * roots early in startup, so it re-ran on every launch — this flag is the fix.)
 * Best-effort throughout — never blocks or breaks startup.
 */
export async function ensureFirstRunDiskIndex(): Promise<void> {
    try {
        // Authoritative once-only gate: have we already auto-seeded?
        if (await isFirstDiskIndexSeeded()) return;

        // Safety: never seed over an existing/intentional setup (e.g. an upgrade
        // from before this flag, or a manual config). If roots already exist,
        // just record that we're seeded and bail.
        await refreshFileSearchStatus().catch(() => {});
        if (get(fileSearchFilenameRoots).length > 0 || get(fileSearchRoots).length > 0) {
            await markFirstDiskIndexSeeded();
            return;
        }
        if (get(fileSearchIndexing)) return;

        // FILENAME index = every drive (the walker engine).
        const drives = await invoke<string[]>('list_logical_drives').catch(() => [] as string[]);
        // CONTENT index = the user's home folder. Wave 6 widened from
        // Desktop+Documents+Downloads to ALL of `C:\Users\<name>` so
        // Pictures / Music / Videos / OneDrive / source repos / AppData
        // configs are all in scope. AppData/Local is excluded server-side
        // (huge app caches with zero user value).
        const contentRoots: string[] = [];
        try {
            const home = await homeDir();
            if (home) contentRoots.push(home);
        } catch {
            // Home dir lookup failed — leave content roots empty.
        }

        if (!drives.length && !contentRoots.length) return; // nothing to seed

        if (drives.length) fileSearchFilenameRoots.set(drives);
        if (contentRoots.length) fileSearchRoots.set(contentRoots);
        fileSearchWatcherEnabled.set(true);
        // Keep the user's PERSISTED default at 'balanced'. Quiet is applied ONLY
        // to the auto build below (as a per-run override), never saved — so when
        // the user indexes manually later, the mode picker is theirs to choose.
        fileSearchPerformanceMode.set('balanced');

        // Persist the config (balanced mode) so File Search Index settings
        // reflects the pre-picked sources + the user's real default.
        const options = currentFileSearchIndexOptions();
        await invoke('save_file_search_index_options', { options }).catch(() => {});

        // Record "seeded" NOW — before the (long) builds — so an interrupted
        // build never causes a full auto re-seed on the next launch. The user
        // can always rebuild manually from Settings → File Search Index.
        await markFirstDiskIndexSeeded();

        // Run the builds in sequence: CONTENT first (just 3 folders → finishes
        // fast, so "search inside" works sooner), THEN the FILENAME index (all
        // drives → slower). Quiet is a PER-RUN override on these build calls
        // only — it does NOT touch the saved config above. The backend commands
        // resolve on build start (background), so this never blocks the UI.
        const buildOptions = { ...options, performanceMode: 'quiet' };
        if (contentRoots.length) {
            await invoke('start_content_search_index', { options: buildOptions }).catch(() => {});
        }
        if (drives.length) {
            await invoke('start_filename_search_index', { options: buildOptions }).catch(() => {});
        }

        // The builds run in background workers and no progress listener is
        // mounted this early, so mark "building" now and start the status poll —
        // the index pages then show live progress (not 0/0/0) if the user opens
        // them while this first-run build is still running.
        fileSearchIndexing.set(true);
        startBuildStatusPoll();
        void refreshFileSearchStatus();
    } catch {
        // Never let first-run indexing break startup.
    }
}

function currentFileSearchIndexOptions() {
    return {
        roots: get(fileSearchRoots),
        filenameRoots: get(fileSearchFilenameRoots),
        includeHidden: get(fileSearchIncludeHidden),
        // `indexContent` is the internal content-vs-filename mode flag (the
        // backend pins it per build type). `contentIndexingEnabled` (Task 4.3)
        // is the user opt-in for "search inside files" — when off, the content
        // worker no-ops and only the (always-built) filename index runs.
        indexContent: true,
        contentIndexingEnabled: get(fileSearchContentIndexingEnabled),
        // Semantic search (beta) opt-in — maps to `semantic_search_enabled`.
        semanticSearchEnabled: get(fileSearchSemanticEnabled),
        maxContentKb: Math.max(8, Math.round(get(fileSearchMaxContentKb))),
        commitEvery: Math.max(300, Math.min(250_000, Math.round(get(fileSearchCommitEvery)))),
        performanceMode: get(fileSearchPerformanceMode),
        watcherEnabled: get(fileSearchWatcherEnabled),
        excludeFolders: get(fileSearchExcludeFolders),
        excludeExtensions: get(fileSearchExcludeExtensions),
        // Wave 8 / task #88 (2026-05-28): OCR-on-Index opt-in.
        ocrOnIndexEnabled: get(fileSearchOcrEnabled),
        ocrOnIndexFolders: get(fileSearchOcrFolders),
        ocrMaxPagesPerFile: Math.max(
            1,
            Math.min(200, Math.round(get(fileSearchOcrMaxPagesPerFile))),
        ),
        ocrLangs: get(fileSearchOcrLangs).trim() || 'eng+rus+kat',
        // Wave 8.2 (2026-05-28): smart-skip knobs.
        ocrMinImageDim: Math.max(0, Math.min(5000, Math.round(get(fileSearchOcrMinImageDim)))),
        ocrMaxAspectRatio: (() => {
            const v = get(fileSearchOcrMaxAspectRatio);
            if (!Number.isFinite(v) || v < 0) return 5;
            return v === 0 ? 0 : Math.max(1, Math.min(50, v));
        })(),
        ocrPerFileTimeoutSecs: Math.max(
            0,
            Math.min(600, Math.round(get(fileSearchOcrPerFileTimeoutSecs))),
        ),
    };
}

export async function startFileSearchIndex() {
    if (get(fileSearchIndexing)) {
        toast('Index build is already running', 'info');
        return;
    }

    const roots = get(fileSearchRoots);
    if (!roots.length) {
        toast('Choose at least one folder', 'error');
        return;
    }

    fileSearchIndexing.set(true);
    fileSearchCancelling.set(false);
    fileSearchProgress.set({
        stage: 'indexing',
        indexKind: 'content',
        indexedFiles: 0,
        scannedEntries: 0,
        skippedFiles: 0,
        currentPath: '',
        finished: false,
        canceled: false,
        success: false,
        message: 'Building index...',
    });
    fileSearchLastMessage.set('Building index...');

    let keepIndexing = false;
    try {
        const result = await invoke<{
            success: boolean;
            canceled: boolean;
            background: boolean;
            indexedFiles: number;
            scannedEntries: number;
            skippedFiles: number;
            message: string;
        }>('start_file_search_index', {
            options: currentFileSearchIndexOptions(),
        });

        fileSearchLastMessage.set(result.message);
        if (result.background) {
            keepIndexing = true;
            fileSearchIndexing.set(true);
            toast('Search indexing started in the background', 'info');
            // Background runs report their finished outcome via the
            // file-search-index-progress event, not here. Skip recording
            // an entry now to avoid logging both "started" and "finished".
        } else if (result.canceled) {
            toast('Index build cancelled', 'info');
            notify({
                level: 'warning',
                title: 'Search indexing cancelled',
                message: `Indexed ${result.indexedFiles} entries before cancellation.`,
                toolId: 'file-search',
            });
            void recordActivity({
                toolId: 'file-search',
                summary: `Search indexing cancelled`,
                details: `Indexed ${result.indexedFiles} entries before cancel`,
                outcome: 'cancelled',
            });
        } else if (result.success) {
            toast(`Indexed ${result.indexedFiles} entries`, 'success');
            notify({
                level: 'success',
                title: 'Search index ready',
                message: `Indexed ${result.indexedFiles} entries. Drive roots need manual or scheduled rebuilds; explicit folders can be live-watched.`,
                toolId: 'file-search',
            });
            void recordActivity({
                toolId: 'file-search',
                summary: `Indexed ${result.indexedFiles} entries`,
                details: result.skippedFiles ? `${result.skippedFiles} skipped` : null,
                outcome: 'success',
            });
        } else {
            toast(result.message, 'error');
            notify({
                level: 'error',
                title: 'Search indexing failed',
                message: result.message,
                toolId: 'file-search',
            });
            void recordActivity({
                toolId: 'file-search',
                summary: `Search indexing failed`,
                details: result.message?.slice(0, 200) ?? null,
                outcome: 'failed',
            });
        }
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        fileSearchLastMessage.set(message);
        toast(message, 'error');
        notify({
            level: 'error',
            title: 'Search indexing failed',
            message,
            toolId: 'file-search',
        });
        void recordActivity({
            toolId: 'file-search',
            summary: `Search indexing failed`,
            details: message.slice(0, 200),
            outcome: 'failed',
        });
    } finally {
        if (!keepIndexing) {
            fileSearchIndexing.set(false);
        }
        fileSearchCancelling.set(false);
        await refreshFileSearchStatus();
    }
}

/**
 * Build (or rebuild) the content-search index from {@link fileSearchRoots}.
 * Returns the same build-result shape the old `start_file_search_index`
 * used, so progress/cancellation handling is identical to
 * {@link startFileSearchIndex}.
 */
export async function startContentSearchIndex() {
    if (get(fileSearchIndexing)) {
        toast('Index build is already running', 'info');
        return;
    }

    const roots = get(fileSearchRoots);
    if (!roots.length) {
        toast('Choose at least one folder', 'error');
        return;
    }

    fileSearchIndexing.set(true);
    fileSearchCancelling.set(false);
    fileSearchProgress.set({
        stage: 'indexing',
        indexKind: 'content',
        indexedFiles: 0,
        scannedEntries: 0,
        skippedFiles: 0,
        currentPath: '',
        finished: false,
        canceled: false,
        success: false,
        message: 'Building content index...',
    });
    fileSearchLastMessage.set('Building content index...');

    let keepIndexing = false;
    try {
        const result = await invoke<{
            success: boolean;
            canceled: boolean;
            background: boolean;
            indexedFiles: number;
            scannedEntries: number;
            skippedFiles: number;
            message: string;
        }>('start_content_search_index', {
            options: currentFileSearchIndexOptions(),
        });

        fileSearchLastMessage.set(result.message);
        if (result.background) {
            keepIndexing = true;
            fileSearchIndexing.set(true);
            toast('Content indexing started in the background', 'info');
            // Background runs report their finished outcome via the
            // file-search-index-progress event, not here. Skip recording
            // an entry now to avoid logging both "started" and "finished".
        } else if (result.canceled) {
            toast('Index build cancelled', 'info');
            notify({
                level: 'warning',
                title: 'Content indexing cancelled',
                message: `Indexed ${result.indexedFiles} entries before cancellation.`,
                toolId: 'file-search',
            });
            void recordActivity({
                toolId: 'file-search',
                summary: `Content indexing cancelled`,
                details: `Indexed ${result.indexedFiles} entries before cancel`,
                outcome: 'cancelled',
            });
        } else if (result.success) {
            toast(`Indexed ${result.indexedFiles} entries`, 'success');
            notify({
                level: 'success',
                title: 'Content search index ready',
                message: `Indexed ${result.indexedFiles} entries. Drive roots need manual or scheduled rebuilds; explicit folders can be live-watched.`,
                toolId: 'file-search',
            });
            void recordActivity({
                toolId: 'file-search',
                summary: `Indexed ${result.indexedFiles} entries`,
                details: result.skippedFiles ? `${result.skippedFiles} skipped` : null,
                outcome: 'success',
            });
        } else {
            toast(result.message, 'error');
            notify({
                level: 'error',
                title: 'Content indexing failed',
                message: result.message,
                toolId: 'file-search',
            });
            void recordActivity({
                toolId: 'file-search',
                summary: `Content indexing failed`,
                details: result.message?.slice(0, 200) ?? null,
                outcome: 'failed',
            });
        }
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        fileSearchLastMessage.set(message);
        toast(message, 'error');
        notify({
            level: 'error',
            title: 'Content indexing failed',
            message,
            toolId: 'file-search',
        });
        void recordActivity({
            toolId: 'file-search',
            summary: `Content indexing failed`,
            details: message.slice(0, 200),
            outcome: 'failed',
        });
    } finally {
        if (!keepIndexing) {
            fileSearchIndexing.set(false);
        }
        fileSearchCancelling.set(false);
        await refreshFileSearchStatus();
    }
}

/**
 * Build (or rebuild) the filename/path index from {@link fileSearchFilenameRoots}.
 * The backend command returns a boolean (true = build kicked off / succeeded);
 * detailed progress lands on the `file-search-index-progress` event and the
 * final filename count surfaces via `status.filenameIndexMessage`.
 */
export async function startFilenameSearchIndex() {
    if (get(fileSearchIndexing)) {
        toast('Index build is already running', 'info');
        return;
    }

    const roots = get(fileSearchFilenameRoots);
    if (!roots.length) {
        toast('Choose at least one folder or drive', 'error');
        return;
    }

    fileSearchIndexing.set(true);
    fileSearchCancelling.set(false);
    fileSearchProgress.set({
        stage: 'indexing',
        indexKind: 'filename',
        indexedFiles: 0,
        scannedEntries: 0,
        skippedFiles: 0,
        currentPath: '',
        finished: false,
        canceled: false,
        success: false,
        message: 'Building filename index...',
    });
    fileSearchLastMessage.set('Building filename index...');

    try {
        const ok = await invoke<boolean>('start_filename_search_index', {
            options: currentFileSearchIndexOptions(),
        });

        if (ok) {
            toast('Filename indexing started', 'info');
            void recordActivity({
                toolId: 'file-search',
                summary: 'Filename indexing started',
                details: null,
                outcome: 'success',
            });
        } else {
            toast('Filename indexing could not start', 'error');
            notify({
                level: 'error',
                title: 'Filename indexing failed',
                message: 'The filename index build could not be started.',
                toolId: 'file-search',
            });
        }
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        fileSearchLastMessage.set(message);
        toast(message, 'error');
        notify({
            level: 'error',
            title: 'Filename indexing failed',
            message,
            toolId: 'file-search',
        });
        void recordActivity({
            toolId: 'file-search',
            summary: 'Filename indexing failed',
            details: message.slice(0, 200),
            outcome: 'failed',
        });
    } finally {
        fileSearchCancelling.set(false);
        await refreshFileSearchStatus();
    }
}

export async function cancelFileSearchIndexBuild() {
    if (!get(fileSearchIndexing)) {
        fileSearchCancelling.set(false);
        if (get(fileSearchLastMessage) === 'Cancelling index build...') {
            fileSearchLastMessage.set('No active index build to cancel');
        }
        return;
    }
    fileSearchCancelling.set(true);
    fileSearchLastMessage.set('Cancelling index build...');

    try {
        await invoke('cancel_file_search_index_build');
        toast('Cancelling search indexing...', 'info');
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        fileSearchLastMessage.set(message);
        fileSearchCancelling.set(false);
        toast(message, 'error');
        notify({
            level: 'error',
            title: 'Cancel failed',
            message,
            toolId: 'file-search',
        });
    }
}

export async function stopFileSearchWatcher() {
    try {
        await invoke('stop_file_search_index_watcher');
        await refreshFileSearchStatus();
        toast('Search watcher stopped', 'info');
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        toast(message, 'error');
        notify({
            level: 'error',
            title: 'Watcher stop failed',
            message,
            toolId: 'file-search',
        });
    }
}

// Wave 6 (2026-05-28): `relaunchKeepItLocalElevated` was the UAC-restart
// command used to enable the MFT fast-path. Removed with the engine.

/**
 * Fold the active size / date quick-filter tokens into the raw query. The
 * tokens are natural-language phrases the backend parser already understands,
 * so a structured filter needs no backend change — but it does need the NL
 * parser to run, hence `forceNatural` whenever a token is present.
 */
function applyStructuredFilters(query: string): { query: string; forceNatural: boolean } {
    const tokens = [get(fileSearchSizeFilter).trim(), get(fileSearchDateFilter).trim()].filter(
        (token) => token.length > 0,
    );
    if (tokens.length === 0) {
        return { query, forceNatural: false };
    }
    return { query: [query, ...tokens].join(' ').trim(), forceNatural: true };
}

export async function runFileSearch(
    options: {
        silentEmpty?: boolean;
        query?: string;
        offset?: number;
        limit?: number;
        append?: boolean;
    } = {},
) {
    const query = (options.query ?? get(fileSearchQuery)).trim();
    const requestId = ++activeFileSearchRequestId;
    if (!query) {
        if (!options.append) {
            fileSearchResult.set(null);
        }
        if (!options.silentEmpty) {
            toast('Enter a search query', 'error');
        }
        return false;
    }

    const isAppend = Boolean(options.append);
    if (!isAppend) {
        fileSearchSearching.set(true);
        fileSearchLastMessage.set('Searching...');
    }

    const { query: effectiveQuery, forceNatural } = applyStructuredFilters(query);

    try {
        const result = await invoke<FileSearchQueryResult>('search_local_files', {
            options: {
                query: effectiveQuery,
                limit: options.limit ?? 80,
                offset: options.offset ?? 0,
                extensionFilter: get(fileSearchExtensionFilter).trim() || null,
                pathFilter: get(fileSearchPathFilter).trim() || null,
                naturalLanguage: get(fileSearchNaturalLanguage) || forceNatural,
            },
        });
        if (requestId !== activeFileSearchRequestId) {
            return false;
        }
        if (options.append) {
            const existing = get(fileSearchResult);
            if (existing && existing.query === result.query) {
                const existingPaths = new Set(existing.results.map((item) => item.path));
                const appended = result.results.filter((item) => !existingPaths.has(item.path));
                const merged = [...existing.results, ...appended];
                const effectiveTotalHits =
                    appended.length === 0 && result.returned === 0
                        ? merged.length
                        : Math.max(result.totalHits, merged.length);
                fileSearchResult.set({
                    ...result,
                    totalHits: effectiveTotalHits,
                    returned: merged.length,
                    results: merged,
                });
            } else {
                fileSearchResult.set(result);
            }
        } else {
            fileSearchResult.set(result);
        }
        const current = get(fileSearchResult);
        if (current) {
            fileSearchLastMessage.set(
                `Found ${current.totalHits} matches, showing ${current.returned} in ${result.tookMs}ms`,
            );
        }
        return true;
    } catch (error) {
        if (requestId !== activeFileSearchRequestId) {
            return false;
        }
        const message = error instanceof Error ? error.message : String(error);
        fileSearchLastMessage.set(message);
        if (!options.silentEmpty) {
            toast(message, 'error');
            notify({
                level: 'error',
                title: 'Search failed',
                message,
                toolId: 'file-search',
            });
        }
        return false;
    } finally {
        if (!isAppend && requestId === activeFileSearchRequestId) {
            fileSearchSearching.set(false);
        }
    }
}

export async function runContentSearch(
    options: {
        silentEmpty?: boolean;
        query?: string;
        offset?: number;
        limit?: number;
        append?: boolean;
    } = {},
) {
    const query = (options.query ?? get(fileSearchQuery)).trim();
    const requestId = ++activeContentSearchRequestId;
    if (!query) {
        if (!options.append) {
            contentSearchResult.set(null);
        }
        if (!options.silentEmpty) {
            toast('Enter text to search inside files', 'error');
        }
        return false;
    }

    const isAppend = Boolean(options.append);
    if (!isAppend) {
        contentSearchSearching.set(true);
        fileSearchLastMessage.set('Searching inside files...');
    }

    const { query: effectiveQuery, forceNatural } = applyStructuredFilters(query);

    try {
        const result = await invoke<ContentSearchQueryResult>('search_file_contents', {
            options: {
                query: effectiveQuery,
                limit: options.limit ?? 50,
                offset: options.offset ?? 0,
                extensionFilter: get(fileSearchExtensionFilter).trim() || null,
                pathFilter: get(fileSearchPathFilter).trim() || null,
                naturalLanguage: get(fileSearchNaturalLanguage) || forceNatural,
            },
        });
        if (requestId !== activeContentSearchRequestId) {
            return false;
        }
        if (options.append) {
            const existing = get(contentSearchResult);
            if (existing && existing.query === result.query) {
                const existingPaths = new Set(existing.results.map((item) => item.path));
                const appended = result.results.filter((item) => !existingPaths.has(item.path));
                const merged = [...existing.results, ...appended];
                const effectiveTotalHits =
                    appended.length === 0 && result.returned === 0
                        ? merged.length
                        : Math.max(result.totalHits, merged.length);
                contentSearchResult.set({
                    ...result,
                    totalHits: effectiveTotalHits,
                    returned: merged.length,
                    results: merged,
                });
            } else {
                contentSearchResult.set(result);
            }
        } else {
            contentSearchResult.set(result);
        }
        const current = get(contentSearchResult);
        if (current) {
            fileSearchLastMessage.set(
                `Found ${current.totalHits} content matches, showing ${current.returned} in ${result.tookMs}ms`,
            );
        }
        return true;
    } catch (error) {
        if (requestId !== activeContentSearchRequestId) {
            return false;
        }
        const message = error instanceof Error ? error.message : String(error);
        fileSearchLastMessage.set(message);
        if (!options.silentEmpty) {
            toast(message, 'error');
            notify({
                level: 'error',
                title: 'Content search failed',
                message,
                toolId: 'file-search',
            });
        }
        return false;
    } finally {
        if (!isAppend && requestId === activeContentSearchRequestId) {
            contentSearchSearching.set(false);
        }
    }
}

export async function runLaunchTargetSearch(options: { silentEmpty?: boolean; query?: string } = {}) {
    const query = (options.query ?? get(fileSearchQuery)).trim();
    const requestId = ++activeLaunchSearchRequestId;
    if (!query) {
        fileSearchLaunchResult.set(null);
        return false;
    }

    fileSearchLaunchSearching.set(true);

    try {
        const result = await invoke<LaunchTargetSearchResult>('search_launch_targets', {
            options: {
                query,
                limit: 12,
            },
        });
        if (requestId !== activeLaunchSearchRequestId) {
            return false;
        }
        fileSearchLaunchResult.set(result);
        return true;
    } catch (error) {
        if (requestId !== activeLaunchSearchRequestId) {
            return false;
        }
        const message = error instanceof Error ? error.message : String(error);
        if (!options.silentEmpty) {
            toast(message, 'error');
            notify({
                level: 'error',
                title: 'Launcher search failed',
                message,
                toolId: 'file-search',
            });
        }
        return false;
    } finally {
        if (requestId === activeLaunchSearchRequestId) {
            fileSearchLaunchSearching.set(false);
        }
    }
}

export async function refreshLaunchTargetCache() {
    try {
        await invoke('refresh_launch_target_cache');
        fileSearchLaunchResult.set(null);
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        toast(message, 'error');
    }
}

export function clearFileSearchResults() {
    fileSearchResult.set(null);
    contentSearchResult.set(null);
    fileSearchLaunchResult.set(null);
    fileSearchLastMessage.set('Results cleared');
}

export async function openLaunchTarget(path: string) {
    if (!path.trim()) return;
    try {
        await invoke('launch_cached_target', { path });
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        toast(message, 'error');
        notify({
            level: 'error',
            title: 'Launch failed',
            message,
            toolId: 'file-search',
        });
    }
}

export async function openFileSearchResult(path: string) {
    if (!path.trim()) return;
    try {
        await invoke('open_search_result_path', { path });
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        toast(message, 'error');
        notify({
            level: 'error',
            title: 'Open failed',
            message,
            toolId: 'file-search',
        });
    }
}

export async function revealFileSearchResult(path: string) {
    if (!path.trim()) return;
    try {
        await revealItemInDir(path);
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        toast(message, 'error');
        notify({
            level: 'error',
            title: 'Open in Explorer failed',
            message,
            toolId: 'file-search',
        });
    }
}

export function formatBytes(bytes: number): string {
    if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let value = bytes;
    let index = 0;
    while (value >= 1024 && index < units.length - 1) {
        value /= 1024;
        index += 1;
    }
    return `${value.toFixed(value >= 10 || index === 0 ? 0 : 1)} ${units[index]}`;
}

export function formatDateTime(value: number): string {
    if (!value || !Number.isFinite(value)) return '-';
    return new Date(value).toLocaleString();
}
