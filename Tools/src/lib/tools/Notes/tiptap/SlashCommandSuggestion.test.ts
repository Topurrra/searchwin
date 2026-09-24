import { describe, expect, it } from 'vitest';
import { SlashCommandSuggestion, filterSlashCommands, type SlashCommand } from './SlashCommandSuggestion';
import { WikiLinkSuggestion } from './WikiLinkSuggestion';

const commands: SlashCommand[] = [
    { title: 'Heading 1', detail: 'Large section heading', keywords: ['title'], run: () => {} },
    { title: 'Checklist', detail: 'Track an action', keywords: ['task'], run: () => {} },
    { title: 'Callout', detail: 'Important context', keywords: ['note'], run: () => {} },
];

describe('filterSlashCommands', () => {
    it('matches an alias for a command', () => {
        expect(filterSlashCommands(commands, 'task').map((command) => command.title)).toEqual([
            'Checklist',
        ]);
    });

    it('uses a different ProseMirror key from wiki-link suggestions', () => {
        const slashPlugin = (SlashCommandSuggestion.config.addProseMirrorPlugins as any).call({
            editor: {},
            options: { getCommands: () => commands },
        })[0];
        const wikiPlugin = (WikiLinkSuggestion.config.addProseMirrorPlugins as any).call({
            editor: {},
            options: { getTitles: () => [] },
        })[0];

        expect(slashPlugin.key).not.toBe(wikiPlugin.key);
    });
});
