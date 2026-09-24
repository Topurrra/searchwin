/*
  commandRegistry tests (#26).

  Covers the deterministic matcher (`matchVoiceCommand`) and the
  grammar builder, plus the registry↔grammar no-drift guarantee: every
  phrase the grammar feeds to Vosk must be a phrase the matcher can
  actually resolve. Tauri APIs are mocked only so the import graph
  (commandRegistry → voiceSafetyGate → voiceActionExecutor) resolves.
*/

import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({
    emit: vi.fn(),
    emitTo: vi.fn(),
    listen: vi.fn(),
}));

import { matchVoiceCommand, buildCommandGrammar } from './commandRegistry';
import { confirmationGrammarPhrases } from './voiceSafetyGate';

describe('matchVoiceCommand', () => {
    it('matches an overlay command exactly', () => {
        const result = matchVoiceCommand('open clipboard', []);
        expect(result.kind).toBe('match');
        if (result.kind === 'match') {
            expect(result.match.definition.id).toBe('overlay.clipboard');
            expect(result.match.matchKind).toBe('exact');
        }
    });

    it('matches a literal keyboard command', () => {
        const result = matchVoiceCommand('press enter', []);
        expect(result.kind).toBe('match');
        if (result.kind === 'match') {
            expect(result.match.definition.literal).toBe(true);
        }
    });

    it('matches a bare web search and captures the query', () => {
        const result = matchVoiceCommand('search rust async', []);
        expect(result.kind).toBe('match');
        if (result.kind === 'match') {
            expect(result.match.definition.id).toBe('search.web');
            expect(result.match.query).toBe('rust async');
        }
    });

    it('matches an app by name', () => {
        const result = matchVoiceCommand('open chrome', []);
        expect(result.kind).toBe('match');
        if (result.kind === 'match') {
            expect(result.match.definition.id).toBe('app.chrome');
        }
    });

    it('returns none for unrecognized speech', () => {
        expect(matchVoiceCommand('zxqw plorb glarn', []).kind).toBe('none');
    });

    it('returns none for an empty transcript', () => {
        expect(matchVoiceCommand('', []).kind).toBe('none');
    });
});

describe('buildCommandGrammar — registry no-drift', () => {
    it('emits a phrase for representative commands', () => {
        const grammar = buildCommandGrammar([]);
        expect(grammar).toContain('open clipboard');
        expect(grammar).toContain('press enter');
    });

    it('every non-confirmation grammar phrase resolves to a command', () => {
        const grammar = buildCommandGrammar([]);
        // The gate's confirm / cancel words are added to the grammar so
        // a parked command can be answered; they are not commands
        // themselves, so they are the one expected non-match.
        const confirmWords = new Set(confirmationGrammarPhrases('en'));
        for (const phrase of grammar) {
            if (confirmWords.has(phrase)) continue;
            expect(
                matchVoiceCommand(phrase, []).kind,
                `grammar phrase "${phrase}" should resolve to a command`,
            ).not.toBe('none');
        }
    });
});
