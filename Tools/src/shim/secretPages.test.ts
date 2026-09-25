import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

// The tool pages whose copy buttons put a secret on the clipboard. They must
// copy through the browser (copySecret → host:clipboard.secret), which marks
// the copy so no clipboard history keeps it; navigator.clipboard marks nothing.
const secretPages = [
    'lib/tools/Utils/PasswordGenerator.svelte',
    'lib/tools/Development/EncryptDecrypt.svelte',
    'lib/tools/Development/devkit/JwtPanel.svelte',
    'lib/tools/Development/devkit/SecretScanPanel.svelte',
    'lib/components/PrivacyBlur.svelte',
];

const source = (path: string) => readFileSync(fileURLToPath(new URL(`../${path}`, import.meta.url)), 'utf8');

describe('pages that copy secrets', () => {
    for (const page of secretPages) {
        it(`${page} copies quietly`, () => {
            const text = source(page);
            expect(text).toContain('copySecret(');
            expect(text).not.toMatch(/navigator\.clipboard\.writeText/);
        });
    }
});
