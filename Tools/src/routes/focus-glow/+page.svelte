<script lang="ts">
    /*
      Focus glow overlay — a transparent, click-through, full-screen window
      (configured in Rust) that flashes a screen-edge glow. Two uses, two
      colors, picked by the `glow-flash` event payload:
        • ORANGE — a blocked app was opened during a Focus session.
        • GREEN  — a reminder fired (alongside the green toast).

      Smoothness: the glow fades IN, holds with a gentle breathe, then fades
      OUT — paced to the `durationMs` from the event, so the Rust-side hide
      lands while the glow is already invisible (no abrupt on/off). We animate
      the SHADOW STRENGTH (`--gs`, via @property) rather than the element's
      opacity, so the element itself always mounts visible — that keeps us
      compliant with the binding overlay rule (no opacity-0 panel reveal, which
      flickers on transparent WebView2 windows) while still fading smoothly.

      Each `glow-flash` bumps `flashId`, and {#key flashId} remounts the layer
      so the CSS animation restarts cleanly on every flash (incl. the focus
      poll re-firing while a blocked app stays focused).
    */
    import { onMount } from 'svelte';
    import { listen, emit } from '@tauri-apps/api/event';

    type GlowColor = 'orange' | 'green';
    const RGB: Record<GlowColor, string> = {
        orange: '245, 158, 11',
        green: '34, 197, 94',
    };

    let color = $state<GlowColor>('orange');
    let durationMs = $state(1500);
    let flashId = $state(0);
    const rgb = $derived(RGB[color]);

    onMount(() => {
        let unlisten: (() => void) | null = null;
        void (async () => {
            unlisten = await listen<{ color?: GlowColor; durationMs?: number }>(
                'glow-flash',
                (e) => {
                    const c = e.payload?.color;
                    if (c === 'orange' || c === 'green') color = c;
                    const d = e.payload?.durationMs;
                    if (typeof d === 'number' && d > 200) durationMs = d;
                    flashId += 1; // restart the fade animation
                },
            );
            // Announce we're mounted + listening so the controller (glow.ts)
            // (re)sends the intended color/duration — fixes the first-creation
            // race that made a green reminder flash show orange.
            await emit('glow-ready');
        })();
        return () => unlisten?.();
    });
</script>

{#key flashId}
    <div
        class="glow"
        style="--glow-rgb: {rgb}; --glow-dur: {durationMs}ms"
        aria-hidden="true"
    ></div>
{/key}

<style>
    :global(html),
    :global(body) {
        background: transparent !important;
        margin: 0;
        height: 100%;
        overflow: hidden;
    }

    /* Animatable shadow strength (0 = no glow, 1 = full). Registering it with
       @property lets it be tweened inside the rgba() alpha below. */
    @property --gs {
        syntax: '<number>';
        inherits: false;
        initial-value: 0;
    }

    .glow {
        position: fixed;
        inset: 0;
        pointer-events: none;
        /* The element is ALWAYS opacity 1 (mounts visible — no flicker). The
           glow itself fades via --gs scaling the inset shadow's alpha, so at
           rest (--gs:0) nothing is drawn. */
        --gs: 0;
        box-shadow:
            inset 0 0 90px 18px rgba(var(--glow-rgb), calc(0.5 * var(--gs))),
            inset 0 0 220px 70px rgba(var(--glow-rgb), calc(0.22 * var(--gs)));
        animation: glow-life var(--glow-dur, 1500ms) ease-in-out both;
    }

    /* Smooth swell: fade in → gentle breathe → fade out, scaled to --glow-dur. */
    @keyframes glow-life {
        0% {
            --gs: 0;
        }
        14% {
            --gs: 1;
        }
        50% {
            --gs: 0.8;
        }
        86% {
            --gs: 1;
        }
        100% {
            --gs: 0;
        }
    }
</style>
