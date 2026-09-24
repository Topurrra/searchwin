// @tauri-apps/api/window, /webview, /webviewWindow and /dpi.
//
// A tool page is a tab, not a window: it can't be moved, sized, minimised
// or dragged by itself, so these answer as a tab would and do nothing.

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
    onDragDropEvent = never;
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
