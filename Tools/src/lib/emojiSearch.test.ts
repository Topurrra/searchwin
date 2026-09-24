import { describe, it, expect } from 'vitest';
import { searchEmoji, emojiForChar } from './emojiSearch';
import { EMOJI } from './emojiData';

const chars = (q: string, recents: string[] = [], limit = 10) =>
    searchEmoji(q, recents, limit).map((e) => e.c);

describe('emojiSearch', () => {
    it('bundles a usable dataset with unique characters', () => {
        expect(EMOJI.length).toBeGreaterThan(1800);
        expect(new Set(EMOJI.map((e) => e.c)).size).toBe(EMOJI.length);
        // names are pre-lowercased — the matcher relies on it
        expect(EMOJI.every((e) => e.n === e.n.toLowerCase())).toBe(true);
    });

    it('matches on the Unicode name', () => {
        expect(chars('grinning face')).toContain('😀');
        expect(chars('rocket')).toContain('🚀');
    });

    it('matches on hand-authored keywords the name lacks', () => {
        expect(chars('thumbsup')).toContain('👍');
        expect(chars('poop')).toContain('💩');
        expect(chars('lol')).toContain('😂');
    });

    it('is AND-token: every token must appear', () => {
        expect(chars('face tears')).toContain('😂');
        expect(chars('rocket banana')).toEqual([]);
    });

    it('ranks an exact name first', () => {
        expect(chars('cat')[0]).toBe('🐈');
        expect(chars('fire')[0]).toBe('🔥');
    });

    it('floats recents to the top', () => {
        const withRecent = chars('face', ['🤖']);
        // 🤖 "robot face" would not otherwise lead the huge "face" list
        expect(withRecent[0]).toBe('🤖');
        expect(chars('face')[0]).not.toBe('🤖');
    });

    it('orders recents by recency, most recent first', () => {
        expect(chars('face', ['🤖', '🐱'])[0]).toBe('🤖');
        expect(chars('face', ['🐱', '🤖'])[0]).toBe('🐱');
    });

    it('empty query returns recents first, then the table', () => {
        const out = searchEmoji('', ['🚀'], 5);
        expect(out[0].c).toBe('🚀');
        expect(out).toHaveLength(5);
        // no duplicate of the recent further down
        expect(out.filter((e) => e.c === '🚀')).toHaveLength(1);
    });

    it('respects the limit', () => {
        expect(searchEmoji('face', [], 3)).toHaveLength(3);
    });

    it('returns the whole table when uncapped (full-picker mode)', () => {
        expect(searchEmoji('', [], Infinity)).toHaveLength(EMOJI.length);
    });

    it('resolves a character back to its entry', () => {
        expect(emojiForChar('🚀')?.n).toBe('rocket');
        expect(emojiForChar('not-an-emoji')).toBeUndefined();
    });

    it('finds keycap sequences by digit and by name', () => {
        expect(chars('5')).toContain('5️⃣');
        expect(chars('keycap nine')).toContain('9️⃣');
    });

    it('finds ZWJ sequences by keyword and exact name', () => {
        expect(chars('developer')).toContain('🧑‍💻');
        expect(chars('developer')).toContain('👩‍💻');
        expect(chars('family man woman girl boy')[0]).toBe('👨‍👩‍👧‍👦');
    });

    it('finds flags by country name, alias, and alpha-2 code', () => {
        expect(chars('georgia')[0]).toBe('🇬🇪');
        expect(chars('sakartvelo')).toContain('🇬🇪');
        expect(chars('usa')).toContain('🇺🇸');
        // 'uk' also substring-matches "ukraine" — GB just needs to surface
        expect(chars('uk')).toContain('🇬🇧');
        expect(chars('united kingdom')[0]).toBe('🇬🇧');
    });

    it('resolves multi-codepoint characters back to their entries', () => {
        expect(emojiForChar('🇬🇪')?.n).toBe('flag georgia');
        expect(emojiForChar('👨‍👩‍👧‍👦')?.n).toBe('family man woman girl boy');
    });
});
