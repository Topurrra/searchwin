import { describe, expect, it } from 'vitest';
import type { Tool } from '$lib/appScreens';
import { filterToolsForProfile, type Profile } from './profiles';

const candidates = [
    { id: 'file-search', category: 'File', packId: 'core' },
    { id: 'encoders', category: 'Utils', packId: 'utils' },
    { id: 'screenshot-redact', category: 'Privacy', packId: 'privacy' },
] as Tool[];

function profile(toolIds: string[]): Profile {
    return {
        id: 'test',
        name: 'Test profile',
        toolIds,
        builtIn: false,
        schemaVersion: 1,
    };
}

describe('filterToolsForProfile', () => {
    it('keeps all tools for the all-tools profile', () => {
        expect(filterToolsForProfile(profile([]), candidates)).toEqual(candidates);
    });

    it('keeps Core while hiding optional tools outside the active profile', () => {
        expect(filterToolsForProfile(profile(['encoders']), candidates).map((tool) => tool.id)).toEqual([
            'file-search',
            'encoders',
        ]);
    });
});
