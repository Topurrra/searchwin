<script lang="ts">
    /*
      SectionHeader — a calm, quiet title row. The deliberate replacement
      for the radial-gradient "hero" blocks (DESIGN.md §8). Optional
      leading icon tile, optional right-aligned actions, optional divider.
    */
    import type { Snippet } from 'svelte';

    interface Props {
        title: string;
        description?: string;
        /** Lucide icon component — shown in a small accent-tinted tile. */
        icon?: any;
        /** Page-level (18px) vs section-level (16px) heading. */
        size?: 'page' | 'section';
        /** Hairline separator beneath the header. */
        divider?: boolean;
        /** Right-aligned actions, typically buttons. */
        actions?: Snippet;
    }

    let {
        title,
        description = '',
        icon: Icon,
        size = 'section',
        divider = false,
        actions,
    }: Props = $props();
</script>

<div class="sh sh-{size} {divider ? 'has-divider' : ''}">
    {#if Icon}
        <span class="sh-tile" aria-hidden="true">
            <Icon class="sh-ico" />
        </span>
    {/if}
    <div class="sh-text">
        <h2 class="sh-title">{title}</h2>
        {#if description}
            <p class="sh-desc">{description}</p>
        {/if}
    </div>
    {#if actions}
        <div class="sh-actions">{@render actions()}</div>
    {/if}
</div>

<style>
    .sh {
        display: flex;
        align-items: center;
        gap: 12px;
    }
    .sh.has-divider {
        padding-bottom: 16px;
        border-bottom: 1px solid var(--color-border);
    }
    .sh-tile {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 32px;
        height: 32px;
        border-radius: var(--radius-control);
        background: var(--color-accent-soft);
        color: var(--color-accent);
    }
    .sh-tile :global(.sh-ico) {
        width: 17px;
        height: 17px;
    }
    .sh-text {
        flex: 1;
        min-width: 0;
    }
    .sh-title {
        margin: 0;
        font-weight: 600;
        color: var(--color-text);
    }
    .sh-section .sh-title {
        font-size: 16px;
    }
    .sh-page .sh-title {
        font-size: 18px;
    }
    .sh-desc {
        margin: 2px 0 0;
        font-size: 13px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }
    .sh-actions {
        flex: none;
        display: flex;
        align-items: center;
        gap: 8px;
    }
</style>
