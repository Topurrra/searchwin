// @tauri-apps/api/path. Joining and splitting happen here; the folders
// themselves come from the browser.

import { call } from './bridge';

export const sep = '\\';
export const delimiter = ';';

const folder = (which: string) => call<string>('host:path', { which });

export const appDataDir = () => folder('appData');
export const appLocalDataDir = () => folder('appData');
export const appConfigDir = () => folder('appData');
export const homeDir = () => folder('home');
export const tempDir = () => folder('temp');
export const documentDir = () => folder('documents');
export const downloadDir = () => folder('downloads');
export const desktopDir = () => folder('desktop');
export const pictureDir = () => folder('pictures');

export async function join(...parts: string[]): Promise<string> {
    return parts
        .filter((part) => part !== '')
        .map((part, i) => (i === 0 ? part.replace(/[\\/]+$/, '') : part.replace(/^[\\/]+|[\\/]+$/g, '')))
        .join(sep);
}

export async function dirname(path: string): Promise<string> {
    const trimmed = path.replace(/[\\/]+$/, '');
    const cut = Math.max(trimmed.lastIndexOf('\\'), trimmed.lastIndexOf('/'));
    if (cut < 0) return '.';
    const parent = trimmed.slice(0, cut);
    return /^[A-Za-z]:$/.test(parent) ? parent + sep : parent;
}

export async function basename(path: string, ext?: string): Promise<string> {
    const name = path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() ?? '';
    return ext && name.endsWith(ext) ? name.slice(0, -ext.length) : name;
}

export async function extname(path: string): Promise<string> {
    const name = await basename(path);
    const dot = name.lastIndexOf('.');
    return dot > 0 ? name.slice(dot + 1) : '';
}
