/*
  transcriptNormalizer tests (#26).

  `normalizeTranscript` is pure — it cleans up Vosk output (whitespace,
  case, and known mishearings of the command / dictation vocabulary)
  before the matcher sees it. No mocks needed.
*/

import { describe, expect, it } from 'vitest';
import { normalizeTranscript } from './transcriptNormalizer';

describe('normalizeTranscript — cleanup contract', () => {
    it('trims, collapses whitespace runs, and lowercases', () => {
        const result = normalizeTranscript('  Hello   World  ', { mode: 'command' });
        expect(result.normalized).toBe('hello world');
        expect(result.tokens).toEqual(['hello', 'world']);
    });

    it('preserves the raw input verbatim', () => {
        expect(normalizeTranscript('  Raw  ', { mode: 'dictation' }).raw).toBe(
            '  Raw  ',
        );
    });

    it('returns empty for blank input', () => {
        const result = normalizeTranscript('   ', { mode: 'command' });
        expect(result.normalized).toBe('');
        expect(result.tokens).toEqual([]);
    });

    it('echoes the requested mode', () => {
        expect(normalizeTranscript('hi', { mode: 'command' }).mode).toBe('command');
        expect(normalizeTranscript('hi', { mode: 'dictation' }).mode).toBe('dictation');
    });
});

describe('normalizeTranscript — alias correctness', () => {
    it('corrects known command-mode mishearings', () => {
        expect(
            normalizeTranscript('open clip board', { mode: 'command' }).normalized,
        ).toBe('open clipboard');
        expect(normalizeTranscript('set tings', { mode: 'command' }).normalized).toBe(
            'settings',
        );
    });

    it('corrects known dictation-mode mishearings', () => {
        expect(normalizeTranscript('new lying', { mode: 'dictation' }).normalized).toBe(
            'new line',
        );
        expect(
            normalizeTranscript('question marc', { mode: 'dictation' }).normalized,
        ).toBe('question mark');
    });

    it('does not apply command aliases in dictation mode', () => {
        // "clip board" is a command-mode alias only — dictation leaves it.
        expect(normalizeTranscript('clip board', { mode: 'dictation' }).normalized).toBe(
            'clip board',
        );
    });

    it('records each correction it made', () => {
        const result = normalizeTranscript('open clip board', { mode: 'command' });
        expect(result.corrections.some((c) => c.to === 'clipboard')).toBe(true);
    });

    it('leaves an already-clean transcript uncorrected', () => {
        const result = normalizeTranscript('open clipboard', { mode: 'command' });
        expect(result.normalized).toBe('open clipboard');
        expect(result.corrections).toEqual([]);
    });
});
