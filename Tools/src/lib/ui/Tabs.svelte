<script lang="ts">
    /*
      Tabs — a macOS-style segmented control. `active` is bindable; the
      active segment lifts onto a raised surface. No accent fill — the
      accent is reserved for primary actions (DESIGN.md §3).
    */
    interface Tab {
        id: string;
        label: string;
        /** Optional leading Lucide icon. */
        icon?: any;
    }

    interface Props {
        tabs: Tab[];
        active?: string;
        size?: 'sm' | 'md';
        onChange?: (id: string) => void;
        /** Accessible name for the tablist. */
        ariaLabel?: string;
    }

    let {
        tabs,
        active = $bindable(tabs[0]?.id ?? ''),
        size = 'md',
        onChange,
        ariaLabel,
    }: Props = $props();

    function select(id: string) {
        active = id;
        onChange?.(id);
    }
</script>

<div class="seg seg-{size}" role="tablist" aria-label={ariaLabel}>
    {#each tabs as tab (tab.id)}
        {@const TabIcon = tab.icon}
        <button
            type="button"
            role="tab"
            aria-selected={active === tab.id}
            class="seg-tab {active === tab.id ? 'is-active' : ''}"
            onclick={() => select(tab.id)}
        >
            {#if TabIcon}
                <TabIcon class="seg-ico" />
            {/if}
            <span>{tab.label}</span>
        </button>
    {/each}
</div>

<style>
    .seg {
        display: inline-flex;
        align-items: center;
        gap: 2px;
        padding: 3px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
    }
    .seg-tab {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 6px;
        border: none;
        background: transparent;
        color: var(--color-text-secondary);
        font-weight: 500;
        border-radius: 6px;
        white-space: nowrap;
    }
    .seg-md .seg-tab {
        height: 28px;
        padding: 0 12px;
        font-size: 12.5px;
    }
    .seg-sm .seg-tab {
        height: 24px;
        padding: 0 10px;
        font-size: 12px;
    }
    .seg-tab:hover:not(.is-active) {
        color: var(--color-text);
    }
    .seg-tab.is-active {
        background: var(--color-panel);
        color: var(--color-text);
        box-shadow: var(--shadow-sm);
    }
    /* Declared after .is-active so a focused active tab shows the ring. */
    .seg-tab:focus-visible {
        outline: none;
        box-shadow:
            0 0 0 2px var(--color-bg),
            0 0 0 4px color-mix(in srgb, var(--color-accent) 55%, transparent);
    }
    .seg :global(.seg-ico) {
        width: 14px;
        height: 14px;
    }
</style>
