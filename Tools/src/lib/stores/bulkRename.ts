import { derived, get, writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { t } from '$lib/i18n';
import { notify } from './notifications';
import { toast } from './toasts';
import { recordActivity } from './activityLog';
import { reportBusy } from './globalBusy';
import { reportOperationOutcome } from './operationOutcome';

export type RenamePreviewItem = {
    original_path: string;
    new_path: string;
    original_name: string;
    new_name: string;
    is_dir: boolean;
    status: 'ready' | 'unchanged' | 'conflict' | 'invalid';
    reason: string | null;
};

export type BulkRenamePreviewResult = {
    success: boolean;
    error: string | null;
    scanned_items: number;
    ready_count: number;
    conflict_count: number;
    unchanged_count: number;
    invalid_count: number;
    items: RenamePreviewItem[];
};

export type RenameApplyResult = {
    original_path: string;
    new_path: string;
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
    bulk_rename: RecoveryTransactionSummary | null;
};

type RenameApplyResponse = {
    results: RenameApplyResult[];
    recovery: RecoveryTransactionSummary | null;
};

/**
 * Quality Pass Wave 1 / BR-3 (2026-05-29): saved rule preset. Captures
 * the full editable rule state — find/replace + regex toggle, prefix/
 * suffix (with their tokens), case modes, numbering. Roots, recovery
 * state, and preview/apply results are NOT part of a preset; those are
 * per-batch context, not a reusable rule template.
 */
export type RenamePreset = {
    id: string;
    name: string;
    find: string;
    replace: string;
    useRegex: boolean;
    prefix: string;
    suffix: string;
    caseMode: 'none' | 'lower' | 'upper' | 'title';
    extensionCase: 'keep' | 'lower' | 'upper';
    numberingEnabled: boolean;
    numberingStart: number;
    numberingPadding: number;
    numberingSeparator: string;
};

const PRESET_STORAGE_KEY = 'keepitlocal.bulkRename.presets';

function loadPresetsFromStorage(): RenamePreset[] {
    if (typeof localStorage === 'undefined') return [];
    try {
        const raw = localStorage.getItem(PRESET_STORAGE_KEY);
        if (!raw) return [];
        const parsed = JSON.parse(raw);
        return Array.isArray(parsed) ? parsed : [];
    } catch {
        return [];
    }
}

function savePresetsToStorage(presets: RenamePreset[]): void {
    if (typeof localStorage === 'undefined') return;
    try {
        localStorage.setItem(PRESET_STORAGE_KEY, JSON.stringify(presets));
    } catch {
        // Quota / disabled — silently ignored; presets just don't persist.
    }
}

export const bulkRenamePresets = writable<RenamePreset[]>(loadPresetsFromStorage());
bulkRenamePresets.subscribe(savePresetsToStorage);

export const bulkRenameRoots = writable<string[]>([]);
export const bulkRenameRecursive = writable(false);
export const bulkRenameIncludeFiles = writable(true);
export const bulkRenameIncludeDirs = writable(false);
export const bulkRenameIncludeHidden = writable(false);

export const bulkRenameFind = writable('');
export const bulkRenameReplace = writable('');
// Quality Pass Wave 1 / BR-1 (2026-05-28): regex toggle. When on,
// `find` is a Rust-regex pattern; `replace` supports `$1` / `$name`
// capture-group substitutions. Off = literal find/replace.
export const bulkRenameUseRegex = writable(false);
export const bulkRenamePrefix = writable('');
export const bulkRenameSuffix = writable('');
export const bulkRenameCaseMode = writable<'none' | 'lower' | 'upper' | 'title'>('none');
export const bulkRenameNumberingEnabled = writable(false);
export const bulkRenameNumberingStart = writable(1);
export const bulkRenameNumberingPadding = writable(3);
export const bulkRenameNumberingSeparator = writable('_');
export const bulkRenameExtensionCase = writable<'keep' | 'lower' | 'upper'>('keep');

export const bulkRenamePreviewing = writable(false);
export const bulkRenameApplying = writable(false);
export const bulkRenamePreview = writable<BulkRenamePreviewResult | null>(null);
export const bulkRenameResults = writable<RenameApplyResult[]>([]);
export const bulkRenameRecovery = writable<RecoveryTransactionSummary | null>(null);
export const bulkRenameLastJob = writable<string | null>(null);

let recoveryInitialized = false;

export const bulkRenameReadyItems = derived(bulkRenamePreview, ($preview) =>
    $preview?.items.filter((item) => item.status === 'ready') ?? [],
);

async function setBusy(busy: boolean) {
    try {
        await invoke('set_busy', { busy });
    } catch {
        // set_busy is best-effort; older builds/tools may not expose it.
    }
}

export function bulkRenameRootGuardMessage(path: string): string | null {
    const normalized = path.replaceAll('\\', '/').toLowerCase().replace(/\/+$/, '');
    if (!normalized || normalized === '/') return t('store.bulkRename.rootBlockedRoot');
    if (/^[a-z]:$/.test(normalized)) return t('store.bulkRename.rootBlockedDrive');

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
            return t('store.bulkRename.rootBlockedSystem');
        }
    }

    return null;
}

export function addBulkRenameRoots(paths: string[]) {
    const current = get(bulkRenameRoots);
    const known = new Set(current);
    const accepted: string[] = [];
    const blocked: string[] = [];

    for (const path of paths) {
        if (known.has(path)) continue;
        const warning = bulkRenameRootGuardMessage(path);
        if (warning) blocked.push(path);
        else accepted.push(path);
    }

    if (blocked.length) {
        toast(t('store.bulkRename.inputsBlockedToast', { values: { count: blocked.length } }), 'error');
        notify({
            level: 'warning',
            title: t('store.bulkRename.inputBlockedTitle'),
            message: t('store.bulkRename.inputBlockedMessage'),
            toolId: 'bulk-rename',
        });
    }

    if (!accepted.length) return;

    bulkRenameRoots.set([...current, ...accepted]);
    bulkRenamePreview.set(null);
    bulkRenameResults.set([]);
}

export function removeBulkRenameRoot(path: string) {
    bulkRenameRoots.update((roots) => roots.filter((root) => root !== path));
    bulkRenamePreview.set(null);
    bulkRenameResults.set([]);
}

export function clearBulkRenameInputs() {
    bulkRenameRoots.set([]);
    bulkRenamePreview.set(null);
    bulkRenameResults.set([]);
}

/**
 * Quality Pass Wave 1 / BR-3 (2026-05-29): capture the current rule
 * state into a named preset. Existing presets with the same trimmed
 * name are overwritten (so users can iterate on a preset without
 * accumulating duplicates).
 */
export function saveBulkRenamePresetFromCurrent(name: string): void {
    const trimmed = name.trim();
    if (!trimmed) return;
    const preset: RenamePreset = {
        id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        name: trimmed,
        find: get(bulkRenameFind),
        replace: get(bulkRenameReplace),
        useRegex: get(bulkRenameUseRegex),
        prefix: get(bulkRenamePrefix),
        suffix: get(bulkRenameSuffix),
        caseMode: get(bulkRenameCaseMode) as RenamePreset['caseMode'],
        extensionCase: get(bulkRenameExtensionCase) as RenamePreset['extensionCase'],
        numberingEnabled: get(bulkRenameNumberingEnabled),
        numberingStart: get(bulkRenameNumberingStart),
        numberingPadding: get(bulkRenameNumberingPadding),
        numberingSeparator: get(bulkRenameNumberingSeparator),
    };
    bulkRenamePresets.update((presets) => {
        const filtered = presets.filter((p) => p.name.trim() !== trimmed);
        return [...filtered, preset];
    });
}

/**
 * Apply a saved preset to the current rule state. Roots / preview /
 * recovery are intentionally NOT touched — a preset is a rule
 * template, not a session state.
 */
export function loadBulkRenamePreset(preset: RenamePreset): void {
    bulkRenameFind.set(preset.find);
    bulkRenameReplace.set(preset.replace);
    bulkRenameUseRegex.set(preset.useRegex);
    bulkRenamePrefix.set(preset.prefix);
    bulkRenameSuffix.set(preset.suffix);
    bulkRenameCaseMode.set(preset.caseMode);
    bulkRenameExtensionCase.set(preset.extensionCase);
    bulkRenameNumberingEnabled.set(preset.numberingEnabled);
    bulkRenameNumberingStart.set(preset.numberingStart);
    bulkRenameNumberingPadding.set(preset.numberingPadding);
    bulkRenameNumberingSeparator.set(preset.numberingSeparator);
}

export function deleteBulkRenamePreset(id: string): void {
    bulkRenamePresets.update((presets) => presets.filter((p) => p.id !== id));
}

export function resetBulkRenameRules() {
    bulkRenameFind.set('');
    bulkRenameReplace.set('');
    bulkRenameUseRegex.set(false);
    bulkRenamePrefix.set('');
    bulkRenameSuffix.set('');
    bulkRenameCaseMode.set('none');
    bulkRenameNumberingEnabled.set(false);
    bulkRenameNumberingStart.set(1);
    bulkRenameNumberingPadding.set(3);
    bulkRenameNumberingSeparator.set('_');
    bulkRenameExtensionCase.set('keep');
    bulkRenamePreview.set(null);
    bulkRenameResults.set([]);
}

export async function initBulkRenameRecovery(): Promise<void> {
    if (recoveryInitialized) return;
    recoveryInitialized = true;
    await refreshBulkRenameRecovery();
}

export async function refreshBulkRenameRecovery(): Promise<void> {
    try {
        const state = await invoke<FileRecoveryState>('get_file_recovery_state');
        bulkRenameRecovery.set(state.bulk_rename ?? null);
    } catch (error) {
        console.warn('Could not load bulk rename recovery state:', error);
    }
}

export async function previewBulkRename(): Promise<void> {
    if (get(bulkRenamePreviewing)) return toast(t('store.bulkRename.previewRunning'), 'info');
    if (get(bulkRenameApplying)) return toast(t('store.bulkRename.applyWaitFinish'), 'info');

    const roots = get(bulkRenameRoots);
    if (!roots.length) return toast(t('store.bulkRename.needRoots'), 'error');
    if (!get(bulkRenameIncludeFiles) && !get(bulkRenameIncludeDirs)) return toast(t('store.bulkRename.needTypes'), 'error');

    bulkRenamePreviewing.set(true);
    bulkRenameLastJob.set(t('store.bulkRename.previewBuildingJob'));
    bulkRenameResults.set([]);
    await setBusy(true);
    const stopBusy = reportBusy('bulk-rename', t('store.bulkRename.previewBusy'));

    const startedAt = Date.now();

    try {
        const response = await invoke<BulkRenamePreviewResult>('preview_bulk_rename', {
            options: {
                roots,
                recursive: get(bulkRenameRecursive),
                include_files: get(bulkRenameIncludeFiles),
                include_dirs: get(bulkRenameIncludeDirs),
                include_hidden: get(bulkRenameIncludeHidden),
                find: get(bulkRenameFind),
                replace: get(bulkRenameReplace),
                use_regex: get(bulkRenameUseRegex),
                prefix: get(bulkRenamePrefix),
                suffix: get(bulkRenameSuffix),
                case_mode: get(bulkRenameCaseMode),
                numbering_enabled: get(bulkRenameNumberingEnabled),
                numbering_start: Math.max(0, Number(get(bulkRenameNumberingStart)) || 0),
                numbering_padding: Math.max(1, Math.min(12, Number(get(bulkRenameNumberingPadding)) || 1)),
                numbering_separator: get(bulkRenameNumberingSeparator),
                extension_case: get(bulkRenameExtensionCase),
            },
        });

        bulkRenamePreview.set(response);
        const elapsed = Math.max(1, Math.round((Date.now() - startedAt) / 1000));

        if (!response.success) {
            const message = response.error ?? t('store.bulkRename.previewFailedFallback');
            notify({ level: 'error', title: t('store.bulkRename.previewFailedTitle'), message, toolId: 'bulk-rename' });
            toast(message, 'error');
        } else if (!response.ready_count) {
            notify({
                level: 'info',
                title: t('store.bulkRename.noRenamesTitle'),
                message: t('store.bulkRename.noRenamesMessage', { values: { count: response.scanned_items } }),
                toolId: 'bulk-rename',
            });
            toast(t('store.bulkRename.noRenamesToast'), 'info');
        } else {
            notify({
                level: 'success',
                title: t('store.bulkRename.renamesReadyTitle', { values: { count: response.ready_count } }),
                message: t('store.bulkRename.renamesReadyMessage', { values: { count: response.scanned_items, elapsed } }),
                toolId: 'bulk-rename',
            });
            toast(t('store.bulkRename.renamesReadyToast', { values: { count: response.ready_count } }), 'success');
        }
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        notify({ level: 'error', title: t('store.bulkRename.previewFailedTitle'), message, toolId: 'bulk-rename' });
        toast(message, 'error');
    } finally {
        bulkRenamePreviewing.set(false);
        bulkRenameLastJob.set(null);
        await setBusy(false);
        stopBusy();
    }
}

export async function applyBulkRename(): Promise<void> {
    if (get(bulkRenameApplying)) return toast(t('store.bulkRename.applyRunning'), 'info');
    if (get(bulkRenamePreviewing)) return toast(t('store.bulkRename.previewWaitFinish'), 'info');

    const readyItems = get(bulkRenameReadyItems);
    if (!get(bulkRenamePreview) || !readyItems.length) return toast(t('store.bulkRename.needPreview'), 'error');

    bulkRenameApplying.set(true);
    bulkRenameLastJob.set(t('store.bulkRename.applyingJob'));
    bulkRenameResults.set([]);
    await setBusy(true);
    const stopBusy = reportBusy('bulk-rename', t('store.bulkRename.applyBusy', { values: { count: readyItems.length } }));

    try {
        const response = await invoke<RenameApplyResponse>('apply_bulk_rename', {
            options: { items: readyItems },
        });

        bulkRenameResults.set(response.results);
        bulkRenameRecovery.set(response.recovery ?? null);
        const ok = response.results.filter((result) => result.success).length;
        const failed = response.results.length - ok;

        if (ok) {
            notify({
                level: failed ? 'warning' : 'success',
                title: failed ? t('store.bulkRename.renamedTitlePartial', { values: { ok, failed } }) : t('store.bulkRename.renamedTitleOk', { values: { count: ok } }),
                message: response.recovery
                    ? failed
                        ? t('store.bulkRename.renamedMessageUndoPartial')
                        : t('store.bulkRename.renamedMessageUndoOk')
                    : failed
                        ? t('store.bulkRename.renamedMessagePartial')
                        : t('store.bulkRename.renamedMessageOk'),
                toolId: 'bulk-rename',
            });
            toast(t('store.bulkRename.renamedToast', { values: { count: ok } }), 'success');
        }

        if (failed) {
            notify({
                level: 'error',
                title: t('store.bulkRename.renameFailedSomeTitle'),
                message: t('store.bulkRename.renameFailedSomeMessage', { values: { count: failed } }),
                toolId: 'bulk-rename',
            });
            toast(t('store.bulkRename.renameFailedSomeToast', { values: { count: failed } }), 'error');
        }

        void recordActivity({
            toolId: 'bulk-rename',
            summary: failed
                ? t('store.bulkRename.renamedActivityPartial', { values: { ok, failed } })
                : t('store.bulkRename.renamedActivityOk', { values: { count: ok } }),
            outcome: ok > 0 ? 'success' : 'failed',
        });

        bulkRenameRoots.set([]);
        bulkRenamePreview.set(null);
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        reportOperationOutcome({
            toolId: 'bulk-rename',
            kind: 'failed',
            notifyTitle: t('store.bulkRename.renameFailedTitle'),
            notifyMessage: message,
            toastMessage: message,
            activitySummary: t('store.bulkRename.renameFailedActivity'),
            activityDetails: message.slice(0, 200),
        });
    } finally {
        bulkRenameApplying.set(false);
        bulkRenameLastJob.set(null);
        await setBusy(false);
        stopBusy();
    }
}

export async function undoBulkRename(): Promise<void> {
    const recovery = get(bulkRenameRecovery);
    if (!recovery) return toast(t('store.bulkRename.noRecovery'), 'info');
    if (get(bulkRenameApplying)) return toast(t('store.bulkRename.renameRunning'), 'info');
    if (get(bulkRenamePreviewing)) return toast(t('store.bulkRename.restorePreviewWait'), 'info');

    bulkRenameApplying.set(true);
    bulkRenameLastJob.set(t('store.bulkRename.restoringJob'));
    await setBusy(true);
    const stopBusy = reportBusy('bulk-rename', t('store.bulkRename.restoreBusy'));

    try {
        const response = await invoke<RenameApplyResponse>('undo_bulk_rename', {
            transactionId: recovery.id,
        });

        bulkRenameResults.set(response.results);
        bulkRenameRecovery.set(response.recovery ?? null);

        const restored = response.results.filter((result) => result.success).length;
        const failed = response.results.length - restored;

        if (restored) {
            notify({
                level: failed ? 'warning' : 'success',
                title: failed ? t('store.bulkRename.restoredTitlePartial', { values: { restored, failed } }) : t('store.bulkRename.restoredTitleOk', { values: { count: restored } }),
                message: response.recovery
                    ? t('store.bulkRename.restoredMessageRecovery')
                    : t('store.bulkRename.restoredMessageOk'),
                toolId: 'bulk-rename',
            });
            toast(t('store.bulkRename.restoredToast', { values: { count: restored } }), failed ? 'info' : 'success');
        }

        if (failed) {
            notify({
                level: 'error',
                title: t('store.bulkRename.restoreFailedSomeTitle'),
                message: t('store.bulkRename.restoreFailedSomeMessage', { values: { count: failed } }),
                toolId: 'bulk-rename',
            });
            toast(t('store.bulkRename.restoreFailedSomeToast', { values: { count: failed } }), 'error');
        }

        void recordActivity({
            toolId: 'bulk-rename',
            summary: failed
                ? t('store.bulkRename.restoredActivityPartial', { values: { restored, failed } })
                : t('store.bulkRename.restoredActivityOk', { values: { count: restored } }),
            outcome: restored > 0 ? 'success' : 'failed',
        });
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        reportOperationOutcome({
            toolId: 'bulk-rename',
            kind: 'failed',
            notifyTitle: t('store.bulkRename.restoreFailedTitle'),
            notifyMessage: message,
            toastMessage: message,
            activitySummary: t('store.bulkRename.restoreFailedActivity'),
            activityDetails: message.slice(0, 200),
        });
    } finally {
        bulkRenameApplying.set(false);
        bulkRenameLastJob.set(null);
        await setBusy(false);
        stopBusy();
    }
}
