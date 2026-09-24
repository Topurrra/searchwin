<script lang="ts">
    /*
      ToolPanel — the calm work-surface container. Replaces the loose
      `rounded-2xl border border-border bg-panel` divs scattered across
      every tool page with a single primitive that:
        - picks padding from the 4/8/12/16/24 scale
        - opt-in elevation via tokenized shadow (NOT auto-magic on bg-panel)
        - optional scroll container that engages flex-grow correctly inside
          a ToolPage main column

      Use as the "card" surface inside a tool. Composes with ToolToolbar
      above and ResultList inside.
    */
    import type { Snippet } from 'svelte';

    interface Props {
        /** Padding — kept honest to the 12/16/24 scale. */
        padding?: 'none' | 'sm' | 'md' | 'lg';
        /** Soft elevation shadow. Default: none — panels lean on border. */
        elevated?: boolean;
        /** Make the panel a scroll container that fills its parent's
         *  free vertical space. Useful when the panel hosts a long
         *  result list and the page header should stay pinned. */
        scroll?: boolean;
        /** Tone — bg-panel (default), bg-panel-2 (recessed), or
         *  accent-tinted (for "active / selected" emphasis surfaces). */
        tone?: 'panel' | 'panel-2' | 'accent';
        /** Render as a `<section>` instead of a `<div>` for semantic clarity. */
        as?: 'div' | 'section';
        children?: Snippet;
    }

    let {
        padding = 'md',
        elevated = false,
        scroll = false,
        tone = 'panel',
        as = 'div',
        children,
    }: Props = $props();
</script>

{#if as === 'section'}
    <section
        class="tp pad-{padding} tone-{tone} {elevated ? 'is-elevated' : ''} {scroll ? 'is-scroll' : ''}"
    >
        {@render children?.()}
    </section>
{:else}
    <div
        class="tp pad-{padding} tone-{tone} {elevated ? 'is-elevated' : ''} {scroll ? 'is-scroll' : ''}"
    >
        {@render children?.()}
    </div>
{/if}

<style>
    .tp {
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        color: var(--color-text);
    }
    .tone-panel {
        background: var(--color-panel);
    }
    .tone-panel-2 {
        background: var(--color-panel-2);
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
    .is-scroll {
        flex: 1;
        min-height: 0;
        overflow: auto;
        /* `scrollbar-gutter: stable` keeps content position constant
           when the scrollbar appears — small but premium touch. */
        scrollbar-gutter: stable;
    }
</style>
