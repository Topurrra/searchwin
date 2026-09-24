/*
  notify — KeepItLocal's notification channel for background events (Pomodoro
  phase changes, due reminders, Focus warnings) that must reach the user even
  when the app isn't the focused window.

  Why NOT the OS notification: on Windows, `@tauri-apps/plugin-notification`
  toasts silently no-op in dev (they need an installed Start-Menu shortcut /
  AUMID) and can be suppressed even when installed (focus assist, quiet hours).
  We hit "the reminder didn't notify me" repeatedly because of this. So the
  reliable channel is our OWN toast: a transparent, always-on-top overlay window
  (`/notify-toast`) that shows a card regardless of focus or tray state. It works
  identically in dev and prod.

  The native OS toast is kept (as `notifyOSNative`) for callers that specifically
  want an Action-Center entry, but it is NOT used by default — to avoid a double
  toast in installed builds.

  Best-effort + no-throw so a notification never breaks a timer or reminder.
*/
import { invoke } from '@tauri-apps/api/core';
import { emitTo, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
    isPermissionGranted,
    requestPermission,
    sendNotification,
} from '@tauri-apps/plugin-notification';

/** Visual category — tints the toast's accent strip. */
export type NotifyKind = 'info' | 'reminder' | 'focus' | 'pomodoro' | 'warn';

const TOAST_WINDOW = 'notify-toast';

/** One-time hook + latest payload for the toast "ready" handshake (mirrors the
 *  focus-glow `glow-ready` handshake). The toast window broadcasts
 *  `notify-toast-ready` when its listener is mounted; we re-send the latest
 *  payload then, so the FIRST toast on a cold window lands reliably even while
 *  the app is minimized (where the 140ms re-emit `setTimeout` is throttled). */
let toastReadyHooked = false;
let pendingToastPayload: { id: string; title: string; body: string; kind: NotifyKind } | null = null;

/**
 * Show a notification toast (the reliable overlay channel). Best-effort.
 * `kind` colors the card (e.g. 'reminder' = green, 'focus' = amber).
 */
export async function notifyOS(title: string, body?: string, kind: NotifyKind = 'info'): Promise<void> {
    if (typeof window === 'undefined') return;
    // 1) Reliable overlay toast — always shown, dev + prod.
    try {
        // One-time: hook the toast window's mount handshake. When it broadcasts
        // 'notify-toast-ready' we re-send the latest payload — this is what makes
        // the FIRST toast on a cold window land while the app is minimized to the
        // tray (the immediate emit below races the listener mount, and the 140ms
        // re-emit is throttled by Chromium for hidden windows). Event-driven, so
        // it is never throttled. Mirrors focus-glow's 'glow-ready' handshake.
        if (!toastReadyHooked) {
            toastReadyHooked = true;
            void listen('notify-toast-ready', () => {
                if (pendingToastPayload) {
                    void emitTo(TOAST_WINDOW, 'notify-toast-show', pendingToastPayload);
                }
            });
        }
        await invoke('show_notify_toast_window_command');
        // A unique id lets the overlay dedupe the race-guard re-emit below, so
        // the first-show (where the listener may not be mounted yet) is covered
        // without ever stacking the same toast twice.
        const payload = {
            id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
            title,
            body: body ?? '',
            kind,
        };
        pendingToastPayload = payload;
        void emitTo(TOAST_WINDOW, 'notify-toast-show', payload);
        setTimeout(() => void emitTo(TOAST_WINDOW, 'notify-toast-show', payload), 140);
    } catch {
        // Overlay unavailable (e.g. plain-browser dev) — silently ignore.
    }
    // 2) ALSO raise a real OS notification when the user isn't looking at the
    // main window — tray mode, minimized, or another surface (the command
    // palette / another app) has focus — so it lands in the Windows Action
    // Center. Skipped when the main window is focused (the overlay toast is
    // enough; a second OS toast would be redundant). NOTE: Windows only shows
    // these from an INSTALLED build (the app needs a Start-Menu shortcut/AUMID);
    // in dev they no-op, which is why the overlay toast above is the always-on
    // channel.
    try {
        if (!(await isMainWindowForeground())) {
            void notifyOSNative(title, body);
        }
    } catch {
        // Window-state query failed — overlay toast already covers it.
    }
}

/** Is the main window the foreground window right now (visible AND focused)?
 *  Drives whether a background OS notification is also warranted. Reads THIS
 *  window's state — every notifyOS caller (reminders, focus, pomodoro, palette
 *  bridge) runs in the main window. Defaults to `true` (suppress the OS
 *  notification) when state is unreadable, so we never accidentally double-notify. */
async function isMainWindowForeground(): Promise<boolean> {
    try {
        const w = getCurrentWindow();
        const [visible, focused] = await Promise.all([w.isVisible(), w.isFocused()]);
        return visible && focused;
    } catch {
        return true;
    }
}

// ─── Native OS notification (kept, opt-in) ─────────────────────────────────
let granted: boolean | null = null;

async function ensurePermission(): Promise<boolean> {
    if (granted !== null) return granted;
    try {
        granted = await isPermissionGranted();
        if (!granted) {
            granted = (await requestPermission()) === 'granted';
        }
    } catch {
        granted = false;
    }
    return granted;
}

/**
 * Fire a real OS notification (Action-Center entry). NOT used by default —
 * unreliable on Windows in dev and redundant with the overlay toast in prod —
 * but kept available for callers that explicitly want system-level delivery.
 * No-throw — best effort.
 */
export async function notifyOSNative(title: string, body?: string): Promise<void> {
    try {
        if (!(await ensurePermission())) return;
        sendNotification(body ? { title, body } : { title });
    } catch {
        // Plugin unavailable (e.g. plain-browser dev) — silently ignore.
    }
}
