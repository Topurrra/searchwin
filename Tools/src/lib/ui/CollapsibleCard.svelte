<script lang="ts">
    /*
      CollapsibleCard — a single panel that groups several related rows
      behind one clickable header, so a settings page reads as a few calm
      surfaces instead of a wall of competing cards.

      Header: optional icon · title · optional right-aligned summary (the
      current value, e.g. the active model or hotkey) · chevron. The body
      animates open/closed via the global `.disclose` helper (grid-rows
      0fr↔1fr, no JS height measurement) and is honoured by the global
      reduced-motion guard.

      Body content is whatever you slot in. Use `.field` rows (full-bleed,
      hairline-separated) for label/control pairs, or wrap free-form
      content in `.kc-pad` for consistent padding — both are defined by the
      consuming page (slotted content keeps the parent's CSS scope).

      Usage:
        <CollapsibleCard title="Activation" icon={Keyboard} summary="Ctrl + Alt + V" open>
          <div class="field"> … </div>
          <div class="field"> … </div>
        </CollapsibleCard>
    */
    import { ChevronDown } from '@lucide/svelte';
    import type { Snippet } from 'svelte';

    let {
        title,
        icon = undefined,
        summary = '',
        open = false,
        children,
    }: {
        title: string;
        icon?: any;
        summary?: string;
        open?: boolean;
        children?: Snippet;
    } = $props();

    // The `open` prop only seeds the initial state; the card owns its
    // expanded/collapsed state thereafter (intentional one-time capture).
    // svelte-ignore state_referenced_locally
    let isOpen = $state(open);
    // Stable, unique id so the header button can label its body region.
    const bodyId = `kc-body-${Math.random().toString(36).slice(2, 9)}`;
</script>

<div class="kc-card" class:is-open={isOpen}>
    <button
        type="button"
        class="kc-head"
        aria-expanded={isOpen}
        aria-controls={bodyId}
        onclick={() => (isOpen = !isOpen)}
    >
        {#if icon}
            {@const Icon = icon}
            <Icon class="kc-ico" />
        {/if}
        <span class="kc-title">{title}</span>
        {#if summary}
            <span class="kc-summary" title={summary}>{summary}</span>
        {/if}
        <ChevronDown class="kc-chev" />
    </button>

    <div class="disclose" data-open={isOpen}>
        <div class="disclose-inner">
            <div id={bodyId} class="kc-body">
                {@render children?.()}
            </div>
        </div>
    </div>
</div>

<style>
    .kc-card {
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        margin-bottom: 12px;
        overflow: hidden;
        transition: border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .kc-card:hover {
        border-color: color-mix(in srgb, var(--color-accent) 22%, var(--color-border));
    }
    .kc-card.is-open {
        border-color: color-mix(in srgb, var(--color-accent) 30%, var(--color-border));
    }

    .kc-head {
        display: flex;
        align-items: center;
        gap: 10px;
        width: 100%;
        padding: 14px 16px;
        background: transparent;
        border: none;
        text-align: left;
        color: var(--color-text);
        cursor: pointer;
        font: inherit;
    }
    .kc-head :global(.kc-ico) {
        width: 16px;
        height: 16px;
        flex: none;
        color: var(--color-text-secondary);
        transition: color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .kc-card.is-open .kc-head :global(.kc-ico) {
        color: var(--color-accent);
    }
    .kc-title {
        /* Pushes the summary + chevron to the far right. */
        margin-right: auto;
        font-size: 13.5px;
        font-weight: 600;
        line-height: 1.3;
    }
    .kc-summary {
        max-width: 46%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 12px;
        color: var(--color-muted);
    }
    .kc-head :global(.kc-chev) {
        width: 16px;
        height: 16px;
        flex: none;
        color: var(--color-muted);
        transition: transform 200ms var(--ease-out, ease);
    }
    .kc-card.is-open .kc-head :global(.kc-chev) {
        transform: rotate(180deg);
    }

    .kc-body {
        /* Full-bleed: slotted `.field` rows carry their own padding so
           their hairline dividers span edge to edge. A top border draws
           the single divider between the header and the first row; it
           animates inside the `.disclose` clip on collapse. */
        border-top: 1px solid color-mix(in srgb, var(--color-border) 55%, transparent);
    }
</style>
