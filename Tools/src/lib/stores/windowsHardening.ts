/*
  windowsHardening — state for the Privacy Hardening tool.

  Like Privacy Audit, this tool is mounted inside the workspace's `{#key}`
  block, so navigating away unmounts the component and wipes component-local
  `$state`. These module-level stores keep the loaded catalog + live state alive
  for the session, so leaving and returning doesn't re-read the registry or lose
  the user's place.

  The toggle position reflects the LIVE registry (read on first load), so a
  setting the user already changed in Windows shows as on. Apply/revert update
  the store optimistically on success — the backend write already happened, so
  the new state IS the requested one.
*/

import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

export interface HardeningTweak {
    id: string;
    title: string;
    detail: string;
    category: string;
    risk: 'low' | 'medium' | 'high';
    /** "apply" = a no-admin toggle this tool flips; "guide" = admin-only, shown
     *  with steps + a copyable command this app never runs. */
    tier: 'apply' | 'guide';
    applied: boolean;
    /** Guide tier only: the copyable elevated command. */
    command: string | null;
    /** Guide tier only: step-by-step instructions. */
    steps: string[];
}

export const tweaks = writable<HardeningTweak[]>([]);
export const loaded = writable(false);
export const loading = writable(false);
export const errorMsg = writable<string | null>(null);
/** Tweak ids with an apply/revert in flight — drives the per-row busy state. */
export const busy = writable<Record<string, boolean>>({});

/** Read every tweak's live state from the registry. Forces a re-read (used by
 *  the Refresh button); guarded only against a concurrent in-flight load. */
export async function loadHardeningState(): Promise<void> {
    if (get(loading)) return;
    loading.set(true);
    errorMsg.set(null);
    try {
        const list = await invoke<HardeningTweak[]>('read_hardening_state');
        tweaks.set(list);
        loaded.set(true);
    } catch (e) {
        errorMsg.set(String(e));
    } finally {
        loading.set(false);
    }
}

/** Apply (true) or revert (false) one tweak. On success the store reflects the
 *  new state; on failure the store is untouched, so the bound toggle snaps back
 *  to the real (unchanged) state. Throws so the caller can surface a toast. */
export async function setTweakApplied(id: string, applied: boolean): Promise<void> {
    busy.update((b) => ({ ...b, [id]: true }));
    try {
        await invoke(applied ? 'apply_hardening_tweak' : 'revert_hardening_tweak', { id });
        tweaks.update((list) => list.map((t) => (t.id === id ? { ...t, applied } : t)));
    } catch (e) {
        // Per-toggle failures surface via the caller's toast; the page-level
        // errorMsg banner is reserved for load failures. Rethrow so the bound
        // toggle stays at its true (unchanged) state.
        throw e;
    } finally {
        busy.update((b) => ({ ...b, [id]: false }));
    }
}

/** Revert every applied tweak, then re-read live state to reflect it. */
export async function revertAll(): Promise<void> {
    errorMsg.set(null);
    try {
        await invoke('revert_all_hardening');
    } finally {
        // Always re-read live state so the toggles reflect reality, success or
        // not. The invoke error propagates so the caller can toast it.
        await loadHardeningState();
    }
}

/** Write a JSON snapshot of the affected registry values to `path`. */
export async function exportBackup(path: string): Promise<void> {
    await invoke('export_hardening_backup', { path });
}
