/*
  pendingNavigation — one-shot tool-navigation handoff for the embedded
  command palette.

  Context: the new unified palette (`/command`) is, in production, meant
  to live in its own Tauri overlay window. Until that window plumbing
  lands (Phase 3.6.6), the palette is being A/B tested by navigating the
  MAIN window's webview to the `/command` route (bound to the Home page
  search). In that embedded mode the palette can't use the cross-window
  `navigate-tool` Tauri event to open a workspace tool — the main route
  isn't mounted while `/command` occupies the webview, so nothing is
  listening.

  This store bridges that gap: the embedded palette stashes the target
  here, then `goto('/')` re-mounts the main route, which reads + clears
  this on mount and applies the selection. Mirrors `settingsTarget`'s
  one-shot deep-link pattern.

  In production (real overlay window) this store is unused — the palette
  takes the `emitTo('main', 'navigate-tool', …)` path instead because
  the main route stays mounted in its own window.
*/

import { writable } from 'svelte/store';

export type PendingNavigation = {
    /** Screen id to select on the main route (e.g. 'file-search', 'jwt-decoder'). */
    toolId: string;
    /** Optional — only meaningful for the file-search screen. */
    searchQuery?: string;
    searchMode?: 'files' | 'content';
    /** Optional — only meaningful when toolId is 'settings'. Deep-links
     *  the Settings page to a section (mirrors the navigate-tool event). */
    settingsSection?: string;
    /** Wave K (2026-05-28): only meaningful when toolId is 'notes'.
     *  Opens this specific `.ki` note in the Notes editor as soon as
     *  it mounts. Mirrors the cross-window `navigate-tool` event's
     *  `notePath` field. */
    notePath?: string;
    /** Optional file staged by a palette action for the destination tool. */
    targetFile?: string;
};

/** Non-null means: when the main route mounts next, jump to this tool.
 *  The main route clears it after reading so it fires exactly once. */
export const pendingNavigation = writable<PendingNavigation | null>(null);
