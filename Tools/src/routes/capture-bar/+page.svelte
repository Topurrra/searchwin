<script lang="ts">
    /*
      Capture toolbar — a tiny, frameless, always-on-top floating bar with the
      live recorder controls (pulsing REC dot, mm:ss timer + filesize, Stop). It
      is created with content protection (WDA_EXCLUDEFROMCAPTURE) by the backend
      on Windows 10 2004+, so the toolbar itself is NOT captured in the recording
      — the user can keep it on screen while recording.

      Contract:
        - polls screenrec_status every 1s for the timer + filesize
        - Stop: invoke screenrec_stop, emit "screenrec:stopped", close this window
    */
    import { onMount, onDestroy } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event';
    import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
    import { Window } from '@tauri-apps/api/window';
    import { Square, Pause, Play } from '@lucide/svelte';

    const win = getCurrentWebviewWindow();
    let stoppedUnlisten: UnlistenFn | null = null;
    let hotkeyUnlisten: UnlistenFn | null = null;

    // The main app is minimized while recording (it's out of the way and out of
    // the capture). This bar drives Stop, so on stop we bring the app back.
    async function restoreMain() {
        try {
            const main = await Window.getByLabel('main');
            await main?.unminimize();
            await main?.setFocus();
        } catch {
            /* main may be gone — ignore */
        }
    }

    let elapsedMs = $state(0);
    let bytes = $state(0);
    let stopping = $state(false);
    let paused = $state(false);
    let pollTimer: ReturnType<typeof setInterval> | null = null;

    function fmtTime(ms: number): string {
        const total = Math.floor(ms / 1000);
        const m = Math.floor(total / 60);
        const s = total % 60;
        return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
    }
    function fmtBytes(b: number): string {
        if (b < 1024) return `${b} B`;
        if (b < 1024 * 1024) return `${(b / 1024).toFixed(0)} KB`;
        return `${(b / 1024 / 1024).toFixed(1)} MB`;
    }

    async function poll() {
        try {
            const s = await invoke<{
                recording: boolean;
                elapsedMs: number;
                outputBytes: number;
                failed: boolean;
                paused: boolean;
            }>('screenrec_status');
            elapsedMs = s.elapsedMs;
            bytes = s.outputBytes;
            paused = s.paused;
            // Encoder died mid-take → stop to finalise the partial (this also
            // restores the app + closes us). Handles the case where the user
            // navigated away from the recorder page, so its poll isn't running.
            if (s.failed && !stopping) {
                void stop();
                return;
            }
            // Backend stopped on its own → bring the app back and close.
            if (!s.recording && !stopping) {
                void restoreMain();
                void win.close();
            }
        } catch {
            /* transient — keep polling */
        }
    }

    async function togglePause() {
        if (stopping) return;
        const next = !paused;
        try {
            await invoke(next ? 'screenrec_pause' : 'screenrec_resume');
            paused = next; // poll re-confirms shortly
        } catch {
            /* the next poll resyncs the real state */
        }
    }

    async function stop() {
        if (stopping) return;
        stopping = true;
        if (pollTimer) {
            clearInterval(pollTimer);
            pollTimer = null;
        }
        let savedPath = '';
        try {
            savedPath = await invoke<string>('screenrec_stop');
        } catch {
            /* main page surfaces the error; we still tear down + close */
        }
        await emit('screenrec:stopped', savedPath);
        await restoreMain();
        await win.close();
    }

    onMount(() => {
        // Frameless + transparent → paint our own rounded pill, clip the corners.
        for (const el of [document.documentElement, document.body]) {
            el.style.background = 'transparent';
            el.style.margin = '0';
            el.style.padding = '0';
            el.style.overflow = 'hidden';
        }
        void poll();
        pollTimer = setInterval(poll, 1000);
        // Any Stop (in-page, or our own) emits this — close ourselves exactly once.
        // We are the SOLE owner of this window's close (no external close), which
        // avoids the double-close that printed an invalid-window-handle warning.
        void listen('screenrec:stopped', () => {
            if (stopping) return;
            stopping = true;
            if (pollTimer) {
                clearInterval(pollTimer);
                pollTimer = null;
            }
            void win.close();
        }).then((un) => (stoppedUnlisten = un));
        // Global hotkey while recording → stop. The toolbar stays alive even when
        // the app is minimized, so this is the reliable "stop from anywhere" path.
        void listen('screenrec:hotkey-toggle', () => {
            void stop();
        }).then((un) => (hotkeyUnlisten = un));
    });
    onDestroy(() => {
        stoppedUnlisten?.();
        hotkeyUnlisten?.();
        if (pollTimer) clearInterval(pollTimer);
    });
</script>

<div class="cb" data-tauri-drag-region>
    <span class="cb-dot" class:stopping class:paused aria-hidden="true"></span>
    <div class="cb-meta" data-tauri-drag-region>
        <span class="cb-time" role="timer" aria-label="Elapsed recording time">
            {fmtTime(elapsedMs)}
        </span>
        <span class="cb-size">{paused ? 'Paused' : fmtBytes(bytes)}</span>
    </div>
    <button
        type="button"
        class="cb-pause"
        onclick={togglePause}
        disabled={stopping}
        aria-label={paused ? 'Resume recording' : 'Pause recording'}
        title={paused ? 'Resume' : 'Pause'}
    >
        {#if paused}
            <Play size={13} fill="currentColor" />
        {:else}
            <Pause size={13} fill="currentColor" />
        {/if}
    </button>
    <button
        type="button"
        class="cb-stop"
        onclick={stop}
        disabled={stopping}
        aria-label="Stop recording"
        title="Stop & save"
    >
        <Square size={13} fill="currentColor" />
        <span>{stopping ? 'Saving…' : 'Stop'}</span>
    </button>
</div>

<style>
    .cb {
        display: flex;
        align-items: center;
        gap: 12px;
        height: 100vh;
        padding: 0 12px;
        box-sizing: border-box;
        background: var(--color-panel, #1b1b1b);
        border: 1px solid var(--color-border, #2c2c2c);
        border-radius: 12px;
        overflow: hidden;
        font-family: 'Inter', system-ui, sans-serif;
        color: var(--color-text, #ececec);
        cursor: grab;
    }
    .cb:active {
        cursor: grabbing;
    }

    .cb-dot {
        flex-shrink: 0;
        width: 11px;
        height: 11px;
        border-radius: 50%;
        background: #ef4444;
        box-shadow: 0 0 0 0 color-mix(in srgb, #ef4444 60%, transparent);
        animation: cb-pulse 1.6s ease-out infinite;
    }
    .cb-dot.stopping {
        animation: none;
        opacity: 0.5;
    }
    .cb-dot.paused {
        animation: none;
        background: #f59e0b; /* amber: recording is held */
        box-shadow: none;
    }
    @keyframes cb-pulse {
        0% {
            box-shadow: 0 0 0 0 color-mix(in srgb, #ef4444 55%, transparent);
        }
        100% {
            box-shadow: 0 0 0 8px transparent;
        }
    }
    @media (prefers-reduced-motion: reduce) {
        .cb-dot {
            animation: none;
        }
    }

    .cb-meta {
        display: flex;
        align-items: baseline;
        gap: 8px;
        min-width: 0;
    }
    .cb-time {
        font-variant-numeric: tabular-nums;
        font-size: 15px;
        font-weight: 600;
        color: var(--color-text, #ececec);
        letter-spacing: 0.02em;
    }
    .cb-size {
        font-variant-numeric: tabular-nums;
        font-size: 11.5px;
        color: var(--color-text-secondary, #9a9a9a);
    }

    .cb-pause {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
        width: 28px;
        height: 28px;
        margin-left: auto;
        border: 1px solid var(--color-border, #2c2c2c);
        border-radius: 8px;
        background: var(--color-panel-2, #232323);
        color: var(--color-text, #ececec);
        cursor: pointer;
    }
    .cb-pause:hover:not(:disabled) {
        background: var(--color-panel-3, #2c2c2c);
    }
    .cb-pause:disabled {
        opacity: 0.5;
        cursor: default;
    }

    .cb-stop {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        height: 28px;
        padding: 0 12px;
        border: none;
        border-radius: 8px;
        background: #ef4444;
        color: #fff;
        font-size: 12.5px;
        font-weight: 600;
        cursor: pointer;
    }
    .cb-stop:hover:not(:disabled) {
        background: #dc2626;
    }
    .cb-stop:disabled {
        opacity: 0.6;
        cursor: default;
    }
</style>
