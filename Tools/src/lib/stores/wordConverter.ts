import { writable } from 'svelte/store';

export type WordConverterTarget = 'markdown' | 'text';
export const wordConverterTarget = writable<WordConverterTarget>('markdown');
