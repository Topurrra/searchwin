/*
  commandMode — KeepItLocal's continuous voice-control mode.

  Voice Commander, Phase B. When the user turns command mode ON (the
  toggle in the voice overlay), KeepItLocal opens a CONTINUOUS,
  GRAMMAR-CONSTRAINED Vosk recognition session: the recognizer is fed
  exactly the command vocabulary — overlay commands, every installed
  tool, every known app (see `buildCommandGrammar`) — which keeps
  recognition fast, accurate, and light enough to leave running. Every
  recognized utterance is matched by the deterministic
  `executeVoiceCommand` parser — no LLM — and, if it is a command,
  executed immediately against the existing search / launcher /
  clipboard overlays.

  Privacy: command mode is OPT-IN and never silent. It runs only after
  an explicit toggle, the microphone "listening" state is shown, and a
  dedicated "Command mode" pill sits in the status bar the whole time
  it is active. It never persists across launches — every app start
  begins with command mode OFF — and it stops automatically when the
  main window is hidden to the tray (KeepItLocal's standing "no
  recording in the background" guarantee).

  Ownership / windows:
    The COORDINATOR runs in the main window (`initCommandMode`, called
    once from the root layout). It is the single place the session is
    started and stopped, owns the `voice-final` listener and the
    matcher, and broadcasts every state change as `command-mode-changed`.
    The toggle lives in the voice overlay — a separate webview — so it
    cannot start/stop directly; it calls `requestToggleCommandMode()`,
    which emits `command-mode-request` for the coordinator to handle.
    Other windows mirror state via `initCommandModeMirror()`. Both the
    main window and the pre-created voice-overlay webview exist from app
    start and wire their listeners then, so neither misses a broadcast —
    no state-sync-on-open handshake is needed. Mirrors the cross-window
    pattern in settings.ts.
*/

import { get, writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event';
import { settings } from './settings';
import { toast } from './toasts';
import { recordActivity } from './activityLog';
import { installedTools } from './toolPacks';
import { executeVoiceCommand, buildCommandGrammar } from './commandRegistry';
import { userVoiceCommands } from './userVoiceCommands';
import { commandOverrides } from './commandOverrides';
import { normalizeTranscript } from './transcriptNormalizer';
import { resolveVoiceContext } from './voiceContext';
import { reportVoiceFeedback, clearVoiceFeedback } from './voiceFeedback';

/** Source label tagged on our continuous session, echoed back in every
 *  `voice-final` event so we accept only our own session's results. */
const COMMAND_SOURCE = 'command-mode';
/** Tauri event — a non-coordinator window (the voice-overlay toggle, or
 *  the status-bar indicator) asks the coordinator to flip command mode. */
const REQUEST_EVENT = 'command-mode-request';
/** Tauri event — a non-coordinator window asks the coordinator to STOP
 *  command mode (a no-op if already off). The voice overlay fires this on
 *  close so "close the palette" reliably ends voice and frees the model,
 *  instead of leaving a continuous session pinned in the main window. */
const STOP_REQUEST_EVENT = 'command-mode-stop-request';
/** Tauri event — the coordinator broadcasts the new state so every
 *  window's mirror store (overlay toggle, status-bar pill) stays true. */
const CHANGED_EVENT = 'command-mode-changed';
/** After a #17 preemption, wait this long with the mic idle before
 *  resuming — long enough not to thrash with a surface (e.g. the
 *  search overlay) whose recognizer briefly frees the mic between
 *  utterances. */
const RESUME_DEBOUNCE_MS = 700;

export type CommandModeState = {
    /** True while a grammar-constrained command session is live. */
    active: boolean;
};

const state = writable<CommandModeState>({ active: false });

/** Read-only store handle. The voice-overlay toggle and the status-bar
 *  indicator subscribe to render their on/off visuals. */
export const commandMode = { subscribe: state.subscribe };

// ─── Coordinator internals (main window only) ────────────────────────

/** Re-entrancy guard — true for the whole lifetime of one session. */
let sessionActive = false;
/** `voice-final` listener handle, held only while a session is live. */
let unlistenFinal: UnlistenFn | null = null;
/** Guards `initCommandMode` against double-wiring. */
let coordinatorWired = false;
/** Guards `initCommandModeMirror` against double-wiring. */
let mirrorWired = false;
/** True when command mode was preempted off the mic (#17) and should
 *  resume once the mic falls idle again. */
let wantResume = false;
/** Debounce timer for the resume — see the `voice-mic-idle` handler. */
let resumeTimer: ReturnType<typeof setTimeout> | null = null;

type FinalPayload = { text?: string; source?: string | null };

/** Broadcast the current active state to every window's mirror store. */
function broadcast(active: boolean): void {
    void emit(CHANGED_EVENT, { active }).catch(() => {});
}

/**
 * Start a grammar-constrained continuous command session.
 *
 * Command mode uses grammar-constrained Vosk recognition, which
 * means a Vosk model must be present on disk.
 */
async function startCommandMode(): Promise<void> {
    if (sessionActive) return;

    const current = get(settings);
    if (!current.voskModelPath) {
        toast(
            'Command mode needs a Vosk model — download one in Settings → Voice.',
            'error',
            6000,
        );
        return;
    }

    sessionActive = true;

    // Context-scoped grammar: command mode's recognizer vocabulary is
    // the global commands plus any scoped to the command-mode surface.
    const grammar = buildCommandGrammar(
        // Hidden tools stay out of the voice vocabulary too — otherwise "open
        // screen recorder" still launches a surface we deliberately hid.
        get(installedTools).filter((tool) => !tool.hidden),
        resolveVoiceContext('command-mode'),
    );

    // Subscribe to finals BEFORE starting the session so an early
    // command can't slip through unheard. Filter to our own source.
    try {
        unlistenFinal = await listen<FinalPayload>('voice-final', (evt) => {
            const evtSource = evt.payload?.source ?? null;
            if (evtSource && evtSource !== COMMAND_SOURCE) return;
            void handleCommandFinal((evt.payload?.text ?? '').trim());
        });
    } catch (error) {
        console.warn('command mode: could not subscribe to voice-final:', error);
    }

    try {
        await invoke('voice_start_continuous', {
            modelPath: current.voskModelPath,
            source: COMMAND_SOURCE,
            grammar,
        });
        wantResume = false;
        state.set({ active: true });
        broadcast(true);
        toast('Command mode on — say "open clipboard", "open settings", …', 'info', 4500);
    } catch (error) {
        // The #17 mic arbiter refuses with `mic_busy` when a higher-or-
        // equal-priority surface holds the mic. On a resume attempt,
        // keep `wantResume` so the next `voice-mic-idle` retries; a
        // fresh user start just gets a quiet heads-up. Any other
        // failure (model missing, mic denied) is terminal.
        if (String(error).includes('mic_busy')) {
            if (!wantResume) {
                toast(
                    'Microphone is in use — try command mode again in a moment.',
                    'info',
                    4500,
                );
            }
        } else {
            wantResume = false;
            toast(`Command mode couldn't start: ${error}`, 'error', 6000);
        }
        await teardown();
    }
}

/**
 * Handle one recognized utterance. Out-of-grammar speech decodes to the
 * Vosk "[unk]" token (or nothing) — ignored, because command mode hears
 * the room continuously and most of what it hears is not a command.
 */
async function handleCommandFinal(text: string): Promise<void> {
    if (!text || text.includes('[unk]')) return;

    // Normalize first — fix Vosk mishearings of the command vocabulary
    // ("clip board" → "clipboard") so the deterministic matcher sees
    // command-exact text. Aggressive mode: command mode is unambiguous.
    const normalized = normalizeTranscript(text, { mode: 'command' }).normalized;
    const outcome = await executeVoiceCommand(normalized, get(installedTools), {
        context: resolveVoiceContext('command-mode'),
    });
    // Surface the result on the main window's VoiceCommandFeedback
    // card — command mode has no UI of its own. `reportVoiceFeedback`
    // is silent for no-match / cooldown, so the "stay silent on
    // non-commands" rule below still holds.
    reportVoiceFeedback(normalized, outcome);
    if (outcome.matched) {
        void recordActivity({
            toolId: 'voice-to-text',
            summary: `Command mode: ${outcome.description ?? text}`,
            outcome: 'success',
        });
    }
    // No match → stay silent. A continuous listener that toasted on
    // every non-command utterance would be unusable.
}

/** Stop the command session and reset all coordinator state. */
async function stopCommandMode(): Promise<void> {
    // An explicit stop cancels any pending #17 resume.
    wantResume = false;
    if (resumeTimer) {
        clearTimeout(resumeTimer);
        resumeTimer = null;
    }
    if (!sessionActive) return;
    try {
        await invoke('voice_stop_continuous');
    } catch (error) {
        console.warn('command mode: voice_stop_continuous failed:', error);
    }
    await teardown();
}

/** Reset coordinator state: drop the `voice-final` listener, clear the
 *  re-entrancy guard, hide the indicator. Idempotent. */
async function teardown(): Promise<void> {
    if (unlistenFinal) {
        unlistenFinal();
        unlistenFinal = null;
    }
    sessionActive = false;
    state.set({ active: false });
    broadcast(false);
    // Drop any feedback line so it can't outlive the session.
    clearVoiceFeedback();
}

/** Restart a live command session so its grammar picks up a changed
 *  user-command set (#21 hot-reload). No-op when no session is active —
 *  the next start builds fresh grammar from the updated registry anyway. */
async function reloadCommandModeGrammar(): Promise<void> {
    if (!sessionActive) return;
    await stopCommandMode();
    await startCommandMode();
}

/** Flip command mode. Coordinator-internal — every window reaches this
 *  through `requestToggleCommandMode`, so it always runs in the main
 *  window where the session and matcher live. */
function toggleCommandMode(): void {
    if (sessionActive) {
        void stopCommandMode();
        return;
    }
    // Never START while the main window is hidden to the tray. `onVisibility`
    // below only fires on a CHANGE, so it stops a running session but cannot
    // catch a session started while already hidden — that would be exactly the
    // background recording the privacy guarantee forbids, and it's the same
    // rule already applied to resume ("no resuming while hidden either", #17).
    //
    // This was unreachable while the only toggle lived in the (since-deleted)
    // voice overlay, which could not exist without the main window. It became
    // reachable the moment a toggle landed in the palette, which the global
    // hotkey can summon while the app sits in the tray.
    if (typeof document !== 'undefined' && document.hidden) return;
    void startCommandMode();
}

// ─── Public API ──────────────────────────────────────────────────────

/**
 * Ask the coordinator to flip command mode on/off. Safe to call from
 * ANY window — it emits `command-mode-request`, which the main-window
 * coordinator handles. The status-bar indicator and the voice-overlay
 * toggle both use this.
 */
export function requestToggleCommandMode(): void {
    void emit(REQUEST_EVENT).catch(() => {});
}

/**
 * Ask the coordinator to STOP command mode if it's running. Safe from any
 * window; a no-op when command mode is already off. The voice overlay calls
 * this on close so "close the palette" reliably ends voice and frees the
 * model, rather than leaving a continuous session pinned in the main window.
 */
export function requestStopCommandMode(): void {
    void emit(STOP_REQUEST_EVENT).catch(() => {});
}

/**
 * Wire the command-mode coordinator. Call ONCE from the main window's
 * root layout. The coordinator owns the session, the recognition
 * listener, and the matcher. Returns a cleanup function.
 */
export function initCommandMode(): () => void {
    if (coordinatorWired) return () => {};
    coordinatorWired = true;

    let unlistenRequest: UnlistenFn | null = null;
    let unlistenStop: UnlistenFn | null = null;
    let unlistenPreempted: UnlistenFn | null = null;
    let unlistenMicIdle: UnlistenFn | null = null;
    void (async () => {
        try {
            unlistenRequest = await listen(REQUEST_EVENT, () => {
                toggleCommandMode();
            });
            // A window (the voice overlay) asks us to stop — e.g. the
            // palette closed. Drop any resume intent and end the session so
            // the model is released instead of staying pinned in RAM.
            unlistenStop = await listen(STOP_REQUEST_EVENT, () => {
                wantResume = false;
                if (resumeTimer) {
                    clearTimeout(resumeTimer);
                    resumeTimer = null;
                }
                if (sessionActive) void stopCommandMode();
            });
            // #17 — a higher-priority surface took the mic. Our
            // `voice-final` stream is dead, so tear the session down,
            // but remember to resume once the mic is free again.
            unlistenPreempted = await listen<{ client?: string }>(
                'voice-preempted',
                (evt) => {
                    if (evt.payload?.client === COMMAND_SOURCE && sessionActive) {
                        void teardown();
                        wantResume = true;
                    }
                },
            );
            // #17 — the mic fell idle. If we were preempted, resume —
            // debounced so a surface that briefly frees the mic between
            // utterances doesn't make command mode thrash.
            unlistenMicIdle = await listen('voice-mic-idle', () => {
                if (!wantResume) return;
                if (resumeTimer) clearTimeout(resumeTimer);
                resumeTimer = setTimeout(() => {
                    resumeTimer = null;
                    const hidden =
                        typeof document !== 'undefined' && document.hidden;
                    if (wantResume && !hidden) void startCommandMode();
                }, RESUME_DEBOUNCE_MS);
            });
        } catch (error) {
            console.warn('command mode: could not wire coordinator listeners:', error);
        }
    })();

    // #21 — when the user's command file hot-reloads, restart a live
    // session so the recognizer's grammar includes the new commands.
    // `subscribe` fires immediately on subscribe; skip that first call.
    let userCommandsReady = false;
    const unsubUserCommands = userVoiceCommands.subscribe(() => {
        if (!userCommandsReady) {
            userCommandsReady = true;
            return;
        }
        if (sessionActive) void reloadCommandModeGrammar();
    });

    // Built-in command phrase overrides changed (the Settings editor): a live
    // session must rebuild its grammar to hear the customized phrases.
    let overridesReady = false;
    const unsubOverrides = commandOverrides.subscribe(() => {
        if (!overridesReady) {
            overridesReady = true;
            return;
        }
        if (sessionActive) void reloadCommandModeGrammar();
    });

    // #23 — keep the backend VAD pre-filter in sync with its setting.
    // `subscribe` fires immediately, so the backend toggle is correct
    // before any continuous session can start; it then tracks changes.
    const unsubVad = settings.subscribe((s) => {
        void invoke('voice_set_vad_enabled', {
            enabled: s.voiceVadEnabled === true,
        }).catch(() => {});
    });

    // Privacy: when the main window is hidden to the tray / minimized,
    // KeepItLocal stops ALL voice capture (voiceSession's visibility
    // watcher fires `voice_stop_continuous`). Stop command mode here too
    // so its state + indicator stay truthful instead of going stale.
    const onVisibility = () => {
        if (typeof document !== 'undefined' && document.hidden) {
            // Hidden to tray — no background recording, and no resuming
            // while hidden either (#17): drop the resume intent.
            wantResume = false;
            if (resumeTimer) {
                clearTimeout(resumeTimer);
                resumeTimer = null;
            }
            if (sessionActive) void stopCommandMode();
        }
    };
    if (typeof document !== 'undefined') {
        document.addEventListener('visibilitychange', onVisibility);
    }

    return () => {
        unlistenRequest?.();
        unlistenRequest = null;
        unlistenStop?.();
        unlistenStop = null;
        unlistenPreempted?.();
        unlistenPreempted = null;
        unlistenMicIdle?.();
        unlistenMicIdle = null;
        unsubUserCommands();
        unsubOverrides();
        unsubVad();
        if (typeof document !== 'undefined') {
            document.removeEventListener('visibilitychange', onVisibility);
        }
        void stopCommandMode();
        coordinatorWired = false;
    };
}

/**
 * Mirror command-mode state into THIS window's `commandMode` store.
 * Call once from a window that shows command-mode UI but does NOT run
 * the coordinator — i.e. the voice overlay (its toggle). The main
 * window does not need this: its coordinator updates the store directly.
 * Returns a cleanup function.
 */
export function initCommandModeMirror(): () => void {
    if (mirrorWired) return () => {};
    mirrorWired = true;

    let unlisten: UnlistenFn | null = null;
    void (async () => {
        try {
            unlisten = await listen<{ active?: boolean }>(CHANGED_EVENT, (evt) => {
                state.set({ active: evt.payload?.active === true });
            });
        } catch (error) {
            console.warn('command mode: could not wire state mirror:', error);
        }
    })();

    return () => {
        unlisten?.();
        unlisten = null;
        mirrorWired = false;
    };
}
