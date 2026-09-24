/*
  userVoiceCommands tests (#26).

  `parseUserVoiceCommands` is the pure command-script parser (#21) —
  text in, `VoiceCommandDefinition`s + per-line errors out. The module
  touches Tauri only inside `initUserVoiceCommands` / `loadUserVoiceCommands`
  (not exercised here), so the Tauri APIs are mocked just so the import
  graph resolves in a plain test process.
*/

import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import { parseUserVoiceCommands } from './userVoiceCommands';

describe('parseUserVoiceCommands — actions', () => {
    it('parses a key command into a keystroke action', () => {
        const { commands, errors } = parseUserVoiceCommands('save it = key ctrl+s');
        expect(errors).toEqual([]);
        expect(commands).toHaveLength(1);
        expect(commands[0].phrases.en).toEqual(['save it']);
        expect(commands[0].literal).toBe(true);
        expect(commands[0].risk).toBe('medium');
        expect(commands[0].action({ query: '' })).toEqual({
            kind: 'keystroke',
            key: 's',
            modifiers: ['ctrl'],
        });
    });

    it('parses a url command and prepends https://', () => {
        const { commands } = parseUserVoiceCommands('my email = url mail.google.com');
        expect(commands[0].action({ query: '' })).toEqual({
            kind: 'open-url',
            url: 'https://mail.google.com',
        });
    });

    it('keeps an explicit http(s):// url', () => {
        const { commands } = parseUserVoiceCommands('site = url https://example.com');
        expect(commands[0].action({ query: '' })).toEqual({
            kind: 'open-url',
            url: 'https://example.com',
        });
    });

    it('ignores comments and blank lines', () => {
        const { commands } = parseUserVoiceCommands(
            '# a comment\n\n   \nsave = key ctrl+s\n',
        );
        expect(commands).toHaveLength(1);
    });
});

describe('parseUserVoiceCommands — per-app scoping (#21)', () => {
    it('scopes commands under an [app: …] header, scoped-first in the result', () => {
        const { commands } = parseUserVoiceCommands(
            'global one = key f1\n[app: chrome]\nscoped = key f2',
        );
        expect(commands).toHaveLength(2);
        expect(commands[0].appScope).toBe('chrome');
        expect(commands[1].appScope).toBeUndefined();
    });

    it('[app: *] clears the scope back to global', () => {
        const { commands } = parseUserVoiceCommands(
            '[app: chrome]\na = key f1\n[app: *]\nb = key f2',
        );
        const scopeByPhrase = Object.fromEntries(
            commands.map((c) => [c.phrases.en[0], c.appScope]),
        );
        expect(scopeByPhrase['a']).toBe('chrome');
        expect(scopeByPhrase['b']).toBeUndefined();
    });

    it('strips a trailing .exe from the app name', () => {
        const { commands } = parseUserVoiceCommands('[app: code.exe]\nx = key f1');
        expect(commands[0].appScope).toBe('code');
    });
});

describe('parseUserVoiceCommands — error reporting', () => {
    it('reports a line missing "="', () => {
        const { errors } = parseUserVoiceCommands('this line has no equals');
        expect(errors).toHaveLength(1);
        expect(errors[0].line).toBe(1);
    });

    it('reports an unknown action keyword', () => {
        const { errors } = parseUserVoiceCommands('x = fly to the moon');
        expect(errors[0].message).toContain('unknown action');
    });

    it('reports an unknown key', () => {
        expect(parseUserVoiceCommands('x = key ctrl+splat').errors).toHaveLength(1);
    });

    it('reports an unknown modifier', () => {
        expect(parseUserVoiceCommands('x = key hyper+s').errors[0].message).toContain(
            'modifier',
        );
    });

    it('reports a duplicate phrase and keeps only the first', () => {
        const { commands, errors } = parseUserVoiceCommands(
            'go = key f1\ngo = key f2',
        );
        expect(commands).toHaveLength(1);
        expect(errors).toHaveLength(1);
        expect(errors[0].message).toContain('duplicate');
    });

    it('rejects a phrase with no usable words', () => {
        const { errors } = parseUserVoiceCommands('123 456 = key f1');
        expect(errors).toHaveLength(1);
    });
});
