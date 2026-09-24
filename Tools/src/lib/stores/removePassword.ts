import { writable } from 'svelte/store';

// Persistent user-produced state for the "Remove Word edit restrictions"
// tool (RemovePassword.svelte). The workspace remounts the tool fresh on
// navigation, wiping any component-local `$state`, so the picked files,
// their per-file unlock results, the chosen output directory, and the
// in-flight flag are lifted into module-level stores that survive leaving
// and returning to the tool.

export type RemovePasswordFileKind = 'docx' | 'unsupported';

export type RemovePasswordResult = {
    success: boolean;
    outputPath?: string;
    error?: string;
    protectionRemoved?: boolean;
};

export type RemovePasswordFile = {
    path: string;
    name: string;
    kind: RemovePasswordFileKind;
    result?: RemovePasswordResult;
};

export const removePasswordFiles = writable<RemovePasswordFile[]>([]);
export const removePasswordOutputDir = writable<string | null>(null);
export const removePasswordWorking = writable(false);
