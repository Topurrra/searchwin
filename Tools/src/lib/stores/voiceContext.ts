/*
  voiceContext — the active-voice-context snapshot.

  Voice upgrade, Layer 7. Whenever a transcript is about to be matched,
  the caller resolves a `VoiceContextSnapshot` — a description of WHERE
  the voice input is happening. `commandRegistry` filters the command
  set (both the Vosk grammar and the matcher) by it, so a command can
  be scoped to one surface instead of being global.

  #12 ships the resolver focused on the KeepItLocal SURFACE — the
  immediately-real context axis. `resolveVoiceContext` is the
  designated extension point: #20/#21 will enrich the snapshot with
  the foreground OS process (for per-application command sets),
  focused-input kind, selection state, etc. Adding a field here will
  not change any call site — callers just pass their surface and
  receive the snapshot.

  Pure and deterministic — no AI, no network.
*/

/** Which KeepItLocal voice surface is producing / handling the
 *  transcript. */
export type VoiceSurface =
    | 'voice-overlay'
    | 'search-overlay'
    | 'voice-to-text'
    | 'push-to-talk'
    | 'command-mode';

/**
 * A snapshot of the voice context at the moment a transcript is
 * matched. #12: the surface only. The struct is the extension point —
 * #20/#21 add the foreground process, focused-input kind, etc.
 */
export interface VoiceContextSnapshot {
    surface: VoiceSurface;
}

/**
 * Resolve the current voice context for `surface`. Trivial today — the
 * snapshot is just the surface — but this is the single place context
 * resolution lives, so later signals (foreground process, overlay
 * state, …) slot in here without touching any caller.
 */
export function resolveVoiceContext(surface: VoiceSurface): VoiceContextSnapshot {
    return { surface };
}
