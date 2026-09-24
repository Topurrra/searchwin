import { invoke } from '@tauri-apps/api/core';
import { derived, get, writable } from 'svelte/store';
import {
    pageScreens,
    screensForPacks,
    toolPacks,
    toolScreens,
    toolScreensForPacks,
    type ToolPackId,
} from '$lib/appScreens';
import { toast } from './toasts';

const STORAGE_KEY = 'keepitlocal_enabled_tool_packs_v1';
const PINNED_TOOLS_STORAGE_KEY = 'keepitlocal_pinned_tool_ids_v1';
const SCHEMA_VERSION = 2;
const DEFAULT_ENABLED_PACKS: ToolPackId[] = ['core', 'media'];
const PINNABLE_TOOL_IDS = new Set([
    ...toolScreens.map((screen) => screen.id),
    'clipboard-history',
    'snippets',
    'voice-to-text',
]);

export type EnabledToolPacksState = {
    schemaVersion: number;
    enabledPackIds: ToolPackId[];
};

let backendInitialized = false;
let restartRequired = false;
let lastSavedPackIds: ToolPackId[] = DEFAULT_ENABLED_PACKS;

function normalizePackIds(ids: string[]): ToolPackId[] {
    const allowed = new Set(toolPacks.map((pack) => pack.id));
    const normalized = ids.filter((id): id is ToolPackId => allowed.has(id as ToolPackId));
    const unique = new Set<ToolPackId>(normalized);
    unique.add('core');
    return [...unique].sort((a, b) => a.localeCompare(b));
}

function migratePackIds(ids: string[], schemaVersion?: number): ToolPackId[] {
    const normalized = normalizePackIds(ids);
    return (schemaVersion ?? 0) >= SCHEMA_VERSION ? normalized : normalizePackIds([...normalized, 'media']);
}

function loadLocal(): ToolPackId[] {
    if (typeof localStorage === 'undefined') return DEFAULT_ENABLED_PACKS;
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        if (!raw) return DEFAULT_ENABLED_PACKS;
        const parsed = JSON.parse(raw) as Partial<EnabledToolPacksState>;
        return migratePackIds(parsed.enabledPackIds ?? DEFAULT_ENABLED_PACKS, parsed.schemaVersion);
    } catch {
        return DEFAULT_ENABLED_PACKS;
    }
}

function saveLocal(ids: ToolPackId[]) {
    if (typeof localStorage === 'undefined') return;
    localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({
            schemaVersion: SCHEMA_VERSION,
            enabledPackIds: normalizePackIds(ids),
        }),
    );
}

function normalizePinnedToolIds(ids: unknown): string[] {
    if (!Array.isArray(ids)) return [];
    return [...new Set(ids.filter((id): id is string => typeof id === 'string' && PINNABLE_TOOL_IDS.has(id)))];
}

function loadPinnedTools(): string[] {
    if (typeof localStorage === 'undefined') return [];
    try {
        return normalizePinnedToolIds(JSON.parse(localStorage.getItem(PINNED_TOOLS_STORAGE_KEY) ?? '[]'));
    } catch {
        return [];
    }
}

export const enabledPackIds = writable<ToolPackId[]>(loadLocal());
export const pinnedToolIds = writable<string[]>(loadPinnedTools());
export const pendingEnabledPackIds = writable<ToolPackId[]>(get(enabledPackIds));
export const toolPackRestartRequired = writable(false);
export const installedTools = derived(enabledPackIds, ($ids) => toolScreensForPacks($ids));
export const installedScreens = derived(enabledPackIds, ($ids) => screensForPacks($ids));
export const installedPages = derived(installedScreens, ($screens) =>
    $screens.filter((screen) => pageScreens.some((page) => page.id === screen.id)),
);
lastSavedPackIds = get(enabledPackIds);

enabledPackIds.subscribe((ids) => {
    saveLocal(ids);
});

pinnedToolIds.subscribe((ids) => {
    if (typeof localStorage === 'undefined') return;
    localStorage.setItem(PINNED_TOOLS_STORAGE_KEY, JSON.stringify(normalizePinnedToolIds(ids)));
});

export async function initToolPacksStore() {
    if (backendInitialized) return;
    backendInitialized = true;

    try {
        const remote = await invoke<EnabledToolPacksState>('load_enabled_tool_packs');
        const normalized = migratePackIds(remote.enabledPackIds ?? DEFAULT_ENABLED_PACKS, remote.schemaVersion);
        enabledPackIds.set(normalized);
        pendingEnabledPackIds.set(normalized);
        lastSavedPackIds = normalized;
    } catch (error) {
        console.warn('Could not load enabled tool packs JSON, using local fallback:', error);
    }
}

/** Wave 7.9 (2026-05-28): force a fresh re-read of the enabled tool
 *  packs from the Rust backend. See the matching helper in
 *  `stores/onboarding.ts` for why — when welcome runs in its own
 *  Tauri window and toggles packs there, main's webview (a separate
 *  JS context) needs an explicit refresh trigger to pick up the
 *  change. Bypasses the `backendInitialized` guard so the re-fetch
 *  actually hits Rust. */
export async function reloadToolPacksStore(): Promise<void> {
    try {
        const remote = await invoke<EnabledToolPacksState>('load_enabled_tool_packs');
        const normalized = migratePackIds(remote.enabledPackIds ?? DEFAULT_ENABLED_PACKS, remote.schemaVersion);
        enabledPackIds.set(normalized);
        pendingEnabledPackIds.set(normalized);
        lastSavedPackIds = normalized;
    } catch (error) {
        console.warn('Could not reload enabled tool packs JSON:', error);
    }
}

export function setPendingToolPack(id: ToolPackId, enabled: boolean) {
    if (id === 'core') return;
    pendingEnabledPackIds.update((ids) => {
        const next = new Set(ids);
        if (enabled) {
            next.add(id);
        } else {
            next.delete(id);
        }
        next.add('core');
        return [...next].sort((a, b) => a.localeCompare(b));
    });
}

export async function savePendingToolPacks() {
    const ids = normalizePackIds(get(pendingEnabledPackIds));
    try {
        const saved = await invoke<EnabledToolPacksState>('save_enabled_tool_packs', {
            state: {
                schemaVersion: SCHEMA_VERSION,
                enabledPackIds: ids,
            },
        });
        const normalized = normalizePackIds(saved.enabledPackIds ?? ids);
        lastSavedPackIds = normalized;
        pendingEnabledPackIds.set(normalized);
        saveLocal(normalized);
        restartRequired = true;
        toolPackRestartRequired.set(true);
        toast('Tool packs saved. Restart KeepItLocal to apply changes.', 'success');
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        toast(message, 'error');
    }
}

export async function saveInitialToolPackSelection(ids: ToolPackId[]) {
    const normalizedIds = normalizePackIds(ids);
    const saved = await invoke<EnabledToolPacksState>('save_enabled_tool_packs', {
        state: {
            schemaVersion: SCHEMA_VERSION,
            enabledPackIds: normalizedIds,
        },
    });
    const normalized = normalizePackIds(saved.enabledPackIds ?? normalizedIds);
    lastSavedPackIds = normalized;
    restartRequired = false;
    enabledPackIds.set(normalized);
    pendingEnabledPackIds.set(normalized);
    toolPackRestartRequired.set(false);
    saveLocal(normalized);
    return normalized;
}

export function discardPendingToolPackChanges() {
    pendingEnabledPackIds.set(restartRequired ? lastSavedPackIds : get(enabledPackIds));
}

export function hasToolPackChange() {
    const active = normalizePackIds(get(enabledPackIds)).join('|');
    const pending = normalizePackIds(get(pendingEnabledPackIds)).join('|');
    return active !== pending || restartRequired;
}

export function isPackEnabled(id: ToolPackId) {
    return get(enabledPackIds).includes(id);
}

export function togglePinnedTool(id: string) {
    if (!PINNABLE_TOOL_IDS.has(id)) return;
    pinnedToolIds.update((ids) =>
        ids.includes(id) ? ids.filter((pinnedId) => pinnedId !== id) : [id, ...ids],
    );
}
