<script lang="ts">
    import { toasts } from '$lib/stores/toasts';
    import { CheckCircle2, XCircle, Info } from '@lucide/svelte';
</script>

<div class="toast-stack">
    {#each $toasts as t, i (t.id)}
        <!-- Status drives a left accent strip (success/error/info) — the
             canonical "tinted edge, neutral surface" pattern — not a fully
             colored border. Phase 7.5 stagger preserved: per-index
             animation-delay (40 ms, capped at 5) so a burst cascades in.
             Slide-up + fade-in, reduced-motion guarded below. -->
        <div
            class="toast toast-{t.level}"
            style="animation-delay: {Math.min(i, 5) * 40}ms"
            role="status"
        >
            {#if t.level === 'success'}
                <CheckCircle2 class="toast-ico" />
            {:else if t.level === 'error'}
                <XCircle class="toast-ico" />
            {:else}
                <Info class="toast-ico" />
            {/if}
            <span class="toast-msg">{t.message}</span>
        </div>
    {/each}
</div>

<style>
    .toast-stack {
        position: fixed;
        bottom: 24px;
        right: 24px;
        z-index: 50;
        display: flex;
        flex-direction: column;
        gap: 8px;
        pointer-events: none;
    }
    .toast {
        pointer-events: auto;
        display: flex;
        align-items: center;
        gap: 10px;
        min-width: 240px;
        max-width: 28rem;
        padding: 12px 14px;
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        /* Left accent strip — neutral by default (info), status-tinted
           for success/error. */
        border-left: 3px solid var(--color-accent);
        box-shadow: var(--shadow-lg);
        color: var(--color-text);
        animation: toast-in 220ms var(--ease-out, cubic-bezier(0.16, 1, 0.3, 1)) both;
    }
    .toast-success {
        border-left-color: var(--color-success);
    }
    .toast-error {
        border-left-color: var(--color-error);
    }
    .toast-info {
        border-left-color: var(--color-accent);
    }
    .toast :global(.toast-ico) {
        width: 16px;
        height: 16px;
        flex: none;
    }
    .toast-success :global(.toast-ico) {
        color: var(--color-success);
    }
    .toast-error :global(.toast-ico) {
        color: var(--color-error);
    }
    .toast-info :global(.toast-ico) {
        color: var(--color-accent);
    }
    .toast-msg {
        font-size: 13px;
        line-height: 1.4;
        color: var(--color-text);
    }
    @keyframes toast-in {
        from {
            opacity: 0;
            transform: translateY(8px);
        }
        to {
            opacity: 1;
            transform: translateY(0);
        }
    }
    @media (prefers-reduced-motion: reduce) {
        .toast {
            animation-duration: 1ms;
        }
    }
</style>
