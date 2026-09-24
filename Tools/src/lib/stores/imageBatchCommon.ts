import { invoke } from '@tauri-apps/api/core';

export type ImageResult = {
    source_path: string;
    output_path: string;
    original_size: number;
    new_size: number;
    original_dims: [number, number];
    new_dims: [number, number];
    success: boolean;
    error: string | null;
};

export type Base64ImageResult = {
    source_path: string;
    file_name: string;
    mime: string;
    data_uri: string;
    raw_base64: string;
    original_size: number;
    base64_len: number;
    success: boolean;
    error: string | null;
};

export type ImageProgress = {
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

export function fmtBytes(value: number) {
    if (!value) return '0 B';
    if (value < 1024) return `${value} B`;
    if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
    if (value < 1024 * 1024 * 1024) return `${(value / 1024 / 1024).toFixed(2)} MB`;
    return `${(value / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function addUnique(existing: string[], incoming: string[]) {
    const seen = new Set(existing);
    const next = [...existing];
    for (const path of incoming) {
        if (!seen.has(path)) {
            seen.add(path);
            next.push(path);
        }
    }
    return next;
}

export function initialProgress(paths: string[], operationId: string, tool: string): Record<string, ImageProgress> {
    return Object.fromEntries(
        paths.map((path, index) => [
            path,
            {
                operation_id: operationId,
                tool,
                source_path: path,
                file_name: fileName(path),
                index,
                total: paths.length,
                stage: 'queued',
                progress: 0,
                message: null,
            },
        ]),
    );
}

export async function setBusy(busy: boolean) {
    try {
        await invoke('set_busy', { busy });
    } catch {
        // set_busy is a UX helper. The tool should still work if unavailable.
    }
}

export async function cancelImageOperation(operationId: string) {
    if (!operationId) return;
    await invoke('cancel_image_operation', { operationId });
}
