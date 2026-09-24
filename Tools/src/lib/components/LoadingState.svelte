<script lang="ts">
    /*
      Standardized loading-state component used across all tools.

      Three variants:
        - inline:  small spinner + label, fits in a row (used inside cards
                   when one part of the UI is loading)
        - block:   centered spinner + label inside a panel (used when the
                   whole tool body is loading)
        - overlay: absolute-positioned over a card with a backdrop, used
                   when an in-place operation runs (button-press → working)
                   without unmounting the surrounding controls

      Intentionally minimal API — `variant`, `label`, `subLabel`. New
      callers should ALWAYS use this instead of rolling their own
      spinner. Standardization is the whole point.
    */
    import { Loader2 } from '@lucide/svelte';
    import { _ } from 'svelte-i18n';

    let {
        variant = 'block',
        label = undefined,
        subLabel = null,
    }: {
        variant?: 'inline' | 'block' | 'overlay';
        label?: string;
        subLabel?: string | null;
    } = $props();

    /** When no explicit label is passed, fall back to the localized
     * "Working…" string. Resolved reactively so it follows locale. */
    let resolvedLabel = $derived(label ?? $_('errors.working'));
</script>

{#if variant === 'inline'}
    <span class="inline">
        <Loader2 class="ls-spinner ls-inline" />
        <span>{resolvedLabel}</span>
    </span>
{:else if variant === 'overlay'}
    <div class="overlay" role="status" aria-live="polite">
        <div class="overlay-card">
            <Loader2 class="ls-spinner ls-overlay" />
            <div class="text">
                <div class="label">{resolvedLabel}</div>
                {#if subLabel}<div class="sub">{subLabel}</div>{/if}
            </div>
        </div>
    </div>
{:else}
    <div class="block" role="status" aria-live="polite">
        <Loader2 class="ls-spinner ls-block" />
        <div class="text">
            <div class="label">{resolvedLabel}</div>
            {#if subLabel}<div class="sub">{subLabel}</div>{/if}
        </div>
    </div>
{/if}

<style>
    /* ─── Spinner sizing per variant ─────────────────────────────── */
    :global(.ls-spinner) {
        animation: ls-spin 800ms linear infinite;
        color: var(--color-accent);
    }
    :global(.ls-inline) {
        width: 14px;
        height: 14px;
    }
    :global(.ls-block) {
        width: 28px;
        height: 28px;
    }
    :global(.ls-overlay) {
        width: 24px;
        height: 24px;
    }

    @keyframes ls-spin {
        to {
            transform: rotate(360deg);
        }
    }

    @media (prefers-reduced-motion: reduce) {
        :global(.ls-spinner) {
            animation: none;
        }
    }
    /* ─── Inline ──────────────────────────────────────────────────── */
    .inline {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-size: 12px;
        color: var(--color-text-secondary);
    }

    /* ─── Block (centered in a card) ──────────────────────────────── */
    .block {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 12px;
        padding: 32px 16px;
        text-align: center;
    }
    .block .label {
        font-size: 13px;
        font-weight: 500;
        color: var(--color-text);
    }
    .block .sub {
        margin-top: 4px;
        font-size: 11.5px;
        color: var(--color-muted);
    }

    /* ─── Overlay (covers a card while operation runs) ────────────── */
    .overlay {
        position: absolute;
        inset: 0;
        z-index: 20;
        display: flex;
        align-items: center;
        justify-content: center;
        background: color-mix(in srgb, var(--color-bg) 88%, transparent);
        border-radius: inherit;
    }
    .overlay-card {
        display: inline-flex;
        align-items: center;
        gap: 10px;
        padding: 10px 16px;
        border-radius: var(--radius-control);
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
    }
    .overlay-card .label {
        font-size: 13px;
        font-weight: 500;
        color: var(--color-text);
        line-height: 1.3;
    }
    .overlay-card .sub {
        font-size: 11px;
        color: var(--color-muted);
        margin-top: 2px;
    }

    .text {
        text-align: center;
    }
    .overlay-card .text {
        text-align: left;
    }
</style>
