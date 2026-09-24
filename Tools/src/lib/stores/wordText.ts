import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { notify } from './notifications';
import { toast } from './toasts';
import { recordActivity } from './activityLog';
import { reportBusy } from './globalBusy';

export type WordTextResult = {
    source_path: string;
    output_path: string;
    paragraphs: number;
    lines: number;
    characters: number;
    success: boolean;
    error: string | null;
    warnings: string[];
};

export type WordTextProgress = {
    operation_id: string;
    current_file: string;
    files_done: number;
    files_total: number;
    cancelled: boolean;
};

export const wordTextFiles = writable<string[]>([]);
export const wordTextOutputDir = writable<string | null>(null);
export const wordTextProcessing = writable(false);
export const wordTextCancelling = writable(false);
export const wordTextProgress = writable<WordTextProgress | null>(null);
export const wordTextResults = writable<WordTextResult[]>([]);
export const wordTextError = writable<string | null>(null);

let activeOperationId: string | null = null;
let initialized = false;
let unlisten: UnlistenFn | null = null;

async function setBusy(busy: boolean) {
    try {
        await invoke('set_busy', { busy });
    } catch {}
}

function makeOperationId() {
    return `word-text-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function normalizePath(path: string) {
    return path.toLowerCase();
}

async function collectWordTextSources(raw: string[]) {
    if (!raw.length) return [] as string[];

    const response = await invoke<{
        paths: string[];
        warnings: string[];
    }>('collect_word_docx_sources', {
        options: {
            paths: raw,
            recursive: true,
        },
    });

    const next = response.paths ?? [];
    if (next.length === 0 && response.warnings.length > 0) {
        wordTextError.set(response.warnings.join('\n'));
    }

    return next;
}

export async function initWordTextListeners() {
    if (initialized) return;
    initialized = true;
    unlisten = await listen<WordTextProgress>('word-text-progress', (event) => {
        if (!activeOperationId || event.payload.operation_id !== activeOperationId) return;
        wordTextProgress.set(event.payload);
    });
}

export async function addWordTextFiles(paths: string[]) {
    const current = get(wordTextFiles);
    const discovered = await collectWordTextSources(paths);
    if (!discovered.length) return;

    const seen = new Set(current.map((path) => normalizePath(path)));
    const next = discovered.filter((path) => !seen.has(normalizePath(path)));
    wordTextFiles.set([...current, ...next]);
}

export function removeWordTextFile(path: string) {
    wordTextFiles.update((files) => files.filter((f) => f !== path));
}

export function clearWordTextState() {
    if (get(wordTextProcessing)) return;
    wordTextFiles.set([]);
    wordTextResults.set([]);
    wordTextError.set(null);
    wordTextProgress.set(null);
}

export function clearWordTextResults() {
    wordTextResults.set([]);
    wordTextError.set(null);
    wordTextProgress.set(null);
}

export async function runWordText() {
    const paths = get(wordTextFiles);
    if (!paths.length || get(wordTextProcessing)) return;

    const operationId = makeOperationId();
    activeOperationId = operationId;
    wordTextProcessing.set(true);
    wordTextCancelling.set(false);
    wordTextResults.set([]);
    wordTextError.set(null);
    wordTextProgress.set({
        operation_id: operationId,
        current_file: '',
        files_done: 0,
        files_total: paths.length,
        cancelled: false,
    });
    await setBusy(true);
    const stopBusy = reportBusy('word-converter', `Converting ${paths.length} doc${paths.length === 1 ? '' : 's'} to text`);

    try {
        const results = await invoke<WordTextResult[]>('word_to_text', {
            options: {
                paths,
                output_dir: get(wordTextOutputDir),
                operation_id: operationId,
            },
        });
        wordTextResults.set(results);

        const ok = results.filter((r) => r.success).length;
        const fail = results.filter((r) => !r.success).length;
        const cancelled = ok + fail < paths.length || get(wordTextCancelling);

        if (cancelled) {
            notify({ level: 'warning', title: 'Word to Text cancelled', message: `${ok} converted before cancellation`, toolId: 'word-text' });
            toast('Word to Text cancelled', 'info');
            void recordActivity({
                toolId: 'word-converter',
                summary: `Word → Plain text cancelled`,
                details: `${ok} converted before cancel · ${paths.length} requested`,
                outcome: 'cancelled',
            });
        } else if (fail === 0) {
            notify({ level: 'success', title: `Converted ${ok} document${ok === 1 ? '' : 's'} to Text`, toolId: 'word-text' });
            toast(`Converted ${ok} to Text`, 'success');
            void recordActivity({
                toolId: 'word-converter',
                summary: `Converted ${ok} Word doc${ok === 1 ? '' : 's'} → Plain text`,
                outcome: 'success',
            });
        } else if (ok === 0) {
            notify({ level: 'error', title: 'Word to Text failed', message: `${fail} document${fail === 1 ? '' : 's'} failed`, toolId: 'word-text' });
            toast('Conversion failed', 'error');
            void recordActivity({
                toolId: 'word-converter',
                summary: `Word → Plain text failed`,
                details: `All ${fail} document${fail === 1 ? '' : 's'} failed`,
                outcome: 'failed',
            });
        } else {
            notify({ level: 'warning', title: 'Word to Text finished with errors', message: `${ok} done, ${fail} failed`, toolId: 'word-text' });
            toast(`${ok} done, ${fail} failed`, 'info');
            void recordActivity({
                toolId: 'word-converter',
                summary: `Word → Plain text finished with errors`,
                details: `${ok} converted · ${fail} failed`,
                outcome: 'success',
            });
        }
    } catch (e) {
        const msg = String(e);
        wordTextError.set(msg);
        const isCancel = msg.includes('Cancelled');
        notify({
            level: isCancel ? 'warning' : 'error',
            title: isCancel ? 'Word to Text cancelled' : 'Word to Text failed',
            message: msg,
            toolId: 'word-text',
        });
        toast(isCancel ? 'Conversion cancelled' : 'Conversion failed', isCancel ? 'info' : 'error');
        void recordActivity({
            toolId: 'word-converter',
            summary: isCancel ? `Word → Plain text cancelled` : `Word → Plain text failed`,
            details: msg.slice(0, 200),
            outcome: isCancel ? 'cancelled' : 'failed',
        });
    } finally {
        wordTextProcessing.set(false);
        wordTextCancelling.set(false);
        activeOperationId = null;
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelWordText() {
    if (!activeOperationId || get(wordTextCancelling)) return;
    wordTextCancelling.set(true);
    try {
        await invoke('cancel_word_text_operation', {
            operationId: activeOperationId,
        });
    } catch (e) {
        wordTextError.set(String(e));
    }
}
