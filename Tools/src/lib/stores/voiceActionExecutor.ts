/*
  voiceActionExecutor — the one place a voice command's side effects
  actually happen.

  Voice upgrade, Layer 5 (#15). Execution is separated from parsing.
  The matcher (commandRegistry) decides WHAT the user asked for; the
  safety gate (voiceSafetyGate) decides WHETHER to run it — but neither
  performs anything. They produce a declarative `VoiceAction`: plain,
  inspectable data ("open the clipboard overlay", "launch this exe",
  "open this url"). This module is the sole consumer of that data —
  `executeAction` is the only function in the voice command pipeline
  that calls Tauri `invoke` to change the world.

  Why the split: a declarative action can be asserted against in a
  test ("'open clipboard' parses to { kind: 'open-overlay', overlay:
  'clipboard' }") without launching anything, and the registry's
  command data stays pure — no closures over `invoke`. Every command
  handler is now a pure `args → VoiceAction` builder.

  `executeAction` always resolves — it never throws — with a structured
  `VoiceActionResult`: a success flag, a human message, an error
  detail, and how long execution took, so the safety gate and #16's
  feedback layer can report precisely what happened.

  Module boundary: this is a leaf — it imports only Tauri APIs and the
  toast store, never commandRegistry or voiceSafetyGate, so it cannot
  be part of an import cycle.
*/

import { invoke } from '@tauri-apps/api/core';
import { emitTo } from '@tauri-apps/api/event';
import { toast } from './toasts';

/** A frecency data-point — records that the user launched `path` (an
 *  app key or a tool id) so the launcher can rank it higher later. */
export interface FrecencyMark {
    kind: 'app' | 'tool';
    path: string;
}

/**
 * A declarative description of what a voice command should do —
 * produced by the registry's command builders, performed by
 * `executeAction`. Plain data: no closures, no side effects, safe to
 * log, compare, and assert against in tests.
 *
 * #15 shipped the launcher / overlay / tool kinds; #18a adds the
 * keyboard / mouse kinds; #19–#22 extend it further (window control,
 * paste-text, …).
 */
export type VoiceAction =
    | { kind: 'open-overlay'; overlay: 'clipboard' | 'search' | 'voice' }
    | { kind: 'open-url'; url: string; frecency?: FrecencyMark }
    | {
          kind: 'launch-app';
          /** System-command id, probed with `check_app_available`. */
          exeId: string;
          /** Display name — used in the not-installed / fallback toasts. */
          label: string;
          /** Web version to open if the native app is not installed. */
          fallbackUrl?: string;
          frecency?: FrecencyMark;
      }
    | { kind: 'open-tool'; toolId: string }
    | {
          /** A keystroke — modifiers held, key tapped, modifiers
           *  released. `key` is a name ("enter", "a", "f5"); each
           *  modifier is "ctrl" / "alt" / "shift" / "win". (#18a) */
          kind: 'keystroke';
          key: string;
          modifiers: string[];
      }
    | { kind: 'mouse-click'; button: 'left' | 'right' | 'middle'; double: boolean }
    | { kind: 'mouse-scroll'; direction: 'up' | 'down'; notches: number }
    | { kind: 'mouse-move'; dx: number; dy: number }
    | { kind: 'open-mouse-grid' }
    /** Open the accessibility element overlay (#20) — numbers every
     *  clickable control in the focused window so it can be clicked by
     *  voice. The elements are enumerated by `executeAction` itself,
     *  before the overlay is shown. */
    | { kind: 'open-ui-elements' }
    | {
          /** Act on the foreground window (#19). */
          kind: 'window-action';
          action:
              | 'minimize'
              | 'maximize'
              | 'restore'
              | 'close'
              | 'snap-left'
              | 'snap-right'
              | 'center';
      }
    | { kind: 'noop' };

/** The structured result of performing a `VoiceAction`. `executeAction`
 *  always resolves with one of these — it never rejects. */
export interface VoiceActionResult {
    /** True when the action did what it set out to do. */
    success: boolean;
    /** Human-readable summary of what happened — for success / info. */
    message?: string;
    /** Failure detail, set when `success` is false. */
    error?: string;
    /** Wall-clock execution time, milliseconds. */
    durationMs: number;
}

/** Fire-and-forget frecency record — the action already ran, so a lost
 *  data point must never surface as a failure of the action itself. */
function markFrecency(mark: FrecencyMark | undefined): void {
    if (!mark) return;
    void invoke('record_frecency_launch', {
        kind: mark.kind,
        path: mark.path,
    }).catch(() => {});
}

/**
 * Perform a `VoiceAction`. The single execution chokepoint of the
 * voice command pipeline — always resolves with a `VoiceActionResult`
 * and never throws, so the safety gate and #16's feedback layer can
 * rely on a structured outcome for every command.
 */
export async function executeAction(action: VoiceAction): Promise<VoiceActionResult> {
    const startedAt = performance.now();
    const finish = (
        partial: Omit<VoiceActionResult, 'durationMs'>,
    ): VoiceActionResult => ({
        ...partial,
        durationMs: Math.round(performance.now() - startedAt),
    });

    switch (action.kind) {
        case 'open-overlay': {
            const command =
                action.overlay === 'clipboard'
                    ? 'show_clipboard_overlay_window_command'
                    : action.overlay === 'search'
                      ? 'show_overlay_window_command'
                      : 'show_voice_overlay_window_command';
            try {
                await invoke(command);
                // Cleanup Wave 1 (2026-05-28): the legacy /overlay,
                // /clipboard-overlay, /voice-overlay routes are gone —
                // these "show_*_overlay_window_command" invokes are
                // now thin redirects to the unified command palette
                // (label "command"). Surface that in the user-facing
                // toast so the action reads accurately: "Opened the
                // clipboard palette" instead of "Opened the clipboard
                // overlay" (which no longer exists).
                const surface =
                    action.overlay === 'clipboard'
                        ? 'clipboard palette'
                        : action.overlay === 'voice'
                          ? 'voice palette'
                          : 'palette';
                return finish({
                    success: true,
                    message: `Opened the ${surface}.`,
                });
            } catch (error) {
                const surface =
                    action.overlay === 'clipboard'
                        ? 'clipboard palette'
                        : action.overlay === 'voice'
                          ? 'voice palette'
                          : 'palette';
                return finish({
                    success: false,
                    error: `Couldn't open the ${surface}: ${error}`,
                });
            }
        }

        case 'open-url': {
            // The frecency point is recorded for the attempt — matching
            // the old launcher, where an app command always counts as a
            // launch regardless of whether the page actually opened.
            markFrecency(action.frecency);
            try {
                await invoke('open_external_url', { url: action.url });
                return finish({ success: true, message: `Opened ${action.url}` });
            } catch (error) {
                return finish({
                    success: false,
                    error: `Couldn't open ${action.url}: ${error}`,
                });
            }
        }

        case 'launch-app': {
            markFrecency(action.frecency);
            // Pre-check so we don't trip Windows' "Find a program"
            // dialog for an app that isn't installed.
            const available = await invoke<boolean>('check_app_available', {
                id: action.exeId,
            }).catch(() => false);
            if (available) {
                try {
                    // Voice-triggered launches are never destructive commands
                // (voice only triggers app launchers, not shutdown/restart).
                // `confirmed: false` signals this; the backend gate accepts
                // it for non-destructive ids and rejects destructive ones.
                await invoke('execute_system_command', { id: action.exeId, confirmed: false });
                    return finish({
                        success: true,
                        message: `Launched ${action.label}.`,
                    });
                } catch (error) {
                    return finish({
                        success: false,
                        error: `Couldn't launch ${action.label}: ${error}`,
                    });
                }
            }
            // Not installed — fall back to the web version if there is
            // one, otherwise report the miss.
            if (action.fallbackUrl) {
                toast(
                    `${action.label} isn't installed — opening the web version instead.`,
                    'info',
                    3500,
                );
                try {
                    await invoke('open_external_url', { url: action.fallbackUrl });
                    return finish({
                        success: true,
                        message: `${action.label} isn't installed — opened the web version.`,
                    });
                } catch (error) {
                    return finish({
                        success: false,
                        error: `Couldn't open ${action.label}: ${error}`,
                    });
                }
            }
            toast(
                `${action.label} doesn't seem to be installed on this machine.`,
                'error',
                5000,
            );
            return finish({
                success: false,
                error: `${action.label} is not installed.`,
            });
        }

        case 'open-tool': {
            // Show the main window and navigate it in parallel; settle
            // both so one failing doesn't mask the other.
            const results = await Promise.allSettled([
                invoke('show_main_window_command'),
                emitTo('main', 'navigate-tool', { toolId: action.toolId }),
            ]);
            markFrecency({ kind: 'tool', path: action.toolId });
            const failure = results.find((r) => r.status === 'rejected');
            if (failure && failure.status === 'rejected') {
                return finish({
                    success: false,
                    error: `Couldn't open the tool: ${failure.reason}`,
                });
            }
            return finish({ success: true, message: `Opened ${action.toolId}.` });
        }

        case 'keystroke': {
            try {
                await invoke('voice_send_keystroke', {
                    key: action.key,
                    modifiers: action.modifiers,
                });
                const combo = [...action.modifiers, action.key].join(' + ');
                return finish({ success: true, message: `Pressed ${combo}.` });
            } catch (error) {
                return finish({
                    success: false,
                    error: `Couldn't send the keystroke: ${error}`,
                });
            }
        }

        case 'mouse-click': {
            try {
                await invoke('voice_mouse_click', {
                    button: action.button,
                    double: action.double,
                });
                return finish({
                    success: true,
                    message: action.double
                        ? `Double-clicked (${action.button}).`
                        : `Clicked (${action.button}).`,
                });
            } catch (error) {
                return finish({ success: false, error: `Couldn't click: ${error}` });
            }
        }

        case 'mouse-scroll': {
            try {
                await invoke('voice_mouse_scroll', {
                    direction: action.direction,
                    notches: action.notches,
                });
                return finish({ success: true, message: `Scrolled ${action.direction}.` });
            } catch (error) {
                return finish({ success: false, error: `Couldn't scroll: ${error}` });
            }
        }

        case 'mouse-move': {
            try {
                await invoke('voice_mouse_move', { dx: action.dx, dy: action.dy });
                return finish({ success: true, message: 'Moved the cursor.' });
            } catch (error) {
                return finish({
                    success: false,
                    error: `Couldn't move the cursor: ${error}`,
                });
            }
        }

        case 'open-mouse-grid': {
            try {
                await invoke('show_mouse_grid_window_command');
                return finish({ success: true, message: 'Opened the mouse grid.' });
            } catch (error) {
                return finish({
                    success: false,
                    error: `Couldn't open the mouse grid: ${error}`,
                });
            }
        }

        case 'open-ui-elements': {
            // Enumerate the focused window's controls BEFORE the overlay
            // is shown — once it appears, IT is the foreground window.
            let count: number;
            try {
                count = await invoke<number>('voice_list_ui_elements');
            } catch (error) {
                return finish({
                    success: false,
                    error: `Couldn't read the focused window: ${error}`,
                });
            }
            if (count === 0) {
                toast(
                    'No clickable elements found in the active window.',
                    'info',
                    3500,
                );
                return finish({
                    success: false,
                    error: 'No clickable elements found in the active window.',
                });
            }
            try {
                await invoke('show_ui_elements_window_command');
                return finish({
                    success: true,
                    message: `Showing ${count} clickable element${
                        count === 1 ? '' : 's'
                    }.`,
                });
            } catch (error) {
                return finish({
                    success: false,
                    error: `Couldn't open the elements overlay: ${error}`,
                });
            }
        }

        case 'window-action': {
            try {
                await invoke('voice_window_action', { action: action.action });
                return finish({
                    success: true,
                    message: `Window: ${action.action.replace('-', ' ')}.`,
                });
            } catch (error) {
                return finish({
                    success: false,
                    error: `Couldn't control the window: ${error}`,
                });
            }
        }

        case 'noop':
            return finish({ success: true, message: 'Nothing to do.' });
    }
}
