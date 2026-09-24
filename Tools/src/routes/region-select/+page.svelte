<script lang="ts">
    /*
      Region selector — a bare, full-monitor transparent overlay used to drag a
      capture rectangle on the live desktop. It covers the primary monitor edge
      to edge; the user drags out a rectangle (or drag-creates then tweaks it via
      8 resize handles + a move-by-drag interior), and confirms with the button
      or Enter. Esc cancels.

      Coordinate rule (single-monitor v1): the window covers the primary monitor
      whose origin is 0,0, so a CSS-pixel rect maps to MONITOR-RELATIVE PHYSICAL
      pixels by multiplying by window.devicePixelRatio. That physical rect is the
      {x,y,w,h} payload the recorder expects.

      Contract:
        - emits "screenrec:region-selected" {x,y,w,h} (physical px) then closes
        - emits "screenrec:region-cancelled" on Esc/Cancel then closes
    */
    import { onMount } from 'svelte';
    import { emit } from '@tauri-apps/api/event';
    import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

    type Rect = { x: number; y: number; w: number; h: number };
    type Handle = 'nw' | 'n' | 'ne' | 'e' | 'se' | 's' | 'sw' | 'w';

    const win = getCurrentWebviewWindow();
    const MIN = 16; // minimum rect size in CSS px

    // The selection rect in CSS px (overlay-local; overlay origin === monitor 0,0).
    let rect = $state<Rect | null>(null);

    // Ephemeral drag state.
    let mode = $state<'idle' | 'create' | 'move' | 'resize'>('idle');
    let dragStart = { x: 0, y: 0 }; // pointer at drag start
    let rectStart: Rect = { x: 0, y: 0, w: 0, h: 0 }; // rect snapshot at drag start
    let activeHandle: Handle | null = null;

    let viewW = $state(0);
    let viewH = $state(0);

    const handles: Handle[] = ['nw', 'n', 'ne', 'e', 'se', 's', 'sw', 'w'];

    function clampRect(r: Rect): Rect {
        // Normalize (positive w/h) and clamp into the viewport.
        let { x, y, w, h } = r;
        if (w < 0) { x += w; w = -w; }
        if (h < 0) { y += h; h = -h; }
        x = Math.max(0, Math.min(x, viewW));
        y = Math.max(0, Math.min(y, viewH));
        w = Math.min(w, viewW - x);
        h = Math.min(h, viewH - y);
        return { x, y, w, h };
    }

    function onBackdropDown(e: PointerEvent) {
        // Left-button only; start a fresh rectangle from this point.
        if (e.button !== 0) return;
        e.preventDefault();
        mode = 'create';
        dragStart = { x: e.clientX, y: e.clientY };
        rect = { x: e.clientX, y: e.clientY, w: 0, h: 0 };
        (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    }

    function onRectDown(e: PointerEvent) {
        if (e.button !== 0 || !rect) return;
        e.preventDefault();
        e.stopPropagation();
        mode = 'move';
        dragStart = { x: e.clientX, y: e.clientY };
        rectStart = { ...rect };
        (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    }

    function onHandleDown(e: PointerEvent, h: Handle) {
        if (e.button !== 0 || !rect) return;
        e.preventDefault();
        e.stopPropagation();
        mode = 'resize';
        activeHandle = h;
        dragStart = { x: e.clientX, y: e.clientY };
        rectStart = { ...rect };
        (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    }

    function onPointerMove(e: PointerEvent) {
        if (mode === 'idle' || !rect) return;
        const dx = e.clientX - dragStart.x;
        const dy = e.clientY - dragStart.y;

        if (mode === 'create') {
            rect = clampRect({ x: dragStart.x, y: dragStart.y, w: dx, h: dy });
        } else if (mode === 'move') {
            const x = Math.max(0, Math.min(rectStart.x + dx, viewW - rectStart.w));
            const y = Math.max(0, Math.min(rectStart.y + dy, viewH - rectStart.h));
            rect = { x, y, w: rectStart.w, h: rectStart.h };
        } else if (mode === 'resize' && activeHandle) {
            rect = resizeRect(rectStart, activeHandle, dx, dy);
        }
    }

    function resizeRect(r: Rect, h: Handle, dx: number, dy: number): Rect {
        let { x, y, w, h: hh } = r;
        const right = x + w;
        const bottom = y + hh;
        if (h.includes('w')) { x = Math.min(x + dx, right - MIN); w = right - x; }
        if (h.includes('e')) { w = Math.max(MIN, w + dx); }
        if (h.includes('n')) { y = Math.min(y + dy, bottom - MIN); hh = bottom - y; }
        if (h.includes('s')) { hh = Math.max(MIN, hh + dy); }
        return clampRect({ x, y, w, h: hh });
    }

    function onPointerUp(e: PointerEvent) {
        if (mode === 'idle') return;
        (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
        // A click without a real drag leaves a zero/tiny rect — discard it.
        if (rect && (rect.w < MIN || rect.h < MIN)) rect = null;
        mode = 'idle';
        activeHandle = null;
    }

    function confirm() {
        if (!rect || rect.w < MIN || rect.h < MIN) return;
        const dpr = window.devicePixelRatio || 1;
        const payload = {
            x: Math.round(rect.x * dpr),
            y: Math.round(rect.y * dpr),
            w: Math.round(rect.w * dpr),
            h: Math.round(rect.h * dpr),
        };
        void emit('screenrec:region-selected', payload).then(() => win.close());
    }

    function cancel() {
        void emit('screenrec:region-cancelled').then(() => win.close());
    }

    function onKeydown(e: KeyboardEvent) {
        if (e.key === 'Escape') {
            e.preventDefault();
            cancel();
        } else if (e.key === 'Enter') {
            e.preventDefault();
            confirm();
        }
    }

    // Physical-pixel readout (what actually gets recorded).
    const phys = $derived.by(() => {
        if (!rect) return null;
        const dpr = window.devicePixelRatio || 1;
        return {
            x: Math.round(rect.x * dpr),
            y: Math.round(rect.y * dpr),
            w: Math.round(rect.w * dpr),
            h: Math.round(rect.h * dpr),
        };
    });

    onMount(() => {
        // Bare overlay: strip all chrome so nothing paints behind our backdrop.
        for (const el of [document.documentElement, document.body]) {
            el.style.background = 'transparent';
            el.style.margin = '0';
            el.style.padding = '0';
            el.style.overflow = 'hidden';
        }
        const sync = () => {
            viewW = window.innerWidth;
            viewH = window.innerHeight;
        };
        sync();
        window.addEventListener('resize', sync);
        return () => window.removeEventListener('resize', sync);
    });
</script>

<svelte:window onkeydown={onKeydown} onpointermove={onPointerMove} onpointerup={onPointerUp} />

<!-- Backdrop: dim everything; the selection rect punches a clear hole via the
     four masking panels so the live desktop shows through inside it. -->
<div
    class="rs-root"
    role="presentation"
    onpointerdown={onBackdropDown}
>
    {#if rect}
        <!-- Four dim panels around the selection so the rect interior stays clear. -->
        <div class="rs-mask" style="left:0; top:0; right:0; height:{rect.y}px;"></div>
        <div
            class="rs-mask"
            style="left:0; top:{rect.y}px; width:{rect.x}px; height:{rect.h}px;"
        ></div>
        <div
            class="rs-mask"
            style="left:{rect.x + rect.w}px; top:{rect.y}px; right:0; height:{rect.h}px;"
        ></div>
        <div class="rs-mask" style="left:0; top:{rect.y + rect.h}px; right:0; bottom:0;"></div>

        <!-- Selection rectangle: marching-ants border + move-by-drag interior. -->
        <div
            class="rs-rect"
            style="left:{rect.x}px; top:{rect.y}px; width:{rect.w}px; height:{rect.h}px;"
            onpointerdown={onRectDown}
            role="presentation"
        >
            {#each handles as h}
                <div
                    class="rs-handle rs-{h}"
                    onpointerdown={(e) => onHandleDown(e, h)}
                    role="presentation"
                ></div>
            {/each}
        </div>

        <!-- Size + position readout, parked just outside the rect (or inside if
             it would clip off the top). -->
        {#if phys}
            <div
                class="rs-readout"
                style="left:{rect.x}px; top:{rect.y > 34 ? rect.y - 30 : rect.y + rect.h + 8}px;"
            >
                {phys.w} × {phys.h}px · ({phys.x}, {phys.y})
            </div>
        {/if}

        <!-- Confirm / cancel toolbar, anchored to the bottom of the rect.
             Stop pointerdown from bubbling to the backdrop — otherwise clicking a
             button starts a fresh zero-size drag that shrinks the rect below MIN
             and confirm() no-ops (only Enter worked before). -->
        <div
            class="rs-actions"
            style="left:{rect.x + rect.w}px; top:{rect.y + rect.h + 10}px;"
            role="presentation"
            onpointerdown={(e) => e.stopPropagation()}
        >
            <button type="button" class="rs-btn rs-cancel" onclick={cancel}>Cancel</button>
            <button
                type="button"
                class="rs-btn rs-confirm"
                onclick={confirm}
                disabled={rect.w < MIN || rect.h < MIN}
            >
                Record this region
            </button>
        </div>
    {:else}
        <!-- Pre-drag hint, centered. -->
        <div class="rs-hint">
            <div class="rs-hint-title">Drag to select a region to record</div>
            <div class="rs-hint-sub">Enter to confirm · Esc to cancel</div>
        </div>
    {/if}
</div>

<style>
    .rs-root {
        position: fixed;
        inset: 0;
        cursor: crosshair;
        user-select: none;
        /* When no rect yet, a faint dim over the whole desktop reads as "modal". */
        background: transparent;
    }
    .rs-mask {
        position: fixed;
        background: rgba(0, 0, 0, 0.42);
        pointer-events: none;
    }
    /* Before any rect, dim the whole surface. */
    .rs-root:has(.rs-hint) {
        background: rgba(0, 0, 0, 0.42);
    }

    .rs-rect {
        position: fixed;
        cursor: move;
        box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.9);
        outline: 1px dashed var(--color-accent, #4f8cff);
        outline-offset: -1px;
        background: transparent;
    }

    .rs-handle {
        position: absolute;
        width: 12px;
        height: 12px;
        background: var(--color-accent, #4f8cff);
        border: 2px solid #fff;
        border-radius: 50%;
        box-shadow: 0 1px 3px rgba(0, 0, 0, 0.5);
    }
    .rs-nw { left: -7px; top: -7px; cursor: nwse-resize; }
    .rs-n  { left: calc(50% - 6px); top: -7px; cursor: ns-resize; }
    .rs-ne { right: -7px; top: -7px; cursor: nesw-resize; }
    .rs-e  { right: -7px; top: calc(50% - 6px); cursor: ew-resize; }
    .rs-se { right: -7px; bottom: -7px; cursor: nwse-resize; }
    .rs-s  { left: calc(50% - 6px); bottom: -7px; cursor: ns-resize; }
    .rs-sw { left: -7px; bottom: -7px; cursor: nesw-resize; }
    .rs-w  { left: -7px; top: calc(50% - 6px); cursor: ew-resize; }

    .rs-readout {
        position: fixed;
        padding: 3px 8px;
        font-family: 'Inter', system-ui, sans-serif;
        font-size: 12px;
        font-variant-numeric: tabular-nums;
        font-weight: 600;
        color: #fff;
        background: rgba(0, 0, 0, 0.75);
        border-radius: 6px;
        pointer-events: none;
        white-space: nowrap;
    }

    .rs-actions {
        position: fixed;
        display: flex;
        gap: 8px;
        transform: translateX(-100%);
    }
    .rs-btn {
        height: 30px;
        padding: 0 14px;
        border-radius: 8px;
        font-family: 'Inter', system-ui, sans-serif;
        font-size: 13px;
        font-weight: 600;
        cursor: pointer;
        border: 1px solid transparent;
    }
    .rs-cancel {
        background: rgba(0, 0, 0, 0.78);
        color: #fff;
        border-color: rgba(255, 255, 255, 0.25);
    }
    .rs-cancel:hover {
        background: rgba(0, 0, 0, 0.92);
    }
    .rs-confirm {
        background: var(--color-accent, #4f8cff);
        color: var(--color-accent-contrast, #fff);
    }
    .rs-confirm:hover {
        filter: brightness(1.08);
    }
    .rs-confirm:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }

    .rs-hint {
        position: fixed;
        left: 50%;
        top: 50%;
        transform: translate(-50%, -50%);
        text-align: center;
        font-family: 'Inter', system-ui, sans-serif;
        color: #fff;
        pointer-events: none;
    }
    .rs-hint-title {
        font-size: 18px;
        font-weight: 600;
        text-shadow: 0 1px 4px rgba(0, 0, 0, 0.6);
    }
    .rs-hint-sub {
        margin-top: 6px;
        font-size: 13px;
        opacity: 0.8;
        text-shadow: 0 1px 4px rgba(0, 0, 0, 0.6);
    }
</style>
