/*
  voiceSafetyGate — the risk gate every matched voice command crosses
  before it is allowed to run.

  Voice upgrade, Layer 5 (#14). The matcher (commandRegistry's
  `matchVoiceCommand`) decides WHAT the user said. This gate decides
  whether it is SAFE TO RUN THAT NOW. Three independent concerns:

    1. Risk × match confidence. Every command carries a `risk` tier
       (safe / medium / dangerous) and arrives with a `matchKind` —
       `exact` (the transcript named the command) or `fuzzy` (a
       token-overlap guess against the tool list). The gate combines
       them:
         · safe       — runs on any match.
         · medium     — runs only on an exact match.
         · dangerous  — runs only on an exact match AND after a spoken
                        confirmation.
       A fuzzy match of anything riskier than `safe` is refused
       outright: KeepItLocal never acts destructively on a guess.

    2. Confirmation. A dangerous command — or any command whose
       definition opts in via `requiresConfirmation` — is not run on
       the spot. It is parked as `pendingConfirmation`; the user then
       says "confirm" to run it or "cancel" to drop it. A parked
       command expires on its own after CONFIRMATION_EXPIRY_MS, so a
       forgotten confirmation can never fire later, out of context.

    3. Repeat cooldown. Command mode listens continuously and Vosk can
       emit the same final twice. The gate swallows a re-run of the
       SAME command within REPEAT_COOLDOWN_MS.

  This module is the single chokepoint between a matched command and
  it running: `runThroughGate` / `resolvePendingConfirmation` build the
  command's declarative `VoiceAction` and hand it to `executeAction`
  (#15 — the sole performer of side effects). Every #14+ command,
  however it was matched, crosses this gate before it can run.

  Module boundary: `commandRegistry` imports this at runtime; this
  type-imports `VoiceCommandMatch` from `commandRegistry` (type-only —
  erased at build) and runtime-imports `executeAction` from the leaf
  module `voiceActionExecutor`, so there is no runtime import cycle.

  Bilingual: the confirm / cancel words are authored per locale. The
  command-mode grammar gets the active model's locale set (see
  `confirmationGrammarPhrases`); `resolvePendingConfirmation` accepts
  any locale's words, so a confirmation is understood regardless of
  which model is loaded.
*/

import { get, writable } from 'svelte/store';
import { toast } from './toasts';
import type { Locale } from './settings';
import type { VoiceCommandMatch } from './commandRegistry';
import { executeAction, type VoiceActionResult } from './voiceActionExecutor';

// ─── Tunables ────────────────────────────────────────────────────────

/** A re-run of the SAME command id within this window is suppressed —
 *  defends against a doubled final from the continuous recognizer. */
const REPEAT_COOLDOWN_MS = 1500;

/** A parked command is dropped if it is not confirmed / cancelled
 *  within this window. Exported so #16's confirmation UI can render a
 *  countdown against `PendingConfirmation.expiresAt`. */
export const CONFIRMATION_EXPIRY_MS = 8000;

// ─── Confirm / cancel vocabulary ─────────────────────────────────────

/** Words that CONFIRM a parked command, per locale. */
const CONFIRM_PHRASES: Record<Locale, string[]> = {
    en: ['confirm', 'yes confirm'],
    ka: ['დაადასტურე', 'დადასტურება'],
};

/** Words that CANCEL a parked command, per locale. */
const CANCEL_PHRASES: Record<Locale, string[]> = {
    en: ['cancel', 'no cancel'],
    ka: ['გააუქმე', 'გაუქმება'],
};

/** Every confirm / cancel word, all locales — `resolvePendingConfirmation`
 *  accepts a confirmation in either language. The transcript is already
 *  normalized by the time it reaches the gate, so no locale lookup is
 *  needed to interpret it. */
const ALL_CONFIRM = [...CONFIRM_PHRASES.en, ...CONFIRM_PHRASES.ka];
const ALL_CANCEL = [...CANCEL_PHRASES.en, ...CANCEL_PHRASES.ka];

/**
 * The confirm + cancel words for `locale` — the command-mode grammar
 * builder adds these to the Vosk vocabulary so a parked command's
 * spoken answer is actually recognizable. A session's grammar is fixed
 * for its lifetime, so these are ALWAYS in the vocabulary, not only
 * while a command happens to be parked.
 */
export function confirmationGrammarPhrases(locale: Locale): string[] {
    return [...CONFIRM_PHRASES[locale], ...CANCEL_PHRASES[locale]];
}

// ─── Gate outcomes ───────────────────────────────────────────────────

/** What the gate did with a matched command. `executed` carries the
 *  executor's structured result; the other outcomes never reached the
 *  executor. */
export type GateOutcome =
    | { status: 'executed'; title: string; result: VoiceActionResult }
    | { status: 'confirmation-required'; title: string }
    | { status: 'cooldown'; title: string }
    | { status: 'blocked'; title: string; reason: string };

/** What `resolvePendingConfirmation` did with a transcript — or `null`
 *  when there was nothing parked, or the transcript was not a confirm /
 *  cancel word (the parked command is left alone to be answered or to
 *  expire). `confirmed` carries the executor's result — the parked
 *  command actually ran. */
export type ConfirmationResolution =
    | { status: 'confirmed'; title: string; result: VoiceActionResult }
    | { status: 'cancelled'; title: string }
    | null;

// ─── Pending confirmation ────────────────────────────────────────────

/** A dangerous command parked, awaiting a spoken confirm / cancel. */
export interface PendingConfirmation {
    /** The matched command held until it is confirmed. */
    match: VoiceCommandMatch;
    /** Human-readable label — what the user is being asked to confirm. */
    title: string;
    /** Epoch ms at which this parked command expires untaken. */
    expiresAt: number;
}

const pending = writable<PendingConfirmation | null>(null);

/** Read-only handle. #16's confirmation UI subscribes to render the
 *  "say confirm or cancel" prompt and its countdown. */
export const pendingConfirmation = { subscribe: pending.subscribe };

/** Live expiry timer for the parked command — cleared whenever the
 *  pending command is taken, replaced, or expires. */
let expiryTimer: ReturnType<typeof setTimeout> | null = null;

/** Drop any parked command and stop its expiry timer. Idempotent. */
function clearPending(): void {
    if (expiryTimer) {
        clearTimeout(expiryTimer);
        expiryTimer = null;
    }
    pending.set(null);
}

/** Park `match` for confirmation, replacing any command already parked
 *  (only one waits at a time) and (re)starting the expiry timer. */
function park(match: VoiceCommandMatch): void {
    clearPending();
    pending.set({
        match,
        title: match.definition.title,
        expiresAt: Date.now() + CONFIRMATION_EXPIRY_MS,
    });
    expiryTimer = setTimeout(() => {
        expiryTimer = null;
        pending.set(null);
        toast('Confirmation timed out — command cancelled.', 'info', 3000);
    }, CONFIRMATION_EXPIRY_MS);
}

// ─── Repeat cooldown ─────────────────────────────────────────────────

/** Id + time of the command the gate last actually ran. */
let lastRun: { id: string; at: number } | null = null;

/** True while a re-run of `id` should be swallowed as a duplicate. */
function inCooldown(id: string): boolean {
    return (
        lastRun !== null &&
        lastRun.id === id &&
        Date.now() - lastRun.at < REPEAT_COOLDOWN_MS
    );
}

/** Run a matched command — build its declarative action, hand it to
 *  the executor — and arm the repeat cooldown. */
async function execute(match: VoiceCommandMatch): Promise<VoiceActionResult> {
    lastRun = { id: match.definition.id, at: Date.now() };
    return executeAction(match.definition.action({ query: match.query }));
}

// ─── The gate ────────────────────────────────────────────────────────

/**
 * Run a matched command through the risk gate — the ONLY path from a
 * `VoiceCommandMatch` to its handler running. Returns what happened so
 * the caller can report it (toast / activity log / #16 feedback).
 */
export async function runThroughGate(
    match: VoiceCommandMatch,
): Promise<GateOutcome> {
    const { definition, matchKind } = match;
    const title = definition.title;

    // A doubled final from the continuous recognizer — swallow it.
    // Literal keyboard / mouse commands (#18a) are exempt: deliberate
    // rapid repeats ("scroll down" several times) are normal use and
    // must not be mistaken for a duplicate.
    if (!definition.literal && inCooldown(definition.id)) {
        return { status: 'cooldown', title };
    }

    // Never act on a guess for anything riskier than `safe`. A fuzzy
    // match is a token-overlap approximation; a medium / dangerous
    // command demands that the user actually named it.
    if (matchKind === 'fuzzy' && definition.risk !== 'safe') {
        const reason = 'Not sure I heard that exactly — say the full command.';
        toast(reason, 'info', 4000);
        return { status: 'blocked', title, reason };
    }

    // Dangerous commands — and any command that opts in — are parked
    // for a spoken confirmation rather than run on the spot.
    if (definition.risk === 'dangerous' || definition.requiresConfirmation) {
        park(match);
        toast(`${title} — say "confirm" or "cancel".`, 'info', CONFIRMATION_EXPIRY_MS);
        return { status: 'confirmation-required', title };
    }

    // Safe (any match) or medium (exact match reached here) — run now.
    const result = await execute(match);
    return { status: 'executed', title, result };
}

/**
 * Run the parked command now. The spoken-"confirm" path routes here,
 * and so does the confirmation prompt's Confirm button (#16). No-op,
 * returning `null`, when nothing is parked.
 */
export async function confirmPending(): Promise<ConfirmationResolution> {
    const current = get(pending);
    if (!current) return null;
    // Take the parked command before running it — clearing first means
    // a second confirm (a doubled final, or a click after a spoken
    // "confirm") finds nothing parked, so the command can never run
    // twice.
    clearPending();
    const result = await execute(current.match);
    return { status: 'confirmed', title: current.title, result };
}

/**
 * Drop the parked command unrun. The spoken-"cancel" path and the
 * confirmation prompt's Cancel button (#16) both route here. No-op
 * when nothing is parked.
 */
export function cancelPending(): ConfirmationResolution {
    const current = get(pending);
    if (!current) return null;
    clearPending();
    toast(`Cancelled — ${current.title}.`, 'info', 2500);
    return { status: 'cancelled', title: current.title };
}

/**
 * Interpret `transcript` as a possible answer to a parked command.
 * Call this BEFORE matching the transcript as a fresh command: while a
 * command is parked, a bare "confirm" / "cancel" answers it and must
 * not be re-parsed.
 *
 *   · nothing parked, or not a confirm / cancel word → `null` (the
 *     caller proceeds to match the transcript normally; an unrelated
 *     command spoken mid-wait simply leaves the parked one to expire).
 *   · "confirm" → runs the parked command, returns `confirmed`.
 *   · "cancel"  → drops the parked command, returns `cancelled`.
 */
export async function resolvePendingConfirmation(
    transcript: string,
): Promise<ConfirmationResolution> {
    if (!get(pending)) return null;
    const text = transcript.trim().toLowerCase();
    if (ALL_CONFIRM.includes(text)) return confirmPending();
    if (ALL_CANCEL.includes(text)) return cancelPending();
    return null;
}
