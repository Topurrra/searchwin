import { writable } from 'svelte/store';

export type CsvToolkitMode = 'convert' | 'merge' | 'clean' | 'split' | 'json';
export const csvToolkitMode = writable<CsvToolkitMode>('convert');
