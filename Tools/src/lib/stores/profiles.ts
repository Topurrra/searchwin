import { invoke } from '@tauri-apps/api/core';
import { writable, derived, get } from 'svelte/store';
import { builtInProfileDefinitions, toolIdsByCategory, toolIdsByList, toolPackIdForScreen, tools as allTools, type Tool } from '$lib/appScreens';
import { installedTools } from './toolPacks';

const STORAGE_KEY = 'keepitlocal_profiles_v1';
const ACTIVE_KEY = 'keepitlocal_active_profile_v1';
const SCHEMA_VERSION = 1;

export interface Profile {
    id: string;
    name: string;
    description?: string;
    toolIds: string[];
    builtIn: boolean;
    schemaVersion: number;
}

export interface ProfilesState {
    profiles: Profile[];
    activeId: string;
}

let backendReady = false;
let backendInitialized = false;
let saveTimeout: ReturnType<typeof setTimeout> | null = null;

function buildBuiltIns(): Profile[] {
    return builtInProfileDefinitions.map((profile) => ({
        id: profile.id,
        name: profile.name,
        description: profile.description,
        toolIds: profile.includeAll
            ? []
            : profile.includeToolIds
                ? toolIdsByList(profile.includeToolIds)
                : toolIdsByCategory(profile.includeCategories ?? []),
        builtIn: true,
        schemaVersion: SCHEMA_VERSION,
    }));
}

function isValidProfile(p: unknown): p is Profile {
    if (!p || typeof p !== 'object') return false;
    const obj = p as Record<string, unknown>;
    return (
        typeof obj.id === 'string' &&
        obj.id.length > 0 &&
        typeof obj.name === 'string' &&
        obj.name.length > 0 &&
        Array.isArray(obj.toolIds) &&
        obj.toolIds.every((t) => typeof t === 'string') &&
        typeof obj.builtIn === 'boolean'
    );
}

function migrateProfile(p: Profile): Profile {
    return { ...p, schemaVersion: p.schemaVersion ?? SCHEMA_VERSION };
}

function mergeState(userProfiles: Profile[], activeId: string): ProfilesState {
    const builtIns = buildBuiltIns();
    const userOnly = userProfiles.filter((profile) => !profile.builtIn).map(migrateProfile);
    const profiles = [...builtIns, ...userOnly];
    const safeActiveId = profiles.some((profile) => profile.id === activeId) ? activeId : 'all';
    return { profiles, activeId: safeActiveId };
}

function loadLocal(): ProfilesState {
    const fallback = mergeState([], 'all');

    if (typeof localStorage === 'undefined') return fallback;

    try {
        const rawProfiles = localStorage.getItem(STORAGE_KEY);
        const rawActive = localStorage.getItem(ACTIVE_KEY);

        const parsed = rawProfiles ? JSON.parse(rawProfiles) : [];
        const userProfiles = Array.isArray(parsed) ? parsed.filter(isValidProfile) : [];
        const activeId = typeof rawActive === 'string' && rawActive.length > 0 ? rawActive : 'all';
        return mergeState(userProfiles, activeId);
    } catch (error) {
        console.warn('Failed to load profiles, using defaults:', error);
        return fallback;
    }
}

function persistLocal(state: ProfilesState): void {
    if (typeof localStorage === 'undefined') return;
    try {
        const userOnly = state.profiles.filter((profile) => !profile.builtIn);
        localStorage.setItem(STORAGE_KEY, JSON.stringify(userOnly));
        localStorage.setItem(ACTIVE_KEY, state.activeId);
    } catch (error) {
        console.warn('Failed to persist profiles:', error);
    }
}

const initial = loadLocal();
export const profiles = writable<Profile[]>(initial.profiles);
export const activeProfileId = writable<string>(initial.activeId);

function scheduleSave() {
    const state = {
        profiles: get(profiles),
        activeId: get(activeProfileId),
    };

    persistLocal(state);

    if (!backendReady) return;
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(async () => {
        saveTimeout = null;
        try {
            const userOnly = get(profiles).filter((profile) => !profile.builtIn);
            await invoke('save_profiles_state', {
                state: {
                    profiles: userOnly,
                    activeId: get(activeProfileId),
                },
            });
        } catch (error) {
            console.warn('Could not persist profiles to JSON:', error);
        }
    }, 150);
}

profiles.subscribe(scheduleSave);
activeProfileId.subscribe(scheduleSave);

export async function initProfilesStore() {
    if (backendInitialized) return;
    backendInitialized = true;

    try {
        const remote = await invoke<ProfilesState>('load_profiles_state');
        const merged = mergeState(remote.profiles ?? [], remote.activeId ?? 'all');
        profiles.set(merged.profiles);
        activeProfileId.set(merged.activeId);
    } catch (error) {
        console.warn('Could not load profiles JSON, using local fallback:', error);
    } finally {
        backendReady = true;
    }
}

export const activeProfile = derived(
    [profiles, activeProfileId],
    ([$profiles, $activeId]) => $profiles.find((profile) => profile.id === $activeId) ?? $profiles[0]
);

export function filterToolsForProfile(profile: Profile | null | undefined, candidates: Tool[]): Tool[] {
    if (!profile || profile.toolIds.length === 0) return candidates;
    const selectedIds = new Set(profile.toolIds);
    return candidates.filter(
        (tool) => toolPackIdForScreen(tool) === 'core' || selectedIds.has(tool.id),
    );
}

export const visibleTools = derived(
    [activeProfile, installedTools],
    ([$active, $installedTools]): Tool[] => filterToolsForProfile($active, $installedTools),
);

export function setActiveProfile(id: string): void {
    const exists = get(profiles).some((profile) => profile.id === id);
    if (exists) activeProfileId.set(id);
}

export function createProfile(name: string, toolIds: string[] = [], description?: string): Profile {
    const trimmedName = name.trim();
    if (!trimmedName) throw new Error('Profile name cannot be empty');

    const profile: Profile = {
        id: `user_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
        name: trimmedName.slice(0, 50),
        description: description?.slice(0, 200),
        toolIds: [...new Set(toolIds)],
        builtIn: false,
        schemaVersion: SCHEMA_VERSION,
    };

    profiles.update((items) => [...items, profile]);
    return profile;
}

export function updateProfile(id: string, patch: Partial<Pick<Profile, 'name' | 'description' | 'toolIds'>>): void {
    profiles.update((items) =>
        items.map((profile) => {
            if (profile.id !== id) return profile;
            if (profile.builtIn) {
                console.warn(`Cannot modify built-in profile: ${id}`);
                return profile;
            }
            return {
                ...profile,
                ...(patch.name !== undefined && { name: patch.name.trim().slice(0, 50) }),
                ...(patch.description !== undefined && { description: patch.description?.slice(0, 200) }),
                ...(patch.toolIds !== undefined && { toolIds: [...new Set(patch.toolIds)] }),
            };
        })
    );
}

export function duplicateProfile(id: string, newName?: string): Profile | null {
    const source = get(profiles).find((profile) => profile.id === id);
    if (!source) return null;

    return createProfile(
        newName ?? `${source.name} (copy)`,
        source.toolIds.length === 0 ? allTools.map((tool) => tool.id) : source.toolIds,
        source.description
    );
}

export function deleteProfile(id: string): boolean {
    const target = get(profiles).find((profile) => profile.id === id);
    if (!target || target.builtIn) return false;

    profiles.update((items) => items.filter((profile) => profile.id !== id));
    if (get(activeProfileId) === id) activeProfileId.set('all');
    return true;
}

export function isToolVisibleInProfile(profile: Profile, toolId: string): boolean {
    if (profile.toolIds.length === 0) return true;
    return profile.toolIds.includes(toolId);
}
