<script lang="ts">
    /*
      Standardized empty-state component for tools.

      Use this whenever a tool has nothing to show yet — no files dropped,
      no results, no items in a list. The shape is always the same:
        [icon] [title] [description] [optional CTA button(s)]

      Why standardize: empty states are where new users decide whether to
      use a tool. A blank `bg-panel` with no copy reads as "this is broken
      or hidden". A consistent empty state with a clear CTA reads as
      "here's what to do next".

      Three variants control the visual weight:
        - hero:    big icon, big title, generous padding (use for the
                   primary surface of a tool that has no data yet)
        - compact: small icon, single line of copy (use inside a list
                   that's filterable — "no matches for X")
        - dashed:  dashed-border drop zone style (use for "drop files
                   here" type empty states; pairs naturally with the
                   actual drag-drop handler)
    */
    import type { Snippet } from 'svelte';

    let {
        icon: IconCmp,
        title,
        description = '',
        variant = 'hero',
        actions,
    }: {
        icon: any;
        title: string;
        description?: string;
        variant?: 'hero' | 'compact' | 'dashed';
        actions?: Snippet;
    } = $props();
</script>

<div class="empty empty-{variant}" role="status">
    <div class="icon-tile" aria-hidden="true">
        <IconCmp class="empty-icon" />
    </div>
    <div class="text">
        <div class="title">{title}</div>
        {#if description}
            <div class="description">{description}</div>
        {/if}
    </div>
    {#if actions}
        <div class="actions">
            {@render actions()}
        </div>
    {/if}
</div>

<style>
    .empty {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        text-align: center;
        gap: 12px;
    }

    .empty-hero {
        padding: 40px 24px;
        border-radius: var(--radius-control);
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        gap: 14px;
    }

    .empty-compact {
        padding: 18px 16px;
        gap: 8px;
    }

    .empty-dashed {
        padding: 32px 24px;
        border-radius: var(--radius-control);
        border: 1.5px dashed var(--color-border);
        background: transparent;
        gap: 12px;
    }

    .icon-tile {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: 0 0 auto;
        color: var(--color-text-secondary);
    }

    .empty-hero .icon-tile {
        width: 28px;
        height: 28px;
    }
    .empty-hero :global(.empty-icon) {
        width: 26px;
        height: 26px;
    }

    .empty-compact .icon-tile {
        width: 20px;
        height: 20px;
    }
    .empty-compact :global(.empty-icon) {
        width: 18px;
        height: 18px;
    }

    .empty-dashed .icon-tile {
        width: 24px;
        height: 24px;
    }
    .empty-dashed :global(.empty-icon) {
        width: 22px;
        height: 22px;
    }

    .text {
        max-width: 480px;
    }

    .title {
        font-size: 14px;
        font-weight: 600;
        color: var(--color-text);
        line-height: 1.4;
    }
    .empty-hero .title {
        font-size: 16px;
    }

    .description {
        margin-top: 4px;
        font-size: 12.5px;
        line-height: 1.5;
        color: var(--color-muted);
    }
    .empty-compact .description {
        font-size: 11.5px;
        margin-top: 2px;
    }

    .actions {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        justify-content: center;
        gap: 8px;
        margin-top: 4px;
    }
</style>
