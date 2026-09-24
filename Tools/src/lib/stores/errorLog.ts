/*
  Frontend client for the local diagnostic log.

  How errors get into the log:
    1. Explicitly via `recordError(source, message, details?)` from
       any code that wants to flag a problem.
    2. Automatically by routing every `toast(..., 'error')` through
       this module (the toast store calls `recordError` for level
       'error'). This catches the bulk of user-visible failures
       without each tool having to remember to log.
    3. Automatically by `installGlobalErrorCapture()` mounted in
       +layout.svelte — hooks `window.addEventListener('error', …)`
       and `unhandledrejection` so render-tree-leaking errors land
       in the log instead of vanishing into the console.

  The Settings → Diagnostics panel reads via `listLogEntries` and
  exposes export / clear / open-folder buttons.

  Failures of the log writes themselves are silently swallowed —
  the caller's primary action already failed; we don't want a
  logging hiccup to look like a second cascade.
*/

import { invoke } from '@tauri-apps/api/core';

export type LogLevel = 'error' | 'warn' | 'info';

export interface LogEntry {
    timestampMs: number;
    level: LogLevel | string;
    source: string;
    message: string;
    details: string | null;
}

export interface RecordInput {
    level: LogLevel;
    source: string;
    message: string;
    details?: string | null;
}

/** Append a single entry to the on-disk log. Best-effort. */
export async function recordLog(input: RecordInput): Promise<void> {
    try {
        await invoke('log_event', {
            input: {
                level: input.level,
                source: input.source,
                message: input.message,
                details: input.details ?? null,
            },
        });
    } catch (error) {
        // Don't let a failed log write become a visible error.
        console.warn('error log write failed:', error);
    }
}

/** Convenience wrapper for the common case. */
export function recordError(source: string, message: string, details?: string | null) {
    void recordLog({ level: 'error', source, message, details });
}

export function recordWarning(source: string, message: string, details?: string | null) {
    void recordLog({ level: 'warn', source, message, details });
}

export function recordInfo(source: string, message: string, details?: string | null) {
    void recordLog({ level: 'info', source, message, details });
}

/** Read recent entries (newest first), optionally capped. */
export async function listLogEntries(limit?: number): Promise<LogEntry[]> {
    return invoke<LogEntry[]>('list_log_entries', { limit: limit ?? null });
}

export async function clearLogEntries(): Promise<void> {
    await invoke('clear_log_entries');
}

export async function getLogFolder(): Promise<string> {
    return invoke<string>('get_log_folder');
}

export async function exportLogText(): Promise<string> {
    return invoke<string>('export_log_text');
}

/**
 * Install global capture for uncaught browser errors + unhandled
 * promise rejections. Mounted once per webview from +layout.svelte.
 *
 * Returns a cleanup function the layout's onDestroy can call (mostly
 * for hot-reload tidiness — these listeners normally live for the
 * lifetime of the webview).
 */
export function installGlobalErrorCapture(): () => void {
    if (typeof window === 'undefined') return () => {};

    const onError = (event: ErrorEvent) => {
        // Skip resource-load errors (image 404s etc.) — they have
        // event.error === null and aren't useful diagnostic signal.
        if (!event.error) return;
        const err = event.error;
        const message = err instanceof Error ? err.message : String(err);
        const stack = err instanceof Error ? err.stack ?? null : null;
        recordError('frontend', message, stack);
    };

    const onUnhandled = (event: PromiseRejectionEvent) => {
        const reason = event.reason;
        const message =
            reason instanceof Error ? reason.message : String(reason ?? 'Unhandled rejection');
        const stack = reason instanceof Error ? reason.stack ?? null : null;
        recordError('frontend:promise', message, stack);
    };

    window.addEventListener('error', onError);
    window.addEventListener('unhandledrejection', onUnhandled);

    return () => {
        window.removeEventListener('error', onError);
        window.removeEventListener('unhandledrejection', onUnhandled);
    };
}

/**
 * Format a unix-ms stamp for display in the Diagnostics panel.
 * Uses the user's local time zone (vs. the export-text format
 * which is UTC) — when scanning recent entries you want them in
 * "your" time.
 */
export function formatLogTime(ms: number): string {
    const d = new Date(ms);
    const pad = (n: number) => n.toString().padStart(2, '0');
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}
