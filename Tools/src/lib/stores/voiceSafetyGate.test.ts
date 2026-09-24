/*
  voiceSafetyGate tests (#26).

  The safety gate is the single chokepoint every matched command
  crosses (#14). These tests pin the risk × match-kind matrix, the
  confirmation flow, the repeat cooldown, and the literal-command
  cooldown exemption (#18a). Test commands use a `noop` action, so
  `executeAction` never invokes Tauri; the APIs are mocked only so the
  import graph resolves.
*/

import { describe, expect, it, beforeEach, vi } from 'vitest';
import { get } from 'svelte/store';

vi.mock('@tauri-apps/api/core', () => ({
    invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('@tauri-apps/api/event', () => ({
    emit: vi.fn(),
    emitTo: vi.fn(),
    listen: vi.fn(),
}));

import {
    runThroughGate,
    cancelPending,
    pendingConfirmation,
    CONFIRMATION_EXPIRY_MS,
} from './voiceSafetyGate';
import type {
    CommandRisk,
    VoiceCommandDefinition,
    VoiceCommandMatch,
    CommandMatchKind,
} from './commandRegistry';

/** A minimal command definition with a side-effect-free `noop` action. */
function def(
    risk: CommandRisk,
    opts: Partial<VoiceCommandDefinition> = {},
): VoiceCommandDefinition {
    return {
        id: 'test.cmd',
        title: 'Test command',
        phrases: { en: ['test'], ka: ['test'] },
        risk,
        context: 'global',
        requiresConfirmation: false,
        capturesQuery: false,
        action: () => ({ kind: 'noop' }),
        ...opts,
    };
}

function match(
    definition: VoiceCommandDefinition,
    matchKind: CommandMatchKind,
): VoiceCommandMatch {
    return { definition, query: '', matchKind };
}

describe('voiceSafetyGate — risk × match-kind matrix', () => {
    beforeEach(() => {
        cancelPending();
    });

    it('runs a safe command on a fuzzy match', async () => {
        const outcome = await runThroughGate(
            match(def('safe', { id: 'safe.fuzzy' }), 'fuzzy'),
        );
        expect(outcome.status).toBe('executed');
    });

    it('blocks a medium command on a fuzzy match', async () => {
        const outcome = await runThroughGate(
            match(def('medium', { id: 'medium.fuzzy' }), 'fuzzy'),
        );
        expect(outcome.status).toBe('blocked');
    });

    it('runs a medium command on an exact match', async () => {
        const outcome = await runThroughGate(
            match(def('medium', { id: 'medium.exact' }), 'exact'),
        );
        expect(outcome.status).toBe('executed');
    });

    it('blocks a dangerous command on a fuzzy match', async () => {
        const outcome = await runThroughGate(
            match(def('dangerous', { id: 'dangerous.fuzzy' }), 'fuzzy'),
        );
        expect(outcome.status).toBe('blocked');
    });
});

describe('voiceSafetyGate — confirmation flow', () => {
    beforeEach(() => {
        cancelPending();
    });

    it('parks a dangerous command for confirmation', async () => {
        const outcome = await runThroughGate(
            match(def('dangerous', { id: 'dangerous.exact' }), 'exact'),
        );
        expect(outcome.status).toBe('confirmation-required');
        expect(get(pendingConfirmation)).not.toBeNull();
    });

    it('parks any command flagged requiresConfirmation', async () => {
        const outcome = await runThroughGate(
            match(
                def('safe', { id: 'confirm.flagged', requiresConfirmation: true }),
                'exact',
            ),
        );
        expect(outcome.status).toBe('confirmation-required');
    });

    it('drops a parked confirmation after the expiry timeout', async () => {
        vi.useFakeTimers();
        try {
            await runThroughGate(
                match(def('dangerous', { id: 'confirm.expiry' }), 'exact'),
            );
            expect(get(pendingConfirmation)).not.toBeNull();
            vi.advanceTimersByTime(CONFIRMATION_EXPIRY_MS + 100);
            expect(get(pendingConfirmation)).toBeNull();
        } finally {
            vi.useRealTimers();
        }
    });
});

describe('voiceSafetyGate — repeat cooldown', () => {
    beforeEach(() => {
        cancelPending();
    });

    it('swallows an immediate repeat of the same command', async () => {
        const command = def('safe', { id: 'cooldown.repeat' });
        const first = await runThroughGate(match(command, 'exact'));
        const second = await runThroughGate(match(command, 'exact'));
        expect(first.status).toBe('executed');
        expect(second.status).toBe('cooldown');
    });

    it('exempts literal commands from the cooldown (#18a)', async () => {
        const command = def('medium', { id: 'cooldown.literal', literal: true });
        const first = await runThroughGate(match(command, 'exact'));
        const second = await runThroughGate(match(command, 'exact'));
        expect(first.status).toBe('executed');
        expect(second.status).toBe('executed');
    });
});
