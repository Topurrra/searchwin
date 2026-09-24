import { writable } from 'svelte/store';
import { recordLog } from './errorLog';

export type ToastLevel = 'info' | 'success' | 'error';

export interface Toast {
    id: string;
    level: ToastLevel;
    message: string;
}

export const toasts = writable<Toast[]>([]);

export function toast(message: string, level: ToastLevel = 'info', durationMs = 2500) {
    const id = `${Date.now()}_${Math.random().toString(36).slice(2, 6)}`;
    toasts.update(arr => [...arr, { id, level, message }]);
    setTimeout(() => {
        toasts.update(arr => arr.filter(t => t.id !== id));
    }, durationMs);

    // Auto-route error toasts into the diagnostic log so a user
    // reporting "I keep seeing 'Foo bar failed'" can dump the log
    // without us having to instrument every call site individually.
    // Best-effort fire-and-forget — recordLog swallows its own errors.
    if (level === 'error') {
        void recordLog({ level: 'error', source: 'toast', message });
    }
}