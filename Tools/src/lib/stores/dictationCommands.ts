/*
  dictationCommands — in-dictation editing & formatting commands.

  Voice Commander, Phase A. When the user dictates text — via the voice
  overlay, push-to-talk, or the Voice to Text tool's continuous mode —
  certain spoken phrases are FORMATTING / EDITING commands rather than
  literal words to transcribe:

    "comma"            → ,            "new line"        → line break
    "period"           → .            "new paragraph"   → blank line
    "question mark"    → ?            "capitalize that" → Capitalize prev word
    "exclamation mark" → !            "scratch that"    → delete prev word/mark
    "colon" / "semicolon" → : ;       "delete that"     → (same as scratch)
    "open/close paren" → ( )          "undo"            → revert last command

  `processDictation()` takes ONE raw transcript and returns it with
  these commands applied. It is a PURE, DETERMINISTIC function — no AI,
  no network, no state kept between calls. That is what lets dictation
  formatting run on the lightest hardware and stay verifiably local
  (KeepItLocal's golden rules): there is nothing here to audit but a
  lookup table.

  Why a single per-transcript pass (and not a persistent buffer):
  the two surfaces that insert dictation into other apps — the voice
  overlay and push-to-talk — each hand us the COMPLETE dictation as one
  string (PTT joins all of its `voice-final` utterances before calling
  us). A single left-to-right pass over that string therefore sees the
  whole utterance, so "scratch that" / "undo" can reach anything earlier
  in the same dictation. No cross-call state is needed, which keeps this
  trivially correct. (The Voice to Text tool processes each utterance as
  it arrives, so there a command only reaches within its own utterance —
  acceptable, since that tool's transcript box is freely editable.)
*/

import type { Locale } from './settings';

/** What a matched command phrase does. */
type DictationAction =
    | { type: 'punct'; ch: string }
    | { type: 'open'; ch: string }
    | { type: 'close'; ch: string }
    | { type: 'newline'; text: string }
    | { type: 'capitalize' }
    | { type: 'scratch' }
    | { type: 'undo' };

/**
 * The phrase → action table, per locale (#22 — bilingual). Keys are
 * lowercase, space-separated phrases; matching is greedy longest-first
 * so "new paragraph" beats "new" and "question mark" is never split.
 * Single-word triggers are deliberately limited to the few a user
 * expects to be commands — multi-word phrases are far less likely to
 * collide with literal prose.
 *
 * The locale is the LOADED Vosk model's language, not the UI locale: a
 * Georgian model transcribes Georgian, so it needs the Georgian command
 * words. The `ka` set is a sensible starting point — tune it against
 * real `ka`-model output (same caveat as the #11 command registry).
 * `ka` has no `capitalize`: the Mkhedruli script is unicameral, so
 * "capitalize" is not a meaningful dictation command for Georgian.
 */
const COMMAND_PHRASES: Record<Locale, Record<string, DictationAction>> = {
    en: {
        comma: { type: 'punct', ch: ',' },
        period: { type: 'punct', ch: '.' },
        'full stop': { type: 'punct', ch: '.' },
        'question mark': { type: 'punct', ch: '?' },
        'exclamation mark': { type: 'punct', ch: '!' },
        'exclamation point': { type: 'punct', ch: '!' },
        colon: { type: 'punct', ch: ':' },
        semicolon: { type: 'punct', ch: ';' },
        'open parenthesis': { type: 'open', ch: '(' },
        'open paren': { type: 'open', ch: '(' },
        'close parenthesis': { type: 'close', ch: ')' },
        'close paren': { type: 'close', ch: ')' },
        'new line': { type: 'newline', text: '\n' },
        'new paragraph': { type: 'newline', text: '\n\n' },
        'capitalize that': { type: 'capitalize' },
        'scratch that': { type: 'scratch' },
        'delete that': { type: 'scratch' },
        undo: { type: 'undo' },
        'undo that': { type: 'undo' },
    },
    ka: {
        მძიმე: { type: 'punct', ch: ',' },
        წერტილი: { type: 'punct', ch: '.' },
        'კითხვის ნიშანი': { type: 'punct', ch: '?' },
        'ძახილის ნიშანი': { type: 'punct', ch: '!' },
        ორწერტილი: { type: 'punct', ch: ':' },
        წერტილმძიმე: { type: 'punct', ch: ';' },
        'ფრჩხილის გახსნა': { type: 'open', ch: '(' },
        'ფრჩხილის დახურვა': { type: 'close', ch: ')' },
        'ახალი ხაზი': { type: 'newline', text: '\n' },
        'ახალი აბზაცი': { type: 'newline', text: '\n\n' },
        'წაშალე ეს': { type: 'scratch' },
        'გააუქმე ეს': { type: 'scratch' },
        'უკან დააბრუნე': { type: 'undo' },
        დააბრუნე: { type: 'undo' },
    },
};

/** Longest phrase length per locale — the greedy matcher's window. */
const MAX_PHRASE_WORDS: Record<Locale, number> = {
    en: Math.max(1, ...Object.keys(COMMAND_PHRASES.en).map((p) => p.split(' ').length)),
    ka: Math.max(1, ...Object.keys(COMMAND_PHRASES.ka).map((p) => p.split(' ').length)),
};

/** One emitted piece of output. `kind` drives spacing in `render()`:
 *   - word    — space-separated from its neighbours
 *   - punct   — , . ? ! : ; — hugs the preceding text, space after
 *   - open    — ( — space before, hugs the following text
 *   - close   — ) — hugs the preceding text, space after
 *   - newline — \n / \n\n — no surrounding spaces */
type SegmentKind = 'word' | 'punct' | 'open' | 'close' | 'newline';
interface Segment {
    kind: SegmentKind;
    text: string;
}

/**
 * Apply in-dictation formatting/editing commands to a raw transcript.
 *
 * `locale` selects the command-phrase set — it is the loaded Vosk
 * model's language (a Georgian model needs the Georgian words), not the
 * UI locale; callers pass `voiceModelLocale()`. Defaults to English.
 *
 * Deterministic and side-effect free: the same input always yields the
 * same output. Unrecognized words pass through verbatim (original case
 * preserved), so non-command dictation comes back essentially
 * unchanged — only re-spaced around any punctuation the user dictated.
 */
export function processDictation(raw: string, locale: Locale = 'en'): string {
    const trimmed = raw.trim();
    if (trimmed.length === 0) return '';

    const words = trimmed.split(/\s+/);
    const table = COMMAND_PHRASES[locale];

    // `segments` is the output built so far; `history` holds a snapshot
    // of it taken BEFORE each mutating step, so "undo" can restore the
    // previous state. Snapshots are shallow array copies — safe because
    // segments are never mutated in place (capitalize REPLACES an
    // element; push/pop only change the array length).
    let segments: Segment[] = [];
    const history: Segment[][] = [];

    let i = 0;
    while (i < words.length) {
        // Greedy longest-match at position i: try the widest phrase
        // window first so "new paragraph" beats "new", "scratch that"
        // beats a bare "scratch".
        let action: DictationAction | null = null;
        let consumed = 1;
        const maxLen = Math.min(MAX_PHRASE_WORDS[locale], words.length - i);
        for (let len = maxLen; len >= 1; len--) {
            const phrase = words
                .slice(i, i + len)
                .join(' ')
                .toLowerCase();
            const hit = table[phrase];
            if (hit) {
                action = hit;
                consumed = len;
                break;
            }
        }

        if (!action) {
            // Literal word — emit verbatim.
            history.push(segments.slice());
            segments.push({ kind: 'word', text: words[i] });
            i += 1;
            continue;
        }

        i += consumed;

        if (action.type === 'undo') {
            // Revert one step. Undo is itself not recorded, so two
            // "undo"s in a row revert two steps (the intuitive feel);
            // with nothing left to revert it is a no-op.
            const prev = history.pop();
            if (prev) segments = prev;
            continue;
        }

        history.push(segments.slice());
        applyAction(segments, action);
    }

    return render(segments);
}

/** Apply one matched command to `segments` (mutates it in place).
 *  `undo` is handled by the caller — it restores a snapshot rather than
 *  pushing or editing a segment. */
function applyAction(segments: Segment[], action: DictationAction): void {
    switch (action.type) {
        case 'punct':
            segments.push({ kind: 'punct', text: action.ch });
            break;
        case 'open':
            segments.push({ kind: 'open', text: action.ch });
            break;
        case 'close':
            segments.push({ kind: 'close', text: action.ch });
            break;
        case 'newline':
            segments.push({ kind: 'newline', text: action.text });
            break;
        case 'capitalize': {
            // Capitalize the first letter of the most recent WORD
            // segment, skipping over any trailing punctuation/newlines.
            for (let j = segments.length - 1; j >= 0; j--) {
                if (segments[j].kind === 'word') {
                    const w = segments[j].text;
                    segments[j] = {
                        kind: 'word',
                        text: w.charAt(0).toUpperCase() + w.slice(1),
                    };
                    break;
                }
            }
            break;
        }
        case 'scratch':
            // Drop the most recent emitted piece — a word, a mark, or a
            // line break, whichever came last. pop() on an empty list
            // is a harmless no-op.
            segments.pop();
            break;
    }
}

/** Join segments into the final string, applying the spacing rules. */
function render(segments: Segment[]): string {
    let out = '';
    for (let i = 0; i < segments.length; i++) {
        if (i > 0) out += separator(segments[i - 1], segments[i]);
        out += segments[i].text;
    }
    return out;
}

/** The string that goes BETWEEN two adjacent segments. */
function separator(prev: Segment, cur: Segment): string {
    // A line break never wants padding on either side.
    if (cur.kind === 'newline' || prev.kind === 'newline') return '';
    // Closing punctuation and , . ? ! : ; hug whatever precedes them.
    if (cur.kind === 'punct' || cur.kind === 'close') return '';
    // An opening bracket hugs whatever follows it.
    if (prev.kind === 'open') return '';
    return ' ';
}
