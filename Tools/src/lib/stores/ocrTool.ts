import { writable } from 'svelte/store';

// Persists the OCR tool's user-produced state across navigation. The app wraps
// each tool in a {#key} block, so leaving + returning unmounts and re-mounts a
// fresh component instance; component-local $state is wiped. Lifting the picked
// image, recognized text, and the two options keeps them alive on return.
// The on-mount availability/language probe stays component-local (transient).
// See stores/duplicateFinder.ts for the established pattern.

export const ocrImagePath = writable<string | null>(null);
export const ocrResultText = writable('');
export const ocrLang = writable('eng');
export const ocrPreprocess = writable(true);
export const ocrUniformBlock = writable(false);
export const ocrRunning = writable(false);
export const ocrCancelRequested = writable(false);
export const ocrCurrentOperationId = writable<string | null>(null);
export const ocrError = writable<string | null>(null);
