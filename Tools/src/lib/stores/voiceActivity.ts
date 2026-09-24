/*
  voiceActivity — global, surface-agnostic "is the mic hot right now?"
  signal for the StatusBar (and anyone else that wants a one-stop
  truth).

  Why this exists separately from `voiceSession`:
    `voiceSession` is the *coordinator* for overlay surfaces (search,
    clipboard, voice-overlay) — it owns armVoice/disarmVoice +
    transcript subscribers. The Voice tool page bypasses it and calls
    `voice_recognize_once` / `voice_start_continuous` directly because
    its UX (continuous dictation building a transcript) doesn't fit
    the voiceSession surface-ownership model.

    That split means `voiceSession.active` does NOT cover the Voice
    tool page's mic. For a global indicator we need something that
    sees *every* capture path. The backend's `voice-partial` and
    `voice-final` events are emitted from the shared Vosk capture
    code path, so listening to them gives a complete view regardless
    of caller.

  Lifecycle:
    - Mic-press / continuous-start → backend opens audio stream →
      emits "(listening — start speaking)" partial → this store
      flips `active=true`.
    - User speaks → "(recording…)" / live-text partials → we keep
      `active=true` and refresh a watchdog timestamp.
    - Capture ends → backend either:
        a) emits final via `voice-final` → flip false immediately, OR
        b) returns a single-shot result with no `voice-final` → no
           explicit "off" event, so we lean on the watchdog.
    - Watchdog: if no partial has arrived for 1.5s AND no final
      landed, presume the session ended (no leftover stale red dot).
      1.5s comfortably exceeds Vosk's gap between partial events;
      anything longer is genuinely over.
*/

import { writable } from 'svelte/store';
import { listen } from '@tauri-apps/api/event';

type PartialPayload = { text?: string; source?: string | null };
type FinalPayload = { text?: string; source?: string | null };

const WATCHDOG_MS = 1500;

/** True whenever the backend has an open audio stream OR is
 *  mid-transcription. Drives the StatusBar's flickering-red
 *  "Listening" pill. */
export const voiceActive = writable(false);

let watchdog: ReturnType<typeof setTimeout> | null = null;
let started = false;

function bumpWatchdog() {
    voiceActive.set(true);
    if (watchdog) clearTimeout(watchdog);
    watchdog = setTimeout(() => {
        // No partial in WATCHDOG_MS — assume the session quietly
        // ended (single-shot completion path doesn't emit final).
        voiceActive.set(false);
        watchdog = null;
    }, WATCHDOG_MS);
}

function clearWatchdog() {
    if (watchdog) {
        clearTimeout(watchdog);
        watchdog = null;
    }
    voiceActive.set(false);
}

/**
 * Wire up the global listeners exactly once. Safe to call from
 * multiple components — subsequent calls are no-ops.
 *
 * Currently called from the root layout so the store is hot before
 * any voice-capable surface mounts.
 */
export function initVoiceActivity() {
    if (started) return;
    started = true;
    void listen<PartialPayload>('voice-partial', () => {
        // Any partial = backend is doing voice work right now. We
        // don't care which surface emitted it — the indicator is
        // global.
        bumpWatchdog();
    });
    void listen<FinalPayload>('voice-final', () => {
        // Final landed → capture cycle complete. In continuous mode
        // a new partial will arrive within ~150ms (next iteration's
        // "(listening…)") and re-bump the watchdog, so the dot stays
        // lit through the gap.
        clearWatchdog();
    });
}
