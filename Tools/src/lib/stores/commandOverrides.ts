/*
  commandOverrides — user customizations of the BUILT-IN voice commands.

  The built-in voice commands (overlay/navigation + keyboard/mouse/window
  input primitives) ship with phrases hardcoded in `commandRegistry.ts`.
  This store lets the user override a built-in command's trigger phrases
  (per locale) and reset them. Overrides are a map keyed by command id:

      { "overlay.clipboard": { en: ["clip"], ka: ["ბუფერი"] }, … }

  Persisted through the backend (redb, DPAPI-encrypted) via
  `voice_get_command_overrides` / `voice_set_command_overrides`, so they
  survive restarts. `commandRegistry.buildVoiceCommandRegistry` reads the
  synchronous snapshot and swaps a command's phrases when an override exists.

  Module boundary: a LEAF — it imports only Tauri + svelte/store (never
  `commandRegistry`), so the registry can import THIS with no runtime cycle,
  exactly like `userVoiceCommands`.
*/

import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event';

/** Broadcast when overrides change so every window re-reads (overlays keep
 *  their own snapshot, like the user-command file's change event). */
const CHANGED_EVENT = 'voice-command-overrides-changed';

/** A built-in command's overridden phrases, per locale. */
export interface PhraseOverride {
    en: string[];
    ka: string[];
}

/** commandId → overridden phrases. */
export type CommandOverrides = Record<string, PhraseOverride>;

const store = writable<CommandOverrides>({});

/** Read-only store handle — drives the Settings editor. */
export const commandOverrides = { subscribe: store.subscribe };

/** Synchronous snapshot — `buildVoiceCommandRegistry` (sync) reads this. */
let snapshot: CommandOverrides = {};

/** The overrides for the registry's synchronous build. */
export function getCommandPhraseOverrides(): CommandOverrides {
    return snapshot;
}

function commit(next: CommandOverrides) {
    snapshot = next;
    store.set(next);
}

/** Load overrides from the backend. Safe to call per-window (each window
 *  keeps its own snapshot, like userVoiceCommands). */
export async function loadCommandOverrides(): Promise<void> {
    try {
        const raw = await invoke<CommandOverrides>('voice_get_command_overrides');
        commit(raw && typeof raw === 'object' ? raw : {});
    } catch {
        commit({});
    }
}

async function persist(next: CommandOverrides): Promise<void> {
    commit(next);
    try {
        await invoke('voice_set_command_overrides', { overrides: next });
        // Tell other windows (overlays) to re-read so their snapshot stays in
        // sync — mirrors the user-command file's change event.
        void emit(CHANGED_EVENT).catch(() => {});
    } catch {
        // best effort — the in-memory snapshot is already updated
    }
}

/** Set the override for one command. Trims + drops blank phrases; if both
 *  locales end up empty the override is removed (= reset to default). */
export async function setCommandOverride(
    id: string,
    en: string[],
    ka: string[],
): Promise<void> {
    const cleanEn = en.map((p) => p.trim()).filter((p) => p.length > 0);
    const cleanKa = ka.map((p) => p.trim()).filter((p) => p.length > 0);
    const next = { ...snapshot };
    if (cleanEn.length === 0 && cleanKa.length === 0) {
        delete next[id];
    } else {
        next[id] = { en: cleanEn, ka: cleanKa };
    }
    await persist(next);
}

/** Reset one command to its built-in default (drop its override). */
export async function resetCommandOverride(id: string): Promise<void> {
    if (!(id in snapshot)) return;
    const next = { ...snapshot };
    delete next[id];
    await persist(next);
}

/** Reset every built-in command to its default. */
export async function resetAllCommandOverrides(): Promise<void> {
    await persist({});
}

/** Replace the whole override map in a single write — the editor's "Save"
 *  computes the full set (only commands that differ from default) and
 *  persists it at once, instead of one write per command. */
export async function replaceAllCommandOverrides(next: CommandOverrides): Promise<void> {
    await persist({ ...next });
}

let initialized = false;
let unlistenChanged: UnlistenFn | null = null;

/** Load overrides + subscribe to cross-window change events. Call once per
 *  command-matching window (from the root layout). Returns a cleanup. */
export function initCommandOverrides(): () => void {
    if (initialized) return () => {};
    initialized = true;
    void (async () => {
        await loadCommandOverrides();
        try {
            unlistenChanged = await listen(CHANGED_EVENT, () => {
                void loadCommandOverrides();
            });
        } catch {
            // no cross-window sync available — local edits still apply here
        }
    })();
    return () => {
        unlistenChanged?.();
        unlistenChanged = null;
        initialized = false;
    };
}
