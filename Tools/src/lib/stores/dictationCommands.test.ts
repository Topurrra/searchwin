/*
  dictationCommands tests (#26).

  `processDictation` is a pure, deterministic function — no IO, no
  Tauri, no store reads (the locale is a parameter) — so it unit-tests
  with no mocks. Covers the English set, the Georgian set (#22), and
  the locale separation between them.
*/

import { describe, expect, it } from 'vitest';
import { processDictation } from './dictationCommands';

describe('processDictation — English', () => {
    it('returns empty for empty or whitespace input', () => {
        expect(processDictation('', 'en')).toBe('');
        expect(processDictation('   ', 'en')).toBe('');
    });

    it('passes plain prose through unchanged', () => {
        expect(processDictation('the quick brown fox', 'en')).toBe(
            'the quick brown fox',
        );
    });

    it('inserts punctuation hugging the preceding word', () => {
        expect(processDictation('hello comma world', 'en')).toBe('hello, world');
        expect(processDictation('done period', 'en')).toBe('done.');
        expect(processDictation('really question mark', 'en')).toBe('really?');
    });

    it('handles new line and new paragraph', () => {
        expect(processDictation('one new line two', 'en')).toBe('one\ntwo');
        expect(processDictation('one new paragraph two', 'en')).toBe('one\n\ntwo');
    });

    it('"scratch that" drops the previous word', () => {
        expect(processDictation('keep this scratch that', 'en')).toBe('keep');
    });

    it('"undo" reverts the last command', () => {
        // "hello comma" → "hello,"; "undo" reverts the comma.
        expect(processDictation('hello comma undo', 'en')).toBe('hello');
    });

    it('parentheses hug their content', () => {
        expect(processDictation('value open paren x close paren', 'en')).toBe(
            'value (x)',
        );
    });

    it('capitalizes the most recent word', () => {
        expect(processDictation('hello capitalize that', 'en')).toBe('Hello');
    });
});

describe('processDictation — Georgian (#22)', () => {
    it('inserts a Georgian-dictated comma', () => {
        expect(processDictation('გამარჯობა მძიმე მსოფლიო', 'ka')).toBe(
            'გამარჯობა, მსოფლიო',
        );
    });

    it('"ახალი ხაზი" is a new line', () => {
        expect(processDictation('ერთი ახალი ხაზი ორი', 'ka')).toBe('ერთი\nორი');
    });

    it('"წაშალე ეს" drops the previous word', () => {
        expect(processDictation('სიტყვა წაშალე ეს', 'ka')).toBe('');
    });
});

describe('processDictation — locale separation', () => {
    it('does not apply English command words in a Georgian transcript', () => {
        // "comma" is not a Georgian command word — it stays literal text.
        expect(processDictation('test comma', 'ka')).toBe('test comma');
    });

    it('does not apply Georgian command words in an English transcript', () => {
        expect(processDictation('test მძიმე', 'en')).toBe('test მძიმე');
    });
});
