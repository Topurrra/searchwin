<script lang="ts">
    /*
      ToolToolbar — the action row that sits beneath a ToolPage header.
      Standardised height (40px), gap (12px), and alignment so every
      tool's action strip rhymes visually with the rest of the app.

      Lefts and rights are two snippet slots so the common pattern of
      "search/input on the left, actions on the right" reads as
      structure in the JSX rather than as a stretch of utility
      classes. If you only need one side, omit the other.
    */
    import type { Snippet } from 'svelte';

    interface Props {
        /** Snippet rendered at the start of the row. */
        left?: Snippet;
        /** Snippet rendered at the end of the row, right-aligned. */
        right?: Snippet;
        /** Catch-all snippet for cases where the simple left/right
         *  split doesn't fit. When provided, left/right are ignored. */
        children?: Snippet;
        /** Visual variant.
         *  - bare    : no surface; rests directly on the page bg
         *  - panel   : panel-backed card (border + radius)         */
        variant?: 'bare' | 'panel';
        /** Add a hairline below the toolbar — useful when the toolbar
         *  separates a header from a result area without a card. */
        divider?: boolean;
    }

    let {
        left,
        right,
        children,
        variant = 'bare',
        divider = false,
    }: Props = $props();
</script>

<div class="tt tt-{variant} {divider ? 'has-divider' : ''}">
    {#if children}
        {@render children()}
    {:else}
        {#if left}<div class="tt-left">{@render left()}</div>{/if}
        <div class="tt-spacer"></div>
        {#if right}<div class="tt-right">{@render right()}</div>{/if}
    {/if}
</div>

<style>
    .tt {
        display: flex;
        align-items: center;
        gap: 12px;
        min-height: 40px;
    }
    .tt-bare {
        padding: 0;
    }
    .tt-panel {
        padding: 8px 12px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
    }
    .tt.has-divider {
        padding-bottom: 12px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .tt-left,
    .tt-right {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }
    .tt-left {
        flex: 1;
        min-width: 0;
    }
    .tt-spacer {
        flex: 1;
    }
    .tt-right {
        flex: none;
    }
</style>
