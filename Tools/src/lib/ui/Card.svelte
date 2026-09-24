<script lang="ts">
    /*
      Card — a content surface. Depth comes from a near-invisible border
      plus an optional soft elevation shadow, never heavy outlines
      (DESIGN.md §6). Pass `onclick` to make it a real, focusable button.
    */
    import type { Snippet } from 'svelte';
    import type { HTMLAttributes } from 'svelte/elements';

    interface Props extends HTMLAttributes<HTMLElement> {
        padding?: 'none' | 'sm' | 'md' | 'lg';
        /** Soft elevation shadow — for surfaces that float above the page. */
        elevated?: boolean;
        children?: Snippet;
    }

    let {
        padding = 'md',
        elevated = false,
        onclick,
        class: className = '',
        children,
        ...rest
    }: Props = $props();
</script>

{#if onclick}
    <button
        type="button"
        class="card pad-{padding} is-clickable {elevated ? 'is-elevated' : ''} {className}"
        {onclick}
        {...rest}
    >
        {@render children?.()}
    </button>
{:else}
    <div
        class="card pad-{padding} {elevated ? 'is-elevated' : ''} {className}"
        {...rest}
    >
        {@render children?.()}
    </div>
{/if}

<style>
    .card {
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        color: var(--color-text);
        text-align: left;
    }
    .pad-none {
        padding: 0;
    }
    .pad-sm {
        padding: 12px;
    }
    .pad-md {
        padding: 16px;
    }
    .pad-lg {
        padding: 24px;
    }
    .is-elevated {
        box-shadow: var(--shadow-md);
    }

    .is-clickable {
        display: block;
        width: 100%;
        cursor: pointer;
    }
    .is-clickable:hover {
        border-color: var(--color-border-strong);
        box-shadow: var(--shadow-lg);
        transform: translateY(-1px);
    }
    .is-clickable:active {
        transform: translateY(0);
    }
    .is-clickable:focus-visible {
        outline: none;
        box-shadow:
            0 0 0 2px var(--color-bg),
            0 0 0 4px color-mix(in srgb, var(--color-accent) 55%, transparent);
    }
</style>
