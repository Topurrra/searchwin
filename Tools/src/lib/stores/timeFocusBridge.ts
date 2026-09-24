/*
  timeFocusBridge — lets the command palette (its own window) drive the Time &
  Focus features that live in the MAIN window.

  Why a bridge: the palette is a separate Tauri window, and the Focus timer
  is in-memory per-window state — calling the stores from the
  palette would act on a different instance. So the palette EMITS an event and
  the main window (where the real timers + reminder scheduler run) handles it.
  Reminders go this way too (rather than cross-window localStorage sync, which
  doesn't fire reliably across WebView2 windows).

  Feedback: a palette command runs in the background (the main window usually
  isn't shown), so each action fires a SYSTEM notification to confirm it — the
  only feedback that's reliably visible regardless of window state.

  Started once from the main window (+page). Importing this module pulls in the
  focus store so the actions work even if the user never opened those tool
  pages this session.
*/
import { listen } from '@tauri-apps/api/event';
import { get } from 'svelte/store';
import { focusConfig, startFocus, endFocus } from './focusMode';
import { addReminder } from './reminders';
import { captureNote } from './notes';
import { notifyOS } from './notify';

let started = false;

/** Wire up the palette → main event listeners. Idempotent. */
export async function startTimeFocusBridge() {
    if (started || typeof window === 'undefined') return;
    started = true;
    try {
        await listen<{ text?: string; dueMs?: number }>('palette-create-reminder', (event) => {
            const { text, dueMs } = event.payload ?? {};
            if (typeof text === 'string' && typeof dueMs === 'number' && dueMs > Date.now()) {
                addReminder(text, dueMs);
                void notifyOS('Reminder set', text, 'reminder');
            }
        });

        // Quick-capture a note from the palette ("note: <text>"). Created here
        // in the main window so the Notes list refreshes; a system toast
        // confirms since the palette has already closed.
        await listen<{ text?: string }>('palette-create-note', (event) => {
            const text = event.payload?.text;
            if (typeof text === 'string' && text.trim()) {
                void captureNote(text).then((summary) => {
                    if (summary) void notifyOS('Note saved', summary.title, 'info');
                });
            }
        });

        await listen<{ action?: string }>('palette-focus', (event) => {
            switch (event.payload?.action) {
                case 'start': {
                    // startFocus no-ops on an empty list; tell the user why
                    // instead of silently doing nothing. Which list matters
                    // depends on the mode (block-list vs allow-list).
                    const cfg = get(focusConfig);
                    const list = cfg.mode === 'allowlist' ? cfg.allowlist : cfg.blocklist;
                    if (list.length === 0) {
                        void notifyOS(
                            cfg.mode === 'allowlist'
                                ? 'Focus needs an allow list'
                                : 'Focus needs a blocklist',
                            cfg.mode === 'allowlist'
                                ? 'Add the apps you want to allow in the Focus Mode tool first.'
                                : 'Add apps to block in the Focus Mode tool first.',
                            'warn',
                        );
                    } else {
                        startFocus(); // the store fires its own "Focus mode on" notification
                    }
                    break;
                }
                case 'stop':
                    endFocus('Focus ended from the command palette.');
                    break;
            }
        });
    } catch {
        // events unavailable (plain-browser dev) — palette integration is a no-op
    }
}
