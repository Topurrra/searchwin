/*
  Snippets store. Thin wrapper around the backend `*_snippet` commands
  that keeps an in-memory list synced for both the management screen
  AND the clipboard overlay (which needs to look up triggers as the
  user types).

  All persistence goes through redb on the backend — the frontend just
  caches the latest list. Mutations call the backend, then refresh.
*/

import { invoke } from '@tauri-apps/api/core';
import { writable, get } from 'svelte/store';
import { settings } from './settings';

export type Snippet = {
    id: number;
    trigger: string;
    label: string;
    template: string;
    createdAtMs: number;
    updatedAtMs: number;
    useCount: number;
};

export type SnippetInput = {
    trigger: string;
    label: string;
    template: string;
};

export type SnippetUpdate = SnippetInput & { id: number };

export const snippets = writable<Snippet[]>([]);
export const snippetsLoading = writable<boolean>(false);

/** True once we've done the first refresh — used by the overlay to
 * skip its expansion lookup until snippets are actually loaded. */
export const snippetsReady = writable<boolean>(false);

// ─── Snippet variables (user-defined {{vars}}) ─────────────────────────
// Moved out of the plaintext `settings` localStorage blob into the DPAPI-
// encrypted backend (via secure_kv), since values can be personal. Same
// async-load + one-time-migration pattern as the myCommands store.
const SNIPPET_VARS_KEY = 'snippet_variables_v1';
const LEGACY_SETTINGS_KEY = 'keepitlocal_settings_v1';

export const snippetVariables = writable<Record<string, string>>({});
let snippetVarsApplying = true;

snippetVariables.subscribe((vars) => {
    if (snippetVarsApplying) return;
    void invoke('secure_kv_set', {
        key: SNIPPET_VARS_KEY,
        value: JSON.stringify(vars),
    }).catch(() => {});
});

function parseVars(raw: string | null | undefined): Record<string, string> {
    if (!raw) return {};
    try {
        const parsed = JSON.parse(raw);
        if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
        const out: Record<string, string> = {};
        for (const [k, v] of Object.entries(parsed)) {
            if (typeof v === 'string') out[k] = v;
        }
        return out;
    } catch {
        return {};
    }
}

/** One-time migration: pull snippet variables out of the legacy plaintext
 *  settings blob and DELETE them from it, so they no longer sit in plaintext. */
function migrateSnippetVarsFromSettings(): Record<string, string> {
    if (typeof localStorage === 'undefined') return {};
    const raw = localStorage.getItem(LEGACY_SETTINGS_KEY);
    if (!raw) return {};
    try {
        const obj = JSON.parse(raw);
        const result: Record<string, string> = {};
        const vars = obj?.snippetVariables;
        if (vars && typeof vars === 'object' && !Array.isArray(vars)) {
            for (const [k, v] of Object.entries(vars)) {
                if (typeof v === 'string') result[k] = v;
            }
        }
        if (obj && typeof obj === 'object' && 'snippetVariables' in obj) {
            delete obj.snippetVariables;
            localStorage.setItem(LEGACY_SETTINGS_KEY, JSON.stringify(obj));
        }
        return result;
    } catch {
        return {};
    }
}

/** Load snippet variables from the encrypted backend (migrating the legacy
 *  plaintext settings copy on first run). Runs on import. */
export async function initSnippetVariables(): Promise<void> {
    let vars: Record<string, string> = {};
    try {
        vars = parseVars(await invoke<string | null>('secure_kv_get', { key: SNIPPET_VARS_KEY }));
    } catch {
        /* fall through to migration */
    }
    if (Object.keys(vars).length === 0) {
        const migrated = migrateSnippetVarsFromSettings();
        if (Object.keys(migrated).length > 0) {
            vars = migrated;
            try {
                await invoke('secure_kv_set', {
                    key: SNIPPET_VARS_KEY,
                    value: JSON.stringify(vars),
                });
            } catch {
                /* retry next launch */
            }
        }
    }
    snippetVarsApplying = true;
    snippetVariables.set(vars);
    snippetVarsApplying = false;
}

void initSnippetVariables();

/** Fetch all snippets from the backend. Replaces the in-memory cache
 * on success; on failure leaves the cache as-is and surfaces the
 * error to the caller. */
export async function refreshSnippets(): Promise<void> {
    snippetsLoading.set(true);
    try {
        const fresh = await invoke<Snippet[]>('list_snippets');
        snippets.set(fresh ?? []);
        snippetsReady.set(true);
        // Keep the auto-expand watcher's snapshot fresh. Best-effort — a
        // backend hiccup here must never break the management UI.
        void syncSnippetAutoExpand().catch(() => {});
    } finally {
        snippetsLoading.set(false);
    }
}

export async function createSnippet(input: SnippetInput): Promise<Snippet> {
    const created = await invoke<Snippet>('create_snippet', { input });
    await refreshSnippets();
    return created;
}

export async function updateSnippet(update: SnippetUpdate): Promise<Snippet> {
    const updated = await invoke<Snippet>('update_snippet', { update });
    await refreshSnippets();
    return updated;
}

export async function deleteSnippet(id: number): Promise<void> {
    await invoke('delete_snippet', { id });
    await refreshSnippets();
}

/** Bump `use_count` on an expansion event. Best-effort — failures
 * don't bubble (the expansion already succeeded, we just lose
 * analytics for that one use). */
export async function recordSnippetUse(id: number): Promise<void> {
    try {
        await invoke('record_snippet_use', { id });
        await refreshSnippets();
    } catch (error) {
        console.warn('record_snippet_use failed:', error);
    }
}

/** Get the expanded body of a template, with all `{{variables}}`
 * resolved server-side using the current date/time + clipboard. We
 * delegate to the backend so the expansion logic stays in one place
 * (matches what the overlay's paste path would use). */
export async function previewSnippet(
    template: string,
    clipboardText?: string | null,
): Promise<string> {
    const result = await invoke<string>('preview_snippet_expansion', {
        template,
        clipboardText: clipboardText ?? null,
    });
    // {{name}} resolves client-side: the user's name lives in settings, not the
    // backend. The server leaves unknown variables verbatim, so we substitute
    // here — covering BOTH the management preview and the overlay paste path
    // (both call previewSnippet). Matches optional inner whitespace, mirroring
    // the backend's trim of variable names.
    const appSettings = get(settings);
    const name = (appSettings.userName ?? '').trim();
    let expanded = result.replace(/\{\{\s*name\s*\}\}/g, name);
    // User-defined variables ({{surname}} → "Smith"). The backend leaves
    // unknown {{...}} verbatim, so we resolve them here after the built-ins.
    // Keys are regex-escaped (user-provided) and match optional inner
    // whitespace, mirroring the {{name}} substitution above.
    const vars = get(snippetVariables);
    for (const [key, value] of Object.entries(vars)) {
        const trimmedKey = key.trim();
        if (!trimmedKey) continue;
        const escaped = trimmedKey.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
        expanded = expanded.replace(new RegExp(`\\{\\{\\s*${escaped}\\s*\\}\\}`, 'g'), value);
    }
    return expanded;
}

/** Synchronous snippet lookup against the current snapshot. Used by the
 * clipboard overlay's search matching — `O(n)` is fine since the snippet
 * list is tiny (10s, not 1000s).
 *
 * Matches the trigger AND the label (case-insensitive), so a snippet is
 * reachable by its human name — "email" finds the snippet labelled "Email
 * signature" even if its trigger is `sig`. Results are ranked by match
 * strength: exact trigger, then trigger prefix, then label prefix, then a
 * trigger/label substring. Ties keep the backend's most-used-first order
 * (Array.sort is stable). A leading `/` is ignored; an empty query returns
 * the full list. */
export function findSnippets(query: string): Snippet[] {
    const q = query.trim().toLowerCase().replace(/^\//, '');
    const list = get(snippets);
    if (!q) return list;
    const ranked: { snippet: Snippet; score: number }[] = [];
    for (const s of list) {
        const trigger = s.trigger.toLowerCase();
        const label = s.label.toLowerCase();
        let score: number;
        if (trigger === q) score = 0;
        else if (trigger.startsWith(q)) score = 1;
        else if (label.startsWith(q)) score = 2;
        else if (trigger.includes(q)) score = 3;
        else if (label.includes(q)) score = 4;
        else continue;
        ranked.push({ snippet: s, score });
    }
    ranked.sort((a, b) => a.score - b.score);
    return ranked.map((r) => r.snippet);
}

/** Push the current snippet set + the user's name/variables + exclusions to
 *  the backend auto-expand watcher. Safe to call whenever any input changes;
 *  cheap. No effect until the feature is enabled (the backend keeps the data
 *  but only the active hook reads it). Best-effort — callers wrap in try/catch
 *  so a backend hiccup never breaks the management UI. */
export async function syncSnippetAutoExpand(): Promise<void> {
    const appSettings = get(settings);
    await invoke('sync_snippet_expand_data', {
        data: {
            snippets: get(snippets),
            name: (appSettings.userName ?? '').trim() || null,
            variables: get(snippetVariables),
            excludedApps: appSettings.snippetAutoExpandExcludedApps ?? [],
        },
    });
}
