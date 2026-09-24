import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { notify } from './notifications';
import { toast } from './toasts';
import { recordActivity } from './activityLog';
import { reportBusy } from './globalBusy';

export type WordMarkdownResult = {
    source_path: string;
    output_path: string;
    paragraphs: number;
    headings: number;
    characters: number;
    success: boolean;
    error: string | null;
    warnings: string[];
};

export type WordMarkdownProgress = {
    operation_id: string;
    current_file: string;
    files_done: number;
    files_total: number;
    cancelled: boolean;
};

export const wordMarkdownFiles = writable<string[]>([]);
export const wordMarkdownOutputDir = writable<string | null>(null);
export const wordMarkdownProcessing = writable(false);
export const wordMarkdownCancelling = writable(false);
export const wordMarkdownProgress = writable<WordMarkdownProgress | null>(null);
export const wordMarkdownResults = writable<WordMarkdownResult[]>([]);
export const wordMarkdownError = writable<string | null>(null);

let activeOperationId: string | null = null;
let initialized = false;
let unlisten: UnlistenFn | null = null;

async function setBusy(busy: boolean) {
    try {
        await invoke('set_busy', { busy });
    } catch {}
}

function makeOperationId() {
    return `word-markdown-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function normalizePath(path: string) {
    return path.toLowerCase();
}

async function collectWordMarkdownSources(raw: string[]) {
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
        wordMarkdownError.set(response.warnings.join('\n'));
    }

    return next;
}

export async function initWordMarkdownListeners() {
    if (initialized) return;
    initialized = true;
    unlisten = await listen<WordMarkdownProgress>('word-markdown-progress', (event) => {
        if (!activeOperationId || event.payload.operation_id !== activeOperationId) return;
        wordMarkdownProgress.set(event.payload);
    });
}

export async function addWordMarkdownFiles(paths: string[]) {
    const current = get(wordMarkdownFiles);
    const discovered = await collectWordMarkdownSources(paths);
    if (!discovered.length) return;

    const seen = new Set(current.map((path) => normalizePath(path)));
    const next = discovered.filter((path) => !seen.has(normalizePath(path)));
    wordMarkdownFiles.set([...current, ...next]);
}

export function removeWordMarkdownFile(path: string) {
    wordMarkdownFiles.update((files) => files.filter((f) => f !== path));
}

export function clearWordMarkdownState() {
    if (get(wordMarkdownProcessing)) return;
    wordMarkdownFiles.set([]);
    wordMarkdownResults.set([]);
    wordMarkdownError.set(null);
    wordMarkdownProgress.set(null);
}

export function clearWordMarkdownResults() {
    wordMarkdownResults.set([]);
    wordMarkdownError.set(null);
    wordMarkdownProgress.set(null);
}

export async function runWordMarkdown() {
    const paths = get(wordMarkdownFiles);
    if (!paths.length || get(wordMarkdownProcessing)) return;

    const operationId = makeOperationId();
    activeOperationId = operationId;
    wordMarkdownProcessing.set(true);
    wordMarkdownCancelling.set(false);
    wordMarkdownResults.set([]);
    wordMarkdownError.set(null);
    wordMarkdownProgress.set({
        operation_id: operationId,
        current_file: '',
        files_done: 0,
        files_total: paths.length,
        cancelled: false,
    });
    await setBusy(true);
    const stopBusy = reportBusy('word-converter', `Converting ${paths.length} doc${paths.length === 1 ? '' : 's'} to Markdown`);

    try {
        const results = await invoke<WordMarkdownResult[]>('word_to_markdown', {
            options: {
                paths,
                output_dir: get(wordMarkdownOutputDir),
                operation_id: operationId,
            },
        });
        wordMarkdownResults.set(results);

        const ok = results.filter((r) => r.success).length;
        const fail = results.filter((r) => !r.success).length;
        const cancelled = ok + fail < paths.length || get(wordMarkdownCancelling);

        if (cancelled) {
            notify({ level: 'warning', title: 'Word to Markdown cancelled', message: `${ok} converted before cancellation`, toolId: 'word-markdown' });
            toast('Word to Markdown cancelled', 'info');
            void recordActivity({
                toolId: 'word-converter',
                summary: `Word → Markdown cancelled`,
                details: `${ok} converted before cancel · ${paths.length} requested`,
                outcome: 'cancelled',
            });
        } else if (fail === 0) {
            notify({ level: 'success', title: `Converted ${ok} document${ok === 1 ? '' : 's'} to Markdown`, toolId: 'word-markdown' });
            toast(`Converted ${ok} to Markdown`, 'success');
            void recordActivity({
                toolId: 'word-converter',
                summary: `Converted ${ok} Word doc${ok === 1 ? '' : 's'} → Markdown`,
                outcome: 'success',
            });
        } else if (ok === 0) {
            notify({ level: 'error', title: 'Word to Markdown failed', message: `${fail} document${fail === 1 ? '' : 's'} failed`, toolId: 'word-markdown' });
            toast('Conversion failed', 'error');
            void recordActivity({
                toolId: 'word-converter',
                summary: `Word → Markdown failed`,
                details: `All ${fail} document${fail === 1 ? '' : 's'} failed`,
                outcome: 'failed',
            });
        } else {
            notify({ level: 'warning', title: 'Word to Markdown finished with errors', message: `${ok} done, ${fail} failed`, toolId: 'word-markdown' });
            toast(`${ok} done, ${fail} failed`, 'info');
            void recordActivity({
                toolId: 'word-converter',
                summary: `Word → Markdown finished with errors`,
                details: `${ok} converted · ${fail} failed`,
                outcome: 'success',
            });
        }
    } catch (e) {
        const msg = String(e);
        wordMarkdownError.set(msg);
        const isCancel = msg.includes('Cancelled');
        notify({ level: isCancel ? 'warning' : 'error', title: isCancel ? 'Word to Markdown cancelled' : 'Word to Markdown failed', message: msg, toolId: 'word-markdown' });
        toast(isCancel ? 'Conversion cancelled' : 'Conversion failed', isCancel ? 'info' : 'error');
        void recordActivity({
            toolId: 'word-converter',
            summary: isCancel ? `Word → Markdown cancelled` : `Word → Markdown failed`,
            details: msg.slice(0, 200),
            outcome: isCancel ? 'cancelled' : 'failed',
        });
    } finally {
        wordMarkdownProcessing.set(false);
        wordMarkdownCancelling.set(false);
        activeOperationId = null;
        await setBusy(false);
        stopBusy();
    }
}

export async function cancelWordMarkdown() {
    if (!activeOperationId || get(wordMarkdownCancelling)) return;
    wordMarkdownCancelling.set(true);
    try {
        await invoke('cancel_word_markdown_operation', {
            operationId: activeOperationId,
        });
    } catch (e) {
        wordMarkdownError.set(String(e));
    }
}
