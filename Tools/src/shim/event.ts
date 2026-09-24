// @tauri-apps/api/event: the engine's events, and the pages' own.

import { call, deliver, subscribe } from './bridge';

export type UnlistenFn = () => void;
export type EventCallback<T> = (event: { event: string; id: number; payload: T }) => void;

export async function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
    return subscribe(event, handler as EventCallback<unknown>);
}

export async function once<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
    const stop = subscribe(event, (e) => {
        stop();
        (handler as EventCallback<unknown>)(e);
    });
    return stop;
}

/** Heard by this page at once, and by the browser's other tool tabs. */
export async function emit(event: string, payload?: unknown): Promise<void> {
    deliver(event, payload);
    await call('host:emit', { event, payload }).catch(() => {});
}

export async function emitTo(_target: unknown, event: string, payload?: unknown): Promise<void> {
    await emit(event, payload);
}
