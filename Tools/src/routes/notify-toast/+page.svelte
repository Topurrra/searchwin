<script lang="ts">
    /*
      Notification toast overlay — a transparent, click-through, always-on-top
      full-screen window (configured in Rust) that stacks small toast cards in
      the bottom-right corner. KeepItLocal's RELIABLE notification channel:
      Windows toast notifications silently no-op in dev (no installed Start-Menu
      shortcut / AUMID) and can be suppressed even in prod, but a due reminder /
      Pomodoro phase change / Focus warning has to be seen. Cards show regardless
      of which app is focused or whether KeepItLocal is minimized to the tray.

      Driven by `notify.ts` → `notifyOS()`, which shows this window and emits a
      `notify-toast-show` event. Each event carries an `id` so the first-show
      re-emit (which guards the listener-mount race) never double-stacks.

      Colors / type are hard-coded to KeepItLocal's DARK tokens (a standalone
      overlay window can't rely on the themed CSS variables). Entrance is
      transform-only (opacity stays at 1) so there's no transparent-overlay
      flicker — per the binding overlay rules.
    */
    import { onMount } from 'svelte';
    import { listen, emit } from '@tauri-apps/api/event';
    import { invoke } from '@tauri-apps/api/core';
    import { fade } from 'svelte/transition';
    import { Info, Bell, Target, Timer, TriangleAlert, type Icon as LucideIcon } from '@lucide/svelte';

    type Kind = 'info' | 'reminder' | 'focus' | 'pomodoro' | 'warn';
    interface Toast {
        id: number;
        title: string;
        body: string;
        kind: Kind;
    }

    /** Per-kind accent (RGB triple) + icon. Accents are KeepItLocal's palette:
     *  emerald brand for info, green for reminders, amber for focus, violet for
     *  the Time & Focus pack, red for warnings. */
    const META: Record<Kind, { rgb: string; icon: typeof LucideIcon }> = {
        info: { rgb: '16, 185, 129', icon: Info },
        reminder: { rgb: '34, 197, 94', icon: Bell },
        focus: { rgb: '245, 158, 11', icon: Target },
        pomodoro: { rgb: '139, 92, 246', icon: Timer },
        warn: { rgb: '239, 68, 68', icon: TriangleAlert },
    };

    const DURATION = 5000; // how long each card lingers
    const MAX = 4; // cap simultaneous cards

    let toasts = $state<Toast[]>([]);
    let seq = 0;
    const seenIds = new Set<string>();
    /** Bottom padding for the stack — lifted clear of the Windows taskbar
     *  (computed on mount; see onMount). */
    let bottomInset = $state(28);

    function remove(id: number) {
        toasts = toasts.filter((t) => t.id !== id);
        // Nothing left → hand the window back so it isn't sitting on top of
        // everything doing nothing.
        if (toasts.length === 0) void invoke('hide_notify_toast_window_command').catch(() => {});
    }

    function push(title: string, body: string, kind: Kind) {
        const id = ++seq;
        toasts = [...toasts, { id, title, body, kind }].slice(-MAX);
        setTimeout(() => remove(id), DURATION);
    }

    /** Transform-only entrance — no opacity ramp, so a transparent overlay
     *  never flickers on first paint (binding overlay rule). */
    function slideIn(_node: Element, { duration = 260 } = {}) {
        return {
            duration,
            css: (t: number) => {
                const eased = 1 - Math.pow(1 - t, 3);
                return `transform: translateX(${(1 - eased) * 28}px)`;
            },
        };
    }

    onMount(() => {
        // Lift the stack clear of the Windows taskbar. This overlay window
        // covers the FULL monitor (incl. the strip behind the taskbar), so a
        // bottom-anchored card sits half-under it. screen.availHeight excludes
        // the taskbar, so (height - availHeight) is its height; pad by that plus
        // a margin. Falls back to a small inset for auto-hidden / side / top
        // taskbars (where the bottom isn't occluded).
        try {
            const tb = window.screen.height - window.screen.availHeight;
            bottomInset = Number.isFinite(tb) && tb > 0 ? tb + 22 : 28;
        } catch {
            bottomInset = 28;
        }

        let unlisten: (() => void) | null = null;
        void (async () => {
            unlisten = await listen<{ id?: string; title?: string; body?: string; kind?: Kind }>(
                'notify-toast-show',
                (e) => {
                    const p = e.payload ?? {};
                    // Dedupe the race-guard re-emit (same id arrives twice).
                    if (p.id) {
                        if (seenIds.has(p.id)) return;
                        seenIds.add(p.id);
                        if (seenIds.size > 64) seenIds.clear();
                    }
                    const kind: Kind = p.kind && p.kind in META ? p.kind : 'info';
                    push(p.title || 'KeepItLocal', p.body || '', kind);
                },
            );
            // Announce we're mounted + listening so notify.ts re-sends the latest
            // payload. Fixes the cold-window race: the immediate emit lands before
            // this listener mounts, and the 140ms re-emit is throttled while the
            // app is minimized to the tray. Mirrors focus-glow's 'glow-ready'.
            await emit('notify-toast-ready');
        })();
        return () => unlisten?.();
    });
</script>

<div class="toast-root" style="padding-bottom: {bottomInset}px" aria-hidden="true">
    {#each toasts as t (t.id)}
        {@const Icon = META[t.kind].icon}
        <div
            class="toast"
            style="--accent: {META[t.kind].rgb}"
            in:slideIn
            out:fade={{ duration: 200 }}
        >
            <span class="toast-chip"><Icon class="toast-chip-ico" /></span>
            <div class="toast-content">
                <div class="toast-title">{t.title}</div>
                {#if t.body}<div class="toast-text">{t.body}</div>{/if}
            </div>
        </div>
    {/each}
</div>

<style>
    :global(html),
    :global(body) {
        background: transparent !important;
        margin: 0;
        height: 100%;
        overflow: hidden;
    }
    .toast-root {
        position: fixed;
        inset: 0;
        pointer-events: none; /* click-through — informational only */
        display: flex;
        flex-direction: column;
        justify-content: flex-end;
        align-items: flex-end;
        gap: 10px;
        padding: 22px 22px 26px;
        /* App's UI font (loaded via the global stylesheet) with native
           fallbacks so the toast reads as part of KeepItLocal. */
        font-family: 'Inter', 'Segoe UI', system-ui, -apple-system, sans-serif;
    }
    .toast {
        display: flex;
        align-items: flex-start;
        gap: 12px;
        width: 350px;
        max-width: calc(100vw - 44px);
        padding: 13px 15px 13px 13px;
        border-radius: 14px;
        /* KeepItLocal dark surface tokens, hard-coded (standalone window). The
           inset accent ring ties the card to its kind without a loud strip. */
        background:
            linear-gradient(
                180deg,
                color-mix(in srgb, rgb(var(--accent)) 6%, #151517),
                #151517 60%
            );
        border: 1px solid #222226;
        box-shadow:
            0 4px 6px rgba(0, 0, 0, 0.3),
            0 14px 32px rgba(0, 0, 0, 0.55),
            inset 0 0 0 1px rgba(var(--accent), 0.1),
            inset 0 1px 0 rgba(255, 255, 255, 0.04);
    }
    /* Tinted icon chip — mirrors the app's card/tool icon treatment. */
    .toast-chip {
        flex: none;
        width: 34px;
        height: 34px;
        display: grid;
        place-items: center;
        border-radius: 10px;
        color: rgb(var(--accent));
        background: rgba(var(--accent), 0.14);
        box-shadow: inset 0 0 0 1px rgba(var(--accent), 0.22);
    }
    .toast :global(.toast-chip-ico) {
        width: 17px;
        height: 17px;
    }
    .toast-content {
        flex: 1;
        min-width: 0;
        padding-top: 1px;
    }
    .toast-title {
        font-size: 13.5px;
        font-weight: 650;
        letter-spacing: -0.005em;
        color: #ededee;
        line-height: 1.3;
    }
    .toast-text {
        margin-top: 3px;
        font-size: 12.5px;
        line-height: 1.4;
        color: #a3a3a3;
        /* Wrap, but don't let a wall of text run forever. */
        display: -webkit-box;
        -webkit-line-clamp: 4;
        line-clamp: 4;
        -webkit-box-orient: vertical;
        overflow: hidden;
    }
</style>
