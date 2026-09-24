// Encrypt/Decrypt — typed bindings over the Rust AES-256-GCM + Argon2id commands.

import { invoke } from '@tauri-apps/api/core';

export interface CryptoResult {
    outputPath: string;
}

export interface CryptoInspect {
    valid: boolean;
    keyfileRequired: boolean;
}

export const encryptFile = (args: {
    inputPath: string;
    outputPath?: string;
    password: string;
    keyfilePath?: string;
    operationId?: string;
}) => invoke<CryptoResult>('crypto_encrypt_file', args);

export const decryptFile = (args: {
    inputPath: string;
    outputPath?: string;
    password: string;
    keyfilePath?: string;
    operationId?: string;
}) => invoke<CryptoResult>('crypto_decrypt_file', args);

export const encryptText = (args: { text: string; password: string; keyfilePath?: string }) =>
    invoke<string>('crypto_encrypt_text', args);

export const decryptText = (args: { payload: string; password: string; keyfilePath?: string }) =>
    invoke<string>('crypto_decrypt_text', args);

export const inspectFile = (inputPath: string) =>
    invoke<CryptoInspect>('crypto_inspect_file', { inputPath });

/** Signal a running crypto file op to abort. The backend checks the
 *  flag at every 256 KiB chunk boundary in encrypt/decrypt streams; on
 *  cancel it cleans up the half-written ciphertext/plaintext file and
 *  returns Err("Cancelled"). Wave 2.5. */
export const cancelCryptoOperation = (operationId: string) =>
    invoke<void>('cancel_crypto_operation', { operationId });

/** Cheap local password-strength estimate → 0..4 with a label + color. */
export function passwordStrength(pw: string): { score: number; label: string; color: string } {
    if (!pw) return { score: 0, label: '', color: '#475569' };
    let score = 0;
    if (pw.length >= 8) score++;
    if (pw.length >= 12) score++;
    if (/[a-z]/.test(pw) && /[A-Z]/.test(pw)) score++;
    if (/\d/.test(pw)) score++;
    if (/[^A-Za-z0-9]/.test(pw)) score++;
    score = Math.min(4, score);
    const labels = ['Very weak', 'Weak', 'Fair', 'Good', 'Strong'];
    const colors = ['#ef4444', '#f59e0b', '#eab308', '#22c55e', '#10b981'];
    return { score, label: labels[score], color: colors[score] };
}
