/*
  pushToTalk — global push-to-talk (PTT) voice-command coordinator.

  What it does:
    PTT is a hold-to-fire alternative to command mode. The backend
    registers a global hotkey and emits two events to the main webview:
      • `voice-ptt-start` — the hold key went down
      • `voice-ptt-stop`  — the hold key was released
    While the key is held, PTT runs a GRAMMAR-CONSTRAINED Vosk session —
    the same kind command mode uses — and every recognized utterance is
    matched and executed by `executeVoiceCommand`. PTT does COMMANDS,
    not dictation: "hold the key, say a command, release — it runs."

  PTT vs command mode:
    Both do voice commands; they differ only in activation. Command
    mode is a toggle — continuous, hands-free, listening until it is
    turned off. PTT is momentary — it listens ONLY while the key is
    held, which makes it the more private option (no always-on mic)
    and the natural "fire one command" gesture. Dictation lives on the
    other surfaces (the voice overlay, the Voice tool) — never here.

  The flow:
    voice-ptt-start →
      start a grammar-constrained continuous session, subscribe to
      `voice-final`, show the push-to-talk indicator.
    each `voice-final` (while held) →
      normalize → executeVoiceCommand → report the result on the
      main-window feedback card.
    voice-ptt-stop →
      wait a short GRACE delay (trailing audio), stop the session,
      hold on through the worker's flush window so a command spoken
      right before release still runs, then tear down.

  Why this lives separately from `voiceSession`:
    `voiceSession` is the surface coordinator for the overlay windows —
    a single-utterance recognize loop. PTT's "hold to run a command"
    model talks to the continuous backend commands directly, so it gets
    its own tiny coordinator here. (#17 will put a real microphone
    arbiter in front of all of these surfaces.)

  Initialized once at app startup from the root layout (main window
  only — overlay webviews don't run this).
*/

import { get, writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { settings } from './settings';
import { toast } from './toasts';
import { recordActivity } from './activityLog';
import { installedTools } from './toolPacks';
import { executeVoiceCommand, buildCommandGrammar } from './commandRegistry';
import { normalizeTranscript } from './transcriptNormalizer';
import { resolveVoiceContext } from './voiceContext';
import { reportVoiceFeedback } from './voiceFeedback';

/**
 * Grace delay between the PTT key being released and us stopping the
 * session.
 *
 * WHY THIS MATTERS: the user often releases the hold key a fraction
 * before the last word's sound has fully finished. Cutting the audio
 * the instant the key comes up would clip the tail of the command.
 * Keeping the session running ~400 ms after release lets the
 * microphone capture that trailing audio so the recognizer has the
 * whole phrase to decode.
 */
const GRACE_DELAY_MS = 400;

/**
 * Delay after `voice_stop_continuous` before tearing the session down.
 *
 * WHY THIS MATTERS: `voice_stop_continuous` only SIGNALS the backend's
 * continuous worker to stop — it returns immediately. The worker then
 * finishes its current poll cycle (~200 ms), tears down the audio
 * stream, and flushes the recognizer's residual decoded text as one
 * last `voice-final`. Holding the `voice-final` listener open through
 * this window lets a command spoken right before release still run.
 * 400 ms covers the worker's poll interval plus teardown with margin
 * for a slow, loaded low-end machine (the hardware tier KeepItLocal
 * targets).
 */
const STOP_FLUSH_DELAY_MS = 400;

/** The source label tagged onto our continuous session. Echoed back in
 *  every `voice-final` event so we accept only our own session's
 *  results and ignore concurrent capture from another surface. */
const PTT_SOURCE = 'push-to-talk';

export type PushToTalkState = {
    /** True from `voice-ptt-start` until the post-release teardown
     *  completes. Drives the "push-to-talk" listening indicator. */
    listening: boolean;
};

const state = writable<PushToTalkState>({ listening: false });

/**
 * Read-only store handle. Components subscribe to render the
 * "push-to-talk" listening indicator.
 */
export const pushToTalk = {
    subscribe: state.subscribe,
};

/** Re-entrancy guard. True for the whole lifetime of one PTT session
 *  (start → release → grace → stop → flush → teardown). A second
 *  `voice-ptt-start` arriving while this is true is ignored — the OS
 *  can repeat-fire a held hotkey, and two overlapping sessions must
 *  never race on the audio device. */
let sessionActive = false;

/** Unlisten handle for the `voice-final` subscription. Held only while
 *  a session is active — subscribed on start, dropped on teardown. */
let unlistenFinal: UnlistenFn | null = null;

/** Pending grace-delay timer between release and stop. */
let graceTimer: ReturnType<typeof setTimeout> | null = null;

/** Guards `initPushToTalk` against double-wiring if called twice. */
let initialized = false;

type FinalPayload = { text?: string; source?: string | null };

/**
 * Handle one recognized utterance during a PTT hold: match it against
 * the command registry and run it. Out-of-grammar speech decodes to
 * the Vosk "[unk]" token — ignored.
 */
async function handlePttFinal(text: string): Promise<void> {
    if (!text || text.includes('[unk]')) return;

    // Aggressive (command-mode) normalization — PTT is unambiguously a
    // command surface, so mishearings of the command vocabulary are
    // corrected hard before the deterministic matcher sees the text.
    const normalized = normalizeTranscript(text, { mode: 'command' }).normalized;
    const outcome = await executeVoiceCommand(normalized, get(installedTools), {
        context: resolveVoiceContext('push-to-talk'),
    });
    // Surface the result on the main window's VoiceCommandFeedback
    // card — PTT does things, so the user should see what happened.
    reportVoiceFeedback(normalized, outcome);
    if (outcome.matched) {
        void recordActivity({
            toolId: 'voice-to-text',
            summary: `Push-to-talk: ${outcome.description ?? normalized}`,
            outcome: 'success',
        });
    }
}

/**
 * Handle the PTT key going down: start a grammar-constrained command
 * session and begin matching utterances.
 *
 * Ignored if a session is already active (re-entrancy guard) or if PTT
 * is disabled in settings — the backend only emits these events when
 * the hotkey is registered, but we re-check the setting defensively.
 */
async function handlePttStart() {
    const current = get(settings);
    if (!current.pushToTalkEnabled) return;
    // Re-entrancy guard — a held hotkey can repeat-fire Pressed.
    if (sessionActive) return;

    // Command recognition is grammar-constrained, which needs a Vosk
    // model on disk. Fail early with a clear message rather than the
    // generic start error.
    if (!current.voskModelPath) {
        toast(
            'Push-to-talk needs a Vosk model — download one in Settings → Voice.',
            'error',
            6000,
        );
        return;
    }

    sessionActive = true;
    // Cancel any stale grace timer from a previous session.
    if (graceTimer) {
        clearTimeout(graceTimer);
        graceTimer = null;
    }

    // Context-scoped command grammar — the same vocabulary command mode
    // feeds Vosk, so PTT recognition is just as fast and accurate.
    const grammar = buildCommandGrammar(
        get(installedTools),
        resolveVoiceContext('push-to-talk'),
    );

    // Subscribe to finalized utterances BEFORE starting the session so
    // we never miss an early command. Filter to our own source label.
    try {
        unlistenFinal = await listen<FinalPayload>('voice-final', (evt) => {
            const evtSource = evt.payload?.source ?? null;
            if (evtSource && evtSource !== PTT_SOURCE) return;
            void handlePttFinal((evt.payload?.text ?? '').trim());
        });
    } catch (error) {
        console.warn('PTT: could not subscribe to voice-final:', error);
    }

    try {
        await invoke('voice_start_continuous', {
            modelPath: current.voskModelPath,
            source: PTT_SOURCE,
            grammar,
        });
        // Only show the indicator once the session actually started.
        state.set({ listening: true });
    } catch (error) {
        // Start failed (mic permission denied, engine not bundled, …) —
        // surface it and tear down so the next press starts clean.
        toast(`Push-to-talk couldn't start: ${error}`, 'error', 6000);
        await teardownSession();
    }
}

/**
 * Handle the PTT key being released: wait out the grace delay, then
 * stop the session.
 *
 * Ignored if no session is active (a stray Released with no matching
 * Pressed).
 */
function handlePttStop() {
    if (!sessionActive) return;

    // Grace delay: give the recognizer a beat of trailing audio to
    // finish the last phrase before we cut the stream (see
    // GRACE_DELAY_MS above).
    if (graceTimer) clearTimeout(graceTimer);
    graceTimer = setTimeout(() => {
        graceTimer = null;
        void finishSession();
    }, GRACE_DELAY_MS);
}

/**
 * Stop the continuous session. The `voice-final` listener stays
 * subscribed through the post-stop flush window so a command spoken
 * right before release still runs; then the session is torn down.
 * Always tears down at the end so a failure can't wedge PTT "active".
 */
async function finishSession() {
    // Stop the continuous session. `voice_stop_continuous` only signals
    // the backend worker — it then flushes one last `voice-final`.
    try {
        await invoke('voice_stop_continuous');
    } catch (error) {
        console.warn('PTT: voice_stop_continuous failed:', error);
    }

    // Hold the listener open for that flushed final to arrive and be
    // handled (see STOP_FLUSH_DELAY_MS) — the last command must still
    // get its chance to run — then drop everything.
    await new Promise((resolve) => setTimeout(resolve, STOP_FLUSH_DELAY_MS));
    await teardownSession();
}

/**
 * Reset all session state: drop the `voice-final` listener, clear the
 * grace timer + the re-entrancy guard, hide the indicator. Idempotent.
 */
async function teardownSession() {
    if (unlistenFinal) {
        unlistenFinal();
        unlistenFinal = null;
    }
    if (graceTimer) {
        clearTimeout(graceTimer);
        graceTimer = null;
    }
    sessionActive = false;
    state.set({ listening: false });
}

/**
 * Wire up the global PTT event listeners. Call exactly once at app
 * startup (root layout, main window only). Subsequent calls are no-ops.
 *
 * Returns a cleanup function that removes the listeners — used by the
 * layout's onDestroy (hot-reload / teardown). In normal operation the
 * listeners live for the lifetime of the webview.
 */
export function initPushToTalk(): () => void {
    if (initialized) return () => {};
    initialized = true;

    let unlistenStart: UnlistenFn | null = null;
    let unlistenStop: UnlistenFn | null = null;

    void (async () => {
        try {
            unlistenStart = await listen('voice-ptt-start', () => {
                void handlePttStart();
            });
            unlistenStop = await listen('voice-ptt-stop', () => {
                handlePttStop();
            });
        } catch (error) {
            console.warn('PTT: could not wire up event listeners:', error);
        }
    })();

    return () => {
        unlistenStart?.();
        unlistenStop?.();
        unlistenStart = null;
        unlistenStop = null;
        void teardownSession();
        initialized = false;
    };
}
