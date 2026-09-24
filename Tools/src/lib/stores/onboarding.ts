import { invoke } from '@tauri-apps/api/core';
import { get, writable } from 'svelte/store';

const STORAGE_KEY = 'keepitlocal_onboarding_v1';
const SCHEMA_VERSION = 1;

export interface OnboardingState {
    schemaVersion: number;
    welcomeCompleted: boolean;
    completedAtMs: number | null;
    /** True once the first-run disk index has been auto-seeded. Persisted in
     *  the backend onboarding file so it survives normal restarts but resets on
     *  a data wipe — the safe "run the auto-index exactly once" signal. */
    firstDiskIndexSeeded: boolean;
}

const DEFAULT_STATE: OnboardingState = {
    schemaVersion: SCHEMA_VERSION,
    welcomeCompleted: false,
    completedAtMs: null,
    firstDiskIndexSeeded: false,
};

let backendInitialized = false;

function normalizeState(state: Partial<OnboardingState> | null | undefined): OnboardingState {
    return {
        schemaVersion: SCHEMA_VERSION,
        welcomeCompleted: Boolean(state?.welcomeCompleted),
        completedAtMs: typeof state?.completedAtMs === 'number' ? state.completedAtMs : null,
        firstDiskIndexSeeded: Boolean(state?.firstDiskIndexSeeded),
    };
}

function loadLocal(): OnboardingState {
    if (typeof localStorage === 'undefined') return DEFAULT_STATE;
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        return raw ? normalizeState(JSON.parse(raw)) : DEFAULT_STATE;
    } catch {
        return DEFAULT_STATE;
    }
}

function saveLocal(state: OnboardingState) {
    if (typeof localStorage === 'undefined') return;
    localStorage.setItem(STORAGE_KEY, JSON.stringify(normalizeState(state)));
}

export const onboardingState = writable<OnboardingState>(loadLocal());
export const onboardingLoaded = writable(false);

onboardingState.subscribe((state) => {
    saveLocal(state);
});

export async function initOnboardingStore() {
    if (backendInitialized) {
        onboardingLoaded.set(true);
        return;
    }
    backendInitialized = true;

    try {
        const remote = await invoke<OnboardingState>('load_onboarding_state');
        onboardingState.set(normalizeState(remote));
    } catch (error) {
        console.warn('Could not load onboarding JSON, using local fallback:', error);
    } finally {
        onboardingLoaded.set(true);
    }
}

/** Wave 7.9 (2026-05-28): force a fresh re-read of the onboarding
 *  state from the Rust backend, regardless of whether the store has
 *  already initialized. Used by the main window's `kit-state-refresh`
 *  listener after `welcome_finished` — at that point the welcome
 *  window persisted `welcomeCompleted = true` (and possibly other
 *  fields) but main's in-memory copy still reflects the pre-welcome
 *  state because each Tauri window has its own JS module instance.
 *  This bypasses the `backendInitialized` short-circuit so the
 *  re-fetch actually happens. */
export async function reloadOnboardingStore(): Promise<void> {
    try {
        const remote = await invoke<OnboardingState>('load_onboarding_state');
        onboardingState.set(normalizeState(remote));
    } catch (error) {
        console.warn('Could not reload onboarding JSON:', error);
    }
}

export async function completeWelcome() {
    const next = normalizeState({
        ...get(onboardingState),
        welcomeCompleted: true,
        completedAtMs: Date.now(),
    });

    try {
        const saved = await invoke<OnboardingState>('save_onboarding_state', { state: next });
        onboardingState.set(normalizeState(saved));
    } catch (error) {
        onboardingState.set(next);
        console.warn('Could not persist onboarding JSON:', error);
    }
}

/** Has the first-run disk index already been auto-seeded? Reads the backend
 *  onboarding file FRESH (not the store) so it's correct even very early in
 *  startup before `initOnboardingStore` has finished loading. */
export async function isFirstDiskIndexSeeded(): Promise<boolean> {
    try {
        const remote = normalizeState(await invoke<OnboardingState>('load_onboarding_state'));
        return remote.firstDiskIndexSeeded;
    } catch {
        return get(onboardingState).firstDiskIndexSeeded;
    }
}

/** Record that the first-run disk index has been seeded. Merges onto the FRESH
 *  backend state so it never clobbers `welcomeCompleted` if the store hasn't
 *  loaded yet. */
export async function markFirstDiskIndexSeeded(): Promise<void> {
    let base: OnboardingState;
    try {
        base = normalizeState(await invoke<OnboardingState>('load_onboarding_state'));
    } catch {
        base = get(onboardingState);
    }
    const next = normalizeState({ ...base, firstDiskIndexSeeded: true });
    try {
        const saved = await invoke<OnboardingState>('save_onboarding_state', { state: next });
        onboardingState.set(normalizeState(saved));
    } catch (error) {
        onboardingState.set(next);
        console.warn('Could not persist onboarding JSON:', error);
    }
}
