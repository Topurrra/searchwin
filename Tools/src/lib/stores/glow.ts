/*
  glow — briefly flashes the full-screen, click-through screen-edge glow overlay
  (the `focus-glow` window, configured in Rust). Shared by:
    • Focus mode  → ORANGE, when a blocked app is focused.
    • Reminders   → GREEN,  when a reminder fires (alongside the green toast).

  Cross-window: callers run in the MAIN window; the glow lives in its own window.
  We show it (Rust), push the color over an event, then hide it after a beat.

  Color delivery is the tricky part — the glow window is created lazily on first
  use, and a brand-new WebView can take a few hundred ms to mount its listener.
  An immediate emit then races that mount, which is what made the first GREEN
  reminder flash show ORANGE (the page's default). Three things guarantee the
  right color:
    1. immediate emit          — covers an already-mounted window,
    2. a short re-emit (140ms)  — covers a fast mount,
    3. a 'glow-ready' handshake — the window announces when it's mounted +
       listening, and we (re)send the LATEST requested color then. (3) is what
       makes first-creation reliable.
*/
import { invoke } from '@tauri-apps/api/core';
import { emitTo, listen } from '@tauri-apps/api/event';

export type GlowColor = 'orange' | 'green';

const GLOW_WINDOW = 'focus-glow';
let hideTimer: ReturnType<typeof setTimeout> | null = null;
/** Latest requested color + duration — re-sent when the glow window announces
 *  it's ready (so the fade is paced to the same window we hide). */
let pendingColor: GlowColor = 'orange';
let pendingDuration = 1500;
let readyHooked = false;

/**
 * Flash the screen-edge glow in `color` for `durationMs` (default 1.5s).
 * Overlap-safe: a new flash resets the hide timer. Best-effort, no-throw.
 */
export async function flashGlow(color: GlowColor = 'orange', durationMs = 1500): Promise<void> {
    if (typeof window === 'undefined') return;
    pendingColor = color;
    pendingDuration = durationMs;
    try {
        // One-time: when the glow window finishes mounting it broadcasts
        // 'glow-ready'; we (re)send the current color + duration then. This is
        // what makes a GREEN reminder flash actually show green on the window's
        // very first creation (when the emits below race the listener mounting).
        if (!readyHooked) {
            readyHooked = true;
            void listen('glow-ready', () => {
                void emitTo(GLOW_WINDOW, 'glow-flash', {
                    color: pendingColor,
                    durationMs: pendingDuration,
                });
            });
        }
        await invoke('show_focus_glow_window_command');
        void emitTo(GLOW_WINDOW, 'glow-flash', { color, durationMs });
        setTimeout(
            () =>
                void emitTo(GLOW_WINDOW, 'glow-flash', {
                    color: pendingColor,
                    durationMs: pendingDuration,
                }),
            140,
        );

        // Hide exactly when the fade-out finishes (the glow page paces its
        // fade-in → hold → fade-out to this same durationMs), so the window
        // disappears while the glow is already invisible — no abrupt cut.
        if (hideTimer) clearTimeout(hideTimer);
        hideTimer = setTimeout(() => {
            hideTimer = null;
            void invoke('hide_focus_glow_window_command');
        }, durationMs);
    } catch {
        // glow window unavailable (plain-browser dev) — no-op.
    }
}
