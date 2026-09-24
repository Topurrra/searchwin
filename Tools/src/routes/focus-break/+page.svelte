<script lang="ts">
    /*
      Focus break overlay — a full-screen, opaque, always-on-top takeover
      (window configured in Rust) shown when a Pomodoro work phase ends and a
      break begins. Receives { kind, endsAt } from the main window over a Tauri
      event and mirrors a live countdown from the absolute end-time (so no
      cross-window timer desync). Skip/Dismiss control the main timer.

      Colors are hard-coded — a standalone overlay window doesn't carry the
      app's theme variables.
    */
    import { onMount, onDestroy } from 'svelte';
    import { listen, emitTo, type UnlistenFn } from '@tauri-apps/api/event';
    import { invoke } from '@tauri-apps/api/core';
    import { Coffee, SkipForward, X } from '@lucide/svelte';

    let kind = $state<'short' | 'long'>('short');
    let endsAt = $state(0);
    let now = $state(Date.now());
    let unlisten: UnlistenFn | null = null;
    let clock: ReturnType<typeof setInterval> | null = null;

    onMount(async () => {
        clock = setInterval(() => {
            now = Date.now();
            if (endsAt && now >= endsAt) void dismiss();
        }, 250);
        try {
            unlisten = await listen<{ kind: 'short' | 'long'; endsAt: number }>(
                'focus-break-info',
                (event) => {
                    kind = event.payload.kind;
                    endsAt = event.payload.endsAt;
                },
            );
            // Handshake: tell the main window we're mounted + listening, so it
            // (re)sends the break info — covers the first-show race where this
            // window mounts after the initial emit.
            await emitTo('main', 'focus-break-ready');
        } catch {
            // events unavailable (plain-browser dev) — render with defaults
        }
    });
    onDestroy(() => {
        if (unlisten) unlisten();
        if (clock) clearInterval(clock);
    });

    let remainMs = $derived(Math.max(0, endsAt - now));
    let mmss = $derived(
        `${Math.floor(remainMs / 60000)
            .toString()
            .padStart(2, '0')}:${Math.floor((remainMs % 60000) / 1000)
            .toString()
            .padStart(2, '0')}`,
    );
    let label = $derived(kind === 'long' ? 'Long break' : 'Short break');

    async function dismiss() {
        try {
            await invoke('hide_focus_break_window_command');
        } catch {
            // ignore
        }
    }
    async function skip() {
        try {
            await emitTo('main', 'focus-break-skip');
        } catch {
            // ignore
        }
        await dismiss();
    }
</script>

<div class="bk">
    <div class="bk-ico"><Coffee /></div>
    <div class="bk-kicker">Time for a break</div>
    <div class="bk-label">{label}</div>
    <div class="bk-time">{mmss}</div>
    <div class="bk-actions">
        <button type="button" class="bk-btn bk-skip" onclick={skip}>
            <SkipForward class="bk-btn-ico" /> Skip break
        </button>
        <button type="button" class="bk-btn bk-dismiss" onclick={dismiss}>
            <X class="bk-btn-ico" /> Dismiss
        </button>
    </div>
    <div class="bk-hint">The break keeps running — dismiss to hide this screen.</div>
</div>

<style>
    :global(html),
    :global(body) {
        margin: 0;
        height: 100%;
        overflow: hidden;
    }
    .bk {
        position: fixed;
        inset: 0;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 12px;
        text-align: center;
        color: #f8fafc;
        background:
            radial-gradient(120% 120% at 50% 0%, rgba(16, 185, 129, 0.18), transparent 60%),
            #0e1116;
        font-family:
            'Inter',
            -apple-system,
            BlinkMacSystemFont,
            'Segoe UI',
            sans-serif;
    }
    .bk-ico {
        width: 56px;
        height: 56px;
        display: grid;
        place-items: center;
        border-radius: 16px;
        color: #10b981;
        background: rgba(16, 185, 129, 0.14);
        border: 1px solid rgba(16, 185, 129, 0.3);
    }
    .bk-ico :global(svg) {
        width: 30px;
        height: 30px;
    }
    .bk-kicker {
        margin-top: 6px;
        font-size: 13px;
        font-weight: 600;
        letter-spacing: 0.08em;
        text-transform: uppercase;
        color: #94a3b8;
    }
    .bk-label {
        font-size: 22px;
        font-weight: 700;
    }
    .bk-time {
        font-size: 96px;
        font-weight: 800;
        line-height: 1;
        font-variant-numeric: tabular-nums;
    }
    .bk-actions {
        display: flex;
        gap: 12px;
        margin-top: 14px;
    }
    .bk-btn {
        display: inline-flex;
        align-items: center;
        gap: 7px;
        padding: 10px 18px;
        font-size: 14px;
        font-weight: 600;
        border-radius: 10px;
        cursor: pointer;
        border: 1px solid transparent;
    }
    .bk-skip {
        color: #0e1116;
        background: #10b981;
    }
    .bk-skip:hover {
        background: #34d399;
    }
    .bk-dismiss {
        color: #e2e8f0;
        background: rgba(255, 255, 255, 0.06);
        border-color: rgba(255, 255, 255, 0.14);
    }
    .bk-dismiss:hover {
        background: rgba(255, 255, 255, 0.12);
    }
    .bk-btn :global(.bk-btn-ico) {
        width: 16px;
        height: 16px;
    }
    .bk-hint {
        margin-top: 10px;
        font-size: 12.5px;
        color: #64748b;
    }
</style>
