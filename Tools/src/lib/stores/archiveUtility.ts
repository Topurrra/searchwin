import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { get, writable } from 'svelte/store';
import { toast } from '$lib/stores/toasts';

export type ArchiveFormat = 'zip' | '7z' | 'tar.zst' | 'tar.gz' | 'tar.xz';
export type ArchiveTab = 'create' | 'extract';

export type ArchiveProgress = {
    current_file: string;
    files_done: number;
    files_total: number;
    bytes_done: number;
    bytes_total: number;
};

export type ArchiveEntry = {
    name: string;
    size: number;
    is_dir: boolean;
    encrypted: boolean;
};

export type ArchiveInfo = {
    format: string;
    entries: ArchiveEntry[];
    total_size: number;
    has_encryption: boolean;
};

export type ArchiveCreateResult = {
    output_path: string;
    total_files: number;
    original_size: number;
    archive_size: number;
    elapsed_ms: number;
};

export type ArchiveExtractResult = {
    output_dir: string;
    format: string;
    total_files: number;
    total_bytes: number;
    elapsed_ms: number;
    skipped: string[];
};

export const archiveTab = writable<ArchiveTab>('create');
export const archiveCreatePaths = writable<string[]>([]);
export const archiveCreateFormat = writable<ArchiveFormat>('zip');
export const archiveCreateLevel = writable(6);
export const archiveCreateOutputPath = writable('');
export const archiveCreateProcessing = writable(false);
export const archiveCreateCancelling = writable(false);
export const archiveCreateProgress = writable<ArchiveProgress | null>(null);
export const archiveCreateResult = writable<ArchiveCreateResult | null>(null);
export const archiveCreateError = writable<string | null>(null);

export const archiveExtractPath = writable('');
export const archiveExtractInfo = writable<ArchiveInfo | null>(null);
export const archiveExtractOutputDir = writable('');
export const archiveExtractOverwrite = writable(false);
export const archiveExtractProcessing = writable(false);
export const archiveExtractCancelling = writable(false);
export const archiveExtractProgress = writable<ArchiveProgress | null>(null);
export const archiveExtractResult = writable<ArchiveExtractResult | null>(null);
export const archiveExtractError = writable<string | null>(null);

let progressUnlisten: UnlistenFn | null = null;
let progressListenerStarting = false;

function isArchiveBusy(): boolean {
    return get(archiveCreateProcessing) || get(archiveExtractProcessing);
}

function isArchiveCancellation(cause: unknown): boolean {
    return String(cause).includes('Cancelled by user');
}

export async function initArchiveUtility(): Promise<void> {
    if (progressUnlisten || progressListenerStarting) return;
    progressListenerStarting = true;
    try {
        progressUnlisten = await listen<ArchiveProgress>('archive-progress', (event) => {
            if (get(archiveCreateProcessing)) archiveCreateProgress.set(event.payload);
            if (get(archiveExtractProcessing)) archiveExtractProgress.set(event.payload);
        });
    } finally {
        progressListenerStarting = false;
    }
}

export function addArchiveCreatePaths(paths: string[]): void {
    if (isArchiveBusy()) return;
    archiveCreatePaths.update((current) => [
        ...current,
        ...paths.filter((path) => path && !current.includes(path)),
    ]);
    archiveCreateResult.set(null);
    archiveCreateError.set(null);
}

export function removeArchiveCreatePath(path: string): void {
    if (isArchiveBusy()) return;
    archiveCreatePaths.update((current) => current.filter((item) => item !== path));
}

export function clearArchiveCreate(): void {
    if (isArchiveBusy()) return;
    archiveCreatePaths.set([]);
    archiveCreateOutputPath.set('');
    archiveCreateResult.set(null);
    archiveCreateError.set(null);
    archiveCreateProgress.set(null);
}

export async function runArchiveCreate(password: string | null): Promise<ArchiveCreateResult | null> {
    if (get(archiveCreateProcessing) || get(archiveExtractProcessing)) return null;

    const paths = get(archiveCreatePaths);
    const output_path = get(archiveCreateOutputPath);
    if (!paths.length || !output_path) return null;

    archiveCreateProcessing.set(true);
    archiveCreateCancelling.set(false);
    archiveCreateProgress.set(null);
    archiveCreateResult.set(null);
    archiveCreateError.set(null);
    try {
        const result = await invoke<ArchiveCreateResult>('create_archive', {
            options: {
                paths,
                output_path,
                format: get(archiveCreateFormat),
                password: password || null,
                compression_level: get(archiveCreateLevel),
            },
        });
        archiveCreateResult.set(result);
        toast('Archive created.', 'success');
        return result;
    } catch (cause) {
        if (isArchiveCancellation(cause)) toast('Archive creation cancelled.', 'info');
        else archiveCreateError.set(String(cause));
        return null;
    } finally {
        archiveCreateProcessing.set(false);
        archiveCreateCancelling.set(false);
        archiveCreateProgress.set(null);
    }
}

export async function inspectArchive(path: string, password: string | null = null): Promise<ArchiveInfo | null> {
    if (isArchiveBusy()) return null;
    archiveExtractPath.set(path);
    archiveExtractInfo.set(null);
    archiveExtractResult.set(null);
    archiveExtractError.set(null);
    try {
        const info = await invoke<ArchiveInfo>('inspect_archive', {
            options: { path, password: password || null },
        });
        archiveExtractInfo.set(info);
        return info;
    } catch (cause) {
        archiveExtractError.set(String(cause));
        return null;
    }
}

export function clearArchiveExtract(): void {
    if (isArchiveBusy()) return;
    archiveExtractPath.set('');
    archiveExtractInfo.set(null);
    archiveExtractOutputDir.set('');
    archiveExtractOverwrite.set(false);
    archiveExtractResult.set(null);
    archiveExtractError.set(null);
    archiveExtractProgress.set(null);
}

export async function runArchiveExtract(password: string | null): Promise<ArchiveExtractResult | null> {
    if (get(archiveCreateProcessing) || get(archiveExtractProcessing)) return null;

    const archive_path = get(archiveExtractPath);
    const output_dir = get(archiveExtractOutputDir);
    if (!archive_path || !output_dir) return null;

    archiveExtractProcessing.set(true);
    archiveExtractCancelling.set(false);
    archiveExtractProgress.set(null);
    archiveExtractResult.set(null);
    archiveExtractError.set(null);
    try {
        const result = await invoke<ArchiveExtractResult>('extract_archive', {
            options: {
                archive_path,
                output_dir,
                password: password || null,
                overwrite: get(archiveExtractOverwrite),
            },
        });
        archiveExtractResult.set(result);
        toast(`Extracted ${result.total_files} file${result.total_files === 1 ? '' : 's'}.`, 'success');
        return result;
    } catch (cause) {
        if (isArchiveCancellation(cause)) {
            toast('Extraction cancelled. Files already extracted remain in the destination.', 'info');
        } else {
            archiveExtractError.set(String(cause));
        }
        return null;
    } finally {
        archiveExtractProcessing.set(false);
        archiveExtractCancelling.set(false);
        archiveExtractProgress.set(null);
    }
}

export async function cancelArchiveOperation(): Promise<void> {
    const creating = get(archiveCreateProcessing);
    const extracting = get(archiveExtractProcessing);
    if (!creating && !extracting) return;
    if (creating) archiveCreateCancelling.set(true);
    if (extracting) archiveExtractCancelling.set(true);
    try {
        await invoke('cancel_archive_operation');
        toast('Cancelling archive operation…', 'info');
    } catch (cause) {
        if (creating) archiveCreateCancelling.set(false);
        if (extracting) archiveExtractCancelling.set(false);
        const message = String(cause);
        if (creating) archiveCreateError.set(message);
        if (extracting) archiveExtractError.set(message);
    }
}
