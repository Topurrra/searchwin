// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// The page's side of a drop from Explorer. The browser (ToolsHost) is the
// only one who can read where dropped files are; here it's a stand-in that
// answers host:drop.paths for the File objects it was handed.
type Sent = { message: any; objects: ArrayLike<unknown> };

let sent: Sent[];
let reply: ((data: unknown) => void) | undefined;
let stops: Array<() => void>;

beforeEach(() => {
    sent = [];
    stops = [];
    vi.resetModules();
    vi.stubGlobal('chrome', {
        webview: {
            postMessage: vi.fn(),
            postMessageWithAdditionalObjects: (message: unknown, objects: ArrayLike<unknown>) =>
                sent.push({ message, objects }),
            addEventListener: (_type: string, listener: (event: { data: unknown }) => void) => {
                reply = (data) => listener({ data });
            },
        },
    });
});

afterEach(() => {
    for (const stop of stops) stop();
    vi.unstubAllGlobals();
});

function drag(type: string, types: string[], files: File[] = []) {
    const event = new Event(type, { bubbles: true, cancelable: true });
    Object.assign(event, { clientX: 10, clientY: 20, relatedTarget: null, dataTransfer: { types, files } });
    window.dispatchEvent(event);
    return event;
}

describe('files dropped on a tool page', () => {
    it('reach onDragDropEvent with the paths the browser reads from the dropped files', async () => {
        const { getCurrentWebview } = await import('./window');
        const events: any[] = [];
        stops.push(await getCurrentWebview().onDragDropEvent((e) => events.push(e.payload)));
        const file = new File(['x'], 'report.pdf');

        expect(drag('dragover', ['Files']).defaultPrevented).toBe(true);
        const drop = drag('drop', ['Files'], [file]);

        // Taken by the page, so the tab doesn't navigate to the file.
        expect(drop.defaultPrevented).toBe(true);
        expect(sent).toHaveLength(1);
        expect(sent[0].message.cmd).toBe('host:drop.paths');
        expect(Array.from(sent[0].objects)).toEqual([file]);

        reply!({ kind: 'reply', id: sent[0].message.id, ok: true, value: ['C:\\Temp\\report.pdf'] });
        await vi.waitFor(() => expect(events.at(-1)?.type).toBe('drop'));
        expect(events.map((e) => e.type)).toEqual(['over', 'drop']);
        expect(events.at(-1).paths).toEqual(['C:\\Temp\\report.pdf']);
    });

    it("leaves a drag of the page's own elements alone", async () => {
        const { getCurrentWebview } = await import('./window');
        const events: any[] = [];
        stops.push(await getCurrentWebview().onDragDropEvent((e) => events.push(e.payload)));

        const drop = drag('drop', ['text/plain']);

        expect(drop.defaultPrevented).toBe(false);
        expect(sent).toHaveLength(0);
        expect(events).toEqual([]);
    });

    it('stops listening once every handler has unlistened', async () => {
        const { getCurrentWebview } = await import('./window');
        const events: any[] = [];
        const stop = await getCurrentWebview().onDragDropEvent((e) => events.push(e.payload));
        stop();

        expect(drag('drop', ['Files'], [new File(['x'], 'a.txt')]).defaultPrevented).toBe(false);
        expect(sent).toHaveLength(0);
    });
});
