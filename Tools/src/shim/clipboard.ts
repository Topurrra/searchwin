// Copying a secret out of a tool page — a generated password, a decrypted
// text, a token — without any clipboard history keeping it.
//
// navigator.clipboard.writeText sets none of the markers clipboard managers
// honour, so with Search's clipboard history on by default every password a
// tool page copied would be kept. In Search the browser copies it instead
// (`host:clipboard.secret`), marked the way password managers mark theirs:
// ExcludeClipboardContentFromMonitorProcessing, CanIncludeInClipboardHistory=0
// and CanUploadToCloudClipboard=0. Outside Search (`pnpm dev`) there is no
// history to keep it, and the page's own clipboard is used.

import { call, inSearch } from './bridge';

export async function writeSecret(text: string): Promise<void> {
    if (inSearch) {
        await call('host:clipboard.secret', { text });
        return;
    }
    await navigator.clipboard.writeText(text);
}
