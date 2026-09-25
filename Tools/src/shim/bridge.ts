// The one road out of a tool page: messages to the Search tab that holds it.
//
// Workspace's pages were written against Tauri. In Search they run in a
// browser tab, and everything they used Tauri for goes through here instead:
//
//   page → browser   {kind: "invoke", id, cmd, args}
//   browser → page   {kind: "reply", id, ok: true, value} | {kind: "reply", id, ok: false, error}
//   browser → page   {kind: "event", event, payload}
//
// Commands named `host:…` are answered by the browser itself (dialogs,
// opening files, paths). Every other command goes on to the engine
// (docs/brain/Engine Protocol.md) under the name the page used.
//
// Opened outside Search (`pnpm dev` in an ordinary browser), there is no
// host: calls go to a small mock so the pages can still be worked on.

import { mockCall } from './mock';

type Waiting = { resolve: (value: unknown) => void; reject: (error: unknown) => void };
type Handler = (event: { event: string; id: number; payload: unknown }) => void;

interface HostView {
    postMessage(message: unknown): void;
    postMessageWithAdditionalObjects?(message: unknown, objects: ArrayLike<unknown>): void;
    addEventListener(type: 'message', listener: (event: { data: any }) => void): void;
}

const host: HostView | undefined = (globalThis as any).chrome?.webview;
const waiting = new Map<number, Waiting>();
const listeners = new Map<string, Set<Handler>>();
let nextId = 1;
let nextListener = 1;

host?.addEventListener('message', ({ data }) => {
    if (!data || typeof data !== 'object') return;
    if (data.kind === 'reply') {
        const call = waiting.get(data.id);
        if (!call) return;
        waiting.delete(data.id);
        if (data.ok) call.resolve(data.value);
        else call.reject(data.error);
    } else if (data.kind === 'event') {
        deliver(String(data.event), data.payload);
    }
});

/** Deliver an event to this page's listeners. */
export function deliver(event: string, payload: unknown) {
    for (const handler of listeners.get(event) ?? []) {
        try {
            handler({ event, id: nextListener, payload });
        } catch (error) {
            console.error(`listener for ${event} failed`, error);
        }
    }
}

/** A command, answered by the browser or the engine. Rejects with the
 *  command's own error value, as Tauri's `invoke` did. */
export function call<T = unknown>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
    if (!host) return mockCall(cmd, args) as Promise<T>;
    const id = nextId++;
    return new Promise<T>((resolve, reject) => {
        waiting.set(id, { resolve: resolve as (v: unknown) => void, reject });
        host.postMessage({ kind: 'invoke', id, cmd, args: args ?? {} });
    });
}

/** Where files dropped on the page are. A page can't see that; the browser
 *  can, when the File objects travel beside the message. */
export function dropped(files: ArrayLike<File>): Promise<string[]> {
    if (!host?.postMessageWithAdditionalObjects || files.length === 0) return Promise.resolve([]);
    const id = nextId++;
    return new Promise<string[]>((resolve, reject) => {
        waiting.set(id, { resolve: resolve as (v: unknown) => void, reject });
        host.postMessageWithAdditionalObjects!({ kind: 'invoke', id, cmd: 'host:drop.paths', args: {} }, files);
    });
}

export function subscribe(event: string, handler: Handler): () => void {
    let set = listeners.get(event);
    if (!set) listeners.set(event, (set = new Set()));
    set.add(handler);
    return () => set!.delete(handler);
}

export const inSearch = Boolean(host);
