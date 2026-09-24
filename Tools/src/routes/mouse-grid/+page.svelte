<script lang="ts">
    /*
      Mouse Grid — #18b. A transparent, full-screen overlay for
      hands-free pointer targeting (Talon's recursive mouse-grid
      technique).

      How it works: the screen is divided into a 3×3 grid labelled 1–9.
      Saying a number narrows the target to that cell and shows a fresh
      3×3 inside it; repeat to zoom in as far as needed. "click" warps
      the cursor to the centre of the current cell and clicks the app
      beneath; "back" undoes one level; "cancel" closes the grid. The
      1–9 / Enter / Backspace / Esc keys mirror the voice commands.

      Voice: this window runs its OWN tiny grammar-constrained Vosk
      recognizer (digits + click / back / cancel, source 'mouse-grid'),
      acquired through the #17 mic arbiter at priority 35 — it preempts
      command mode, which resumes when the grid closes.

      Targeting: the grid works in screen FRACTIONS [0,1]; the Rust
      `voice_mouse_warp` resolves them to pixels (DPI-aware). The window
      hides itself before the click so it lands on the app underneath.
    */
    import { onMount, onDestroy } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { listen, type UnlistenFn } from '@tauri-apps/api/event';
    import { settings } from '$lib/stores/settings';

    /** Source label for this window's recognizer — see #17's arbiter. */
    const GRID_SOURCE = 'mouse-grid';
    /** The recognizer's entire vocabulary while the grid is open. */
    const GRID_GRAMMAR = [
        'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine',
        'click', 'back', 'cancel',
    ];
    /** Spoken digit word → cell number (1–9). */
    const DIGIT_WORDS: Record<string, number> = {
        one: 1, two: 2, three: 3, four: 4, five: 5,
        six: 6, seven: 7, eight: 8, nine: 9,
    };

    /** The chosen-cell path — each entry 1–9; empty = the whole screen. */
    let selections = $state<number[]>([]);
    /** True from the moment a click is dispatched until the grid resets,
     *  so a late voice-final cannot fire a second click. */
    let acting = $state(false);

    let unlistenFinal: UnlistenFn | null = null;
    let unlistenReset: UnlistenFn | null = null;
    let unlistenPreempted: UnlistenFn | null = null;

    /** Current target rectangle in screen fractions, narrowed 3×3 by
     *  each selection. */
    let rect = $derived.by(() => {
        let x = 0;
        let y = 0;
        let w = 1;
        let h = 1;
        for (const n of selections) {
            const col = (n - 1) % 3;
            const row = Math.floor((n - 1) / 3);
            x += (col * w) / 3;
            y += (row * h) / 3;
            w /= 3;
            h /= 3;
        }
        return { x, y, w, h };
    });

    async function armGrid() {
        const modelPath = $settings.voskModelPath;
        if (!modelPath) return;
        try {
            await invoke('voice_start_continuous', {
                modelPath,
                source: GRID_SOURCE,
                grammar: GRID_GRAMMAR,
            });
        } catch {
            // Mic busy / no model — the keyboard fallback still works.
        }
    }

    /** Hide the grid window WITHOUT stopping the recognizer — for the
     *  preemption path, where a higher-priority surface already owns
     *  the mic (stopping it would clobber THAT surface's session). */
    async function hideGridWindow() {
        try {
            await invoke('hide_mouse_grid_window_command');
        } catch {
            /* best-effort */
        }
    }

    /** Close the grid: release the mic, then hide the window. */
    async function closeGrid() {
        void invoke('voice_stop_continuous').catch(() => {});
        await hideGridWindow();
    }

    /** Narrow the target to cell `n` (1–9). */
    function applySelection(n: number) {
        if (acting || n < 1 || n > 9) return;
        selections = [...selections, n];
    }

    /** Undo one level of narrowing. */
    function goBack() {
        if (acting) return;
        selections = selections.slice(0, -1);
    }

    /** Warp the cursor to the centre of the current cell and click —
     *  hiding the grid first so the click lands on the app beneath. */
    async function clickCurrent() {
        if (acting) return;
        acting = true;
        const fx = rect.x + rect.w / 2;
        const fy = rect.y + rect.h / 2;
        void invoke('voice_stop_continuous').catch(() => {});
        await hideGridWindow();
        // Let the window actually disappear before the click is sent.
        await new Promise((r) => setTimeout(r, 140));
        try {
            await invoke('voice_mouse_warp', { fx, fy });
            await invoke('voice_mouse_click', { button: 'left', double: false });
        } catch {
            /* best-effort */
        }
    }

    function handleFinal(text: string) {
        if (acting) return;
        const word = text.trim().toLowerCase();
        if (word.length === 0 || word.includes('[unk]')) return;
        if (word in DIGIT_WORDS) {
            applySelection(DIGIT_WORDS[word]);
        } else if (word === 'click') {
            void clickCurrent();
        } else if (word === 'back') {
            goBack();
        } else if (word === 'cancel') {
            void closeGrid();
        }
    }

    function resetGrid() {
        selections = [];
        acting = false;
        void armGrid();
    }

    function onKeydown(event: KeyboardEvent) {
        if (event.key === 'Escape') {
            event.preventDefault();
            void closeGrid();
        } else if (event.key === 'Enter') {
            event.preventDefault();
            void clickCurrent();
        } else if (event.key === 'Backspace') {
            event.preventDefault();
            goBack();
        } else if (event.key >= '1' && event.key <= '9') {
            event.preventDefault();
            applySelection(Number(event.key));
        }
    }

    onMount(() => {
        void (async () => {
            unlistenFinal = await listen<{ text?: string; source?: string | null }>(
                'voice-final',
                (evt) => {
                    if ((evt.payload?.source ?? null) !== GRID_SOURCE) return;
                    handleFinal(evt.payload?.text ?? '');
                },
            );
            unlistenReset = await listen('mouse-grid-reset', () => resetGrid());
            // #17 — a higher-priority surface (push-to-talk) took the
            // mic. The grid can't function voice-less; just hide it
            // (its recognizer is already stopped — don't re-stop it).
            unlistenPreempted = await listen<{ client?: string }>(
                'voice-preempted',
                (evt) => {
                    if (evt.payload?.client === GRID_SOURCE) void hideGridWindow();
                },
            );
        })();
        // Covers the first show — onMount fires once; the window is
        // reused, and `mouse-grid-reset` re-arms it on later shows.
        resetGrid();
    });

    onDestroy(() => {
        unlistenFinal?.();
        unlistenReset?.();
        unlistenPreempted?.();
        void invoke('voice_stop_continuous').catch(() => {});
    });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="grid-root" role="presentation">
    <div
        class="grid-frame"
        style:left="{rect.x * 100}%"
        style:top="{rect.y * 100}%"
        style:width="{rect.w * 100}%"
        style:height="{rect.h * 100}%"
    >
        {#each [1, 2, 3, 4, 5, 6, 7, 8, 9] as n}
            <div class="grid-cell"><span class="grid-label">{n}</span></div>
        {/each}
    </div>
    <div class="grid-hint" role="status">
        Say a number to zoom in · <strong>“click”</strong> to click ·
        <strong>“back”</strong> · <strong>“cancel”</strong>
    </div>
</div>

<style>
    /* The window is transparent (.transparent(true) in lib.rs). Force
       the webview's own layers transparent so only the grid lines and
       labels paint over whatever app is underneath. */
    :global(html),
    :global(body),
    :global(#app) {
        background: transparent !important;
        margin: 0;
        padding: 0;
        overflow: hidden;
    }

    .grid-root {
        position: fixed;
        inset: 0;
        font-family: system-ui, sans-serif;
    }

    /* The current target rectangle — a 3×3 grid of labelled cells.
       Positioned + sized in screen percentages by the inline styles. */
    .grid-frame {
        position: fixed;
        display: grid;
        grid-template-columns: repeat(3, 1fr);
        grid-template-rows: repeat(3, 1fr);
        box-sizing: border-box;
        outline: 2px solid rgba(255, 207, 51, 0.95);
    }

    .grid-cell {
        border: 1px solid rgba(255, 207, 51, 0.7);
        background: rgba(0, 0, 0, 0.06);
        display: flex;
        align-items: center;
        justify-content: center;
        overflow: hidden;
    }

    /* White numerals with a heavy dark halo so they stay legible over
       any background — light or dark, busy or plain. */
    .grid-label {
        font-size: clamp(14px, 3.5vmin, 44px);
        font-weight: 800;
        color: #ffffff;
        text-shadow:
            0 0 4px #000,
            1px 1px 2px #000,
            -1px -1px 2px #000,
            1px -1px 2px #000,
            -1px 1px 2px #000;
    }

    .grid-hint {
        position: fixed;
        left: 50%;
        bottom: 28px;
        transform: translateX(-50%);
        padding: 7px 14px;
        border-radius: 999px;
        background: rgba(15, 15, 18, 0.92);
        color: #f4f4f5;
        font-size: 12.5px;
        white-space: nowrap;
        box-shadow: 0 6px 22px rgba(0, 0, 0, 0.45);
    }
    .grid-hint strong {
        color: #ffcf33;
        font-weight: 700;
    }
</style>
