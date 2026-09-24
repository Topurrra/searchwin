<script lang="ts">
    /*
      ListRow — one row of a list. Optional leading icon tile, a
      title/subtitle stack, and a `trailing` snippet for meta or actions.
      Pass `onclick` to make the row a focusable button.
    */
    import type { Snippet } from 'svelte';

    interface Props {
        /** Leading Lucide icon component. */
        icon?: any;
        title: string;
        subtitle?: string;
        selected?: boolean;
        onclick?: () => void;
        /** Right-aligned content — meta text, badges, action buttons. */
        trailing?: Snippet;
    }

    let {
        icon: Icon,
        title,
        subtitle,
        selected = false,
        onclick,
        trailing,
    }: Props = $props();
</script>

{#snippet body()}
    {#if Icon}
        <span class="row-tile" aria-hidden="true">
            <Icon class="row-ico" />
        </span>
    {/if}
    <span class="row-text">
        <span class="row-title">{title}</span>
        {#if subtitle}
            <span class="row-sub">{subtitle}</span>
        {/if}
    </span>
    {#if trailing}
        <span class="row-trail">{@render trailing()}</span>
    {/if}
{/snippet}

{#if onclick}
    <button
        type="button"
        class="row is-interactive {selected ? 'is-selected' : ''}"
        {onclick}
    >
        {@render body()}
    </button>
{:else}
    <div class="row {selected ? 'is-selected' : ''}">
        {@render body()}
    </div>
{/if}

<style>
    .row {
        display: flex;
        align-items: center;
        gap: 10px;
        width: 100%;
        padding: 8px 10px;
        border: none;
        border-radius: var(--radius-control);
        background: transparent;
        color: var(--color-text);
        text-align: left;
    }
    .is-interactive {
        cursor: pointer;
    }
    .is-interactive:hover {
        background: color-mix(in srgb, var(--color-text) 6%, transparent);
    }
    .row.is-selected {
        background: var(--color-accent-soft);
    }
    .row-tile {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 28px;
        height: 28px;
        border-radius: 7px;
        background: var(--color-panel-2);
        color: var(--color-text-secondary);
    }
    .row.is-selected .row-tile {
        color: var(--color-accent);
    }
    .row :global(.row-ico) {
        width: 15px;
        height: 15px;
    }
    .row-text {
        display: flex;
        flex-direction: column;
        gap: 1px;
        min-width: 0;
        flex: 1;
    }
    .row-title {
        font-size: 13px;
        font-weight: 500;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .row-sub {
        font-size: 11.5px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .row-trail {
        flex: none;
        display: flex;
        align-items: center;
        gap: 8px;
    }
</style>
