<script lang="ts">
    /*
      Kbd — keyboard key caps. Accepts a single key or a combo; a string
      is split on "+". Caps render side by side, no "+" glyph.
    */
    interface Props {
        keys: string | string[];
    }

    let { keys }: Props = $props();

    let parts = $derived(
        Array.isArray(keys)
            ? keys
            : keys
                  .split('+')
                  .map((k) => k.trim())
                  .filter(Boolean),
    );
</script>

<span class="kbd-group">
    {#each parts as part}
        <kbd class="kbd">{part}</kbd>
    {/each}
</span>

<style>
    .kbd-group {
        display: inline-flex;
        align-items: center;
        gap: 3px;
    }
    .kbd {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        min-width: 18px;
        height: 18px;
        padding: 0 5px;
        font-family: inherit;
        font-size: 11px;
        font-weight: 500;
        line-height: 1;
        color: var(--color-text-secondary);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-bottom-width: 2px;
        border-radius: 5px;
    }
</style>
