import { writable } from 'svelte/store';

export type QrErrorCorrection = 'L' | 'M' | 'Q' | 'H';

// Session-only workspace state. QR content is never written to disk.
export const qrText = writable('https://example.com');
export const qrErrorCorrection = writable<QrErrorCorrection>('M');
export const qrScale = writable(8);
export const qrMargin = writable(2);
export const qrPngBase64 = writable('');
export const qrSvg = writable('');
export const qrSize = writable(0);
export const qrError = writable<string | null>(null);
export const qrGenerating = writable(false);
export const qrGenerationId = writable(0);
// Tracks the inputs that own the current session job, without storing them on disk.
export const qrRequestedSignature = writable<string | null>(null);
export const qrAppliedGenerationId = writable(0);
