import { writable } from 'svelte/store';

// Persists the Format Converter's user-produced state across navigation.
// The app wraps each tool in a {#key} block, so leaving + returning unmounts
// and re-mounts a fresh component instance; component-local $state is wiped.
// Lifting these into a module store keeps the pasted document + its converted
// output alive. See stores/duplicateFinder.ts for the established pattern.

export type FormatConverterFormat = 'json' | 'yaml' | 'toml' | 'xml';

// The original component seeded `input` with a hardcoded sample on each mount.
// Keep that as the first-run default, but only apply it once (the store
// survives nav, so a user who clears the input won't have it spring back).
export const DEFAULT_FORMAT_CONVERTER_INPUT = `{
  "name": "keepitlocal",
  "version": "0.1.0",
  "private": true,
  "tools": ["hash", "encoders", "qr"]
}`;

export const formatConverterFrom = writable<FormatConverterFormat>('json');
export const formatConverterTo = writable<FormatConverterFormat>('yaml');
export const formatConverterInput = writable(DEFAULT_FORMAT_CONVERTER_INPUT);
export const formatConverterOutput = writable('');
export const formatConverterView = writable<'convert' | 'tree'>('convert');
