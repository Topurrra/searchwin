export type CommandPaletteMode = 'default' | 'clipboard' | 'voice';

export type CommandPaletteScope =
    | 'all'
    | 'tools'
    | 'files'
    | 'notes'
    | 'clipboard'
    | 'voice'
    | 'apps'
    | 'windows'
    | 'emoji'
    | 'commands'
    | 'browser';

export function modeForPaletteScope(scope: CommandPaletteScope): CommandPaletteMode {
    if (scope === 'clipboard' || scope === 'voice') return scope;
    return 'default';
}

export function defaultScopeForPaletteMode(mode: CommandPaletteMode): CommandPaletteScope {
    return mode === 'default' ? 'all' : mode;
}

export function cyclePaletteScope(
    scopes: readonly CommandPaletteScope[],
    current: CommandPaletteScope,
    direction: 1 | -1,
): CommandPaletteScope | null {
    if (scopes.length === 0) return null;
    const currentIndex = scopes.indexOf(current);
    if (currentIndex < 0) return scopes[0];
    return scopes[(currentIndex + direction + scopes.length) % scopes.length];
}
