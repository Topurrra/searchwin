// For copy buttons whose output is a secret (passwords, keys, tokens,
// decrypted text): no clipboard history keeps what they copy. See
// src/shim/clipboard.ts.
export { writeSecret as copySecret } from '../../shim/clipboard';
