import { writable } from 'svelte/store';

// Persists the Encoders tool's user-produced state across navigation.
// The app wraps each tool in a {#key} block, so leaving + returning unmounts
// and re-mounts a fresh component instance; component-local $state is wiped.
// Lifting these into a module store keeps the pasted blob + its result alive.
// See stores/duplicateFinder.ts for the established pattern.

export const encodersAlgorithm = writable<string>('base64');
export const encodersMode = writable<'encode' | 'decode'>('encode');
export const encodersInput = writable('');
export const encodersOutput = writable('');
