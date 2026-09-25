import { writable } from 'svelte/store';
import type { ToolPackId } from '$lib/appScreens';

/*
  categoryActiveTool — a one-shot hand-off telling a CategoryWorkspace
  which tool to activate when it mounts.

  Pack tools no longer render as their own top-level screen; they live
  inside their category's workspace page (`category-<packId>`). When any
  existing entry point navigates to a tool id
  (sidebar, command palette, deep link, frecency, "open in main"), the
  router redirects `selected` to the tool's category screen and stashes
  the original tool id here. The CategoryWorkspace reads it on mount,
  activates that tool, and clears the store.

  Null = "open the category at its default (first / last-used) tool."
*/
export const categoryActiveTool = writable<string | null>(null);

/** Selected Library pack, restored after returning from another workspace. */
export const libraryActivePack = writable<ToolPackId | null>(null);

/*
  commandActiveTab — the same one-shot hand-off, but for the merged
  'command' page (Search / Clipboard / Voice tabs, with Snippets living
  under the Clipboard tab). The router redirects the legacy ids
  (file-search → search, clipboard-history → clipboard, voice-to-text →
  voice, snippets → snippets) to `selected = 'command'` and stashes which
  tab to open here. CommandWorkspace reads it, switches tab, and clears it.

  Null = "open the command page at its default (Search) tab."
*/
export type CommandTab = 'search' | 'clipboard' | 'voice' | 'snippets';
export const commandActiveTab = writable<CommandTab | null>(null);

/** Map a legacy screen id to the command tab it now lives in, or null if
 *  the id isn't merged into the command page. Used by the router redirect. */
export function commandTabForId(id: string): CommandTab | null {
    switch (id) {
        case 'file-search':
            return 'search';
        case 'clipboard-history':
            return 'clipboard';
        case 'voice-to-text':
            return 'voice';
        case 'snippets':
            return 'snippets';
        default:
            return null;
    }
}
