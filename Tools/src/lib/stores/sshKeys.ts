// SSH Key Manager — typed bindings over the Rust commands. Private key material
// never crosses this boundary; only metadata, fingerprints, and public keys do.

import { invoke } from '@tauri-apps/api/core';
import { writable } from 'svelte/store';

export interface SshKeyInfo {
    name: string;
    path: string;
    publicPath: string | null;
    algorithm: string;
    bits: number | null;
    comment: string;
    fingerprint: string;
    hasPrivate: boolean;
    encrypted: boolean;
    publicKey: string;
}

export interface SshHost {
    host: string;
    hostName: string;
    user: string;
    port: string;
    identityFile: string;
    extra: string[];
}

export interface SshConfig {
    preamble: string;
    hosts: SshHost[];
}

export interface KnownHost {
    index: number;
    hosts: string;
    keyType: string;
    fingerprint: string;
}

export type SshKeyOperation =
    | 'generate'
    | 'import'
    | 'delete'
    | 'save-config'
    | 'remove-known-host'
    | null;

// Key metadata only. Private key material and form drafts stay component-local.
export const sshKeyList = writable<SshKeyInfo[]>([]);
export const sshKeyOperation = writable<SshKeyOperation>(null);

export const listKeys = () => invoke<SshKeyInfo[]>('ssh_list_keys');

export const generateKey = (args: {
    name: string;
    algorithm: string;
    bits?: number;
    comment: string;
    passphrase: string;
}) => invoke<SshKeyInfo>('ssh_generate_key', args);

export const importKey = (sourcePath: string) => invoke<SshKeyInfo>('ssh_import_key', { sourcePath });
export const deleteKey = (name: string) => invoke<void>('ssh_delete_key', { name });

export const readConfig = () => invoke<SshConfig>('ssh_read_config');
export const writeConfig = (config: SshConfig) => invoke<void>('ssh_write_config', { config });

export const readKnownHosts = () => invoke<KnownHost[]>('ssh_read_known_hosts');
export const removeKnownHost = (index: number) => invoke<void>('ssh_remove_known_host', { index });

export const sshDirPath = () => invoke<string>('ssh_dir_path');

export function emptyHost(): SshHost {
    return { host: '', hostName: '', user: '', port: '', identityFile: '', extra: [] };
}
