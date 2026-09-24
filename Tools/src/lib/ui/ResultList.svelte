<script lang="ts">
    /*
      ResultList — list container with built-in empty / loading / error
      handling. Goes on a page that returns a stream of results
      (FileSearch, ClipboardHistory, Snippets list, etc.).

      Pass children (the rows) when you have results. When `loading`,
      a Skeleton-style placeholder shows. When `empty`, the EmptyState
      kit primitive renders. When `error` (a string), an ErrorState
      renders with an optional retry callback.

      One primitive, four states — no more inline `{#if loading}{:else
      if empty}{:else if error}` ladders on every page.

      Optional `groupLabel` renders a small uppercase label above the
      rows, matching the "INSTALLED APPS" / "TOOLS" pattern in the
      overlay.
    */
    import type { Snippet } from 'svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import ErrorState from './ErrorState.svelte';

    interface Props {
        /** Group label rendered above the rows. */
        groupLabel?: string;
        /** When true, show a list-shaped skeleton in place of children. */
        loading?: boolean;
        /** Number of rows to render in the skeleton state. */
        loadingRows?: number;
        /** When true (and not loading), render an EmptyState. */
        empty?: boolean;
        emptyIcon?: any;
        emptyTitle?: string;
        emptyDescription?: string;
        emptyAction?: Snippet;
        /** When set, render an ErrorState with the message. */
        error?: string | null;
        /** Retry callback paired with error. */
        retry?: () => void;
        /** Container variant — `bare` (no padding/bg, default) or
         *  `panel` (card surface). For `panel`, the list IS the
         *  ToolPanel; don't wrap it in another ToolPanel. */
        variant?: 'bare' | 'panel';
        children?: Snippet;
    }

    let {
        groupLabel,
        loading = false,
        loadingRows = 5,
        empty = false,
        emptyIcon,
        emptyTitle = 'Nothing here yet',
        emptyDescription = '',
        emptyAction,
        error,
        retry,
        variant = 'bare',
        children,
    }: Props = $props();
</script>

<div class="rl rl-{variant}">
    {#if groupLabel}
        <div class="rl-group-label">{groupLabel}</div>
    {/if}

    {#if error}
        <ErrorState
            title="Couldn't load results"
            description={error}
            retry={retry}
        />
    {:else if loading}
        <div class="rl-rows" aria-busy="true" aria-live="polite">
            {#each Array.from({ length: loadingRows }) as _, i (i)}
                <div class="rl-skel">
                    <div class="rl-skel-tile"></div>
                    <div class="rl-skel-text">
                        <div class="rl-skel-line"></div>
                        <div class="rl-skel-line rl-skel-line-short"></div>
                    </div>
                </div>
            {/each}
        </div>
    {:else if empty}
        <div class="rl-empty">
            <EmptyState
                icon={emptyIcon}
                title={emptyTitle}
                description={emptyDescription}
                variant="compact"
            >
                {#snippet actions()}
                    {#if emptyAction}{@render emptyAction()}{/if}
                {/snippet}
            </EmptyState>
        </div>
    {:else}
        <div class="rl-rows">
            {@render children?.()}
        </div>
    {/if}
</div>

<style>
    .rl {
        display: flex;
        flex-direction: column;
        min-width: 0;
    }
    .rl-bare {
        background: transparent;
    }
    .rl-panel {
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        padding: 8px;
    }
    .rl-group-label {
        font-size: 10px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
        padding: 12px 8px 6px;
    }
    .rl-group-label:first-child {
        padding-top: 4px;
    }
    .rl-rows {
        display: flex;
        flex-direction: column;
        gap: 1px;
    }

    /* ─── Empty state holder ─────────────────────────────────────
       EmptyState renders centered; we give it generous breathing
       room when it's the only thing on screen. */
    .rl-empty {
        padding: 32px 12px;
    }

    /* ─── Skeleton rows ─────────────────────────────────────────
       Plain-CSS shimmer (no JS, no library). One animation drives
       every line; the staggered fade-in is left to the parent
       crossfade helper. */
    .rl-skel {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 9px 12px;
    }
    .rl-skel-tile {
        flex: none;
        width: 30px;
        height: 30px;
        border-radius: var(--radius-control);
        background: var(--color-panel-2);
        position: relative;
        overflow: hidden;
    }
    .rl-skel-text {
        flex: 1;
        display: flex;
        flex-direction: column;
        gap: 5px;
    }
    .rl-skel-line {
        height: 10px;
        border-radius: 5px;
        background: var(--color-panel-2);
        position: relative;
        overflow: hidden;
    }
    .rl-skel-line-short {
        width: 38%;
    }
    .rl-skel-tile::after,
    .rl-skel-line::after {
        content: '';
        position: absolute;
        inset: 0;
        background: linear-gradient(
            90deg,
            transparent 0%,
            color-mix(in srgb, var(--color-text) 7%, transparent) 50%,
            transparent 100%
        );
        animation: rl-shimmer 1.4s var(--ease-in-out) infinite;
    }
    @keyframes rl-shimmer {
        0% {
            transform: translateX(-100%);
        }
        100% {
            transform: translateX(100%);
        }
    }
</style>
