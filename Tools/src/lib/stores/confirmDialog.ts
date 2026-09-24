/*
  In-app confirm dialog — themed, queued, drop-in replacement for
  `@tauri-apps/plugin-dialog`'s `confirm()`.

  Why this exists:
    The Tauri dialog plugin shows native OS dialogs. They work, but
    they look like Windows 11 (or macOS, depending on host) — not like
    KeepItLocal. For destructive actions (clear history, delete snippet,
    etc.) we want a confirm that reads as part of our UI: same panel
    background, same border tokens, same accent color, same kbd hints.

  Architecture:
    - A singleton ConfirmDialog component is mounted once in the root
      layout, so every window (main + search overlay) gets its own
      instance via Svelte's normal per-window mounting.
    - A writable store holds the currently-active dialog (or null).
      The component reads it and renders the modal when non-null.
    - `confirm(message, options?)` returns a Promise<boolean> that
      resolves true on confirm, false on cancel/Esc/backdrop-click.
    - Dialogs queue: if a second `confirm()` lands while one is open,
      it waits in line and shows when the current one resolves. Keeps
      multiple async flows from racing each other for the same UI.

  API mirrors the plugin's so the migration is one-line:
    import { confirm } from '@tauri-apps/plugin-dialog';
        →
    import { confirm } from '$lib/stores/confirmDialog';
*/

import { get, writable } from 'svelte/store';

/** Visual variant — drives the icon + accent color of the dialog
 *  header. Mostly cosmetic; semantic for assistive tech. */
export type ConfirmKind = 'warning' | 'error' | 'info' | 'question';

export interface ConfirmOptions {
    /** Title shown in bold at the top of the dialog. Defaults to
     *  "Are you sure?" for question-style dialogs, "Confirm" otherwise. */
    title?: string;
    /** Visual + semantic kind. Defaults to 'question'. */
    kind?: ConfirmKind;
    /** Confirm-button label. Defaults to "OK". */
    confirmLabel?: string;
    /** Cancel-button label. Defaults to "Cancel". */
    cancelLabel?: string;
    /** When true, renders the confirm button in destructive (red)
     *  styling and focuses the cancel button by default — so a
     *  thoughtless Enter doesn't fire a destructive action. */
    danger?: boolean;
}

/**
 * Internal shape of an active dialog — what the component actually
 * renders. Adds the message body and the resolved-on-pick callbacks.
 */
export interface ActiveConfirmDialog extends Required<Omit<ConfirmOptions, 'danger'>> {
    message: string;
    danger: boolean;
    onConfirm: () => void;
    onCancel: () => void;
}

const DEFAULT_TITLES: Record<ConfirmKind, string> = {
    warning: 'Heads up',
    error: 'Problem',
    info: 'Notice',
    question: 'Are you sure?',
};

/** Currently-rendered dialog, or null when nothing's showing. The
 *  ConfirmDialog component subscribes to this and renders accordingly. */
export const activeConfirm = writable<ActiveConfirmDialog | null>(null);

/** Pending dialogs waiting for the current one to resolve. */
const queue: Array<{ options: Required<ConfirmOptions>; message: string; resolve: (b: boolean) => void }> = [];

/**
 * Show a themed confirm dialog. Returns Promise<boolean> resolving
 * `true` on confirm, `false` on cancel / Esc / backdrop click.
 *
 * Drop-in replacement for `@tauri-apps/plugin-dialog`'s `confirm()`.
 */
export function confirm(message: string, options?: ConfirmOptions): Promise<boolean> {
    return new Promise((resolve) => {
        const kind = options?.kind ?? 'question';
        const filled: Required<ConfirmOptions> = {
            title: options?.title ?? DEFAULT_TITLES[kind],
            kind,
            confirmLabel: options?.confirmLabel ?? 'OK',
            cancelLabel: options?.cancelLabel ?? 'Cancel',
            danger: options?.danger ?? (kind === 'warning' || kind === 'error'),
        };
        const item = { options: filled, message, resolve };
        if (get(activeConfirm)) {
            queue.push(item);
        } else {
            present(item);
        }
    });
}

function present(item: { options: Required<ConfirmOptions>; message: string; resolve: (b: boolean) => void }) {
    activeConfirm.set({
        message: item.message,
        title: item.options.title,
        kind: item.options.kind,
        confirmLabel: item.options.confirmLabel,
        cancelLabel: item.options.cancelLabel,
        danger: item.options.danger,
        onConfirm: () => resolveAndAdvance(item, true),
        onCancel: () => resolveAndAdvance(item, false),
    });
}

function resolveAndAdvance(
    item: { resolve: (b: boolean) => void },
    answer: boolean,
) {
    item.resolve(answer);
    activeConfirm.set(null);
    const next = queue.shift();
    if (next) {
        // Microtask defer so the close-then-open transition reads as
        // two distinct dialogs instead of a flash-swap.
        queueMicrotask(() => present(next));
    }
}
