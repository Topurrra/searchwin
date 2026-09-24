<script lang="ts">
    /*
      Redaction selector — a full-monitor overlay to mark rectangles that will be
      BLACKED OUT of the recording (the pixels never enter the file). Drag to add a
      box; click a box's ✕ to remove it; Done commits, Esc cancels. Boxes render
      solid black so you preview exactly what's hidden.

      Coordinate rule (single-monitor v1, mirrors region-select): the overlay covers
      the primary monitor (origin 0,0), so a CSS-pixel rect maps to MONITOR-RELATIVE
      PHYSICAL px by multiplying by window.devicePixelRatio — the {x,y,w,h} the
      recorder's redaction compositor expects.

      Contract:
        - emits "screenrec:redactions-selected" with an array of {x,y,w,h} (physical
          px) then closes; emits "screenrec:redactions-cancelled" on Esc/Cancel.
    */
    import { onMount } from 'svelte';
    import { emit } from '@tauri-apps/api/event';
    import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

    type Rect = { x: number; y: number; w: number; h: number };
    const win = getCurrentWebviewWindow();
    const MIN = 12; // minimum box size in CSS px

    let rects = $state<Rect[]>([]);
    let draft = $state<Rect | null>(null);
    let mode = $state<'idle' | 'create'>('idle');
    let dragStart = { x: 0, y: 0 };
    let viewW = $state(0);
    let viewH = $state(0);

    function clampRect(r: Rect): Rect {
        let { x, y, w, h } = r;
        if (w < 0) {
            x += w;
            w = -w;
        }
        if (h < 0) {
            y += h;
            h = -h;
        }
        x = Math.max(0, Math.min(x, viewW));
        y = Math.max(0, Math.min(y, viewH));
        w = Math.min(w, viewW - x);
        h = Math.min(h, viewH - y);
        return { x, y, w, h };
    }

    function onBackdropDown(e: PointerEvent) {
        if (e.button !== 0) return;
        e.preventDefault();
        mode = 'create';
        dragStart = { x: e.clientX, y: e.clientY };
        draft = { x: e.clientX, y: e.clientY, w: 0, h: 0 };
        (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    }
    function onPointerMove(e: PointerEvent) {
        if (mode !== 'create' || !draft) return;
        draft = clampRect({
            x: dragStart.x,
            y: dragStart.y,
            w: e.clientX - dragStart.x,
            h: e.clientY - dragStart.y,
        });
    }
    function onPointerUp() {
        if (mode === 'create' && draft) {
            if (draft.w >= MIN && draft.h >= MIN) rects = [...rects, draft];
            draft = null;
        }
        mode = 'idle';
    }
    function removeRect(i: number) {
        rects = rects.filter((_, idx) => idx !== i);
    }

    function done() {
        const dpr = window.devicePixelRatio || 1;
        const payload = rects.map((r) => ({
            x: Math.round(r.x * dpr),
            y: Math.round(r.y * dpr),
            w: Math.round(r.w * dpr),
            h: Math.round(r.h * dpr),
        }));
        void emit('screenrec:redactions-selected', payload).then(() => win.close());
    }
    function cancel() {
        void emit('screenrec:redactions-cancelled').then(() => win.close());
    }
    function onKeydown(e: KeyboardEvent) {
        if (e.key === 'Escape') {
            e.preventDefault();
            cancel();
        } else if (e.key === 'Enter') {
            e.preventDefault();
            done();
        }
    }

    onMount(() => {
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

<div class="rd-root" role="presentation" onpointerdown={onBackdropDown}>
    {#each rects as r, i}
        <div
            class="rd-box"
            style="left:{r.x}px; top:{r.y}px; width:{r.w}px; height:{r.h}px;"
            role="presentation"
        >
            <button
                type="button"
                class="rd-x"
                onpointerdown={(e) => e.stopPropagation()}
                onclick={() => removeRect(i)}
                aria-label="Remove this redaction"
            >
                ✕
            </button>
        </div>
    {/each}
    {#if draft}
        <div
            class="rd-box rd-draft"
            style="left:{draft.x}px; top:{draft.y}px; width:{draft.w}px; height:{draft.h}px;"
        ></div>
    {/if}

    <div class="rd-actions" role="presentation" onpointerdown={(e) => e.stopPropagation()}>
        <span class="rd-hint">Drag to add a black-out area · {rects.length} marked</span>
        <button type="button" class="rd-btn rd-cancel" onclick={cancel}>Cancel</button>
        <button type="button" class="rd-btn rd-done" onclick={done}>Done</button>
    </div>

    {#if rects.length === 0 && !draft}
        <div class="rd-tip">
            <div class="rd-tip-title">Drag rectangles over anything to hide it from the recording</div>
            <div class="rd-tip-sub">Those pixels are never captured · Enter to confirm · Esc to cancel</div>
        </div>
    {/if}
</div>

<style>
    .rd-root {
        position: fixed;
        inset: 0;
        cursor: crosshair;
        user-select: none;
        background: rgba(0, 0, 0, 0.28);
    }
    .rd-box {
        position: fixed;
        background: #000;
        border: 2px solid var(--color-accent, #4f8cff);
        box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.5);
    }
    .rd-draft {
        background: rgba(0, 0, 0, 0.85);
        border-style: dashed;
    }
    .rd-x {
        position: absolute;
        top: -11px;
        right: -11px;
        width: 22px;
        height: 22px;
        border-radius: 50%;
        border: 2px solid #fff;
        background: #ef4444;
        color: #fff;
        font-size: 11px;
        line-height: 1;
        cursor: pointer;
        display: grid;
        place-items: center;
    }
    .rd-x:hover {
        background: #dc2626;
    }
    .rd-actions {
        position: fixed;
        bottom: 28px;
        left: 50%;
        transform: translateX(-50%);
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 8px 12px;
        background: rgba(0, 0, 0, 0.82);
        border-radius: 10px;
    }
    .rd-hint {
        color: #fff;
        font-family: 'Inter', system-ui, sans-serif;
        font-size: 12.5px;
        font-variant-numeric: tabular-nums;
        white-space: nowrap;
    }
    .rd-btn {
        height: 30px;
        padding: 0 14px;
        border-radius: 8px;
        font-family: 'Inter', system-ui, sans-serif;
        font-size: 13px;
        font-weight: 600;
        cursor: pointer;
        border: 1px solid transparent;
    }
    .rd-cancel {
        background: rgba(255, 255, 255, 0.14);
        color: #fff;
    }
    .rd-cancel:hover {
        background: rgba(255, 255, 255, 0.24);
    }
    .rd-done {
        background: var(--color-accent, #4f8cff);
        color: var(--color-accent-contrast, #fff);
    }
    .rd-done:hover {
        filter: brightness(1.08);
    }
    .rd-tip {
        position: fixed;
        left: 50%;
        top: 50%;
        transform: translate(-50%, -50%);
        text-align: center;
        font-family: 'Inter', system-ui, sans-serif;
        color: #fff;
        pointer-events: none;
    }
    .rd-tip-title {
        font-size: 18px;
        font-weight: 600;
        text-shadow: 0 1px 4px rgba(0, 0, 0, 0.6);
    }
    .rd-tip-sub {
        margin-top: 6px;
        font-size: 13px;
        opacity: 0.85;
        text-shadow: 0 1px 4px rgba(0, 0, 0, 0.6);
    }
</style>
