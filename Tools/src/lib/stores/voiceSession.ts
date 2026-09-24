/*
  voiceSession — global single-instance voice recognition coordinator.

  Why this exists:
    Voice recognition is a *singleton* resource. Only one part of the UI
    can be holding the mic open at a time, and we need explicit ownership
    so that:
      • The search overlay closing doesn't cancel a manual session armed
        from the search-page mic button (and vice versa).
      • Switching from the search page to another page automatically
        disarms the mic.
      • Hiding the main window (tray mode) cuts the mic — no recording
        in the background.
      • The Vosk model path from settings is honored consistently
        across every entry point.

  Surface usage:
    armVoice('search-overlay')      // overlay onMount
    disarmVoice('search-overlay')    // overlay onDestroy

    armVoice('search-page')          // mic button click
    disarmVoice('search-page')       // mic button click again, or page leave

    onVoiceTranscript((text) => {    // any subscriber
        // append to query, etc.
    })

  Design notes:
    - There is exactly one underlying recognize-loop. Calling armVoice
      while a different source is active swaps ownership: the existing
      loop stops at its next iteration boundary, then a new loop starts
      under the new source.
    - A stop request is honored at the next loop boundary; an in-flight
      recognize is also aborted via `voice_cancel_recognize`. A stop can
      still let one more utterance land if it arrives in the same window.
      Callers should design for this (it's fine for the use cases we have).
    - Subscribers are notified only for `success` results with non-empty
      text. Silence and recoverable errors stay silent so the loop can
      continue without spamming consumers.
*/

import { get, writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { settings } from './settings';
import { toast } from './toasts';
import { recordActivity } from './activityLog';

/** Open Windows' Settings deep-link best-effort. Failures are silent —
 *  the toast that called us still surfaces the textual fallback. */
function openSystemPanel(id: string) {
    // Used for settings panel deep-links (ms-settings:speech, microphone-
    // privacy) — never destructive commands. `confirmed: false` is correct:
    // the backend gate allows non-destructive ids without confirmation.
    void invoke('execute_system_command', { id, confirmed: false }).catch(() => {});
}

export type VoiceSource = 'search-overlay' | 'voice-overlay';

export type VoiceSessionState = {
    /** True between a successful armVoice and the loop fully stopping. */
    active: boolean;
    /** Which surface owns the current session. */
    source: VoiceSource | null;
    /** True only while a single recognize call is in flight. */
    listening: boolean;
    /** Most recent recognized text (also pushed to subscribers). */
    lastText: string | null;
    /** In-progress partial transcript emitted by the Vosk engine
     *  while a recognize call is mid-flight. Reset on every recognize
     *  start and cleared when the final result lands. */
    partialText: string | null;
    /** Most recent fatal error message, if any. */
    lastError: string | null;
};

const INITIAL: VoiceSessionState = {
    active: false,
    source: null,
    listening: false,
    lastText: null,
    partialText: null,
    lastError: null,
};

const state = writable<VoiceSessionState>(INITIAL);

type RecognitionResult = {
    text: string;
    confidence: string;
    status: string;
};

type Subscriber = (text: string, source: VoiceSource) => void;

const transcriptSubscribers = new Set<Subscriber>();
const partialSubscribers = new Set<Subscriber>();

/**
 * Bootstrap the Tauri voice-event listeners once, lazily — the
 * `voice-partial` partial-transcript stream, and `voice-preempted`
 * (the #17 mic arbiter telling us a higher-priority surface took the
 * mic). Wired the first time anyone arms a session.
 */
let voiceListenersStarted = false;
function ensureVoiceListeners() {
    if (voiceListenersStarted) return;
    voiceListenersStarted = true;
    void listen<{ text: string; source: VoiceSource | null }>('voice-partial', (evt) => {
        const text = evt.payload?.text ?? '';
        const eventSource = evt.payload?.source ?? null;
        const owningSource = get(state).source;
        // Only update the partial if the event was emitted for the
        // currently-armed source. Stale events from a previous source
        // (e.g. partial that arrives after the user re-armed under a
        // different surface) get dropped.
        if (!owningSource || (eventSource && eventSource !== owningSource)) {
            return;
        }
        state.update((s) => ({ ...s, partialText: text }));
        partialSubscribers.forEach((cb) => {
            try {
                cb(text, owningSource);
            } catch (error) {
                console.warn('voice partial subscriber threw:', error);
            }
        });
    });
    // #17 — the backend mic arbiter preempted us: a higher-priority
    // surface (push-to-talk, …) took the device. Disarm so the
    // recognize loop stops and the mic indicator stays truthful.
    void listen<{ client?: string }>('voice-preempted', (evt) => {
        const owningSource = get(state).source;
        if (owningSource && evt.payload?.client === owningSource) {
            disarmVoice();
        }
    });
}

/**
 * Loop control. We use `state.active` as the SINGLE source of truth
 * for "should the loop keep running":
 *   - `armVoice` sets it true; the loop starts (or continues).
 *   - `disarmVoice` sets it false; the loop exits at its next
 *     iteration boundary AND we call the backend cancel command so an
 *     in-flight recognize aborts immediately rather than waiting for
 *     the natural end-of-utterance silence.
 *
 * `loopRunning` is the only auxiliary flag — it prevents two loops
 * spinning up if armVoice gets called twice in quick succession. The
 * older two-flag design (`stopRequested` AND `state.active`) caused
 * the "press OFF twice and then can't turn back ON" bug: the loop's
 * finally block clobbered state with INITIAL right after a re-arm
 * had already set it active, leaving the user stuck.
 */
let loopRunning = false;
let currentModelPath = '';

settings.subscribe((s) => {
    // Voice recognition is Vosk-only — always the Vosk model folder.
    currentModelPath = s.voskModelPath;
});

async function recognizeOnce(source: VoiceSource): Promise<RecognitionResult | null> {
    // Reset partial text at the start of each utterance — the previous
    // utterance's final text is preserved in `lastText`, but partial
    // is purely "what is the engine currently hearing right now".
    state.update((s) => ({ ...s, listening: true, partialText: null }));
    try {
        return await invoke<RecognitionResult>('voice_recognize_once', {
            modelPath: currentModelPath || null,
            source,
        });
    } catch (error) {
        state.update((s) => ({ ...s, lastError: String(error) }));
        toast(`Voice recognition failed: ${error}`, 'error');
        return null;
    } finally {
        state.update((s) => ({ ...s, listening: false, partialText: null }));
    }
}

function notifySubscribers(text: string, source: VoiceSource) {
    transcriptSubscribers.forEach((cb) => {
        try {
            cb(text, source);
        } catch (error) {
            console.warn('voice transcript subscriber threw:', error);
        }
    });
}

function surfaceTerminalError(status: string) {
    switch (status) {
        case 'mic_busy':
            // The #17 mic arbiter refused us — a higher-or-equal-
            // priority surface holds the mic.
            toast('Microphone is in use by another voice surface.', 'info', 4000);
            break;
        case 'permission_denied':
            // Open the privacy panel so the user has one less step;
            // the toast tells them which toggle to flip once it's open.
            openSystemPanel('microphone-privacy');
            toast(
                'Microphone access denied. Opening Windows Privacy → Microphone — ' +
                'turn ON "Let desktop apps access your microphone".',
                'error',
                9000,
            );
            break;
        case 'vosk_setup_required':
            toast(
                'Vosk needs a model. Configure it in Settings → Voice.',
                'error',
                7000,
            );
            break;
        case 'vosk_not_compiled':
            toast(
                'Vosk engine isn\'t bundled in this build.',
                'error',
                7000,
            );
            break;
    }
}

async function runLoop() {
    if (loopRunning) return;
    loopRunning = true;
    try {
        // The loop drives itself purely off `state.active` — the only
        // "stop me" signal. armVoice flips it true, disarmVoice flips
        // it false. No second flag, no race window where the loop and
        // the store disagree about whether voice is on.
        while (get(state).active) {
            const owningSource = get(state).source;
            if (!owningSource) break;

            const result = await recognizeOnce(owningSource);

            // Check state.active again AFTER recognize returns —
            // disarmVoice may have flipped it during the in-flight
            // recording. We never dispatch a transcript or restart
            // the loop after the user said stop.
            if (!get(state).active) break;
            if (!result) {
                // Hard error — surfaced via toast inside recognizeOnce.
                state.update((s) => ({ ...s, active: false }));
                break;
            }

            if (result.status === 'success' && result.text.trim().length > 0) {
                state.update((s) => ({ ...s, lastText: result.text }));
                notifySubscribers(result.text, owningSource);
                void recordActivity({
                    toolId: 'voice-to-text',
                    summary: `Dictated ${result.text.length} char${
                        result.text.length === 1 ? '' : 's'
                    } via ${owningSource}`,
                    outcome: 'success',
                });
            } else if (result.status === 'no_speech') {
                // Silence — let the loop re-arm naturally.
            } else if (result.status === 'cancelled') {
                // The recognize was interrupted by `voice_cancel_recognize`.
                // The `if (!get(state).active) break;` check above
                // already exited if disarm is the reason. Reaching here
                // means the user disarmed AND re-armed during the
                // single recognize window — keep going so the new
                // arm-intent isn't dropped. (We DON'T add an extra
                // break here; the cancelled status is only a hint.)
            } else {
                // Terminal engine error — surface, bail, and clear
                // active so the button reflects the state.
                surfaceTerminalError(result.status);
                state.update((s) => ({ ...s, active: false }));
                break;
            }

            // Brief breath between utterances. Without this we
            // sometimes get rapid back-to-back "no_speech" results
            // that burn CPU.
            await new Promise((r) => setTimeout(r, 150));
        }
    } finally {
        // Loop exit is purely "this loop is no longer running". We
        // DO NOT touch state here — armVoice/disarmVoice own it. The
        // earlier state.set(INITIAL) here was responsible for the
        // "can't re-arm" bug: a re-arm during shutdown got clobbered
        // when the dying loop's finally fired.
        loopRunning = false;
    }
}

/**
 * Arm the voice session for a given source.
 *
 * If the session is already armed for the same source, this is a no-op.
 * If it's armed for a *different* source, ownership transfers — the
 * existing loop stops at its next boundary and a new loop starts under
 * the new source.
 */
export function armVoice(source: VoiceSource) {
    // Lazy-install the voice event listeners the first time anyone
    // arms a session. Cheaper than wiring them on every store import.
    ensureVoiceListeners();

    // Set the truth source. The running loop (if any) will pick this
    // up at its next iteration boundary — same source means it just
    // keeps going, different source means recognitions tag with the
    // new source from then on.
    state.set({
        ...INITIAL,
        active: true,
        source,
    });

    // Start a loop only if one isn't running. The single-loop
    // invariant is enforced by `loopRunning`; trying to spawn a
    // second loop while the first is still mid-recognition would
    // race on the cpal device handle anyway.
    if (!loopRunning) {
        void runLoop();
    }
}

/**
 * Disarm the voice session.
 *
 * If `source` is provided, only disarms when it matches the active
 * source. Use this from cleanup hooks (overlay unmount, page unmount)
 * so you don't accidentally cancel a session armed by a different
 * surface that happened to take over.
 *
 * Pass nothing to force-stop regardless of owner — used by the
 * tray-mode and visibility-change handlers.
 */
export function disarmVoice(source?: VoiceSource) {
    const current = get(state);
    if (!current.active) return;
    if (source && current.source !== source) return;
    // Flip the truth flag — the loop sees this and exits at its next
    // boundary. We also fire the backend cancel so an in-flight Vosk
    // recognize aborts within ~50ms instead of waiting for natural
    // end-of-utterance silence (which can take a couple seconds and
    // makes the user think "off" didn't work).
    state.update((s) => ({ ...s, active: false, partialText: null }));
    void invoke('voice_cancel_recognize').catch(() => {});
}

/**
 * Subscribe to recognized utterances. Returns an unsubscribe function.
 *
 * Subscribers get the text and the source that was active when the
 * recognition landed, so a subscriber can filter for "only my source"
 * if needed (e.g., the search page only wants utterances landed under
 * 'search-page', not ones that arrived during an overlay open).
 */
export function onVoiceTranscript(cb: Subscriber): () => void {
    transcriptSubscribers.add(cb);
    return () => {
        transcriptSubscribers.delete(cb);
    };
}

/**
 * Subscribe to live partial transcripts from the Vosk engine.
 * Useful for surfaces that want to show "Hearing: …" feedback
 * while the user is still speaking.
 *
 * Callback fires at up to 5 Hz with the latest in-progress text.
 * Reset on every utterance boundary.
 */
export function onVoicePartial(cb: Subscriber): () => void {
    partialSubscribers.add(cb);
    return () => {
        partialSubscribers.delete(cb);
    };
}

/**
 * Read-only store handle. Components subscribe to render mic indicators,
 * pulse animations, etc. State is only mutated via armVoice / disarmVoice.
 */
export const voiceSession = {
    subscribe: state.subscribe,
};

/**
 * Tray-mode + minimize guard.
 *
 * Wired from +layout.svelte once at app start. When the document
 * becomes hidden (main window hidden to tray via the X button,
 * minimized, etc.), we force-stop EVERY voice path:
 *
 *   1. `disarmVoice()` — covers voiceSession-managed sources
 *      (search overlay, voice overlay).
 *   2. `voice_stop_continuous` — covers the Voice tool page's
 *      continuous-dictation session (which talks to the backend
 *      directly, not through voiceSession).
 *   3. `voice_cancel_recognize` — aborts any in-flight single-shot
 *      recognize, including the one the Voice tool page kicks off
 *      for "Speak once."
 *
 * All three are no-ops when nothing's running, so it's safe to fire
 * them every visibility change.
 *
 * This is opt-in — the layout calls it explicitly so that overlay
 * webviews (which run as separate windows) don't trip it.
 */
export function installVisibilityWatcher() {
    if (typeof document === 'undefined') return;
    const handler = () => {
        if (!document.hidden) return;
        disarmVoice();
        void invoke('voice_stop_continuous').catch(() => {});
        void invoke('voice_cancel_recognize').catch(() => {});
    };
    document.addEventListener('visibilitychange', handler);
    return () => document.removeEventListener('visibilitychange', handler);
}
