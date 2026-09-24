<!--
  Pager — the canonical pagination control for the kit. Drop it under any
  growable list/table and slice the data with the bindable `page`:

      <script>
        let page = $state(1);
        const PAGE = 25;
        const shown = $derived(items.slice((page - 1) * PAGE, page * PAGE));
      </script>
      {#each shown as item}…{/each}
      <Pager total={items.length} bind:page pageSize={PAGE} label="commands" />

  Renders nothing when everything fits on one page, so it's safe to leave
  mounted on short lists. Clamps `page` back into range when `total`/`pageSize`
  change (e.g. after filtering), so callers don't have to reset it themselves.
-->
<script lang="ts">
    import { ChevronLeft, ChevronRight } from '@lucide/svelte';

    let {
        total,
        page = $bindable(1),
        pageSize = 25,
        label = 'items',
    }: {
        total: number;
        page?: number;
        pageSize?: number;
        label?: string;
    } = $props();

    const pageCount = $derived(Math.max(1, Math.ceil(total / Math.max(1, pageSize))));
    const from = $derived(total === 0 ? 0 : (page - 1) * pageSize + 1);
    const to = $derived(Math.min(total, page * pageSize));

    // Keep `page` valid when the dataset shrinks under it (filtering, deletes).
    $effect(() => {
        if (page > pageCount) page = pageCount;
        else if (page < 1) page = 1;
    });

    function prev() {
        if (page > 1) page -= 1;
    }
    function next() {
        if (page < pageCount) page += 1;
    }
</script>

{#if pageCount > 1}
    <div class="pager">
        <span class="pager-range">{from}–{to} of {total} {label}</span>
        <div class="pager-controls">
            <button
                class="pager-btn"
                type="button"
                onclick={prev}
                disabled={page <= 1}
                aria-label="Previous page"
            >
                <ChevronLeft class="pager-ico" />
            </button>
            <span class="pager-page">{page} / {pageCount}</span>
            <button
                class="pager-btn"
                type="button"
                onclick={next}
                disabled={page >= pageCount}
                aria-label="Next page"
            >
                <ChevronRight class="pager-ico" />
            </button>
        </div>
    </div>
{/if}

<style>
    .pager {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        flex-wrap: wrap;
        padding: 0.5rem 0.25rem 0.25rem;
    }
    .pager-range {
        font-size: 0.78rem;
        color: var(--color-muted);
    }
    .pager-controls {
        display: flex;
        align-items: center;
        gap: 0.4rem;
    }
    .pager-page {
        min-width: 3.5rem;
        text-align: center;
        font-size: 0.78rem;
        color: var(--color-text);
        font-variant-numeric: tabular-nums;
    }
    .pager-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.9rem;
        height: 1.9rem;
        border-radius: 0.5rem;
        border: 1px solid var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text);
        cursor: pointer;
        transition:
            border-color 0.12s ease,
            background 0.12s ease;
    }
    .pager-btn:hover:not(:disabled) {
        border-color: var(--color-accent);
    }
    .pager-btn:disabled {
        opacity: 0.4;
        cursor: default;
    }
    .pager-btn :global(.pager-ico) {
        width: 1rem;
        height: 1rem;
    }
</style>
