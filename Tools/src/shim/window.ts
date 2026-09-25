// @tauri-apps/api/window, /webview, /webviewWindow and /dpi.
//
// A tool page is a tab, not a window: it can't be moved, sized, minimised
// or dragged by itself, so these answer as a tab would and do nothing.
// Files dropped on it do arrive, with their paths.

import { dropped } from './bridge';
import type { UnlistenFn } from './event';

export class LogicalSize {
    constructor(public width: number, public height: number) {}
}
export class PhysicalSize {
    constructor(public width: number, public height: number) {}
}
export class LogicalPosition {
    constructor(public x: number, public y: number) {}
}
export class PhysicalPosition {
    constructor(public x: number, public y: number) {}
}

const nothing = async () => {};
const never = async (): Promise<UnlistenFn> => () => {};

// ─── files dragged in from Explorer ─────────────────────────────────────

export type DragDropEvent =
    | { type: 'enter' | 'over'; position: PhysicalPosition }
    | { type: 'drop'; paths: string[]; position: PhysicalPosition }
    | { type: 'leave' };
type DropHandler = (event: { payload: DragDropEvent }) => void;

const dropHandlers = new Set<DropHandler>();

function tell(payload: DragDropEvent) {
    for (const handler of [...dropHandlers]) {
        try {
            handler({ payload });
        } catch (error) {
            console.error('drop handler failed', error);
        }
    }
}

const carriesFiles = (e: DragEvent) => Array.from(e.dataTransfer?.types ?? []).includes('Files');
/** Tauri reported physical pixels. */
const where = (e: DragEvent) =>
    new PhysicalPosition(e.clientX * window.devicePixelRatio, e.clientY * window.devicePixelRatio);

const dropListeners: Array<[string, (e: DragEvent) => void]> = [
    ['dragenter', (e) => carriesFiles(e) && (e.preventDefault(), tell({ type: 'enter', position: where(e) }))],
    ['dragover', (e) => carriesFiles(e) && (e.preventDefault(), tell({ type: 'over', position: where(e) }))],
    // Leaving the page, not one element for another.
    ['dragleave', (e) => carriesFiles(e) && !e.relatedTarget && tell({ type: 'leave' })],
    [
        'drop',
        (e) => {
            if (!carriesFiles(e)) return;
            // Taken here, or the tab would go to the file instead.
            e.preventDefault();
            const position = where(e);
            void dropped(e.dataTransfer!.files).then(
                (paths) => tell({ type: 'drop', paths, position }),
                () => tell({ type: 'leave' }),
            );
        },
    ],
];

/** Files dragged in from Explorer, as Tauri reported them: the pointer
 *  while they're over the page, then the files' paths when they land. */
async function onDragDropEvent(handler: DropHandler): Promise<UnlistenFn> {
    if (dropHandlers.size === 0)
        for (const [type, listener] of dropListeners) window.addEventListener(type, listener as EventListener);
    dropHandlers.add(handler);
    return () => {
        if (!dropHandlers.delete(handler) || dropHandlers.size > 0) return;
        for (const [type, listener] of dropListeners) window.removeEventListener(type, listener as EventListener);
    };
}

export class Window {
    constructor(public label = 'tool') {}
    show = nothing;
    hide = nothing;
    close = nothing;
    destroy = nothing;
    minimize = nothing;
    maximize = nothing;
    unmaximize = nothing;
    toggleMaximize = nothing;
    setFocus = nothing;
    setSize = nothing;
    setPosition = nothing;
    setAlwaysOnTop = nothing;
    setTitle = nothing;
    setDecorations = nothing;
    setIgnoreCursorEvents = nothing;
    startDragging = nothing;
    center = nothing;
    isVisible = async () => document.visibilityState === 'visible';
    isFocused = async () => document.hasFocus();
    isMaximized = async () => true;
    isMinimized = async () => false;
    scaleFactor = async () => window.devicePixelRatio;
    innerSize = async () => new PhysicalSize(window.innerWidth, window.innerHeight);
    outerSize = async () => new PhysicalSize(window.outerWidth, window.outerHeight);
    outerPosition = async () => new PhysicalPosition(0, 0);
    listen = never;
    once = never;
    onResized = never;
    onMoved = never;
    onFocusChanged = never;
    onCloseRequested = never;
    onScaleChanged = never;
    onDragDropEvent = onDragDropEvent;
    emit = nothing;
}

const current = new Window('tool');

export const getCurrentWindow = () => current;
export const getCurrentWebviewWindow = () => current;
export const getCurrentWebview = () => current;
export const getAllWindows = async () => [current];
export const currentMonitor = async () => null;
export const primaryMonitor = async () => null;

export class WebviewWindow extends Window {
    static getByLabel = async (_label: string) => null;
}
