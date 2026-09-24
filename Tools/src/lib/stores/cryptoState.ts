/**
 * Background-task continuity (Wave 3.2, 2026-05-26) — Encrypt / Decrypt.
 *
 * The Tauri command runs on a `spawn_blocking` thread that survives
 * frontend navigation; what was missing is the **UI** side — the
 * `EncryptDecrypt.svelte` component owned its state in component-local
 * `$state`, which unmounts when the user clicks away to another tool.
 * Coming back showed an empty page even though a multi-file batch was
 * still encrypting in the background.
 *
 * This module lifts the operation-runtime state to module-scope
 * `writable()` stores so the component just re-subscribes on remount
 * and the in-flight batch + results + Cancel button are immediately
 * visible again.
 *
 * **What's NOT lifted** (deliberate):
 * - `password`, `password2`, `keyfilePath`, `showPassword`,
 *   `useKeyfile` — security. We don't want a password lingering in
 *   memory after the user navigates away. Keep them as component-local
 *   `$state` so they reset on each mount.
 * - `textInput`, `textOutput` — the text tab is ephemeral; users
 *   typically encrypt-and-copy in one go, then leave.
 * - `message` (the flash toast) — short-lived (5 s self-clear); no
 *   point persisting across nav.
 */

import { writable, get } from 'svelte/store';
import { decryptFile, encryptFile } from './crypto';

export interface CryptoFileResult {
    input: string;
    output?: string;
    error?: string;
}

/** Currently-selected tab: file batch vs text payload. */
export const cryptoTab = writable<'files' | 'text'>('files');
/** Encrypt vs decrypt direction. Reset to "encrypt" each session is fine
 *  for safety (decrypting needs a deliberate user choice anyway). */
export const cryptoMode = writable<'encrypt' | 'decrypt'>('encrypt');
/** True while a file batch is in flight. The component reads this for
 *  the Cancel button visibility + the queue-busy gate. */
export const cryptoBusy = writable(false);
/** The file queue, in user-picked order. Survives navigation so a long
 *  batch's list isn't lost. */
export const cryptoFiles = writable<string[]>([]);
/** Per-file results (output path on success, error string on failure /
 *  cancellation). Built up incrementally during the batch. */
export const cryptoResults = writable<CryptoFileResult[]>([]);
/** Operation ID of the running batch — needed by the Cancel button to
 *  call `cancel_crypto_operation`. Null when nothing is running. */
export const cryptoCurrentOpId = writable<string | null>(null);
/** Set as soon as the user asks to cancel so no later queue item starts. */
export const cryptoCancelRequested = writable(false);

export interface CryptoRunSummary {
    cancelled: boolean;
    ok: number;
    fail: number;
}

/**
 * Module-owned file runner. Secrets are captured only by this async call;
 * runtime state stays in stores so navigation cannot strand a batch as busy.
 */
export async function runCryptoFiles({
    mode,
    files,
    password,
    keyfilePath,
}: {
    mode: 'encrypt' | 'decrypt';
    files: string[];
    password: string;
    keyfilePath?: string;
}): Promise<CryptoRunSummary | null> {
    if (get(cryptoBusy) || files.length === 0) return null;

    const inputs = [...files];
    const operationId = `crypto-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const results: CryptoFileResult[] = [];
    let cancelled = false;

    cryptoBusy.set(true);
    cryptoResults.set([]);
    cryptoCurrentOpId.set(operationId);
    cryptoCancelRequested.set(false);

    try {
        for (const input of inputs) {
            if (get(cryptoCancelRequested)) {
                cancelled = true;
                results.push({ input, error: 'Cancelled' });
                cryptoResults.set([...results]);
                continue;
            }

            try {
                const result = mode === 'encrypt'
                    ? await encryptFile({ inputPath: input, password, keyfilePath, operationId })
                    : await decryptFile({ inputPath: input, password, keyfilePath, operationId });
                results.push({ input, output: result.outputPath });
            } catch (error) {
                const message = String(error);
                results.push({ input, error: message });
                if (message.includes('Cancelled')) cancelled = true;
            }

            if (get(cryptoCancelRequested)) cancelled = true;
            cryptoResults.set([...results]);
        }

        const ok = results.filter((result) => result.output).length;
        return { cancelled, ok, fail: results.length - ok };
    } finally {
        cryptoCurrentOpId.set(null);
        cryptoBusy.set(false);
        cryptoCancelRequested.set(false);
    }
}
