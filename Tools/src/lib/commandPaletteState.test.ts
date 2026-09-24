import { describe, expect, it } from 'vitest';
import {
    cyclePaletteScope,
    defaultScopeForPaletteMode,
    modeForPaletteScope,
    type CommandPaletteScope,
} from './commandPaletteState';

describe('command palette mode and scope mapping', () => {
    it('keeps top-level modes and their canonical scopes together', () => {
        expect(defaultScopeForPaletteMode('default')).toBe('all');
        expect(defaultScopeForPaletteMode('clipboard')).toBe('clipboard');
        expect(defaultScopeForPaletteMode('voice')).toBe('voice');
    });

    it('keeps every search scope in default mode except Clipboard and Voice', () => {
        const defaultScopes: CommandPaletteScope[] = [
            'all',
            'tools',
            'files',
            'notes',
            'apps',
            'windows',
            'emoji',
            'commands',
            'browser',
        ];

        expect(defaultScopes.map(modeForPaletteScope)).toEqual(
            defaultScopes.map(() => 'default'),
        );
        expect(modeForPaletteScope('clipboard')).toBe('clipboard');
        expect(modeForPaletteScope('voice')).toBe('voice');
    });

    it('cycles the available scopes with wraparound', () => {
        const scopes: CommandPaletteScope[] = ['all', 'files', 'commands'];

        expect(cyclePaletteScope(scopes, 'all', 1)).toBe('files');
        expect(cyclePaletteScope(scopes, 'commands', 1)).toBe('all');
        expect(cyclePaletteScope(scopes, 'all', -1)).toBe('commands');
        expect(cyclePaletteScope(scopes, 'browser', 1)).toBe('all');
        expect(cyclePaletteScope([], 'all', 1)).toBeNull();
    });
});
