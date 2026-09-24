// The Tauri plugins the pages use: dialogs, files, opening things,
// notifications and autostart, answered by the browser.

import { call } from './bridge';

// ─── dialog ─────────────────────────────────────────────────────────────

export interface DialogFilter {
    name: string;
    extensions: string[];
}

export interface OpenDialogOptions {
    title?: string;
    defaultPath?: string;
    filters?: DialogFilter[];
    multiple?: boolean;
    directory?: boolean;
}

export async function open(options: OpenDialogOptions = {}): Promise<string | string[] | null> {
    return call('host:dialog.open', options as Record<string, unknown>);
}

export async function save(options: Omit<OpenDialogOptions, 'multiple' | 'directory'> = {}): Promise<string | null> {
    return call('host:dialog.save', options as Record<string, unknown>);
}

type ConfirmOptions = string | { title?: string; kind?: string; okLabel?: string; cancelLabel?: string };

export async function confirm(message: string, options?: ConfirmOptions): Promise<boolean> {
    const extra = typeof options === 'string' ? { title: options } : options ?? {};
    return call('host:dialog.confirm', { message, ...extra });
}

export const ask = confirm;

export async function message(text: string, options?: ConfirmOptions): Promise<void> {
    const extra = typeof options === 'string' ? { title: options } : options ?? {};
    await call('host:dialog.message', { message: text, ...extra });
}

// ─── fs ─────────────────────────────────────────────────────────────────

export async function readTextFile(path: string): Promise<string> {
    return call('host:fs.readText', { path });
}

export async function writeTextFile(path: string, contents: string): Promise<void> {
    await call('host:fs.writeText', { path, contents });
}

export async function writeFile(path: string, data: Uint8Array | ArrayBuffer): Promise<void> {
    const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
    let binary = '';
    for (let i = 0; i < bytes.length; i += 0x8000) {
        binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    }
    await call('host:fs.write', { path, base64: btoa(binary) });
}

// ─── opener ─────────────────────────────────────────────────────────────

/** In Search, a link opens in a new tab. */
export async function openUrl(url: string | URL): Promise<void> {
    await call('host:opener.url', { url: String(url) });
}

export async function openPath(path: string): Promise<void> {
    await call('host:opener.path', { path });
}

export async function revealItemInDir(path: string): Promise<void> {
    await call('host:opener.reveal', { path });
}

// ─── notification ───────────────────────────────────────────────────────

export const isPermissionGranted = async () => true;
export const requestPermission = async () => 'granted' as const;

export function sendNotification(options: string | { title: string; body?: string }) {
    const note = typeof options === 'string' ? { title: options } : options;
    void call('host:notify', note as Record<string, unknown>).catch(() => {});
}

// ─── autostart: Search starts when you start it ─────────────────────────

export const enable = async () => {};
export const disable = async () => {};
export const isEnabled = async () => false;

// ─── app ────────────────────────────────────────────────────────────────

export const getVersion = () => call<string>('host:app.version');
export const getName = async () => 'Search';
