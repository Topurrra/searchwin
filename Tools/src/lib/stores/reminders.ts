/*
  reminders — local one-off reminders that fire a SYSTEM notification when due.

  Persisted to localStorage (survives restart). A single scheduler checks for
  due reminders on a short interval while the app runs — KeepItLocal lives in
  the tray, so "while running" is effectively always. (Firing when the app is
  fully closed would need Windows Task Scheduler integration — out of scope.)

  The scheduler is started once from the main window (+page) so reminders fire
  app-wide, even if you never open the Reminders tool this session.
*/
import { writable, get } from 'svelte/store';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { notifyOS } from './notify';
import { flashGlow } from './glow';

export interface Reminder {
    id: string;
    text: string;
    /** Epoch ms when it's due. */
    dueMs: number;
    /** Set once the notification has fired (kept briefly as history). */
    fired: boolean;
}

const KEY = 'keepitlocal_reminders_v1';

function load(): Reminder[] {
    if (typeof localStorage === 'undefined') return [];
    try {
        const raw = localStorage.getItem(KEY);
        if (!raw) return [];
        const parsed = JSON.parse(raw);
        if (!Array.isArray(parsed)) return [];
        return parsed.filter(
            (r): r is Reminder =>
                r &&
                typeof r.id === 'string' &&
                typeof r.text === 'string' &&
                typeof r.dueMs === 'number' &&
                typeof r.fired === 'boolean',
        );
    } catch {
        return [];
    }
}

export const reminders = writable<Reminder[]>(load());
reminders.subscribe((list) => {
    if (typeof localStorage === 'undefined') return;
    try {
        localStorage.setItem(KEY, JSON.stringify(list));
    } catch {
        // best effort
    }
});

function makeId(): string {
    return `rem-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/** Add a reminder. Returns it. */
export function addReminder(text: string, dueMs: number): Reminder {
    const reminder: Reminder = {
        id: makeId(),
        text: text.trim() || 'Reminder',
        dueMs,
        fired: dueMs <= Date.now(), // past time → treat as already fired (no surprise ping)
    };
    reminders.update((list) => [...list, reminder]);
    void scheduleOsTask(reminder);
    return reminder;
}

export function removeReminder(id: string) {
    reminders.update((list) => list.filter((r) => r.id !== id));
    void cancelOsTask(id);
}

/** Drop all already-fired reminders. */
export function clearFired() {
    reminders.update((list) => list.filter((r) => !r.fired));
}

// ─── Windows Task Scheduler integration (fire even when the app is closed) ──
// Each future reminder also registers a one-time scheduled task that relaunches
// KeepItLocal at its due time; on launch checkDue() below fires the toast+glow.
// All calls are best-effort no-ops outside Tauri / on non-Windows — the in-app
// (tray) firing path still works there.
async function scheduleOsTask(r: Reminder): Promise<void> {
    if (r.fired || r.dueMs <= Date.now()) return;
    try {
        await invoke('create_reminder_task', { id: r.id, dueMs: r.dueMs });
    } catch {
        // non-Tauri dev, non-Windows, or scheduler unavailable — in-app firing still works.
    }
}
async function cancelOsTask(id: string): Promise<void> {
    try {
        await invoke('delete_reminder_task', { id });
    } catch {
        // ignore
    }
}
async function reconcileOsTasks(): Promise<void> {
    try {
        const activeIds = get(reminders)
            .filter((r) => !r.fired)
            .map((r) => r.id);
        await invoke('reconcile_reminder_tasks', { activeIds });
    } catch {
        // ignore
    }
}

// ─── Scheduler ───────────────────────────────────────────────────────────
let started = false;

async function checkDue() {
    const now = Date.now();
    const list = get(reminders);
    const due = list.filter((r) => !r.fired && r.dueMs <= now);
    if (due.length === 0) return;
    // Glow FIRST — awaited so its window is shown before the toast window, which
    // then lands on top in z-order and stays fully legible over the glow. The
    // first-creation color race is fixed via the glow-ready handshake, so this
    // reliably shows GREEN. A touch longer than the focus glow — a reminder is a
    // one-shot, not a repeating pulse.
    await flashGlow('green', 2800);
    for (const r of due) {
        void notifyOS('Reminder', r.text, 'reminder');
        r.fired = true;
        void cancelOsTask(r.id); // one-shot scheduled task is done — clean it up
    }
    reminders.set([...list]);
}

/** Start the due-check loop. Idempotent — safe to call from multiple mounts.
 *
 *  The heartbeat comes from RUST (`reminders-tick`), not a JS setInterval:
 *  Chromium throttles background-window timers to ~once a minute while
 *  KeepItLocal is minimized to the tray, which made due reminders fire late.
 *  The Rust thread isn't throttled, so the check runs on time regardless of
 *  window state — putting reminders on the same footing as the other Rust-side
 *  background work (clipboard capture, file watcher, indexing, hotkeys). The
 *  firing itself still reuses the toast + glow path here. A 60 s setInterval
 *  stays as a fallback for non-Tauri contexts (plain-browser dev), where the
 *  Rust tick never arrives. */
export function startReminderScheduler() {
    if (started || typeof window === 'undefined') return;
    started = true;
    void checkDue(); // catch anything already overdue at startup
    void reconcileOsTasks(); // prune tasks for reminders removed/fired while closed
    void listen('reminders-tick', () => void checkDue()).catch(() => {});
    setInterval(() => void checkDue(), 60_000);
}

// ─── Natural-language time parsing ─────────────────────────────────────────
/**
 * Parse a "when" phrase into an epoch-ms due time, or null if not understood.
 * Handles: "in 20 min", "in 2 hours", "at 3pm", "3:30pm", "15:30", "9am",
 * and an optional leading "today"/"tomorrow". A bare past time-of-day rolls
 * to tomorrow so "at 8am" set at 10am means tomorrow 8am.
 */
export function parseReminderTime(input: string): number | null {
    const text = input.trim().toLowerCase();
    if (!text) return null;

    // Relative: "in N minutes / hours".
    const rel = text.match(/\bin\s+(\d+)\s*(m|min|mins|minute|minutes|h|hr|hrs|hour|hours)\b/);
    if (rel) {
        const n = parseInt(rel[1], 10);
        if (!Number.isFinite(n) || n <= 0) return null;
        const ms = rel[2].startsWith('h') ? n * 3_600_000 : n * 60_000;
        return Date.now() + ms;
    }

    // Optional day offset.
    let dayOffset = 0;
    let rest = text;
    if (/\btomorrow\b/.test(text)) {
        dayOffset = 1;
        rest = text.replace(/\btomorrow\b/, '').trim();
    } else if (/\btoday\b/.test(text)) {
        rest = text.replace(/\btoday\b/, '').trim();
    }

    // Time of day: "at 3pm", "3:30pm", "15:30", "9am".
    const tm = rest.match(/\b(?:at\s+)?(\d{1,2})(?::(\d{2}))?\s*(am|pm)?\b/);
    if (tm) {
        let hour = parseInt(tm[1], 10);
        const min = tm[2] ? parseInt(tm[2], 10) : 0;
        const ap = tm[3];
        if (ap === 'pm' && hour < 12) hour += 12;
        if (ap === 'am' && hour === 12) hour = 0;
        if (hour > 23 || min > 59) return null;
        const d = new Date();
        d.setDate(d.getDate() + dayOffset);
        d.setHours(hour, min, 0, 0);
        if (dayOffset === 0 && d.getTime() <= Date.now()) {
            d.setDate(d.getDate() + 1); // already passed today → tomorrow
        }
        return d.getTime();
    }

    return null;
}

/**
 * Parse a full "remind me to X at Y" phrase into { text, dueMs } — for the
 * command palette's reminder quick-add. Strips the "remind me [to]" lead, then
 * splits the remainder at the first time marker (" in ", " at ", "tomorrow",
 * "today") that yields a future time. Returns null if there's no parseable
 * time or no reminder text. Reuses `parseReminderTime` for the time portion.
 */
export function parseReminderInput(input: string): { text: string; dueMs: number } | null {
    const s = input
        .trim()
        .replace(/^remind\s+me\s+(?:to\s+)?/i, '')
        .replace(/^reminder[:\s]+/i, '')
        .trim();
    if (!s) return null;
    const lower = s.toLowerCase();
    const markers = [' in ', ' at ', ' tomorrow', ' today'];
    for (const marker of markers) {
        let idx = lower.indexOf(marker);
        while (idx !== -1) {
            const dueMs = parseReminderTime(s.slice(idx).trim());
            if (dueMs !== null && dueMs > Date.now()) {
                const text = s.slice(0, idx).trim().replace(/[,;]+$/, '');
                if (text) return { text, dueMs };
            }
            idx = lower.indexOf(marker, idx + 1);
        }
    }
    return null;
}
