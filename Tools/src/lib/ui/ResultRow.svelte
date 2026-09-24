<script lang="ts">
    /*
      ResultRow — Raycast-style result row. The unified row primitive
      for every list-of-things surface (file results, clipboard
      entries, snippets, voice sessions, etc.).

      Anatomy:
        ┌─────────────────────────────────────────────────────────┐
        │  [tile] Title                          [meta] [trailing] │
        │         Subtitle (optional, 1 line, ellipsised)          │
        └─────────────────────────────────────────────────────────┘

      The leading tile is colored by `iconTint` (pack color or
      category color). The right side carries meta (timestamp, size,
      hit count) and a `trailing` snippet for hover-revealed actions.

      Selection state is driven by `selected` — a tinted background
      plus a 3px inset accent strip on the left edge, mirroring the
      sidebar's active-item pattern.

      Pass `onclick` to make the row a focusable button; otherwise it
      renders as a plain row (useful for read-only lists).
    */
    import type { Snippet } from 'svelte';

    interface Props {
        icon?: any;
        /** CSS color value used as the icon-tile tint. Defaults to a
         *  neutral panel-2 background. */
        iconTint?: string;
        title: string;
        subtitle?: string;
        /** Short right-aligned meta string. Time / size / hit count. */
        meta?: string;
        selected?: boolean;
        /** Hover-revealed actions. Rendered to the right of meta. */
        trailing?: Snippet;
        onclick?: () => void;
        /** Allow a row to be focused/activated by keyboard even
         *  without onclick (e.g. when a parent handles selection). */
        focusable?: boolean;
    }

    let {
        icon: Icon,
        iconTint,
        title,
        subtitle,
        meta,
        selected = false,
        trailing,
        onclick,
        focusable = false,
    }: Props = $props();
</script>

{#snippet body()}
    {#if Icon}
        <span
            class="rr-tile"
            style={iconTint
                ? `background: color-mix(in srgb, ${iconTint} 16%, transparent); color: ${iconTint};`
                : ''}
            aria-hidden="true"
        >
            <Icon class="rr-icon" />
        </span>
    {/if}
    <div class="rr-text">
        <span class="rr-title">{title}</span>
        {#if subtitle}
            <span class="rr-sub">{subtitle}</span>
        {/if}
    </div>
    {#if meta}
        <span class="rr-meta">{meta}</span>
    {/if}
    {#if trailing}
        <span class="rr-trail">{@render trailing()}</span>
    {/if}
{/snippet}

{#if onclick}
    <button
        type="button"
        class="rr is-interactive {selected ? 'is-selected' : ''}"
        {onclick}
    >
        {@render body()}
    </button>
{:else if focusable}
    <div
        class="rr is-interactive {selected ? 'is-selected' : ''}"
        role="option"
        tabindex="0"
        aria-selected={selected}
    >
        {@render body()}
    </div>
{:else}
    <div class="rr {selected ? 'is-selected' : ''}">
        {@render body()}
    </div>
{/if}

<style>
    .rr {
        /* `position: relative` is required for the `.is-selected::before`
           pill indicator (drawn below) to absolute-position against
           the row, not against an ancestor. */
        position: relative;
        display: flex;
        align-items: center;
        gap: 12px;
        width: 100%;
        padding: 9px 12px;
        border: 1px solid transparent;
        border-radius: var(--radius-control);
        background: transparent;
        color: var(--color-text);
        text-align: left;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out);
    }
    .is-interactive {
        cursor: pointer;
    }
    .is-interactive:hover:not(.is-selected) {
        background: color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .is-interactive:hover .rr-trail {
        opacity: 1;
    }
    /* Selected state — canonical pattern (see
       feedback_selected_item_pattern.md): neutral `panel-2` background,
       accent pill strip inset top/bottom via ::before, accent stays
       reserved for the indicators (strip + icon tile), surface stays
       neutral. */
    .rr.is-selected {
        background: var(--color-panel-2);
    }
    .rr.is-selected::before {
        content: '';
        position: absolute;
        left: 3px;
        top: 8px;
        bottom: 8px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }

    .rr-tile {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 30px;
        height: 30px;
        border-radius: var(--radius-control);
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
    }
    .rr :global(.rr-icon) {
        width: 16px;
        height: 16px;
    }
    .rr-text {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 1px;
    }
    .rr-title {
        font-size: 13.5px;
        font-weight: 500;
        line-height: 1.25;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        letter-spacing: -0.005em;
    }
    .rr-sub {
        font-size: 11.5px;
        line-height: 1.35;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .rr-meta {
        flex: none;
        font-size: 11.5px;
        color: var(--color-muted);
        font-variant-numeric: tabular-nums;
        white-space: nowrap;
    }
    /* Trailing actions stay invisible at rest, fade in on hover.
       The Raycast trick — keeps the row calm; only reveal-on-intent. */
    .rr-trail {
        flex: none;
        display: flex;
        align-items: center;
        gap: 6px;
        opacity: 0;
        transition: opacity var(--dur-micro) var(--ease-out);
    }
    .rr.is-selected .rr-trail {
        opacity: 1;
    }
</style>
