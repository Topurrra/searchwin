/*
  voiceFeedback — the in-process feedback channel for voice commands.

  Voice upgrade, Layer 5 (#16). The command pipeline runs in-process:
  `executeVoiceCommand` returns a `VoiceCommandOutcome` synchronously to
  whoever called it. A surface that IS a component (the voice / search
  overlays) renders that outcome itself. But command mode's coordinator
  (`commandMode.ts`) is a plain module, not a component — it has no UI
  of its own. This store is the bridge: the coordinator calls
  `reportVoiceFeedback`, and the main window's `VoiceCommandFeedback`
  component renders it.

  Compact + transient: the store holds only the single most-recent
  command feedback and clears itself after VISIBLE_MS, so command mode
  does not accumulate stale lines while it listens continuously.

  Not every outcome surfaces. `no-match` and `cooldown` are silent —
  command mode hears the whole room and most of it is not a command, so
  surfacing those would be noise. `confirmation-required` is silent here
  too: the parked command is rendered from `voiceSafetyGate`'s
  `pendingConfirmation` store instead, as a richer interactive prompt.

  Per-window: like every store in this folder, each window gets its own
  instance. Command mode's coordinator and the main-window component
  share the main window's instance — the only pairing that uses it.
*/

import { writable } from 'svelte/store';
import type { VoiceCommandOutcome } from './commandRegistry';

/** How long a feedback line stays before it clears itself. */
const VISIBLE_MS = 4500;

export type VoiceFeedbackTone = 'success' | 'error' | 'info';

/** One command's feedback, as shown to the user. */
export interface VoiceFeedbackState {
    /** The transcript the recognizer produced — the "Heard …" line. */
    heard: string;
    /** What happened — the execution label or the rejection reason. */
    detail: string;
    /** Visual tone for `detail`. */
    tone: VoiceFeedbackTone;
    /** Set-time, milliseconds — doubles as a `{#key}` so a repeated
     *  outcome still re-triggers the component's entrance animation. */
    at: number;
}

const state = writable<VoiceFeedbackState | null>(null);

/** Read-only handle — the `VoiceCommandFeedback` component subscribes. */
export const voiceFeedback = { subscribe: state.subscribe };

let clearTimer: ReturnType<typeof setTimeout> | null = null;

/** Map an outcome to its feedback line, or `null` when it should show
 *  nothing — silent for no-match / cooldown (continuous-listening
 *  noise) and for confirmation-required / cancelled (the pending-
 *  confirmation prompt owns those). */
function describe(
    outcome: VoiceCommandOutcome,
): { detail: string; tone: VoiceFeedbackTone } | null {
    switch (outcome.status) {
        case 'executed':
        case 'confirmed':
            return { detail: outcome.description ?? 'Done.', tone: 'success' };
        case 'blocked':
            return {
                detail: outcome.description ?? 'Command refused for safety.',
                tone: 'error',
            };
        case 'ambiguous':
            return {
                detail:
                    outcome.description ??
                    "Didn't catch which one — say the full name.",
                tone: 'info',
            };
        default:
            return null;
    }
}

/**
 * Report the result of a voice command for display. Called by the
 * command-mode coordinator after every recognized utterance; silently
 * does nothing for outcomes that should not surface (see `describe`).
 */
export function reportVoiceFeedback(
    heard: string,
    outcome: VoiceCommandOutcome,
): void {
    const described = describe(outcome);
    if (!described) return;
    if (clearTimer) clearTimeout(clearTimer);
    state.set({
        heard,
        detail: described.detail,
        tone: described.tone,
        at: Date.now(),
    });
    clearTimer = setTimeout(() => {
        state.set(null);
        clearTimer = null;
    }, VISIBLE_MS);
}

/** Drop any feedback now — called when command mode stops, so a stale
 *  line can't outlive the session that produced it. */
export function clearVoiceFeedback(): void {
    if (clearTimer) {
        clearTimeout(clearTimer);
        clearTimer = null;
    }
    state.set(null);
}
