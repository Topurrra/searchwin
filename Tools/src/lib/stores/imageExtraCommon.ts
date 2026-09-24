import { invoke } from '@tauri-apps/api/core';

export type ImageExtraProgress = {
    operation_id: string;
    tool: string;
    source_path: string;
    file_name: string;
    index: number;
    total: number;
    stage: string;
    progress: number;
    message: string | null;
};

export function newOperationId(prefix: string) {
    const id = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    return `${prefix}-${id}`;
}

export function fileName(path: string) {
    return path.split(/[\\/]/).pop() || path;
}

export function fileStem(path: string) {
    return fileName(path).replace(/\.[^.]+$/, '') || 'app';
}

export function fmtBytes(value: number) {
    if (!value) return '0 B';
    if (value < 1024) return `${value} B`;
    if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
    if (value < 1024 * 1024 * 1024) return `${(value / 1024 / 1024).toFixed(2)} MB`;
    return `${(value / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export async function setBusy(busy: boolean) {
    try {
        await invoke('set_busy', { busy });
    } catch {
        // Optional UX helper.
    }
}

export async function cancelImageExtraOperation(operationId: string) {
    if (!operationId) return;
    await invoke('cancel_image_extra_operation', { operationId });
}
