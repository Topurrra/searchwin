/*
  emojiSearch — pure ranking over the bundled `emojiData` table.

  Kept out of PaletteV2.svelte so it can be unit-tested (see
  emojiSearch.test.ts). No I/O, no stores, no Tauri — recents are passed
  in by the caller so this stays a pure function of its arguments.
*/

import { EMOJI, type EmojiEntry } from './emojiData';

/** char → entry, so a recents list (which stores only the character)
 *  can be resolved back to its searchable row. */
const BY_CHAR = new Map(EMOJI.map((e) => [e.c, e]));

export function emojiForChar(char: string): EmojiEntry | undefined {
    return BY_CHAR.get(char);
}

/** Everything a row is matched against: Unicode name + hand-authored keywords. */
function haystack(e: EmojiEntry): string {
    return e.k ? `${e.n} ${e.k}` : e.n;
}

/**
 * Rank emoji for `query`.
 *
 * - Empty query → the recents list first, then the table in Unicode order.
 * - Otherwise → AND-token match (every whitespace-separated token must
 *   appear somewhere in name+keywords), mirroring the AND-token contract
 *   the rest of the palette uses.
 *
 * `recents` is a most-recent-first list of emoji characters; a match that
 * appears in it is floated above everything else, in recency order.
 */
export function searchEmoji(query: string, recents: string[] = [], limit = 60): EmojiEntry[] {
    const rank = new Map(recents.map((c, i) => [c, i]));
    const q = query.trim().toLowerCase();

    if (!q) {
        const recent = recents.map((c) => BY_CHAR.get(c)).filter((e): e is EmojiEntry => !!e);
        const seen = new Set(recent.map((e) => e.c));
        return [...recent, ...EMOJI.filter((e) => !seen.has(e.c))].slice(0, limit);
    }

    const tokens = q.split(/[\s_]+/).filter(Boolean);
    const scored: { e: EmojiEntry; score: number }[] = [];

    for (const e of EMOJI) {
        const hay = haystack(e);
        if (!tokens.every((t) => hay.includes(t))) continue;

        let score = 0;
        if (e.n === q) score += 500;
        else if (e.n.startsWith(q)) score += 200;
        else if (hay.split(/[\s]+/).some((w) => w.startsWith(tokens[0]))) score += 100;
        // Shorter names are the more "primary" emoji ("cat" over "cat with
        // wry smile"), so nudge them up within the same tier.
        score += Math.max(0, 40 - e.n.length);

        // Recency must dominate every other signal, and the gap between
        // two recents must be wider than the name-length tiebreak above —
        // otherwise a shorter-named older pick outranks a newer one.
        const r = rank.get(e.c);
        if (r !== undefined) score += 100_000 - r * 1_000;

        scored.push({ e, score });
    }

    scored.sort((a, b) => b.score - a.score);
    return scored.slice(0, limit).map((s) => s.e);
}
