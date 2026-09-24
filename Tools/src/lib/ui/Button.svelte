<script lang="ts">
    /*
      Button — the one button for the whole app. macOS-grade: flat fills,
      fast ease-out motion, the accent reserved for `primary` (one primary
      action per view — DESIGN.md §3 / §10).

      Variants:
        primary    — the single accent action of a view
        secondary  — the default; a filled bezel with a hairline border
        ghost      — chrome-quiet; no fill until hover
        danger     — destructive confirmation
    */
    import type { Snippet } from 'svelte';
    import type { HTMLButtonAttributes } from 'svelte/elements';
    import { Loader2 } from '@lucide/svelte';

    interface Props extends HTMLButtonAttributes {
        variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
        size?: 'sm' | 'md';
        loading?: boolean;
        /** Leading icon — a Lucide component. */
        icon?: any;
        /** Trailing icon — a Lucide component. */
        iconTrailing?: any;
        /** Square, label-less button. Pass an `icon` and a `title`. */
        iconOnly?: boolean;
        /** Stretch to the container's width. */
        full?: boolean;
        children?: Snippet;
    }

    let {
        variant = 'secondary',
        size = 'md',
        loading = false,
        disabled = false,
        type = 'button',
        icon: Icon,
        iconTrailing: IconTrailing,
        iconOnly = false,
        full = false,
        children,
        class: className = '',
        ...rest
    }: Props = $props();
</script>

<button
    {type}
    class="btn btn-{variant} btn-{size} {full ? 'is-full' : ''} {iconOnly ? 'is-icon' : ''} {className}"
    disabled={disabled || loading}
    aria-busy={loading}
    {...rest}
>
    {#if loading}
        <Loader2 class="btn-ico btn-spin" />
    {:else if Icon}
        <Icon class="btn-ico" />
    {/if}
    {#if children && !iconOnly}
        <span class="btn-label">{@render children()}</span>
    {/if}
    {#if IconTrailing && !iconOnly && !loading}
        <IconTrailing class="btn-ico" />
    {/if}
</button>

<style>
    .btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 7px;
        border: 1px solid transparent;
        border-radius: var(--radius-control);
        font-weight: 500;
        letter-spacing: -0.006em;
        white-space: nowrap;
        user-select: none;
    }
    .btn:disabled {
        cursor: not-allowed;
    }
    /* A `loading` button stays full-opacity (it reads as "working"); a
       genuinely disabled one dims. aria-busy distinguishes the two. */
    .btn:disabled:not([aria-busy='true']) {
        opacity: 0.45;
    }

    /* Sizes */
    .btn-md {
        height: 34px;
        padding: 0 14px;
        font-size: 13px;
    }
    .btn-sm {
        height: 28px;
        padding: 0 10px;
        font-size: 12px;
        gap: 6px;
    }
    .btn.is-full {
        width: 100%;
    }
    .btn.is-icon {
        padding: 0;
    }
    .btn.is-icon.btn-md {
        width: 34px;
    }
    .btn.is-icon.btn-sm {
        width: 28px;
    }

    /* Variants */
    .btn-primary {
        background: var(--color-accent);
        color: var(--color-accent-contrast);
    }
    .btn-primary:not(:disabled):hover {
        background: var(--color-accent-hover);
    }

    .btn-secondary {
        background: var(--color-panel-2);
        color: var(--color-text);
        border-color: var(--color-border);
    }
    .btn-secondary:not(:disabled):hover {
        background: var(--color-panel-3);
        border-color: var(--color-border-strong);
    }

    .btn-ghost {
        background: transparent;
        color: var(--color-text-secondary);
    }
    .btn-ghost:not(:disabled):hover {
        background: color-mix(in srgb, var(--color-text) 8%, transparent);
        color: var(--color-text);
    }

    .btn-danger {
        background: var(--color-error);
        /* The error surface is a light rose in the dark theme — white text
           on it fails contrast. accent-contrast is the per-theme readable
           ink for vivid surfaces (dark ink on light surfaces, light on dark). */
        color: var(--color-accent-contrast);
    }
    .btn-danger:not(:disabled):hover {
        filter: brightness(1.08);
    }

    .btn-label {
        line-height: 1;
    }

    /* Icons render inside the Lucide child's <svg> — size via :global. */
    .btn-md :global(.btn-ico) {
        width: 15px;
        height: 15px;
    }
    .btn-sm :global(.btn-ico) {
        width: 14px;
        height: 14px;
    }
    :global(.btn-spin) {
        animation: btn-spin 700ms linear infinite;
    }
    @keyframes btn-spin {
        to {
            transform: rotate(360deg);
        }
    }
</style>
