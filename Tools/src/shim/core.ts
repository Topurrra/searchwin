// @tauri-apps/api/core, for pages running in a Search tab.

import { call } from './bridge';

export function invoke<T = unknown>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    return call<T>(cmd, args);
}

/** A local file the page wants to show (an image preview, a video). The
 *  browser serves it to tool tabs only, one file per request. */
export function convertFileSrc(path: string, _protocol = 'asset'): string {
    return `https://files.search/local?path=${encodeURIComponent(path)}`;
}

export function isTauri(): boolean {
    return false;
}
