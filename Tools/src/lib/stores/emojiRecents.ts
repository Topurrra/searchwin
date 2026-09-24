/*
  emojiRecents — the last N emoji picked from the command palette's
  Emoji chip, most-recent-first.

  Persistence: localStorage (`keepitlocal_emoji_recents_v1`), the same
  mechanism tool packs / command appearance already use — Tauri's webview
  persists localStorage in the app data dir, so it survives restart.

  Deliberately NOT wired into the Rust frecency system: this is a
  cosmetic ordering hint for one palette section, not something the
  backend ranker needs to know about.

  ponytail: no cross-window `storage` listener like myCommands has — the
  Emoji chip only exists inside the palette window, so there is no second
  writer to sync with. Add one if emoji ever get a Settings screen.
*/

import { writable } from 'svelte/store';

const STORAGE_KEY = 'keepitlocal_emoji_recents_v1';
const MAX_RECENTS = 24;

function loadLocal(): string[] {
    if (typeof localStorage === 'undefined') return [];
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        if (!raw) return [];
        const parsed: unknown = JSON.parse(raw);
        if (!Array.isArray(parsed)) return [];
        return parsed.filter((c): c is string => typeof c === 'string').slice(0, MAX_RECENTS);
    } catch {
        return [];
    }
}

export const emojiRecents = writable<string[]>(loadLocal());

emojiRecents.subscribe((list) => {
    if (typeof localStorage === 'undefined') return;
    try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
    } catch {
        // Quota / private-mode failures are not worth surfacing for a
        // most-recently-used list.
    }
});

/** Move `char` to the front of the recents list (deduped, capped). */
export function rememberEmoji(char: string) {
    emojiRecents.update((list) => [char, ...list.filter((c) => c !== char)].slice(0, MAX_RECENTS));
}
