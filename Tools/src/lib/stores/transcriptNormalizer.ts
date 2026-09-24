/*
  transcriptNormalizer — clean up raw speech-recognition output before
  command parsing or dictation formatting.

  Voice upgrade, Layer 3 — the highest-value accuracy layer. Vosk
  produces text that is phonetically close but not command-exact:
  "open clip board" for "open clipboard", "new lying" for "new line",
  "scratch debt" for "scratch that". The deterministic matchers
  downstream (commandRegistry.ts, dictationCommands.ts) do EXACT phrase
  matching, so a one-syllable mishearing means a missed command. This
  module rewrites known mishearings back to their intended form first.

  Pure and deterministic — no AI, no network, no state. A lookup table.

  Two normalization modes:
    - 'command'   — AGGRESSIVE. The utterance is known to be a command
      (command mode), so the full command-vocabulary alias set applies.
    - 'dictation' — CONSERVATIVE. The utterance is free prose, so only
      the in-dictation editing-command mishearings are fixed (so
      dictationCommands.ts still recognizes "new line", "scratch that",
      … even when Vosk mangles them). Arbitrary prose words are never
      rewritten — over-normalizing dictation would corrupt the text.

  Bilingual: separate English and Georgian alias tables. Both tables
  are merged and applied — English alias keys are Latin, Georgian keys
  are Georgian script, so a transcript in one language can never match
  the other language's keys; no locale argument is needed.

  Why TypeScript, not Rust (the plan tags #10 "(Backend)"): the whole
  command/dictation pipeline this feeds — commandRegistry.ts (matcher),
  dictationCommands.ts (formatter), commandMode/pushToTalk/voiceSession
  (coordinators) — is already TypeScript. The normalizer's OUTPUT is
  consumed by those TS modules; a Rust normalizer would just have to
  hand the text straight back across the FFI boundary. Keeping the
  normalizer beside the matchers it feeds is the coherent placement.
  (#11–#16 — registry, context, parser, executor, feedback — are the
  evolution of these same TS modules and live here too.)

  Alias DATA is a curated starter set — its real value comes from
  tuning against observed Vosk output over time. The machinery is the
  deliverable; the tables grow. The Georgian tables are intentionally
  scaffolds: a Georgian alias must correct TO a real Georgian command
  or dictation phrase, and those vocabularies do not exist yet — they
  arrive with #11 (command registry) and #22 (bilingual dictation
  commands). The Georgian tables are populated alongside those tasks.
*/

/** Which alias set to apply — see the module header. */
export type NormalizationMode = 'command' | 'dictation';

export type TranscriptCorrectionKind = 'whitespace' | 'phrase' | 'word';

/** One change the normalizer made, kept for debugging / the #16 feedback
 *  stream. `from` is what the recognizer produced, `to` the correction. */
export interface TranscriptCorrection {
    kind: TranscriptCorrectionKind;
    from: string;
    to: string;
}

export interface NormalizedTranscript {
    /** The raw recognizer output, unchanged. */
    raw: string;
    /** The cleaned transcript — what downstream matchers should use. */
    normalized: string;
    /** `normalized` split on single spaces. */
    tokens: string[];
    /** Which mode was applied. */
    mode: NormalizationMode;
    /** Every correction applied, in order. Empty when nothing changed. */
    corrections: TranscriptCorrection[];
}

// ─── Alias tables ────────────────────────────────────────────────────
// Keys are lowercase mis-heard phrases (single- or multi-word); values
// are the intended phrase. Curated starter sets — expand from observed
// Vosk output.

/** English, command vocabulary — applied in 'command' mode only.
 *  Focused on the EXACT-matched overlay/settings words; app and tool
 *  names already get fuzzy matching + spoken-form aliases in
 *  commandRegistry.ts, so they are not duplicated here. */
const EN_COMMAND_ALIASES: Record<string, string> = {
    'clip board': 'clipboard',
    'clip bored': 'clipboard',
    'set tings': 'settings',
    'screen shot': 'screenshot',
};

/** English, in-dictation editing commands — applied in BOTH modes
 *  (dictation needs them; command mode is unaffected since these
 *  phrases are not command vocabulary). Mirrors dictationCommands.ts's
 *  vocabulary: "new line", "new paragraph", "scratch/delete that",
 *  "capitalize that", spoken punctuation. */
const EN_DICTATION_ALIASES: Record<string, string> = {
    'new lying': 'new line',
    newline: 'new line',
    'new pera graph': 'new paragraph',
    'new para graph': 'new paragraph',
    'scratch debt': 'scratch that',
    'scratch dat': 'scratch that',
    'scratched that': 'scratch that',
    'delete debt': 'delete that',
    'capitalize debt': 'capitalize that',
    'question marc': 'question mark',
};

/** Georgian alias tables — scaffolds. Populated alongside #11 (the
 *  Georgian command vocabulary) and #22 (bilingual dictation commands),
 *  once there are real Georgian target phrases to correct toward and
 *  observed `ka`-model output to curate from. Empty for now — Georgian
 *  transcripts still get whitespace cleanup; the machinery is ready. */
const KA_COMMAND_ALIASES: Record<string, string> = {};
const KA_DICTATION_ALIASES: Record<string, string> = {};

/** Command mode applies the command set. */
const COMMAND_ALIASES: Record<string, string> = {
    ...EN_COMMAND_ALIASES,
    ...KA_COMMAND_ALIASES,
};
/** Dictation mode applies the editing-command set only (conservative). */
const DICTATION_ALIASES: Record<string, string> = {
    ...EN_DICTATION_ALIASES,
    ...KA_DICTATION_ALIASES,
};

/** Widest key (in words) in a table — the greedy matcher's window. */
function maxPhraseWords(table: Record<string, string>): number {
    let max = 1;
    for (const key of Object.keys(table)) {
        const n = key.split(' ').length;
        if (n > max) max = n;
    }
    return max;
}
const COMMAND_MAX_WORDS = maxPhraseWords(COMMAND_ALIASES);
const DICTATION_MAX_WORDS = maxPhraseWords(DICTATION_ALIASES);

/**
 * Greedy longest-phrase alias replacement. Walks the token stream; at
 * each position tries the widest phrase window first. A replacement's
 * own tokens go straight to the output and are NOT re-scanned, so an
 * alias whose result contains another alias's trigger cannot loop.
 */
function applyAliases(
    words: string[],
    table: Record<string, string>,
    maxWords: number,
    corrections: TranscriptCorrection[],
): string[] {
    const out: string[] = [];
    let i = 0;
    while (i < words.length) {
        let replacement: string | null = null;
        let consumed = 1;
        const window = Math.min(maxWords, words.length - i);
        for (let len = window; len >= 1; len--) {
            const phrase = words.slice(i, i + len).join(' ');
            const hit = table[phrase];
            if (hit !== undefined) {
                replacement = hit;
                consumed = len;
                break;
            }
        }
        if (replacement === null) {
            out.push(words[i]);
            i += 1;
            continue;
        }
        corrections.push({
            kind: consumed > 1 ? 'phrase' : 'word',
            from: words.slice(i, i + consumed).join(' '),
            to: replacement,
        });
        for (const w of replacement.split(' ')) out.push(w);
        i += consumed;
    }
    return out;
}

/**
 * Normalize one raw transcript. Deterministic and side-effect free —
 * the same input always yields the same output.
 *
 * Pipeline: whitespace cleanup (trim + collapse runs + lowercase) →
 * alias replacement (the mode-appropriate, longest-match-first set).
 * "Context aliases" — corrections that depend on the active surface or
 * app — are deferred to #12, when the context resolver exists.
 */
export function normalizeTranscript(
    raw: string,
    options: { mode: NormalizationMode },
): NormalizedTranscript {
    const { mode } = options;
    const corrections: TranscriptCorrection[] = [];

    // Stage 1 — whitespace. Trim, collapse internal whitespace runs,
    // lowercase. Vosk already emits lowercase, so the lowercase is a
    // safe no-op that also guards against a future mixed-case engine.
    const cleaned = raw.trim().replace(/\s+/g, ' ').toLowerCase();
    if (cleaned.length === 0) {
        return { raw, normalized: '', tokens: [], mode, corrections };
    }
    if (cleaned !== raw) {
        corrections.push({ kind: 'whitespace', from: raw, to: cleaned });
    }

    // Stage 2 — alias replacement.
    const table = mode === 'command' ? COMMAND_ALIASES : DICTATION_ALIASES;
    const maxWords = mode === 'command' ? COMMAND_MAX_WORDS : DICTATION_MAX_WORDS;
    const tokens = applyAliases(cleaned.split(' '), table, maxWords, corrections);

    return { raw, normalized: tokens.join(' '), tokens, mode, corrections };
}
