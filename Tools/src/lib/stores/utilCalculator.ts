import { writable } from 'svelte/store';

// Persists the Calculator notepad across navigation. The app wraps each tool
// in a {#key} block, so leaving + returning unmounts and re-mounts a fresh
// component instance; component-local $state is wiped. Lifting `doc` (the whole
// typed notepad) into a module store keeps it alive — the per-line results and
// running total are $derived and recompute from the persisted doc on mount.
// See stores/duplicateFinder.ts for the established pattern.

export const utilCalculatorDoc = writable('');
