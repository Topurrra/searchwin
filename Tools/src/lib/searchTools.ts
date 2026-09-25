// The tools Search shows: Workspace's screens sorted into Search's eight
// packs, less what Search leaves out (offInSearch.ts). The tools index, a
// tool's own page and the field (through catalog.json) all read this, so a
// tool is listed, offered and opened in the same places, or in none.
import {
    getScreen,
    packLabels,
    pageScreens,
    toolScreens,
    type AppScreenLoader,
    type Category,
    type ToolPackId,
    type ToolScreen,
} from './appScreens';
import { offInSearch } from './offInSearch';

/** Search's packs, in the order the index shows them. */
export const searchPacks: readonly ToolPackId[] = [
    'utils',
    'file',
    'privacy',
    'image',
    'document',
    'development',
    'media',
    'time-focus',
];

const packOfCategory: Record<Category, ToolPackId> = {
    Utils: 'utils',
    File: 'file',
    Privacy: 'privacy',
    Image: 'image',
    Document: 'document',
    Development: 'development',
    Media: 'media',
    Focus: 'time-focus',
    Automation: 'automation',
};

/** Workspace kept these as pages of its own; in Search they're tools. */
const pagesAsTools: Readonly<Record<string, ToolPackId>> = {
    'privacy-audit': 'privacy',
    'time-tracker': 'time-focus',
};

/** Pages a tool sends you to that aren't tools themselves: they open at
 *  their address, but aren't listed or offered. */
const helperPages = new Set(['tool-packs', 'file-search-index', 'documentation']);

export interface SearchTool {
    id: string;
    name: string;
    description: string;
    pack: ToolPackId;
    icon: ToolScreen['icon'];
    loader: AppScreenLoader;
}

const fromTools: SearchTool[] = toolScreens.map((screen) => ({
    id: screen.id,
    name: screen.name,
    description: screen.description,
    pack: screen.packId ?? packOfCategory[screen.category],
    icon: screen.icon,
    loader: screen.loader,
}));

const fromPages: SearchTool[] = pageScreens
    .filter((screen) => screen.id in pagesAsTools && screen.icon)
    .map((screen) => ({
        id: screen.id,
        name: screen.name,
        description: screen.description,
        pack: pagesAsTools[screen.id],
        icon: screen.icon!,
        loader: screen.loader,
    }));

/** Every tool Search shows, each in one of its packs. */
export const searchTools: SearchTool[] = [...fromTools, ...fromPages].filter(
    (tool) => !(tool.id in offInSearch) && searchPacks.includes(tool.pack),
);

/** The index: each pack with its tools, in order, the empty ones left out. */
export function toolsByPack(tools: SearchTool[] = searchTools) {
    return searchPacks
        .map((pack) => ({ pack, name: packLabels[pack], tools: tools.filter((tool) => tool.pack === pack) }))
        .filter((group) => group.tools.length > 0);
}

/** catalog.json (written by catalog.js after the build): the contract with
 *  Search's field, which offers these as commands. The index's order. */
export function catalog() {
    return toolsByPack().flatMap((group) =>
        group.tools.map((tool) => ({ id: tool.id, name: tool.name, description: tool.description, pack: group.name })),
    );
}

/** What `#/tool/<id>` opens: the screen, or why it won't. */
export function openTool(
    id: string,
): { name: string; icon?: SearchTool['icon']; loader: AppScreenLoader } | { refused: string } {
    const tool = searchTools.find((candidate) => candidate.id === id);
    if (tool) return tool;
    const screen = getScreen(id);
    if (screen && helperPages.has(id)) return screen;
    if (id in offInSearch) return { refused: offInSearch[id] };
    return { refused: screen ? `${screen.name} isn't part of Search.` : `There's no tool called “${id}”.` };
}
