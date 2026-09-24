/*
  privacyAck — finding acknowledgements for the Privacy Audit.

  An audit you re-run is only useful if it stops nagging about things you've
  already decided are fine ("yes, Zoom can use my mic — I know"). The user
  acknowledges a finding by its stable id; acknowledged findings drop out of
  the active list (into a collapsed "Acknowledged" section) and stop counting
  against the privacy score.

  Persisted through the backend (redb, DPAPI-encrypted) — the same store
  everything else in the app uses — so acknowledgements survive restarts. (The
  earlier localStorage version did not persist reliably.) Finding ids are stable
  (e.g. "microphone:Zoom.exe", "conn:chrome", "pii:C:\…\notes.txt"), so an
  acknowledgement re-applies to the same item across re-scans.
*/

import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

export const acknowledged = writable<string[]>([]);

let loaded = false;

/** Load acknowledgements from the backend once (call on Privacy Audit mount). */
export async function initPrivacyAcks() {
    if (loaded) return;
    loaded = true;
    try {
        const list = await invoke<string[]>('privacy_get_acknowledged');
        acknowledged.set(Array.isArray(list) ? list : []);
    } catch {
        // Backend not ready / unavailable — leave empty; acks just won't show.
    }
}

function persist(list: string[]) {
    invoke('privacy_set_acknowledged', { ids: list }).catch(() => {
        // best effort
    });
}

/** Acknowledge a finding (idempotent). */
export function acknowledge(id: string) {
    acknowledged.update((list) => {
        if (list.includes(id)) return list;
        const next = [...list, id];
        persist(next);
        return next;
    });
}

/** Restore a previously-acknowledged finding so it shows again. */
export function unacknowledge(id: string) {
    acknowledged.update((list) => {
        if (!list.includes(id)) return list;
        const next = list.filter((x) => x !== id);
        persist(next);
        return next;
    });
}
