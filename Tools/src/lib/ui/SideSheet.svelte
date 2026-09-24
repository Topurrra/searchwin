<script lang="ts">
    /*
      SideSheet — slide-in right-side panel for tool-local settings.
      Replaces the "inline collapsible settings panel" pattern that
      makes pages bloat vertically (ClipboardHistory does this today).

      Behaviour:
        - Slides in from the right at 420px width (configurable).
        - Backdrop scrim dims the page; click-outside closes.
        - Esc closes.
        - Translate3d-only entrance (DesignPro §motion rule 6 — never
          opacity-fade a slide). Scrim is allowed an opacity fade since
          it's a backdrop, not the slide.
        - `prefers-reduced-motion` collapses transitions to ~1ms
          via the global guard in styles.css.

      Caller owns the `open` state. Wire close in two places: the
      `onclose` callback (fired by Esc / backdrop / close button) and
      whatever opens it should set `open = false` when done.

      The sheet is rendered next to where it's used in the markup;
      a portal isn't needed since `position: fixed` already escapes
      its parent's clipping.
    */
    import type { Snippet } from 'svelte';
    import { onMount, onDestroy } from 'svelte';
    import { X } from '@lucide/svelte';

    interface Props {
        open: boolean;
        title?: string;
        /** Width in px. Default 420. */
        width?: number;
        /** Fired by Esc, scrim click, or the close button. */
        onclose?: () => void;
        children?: Snippet;
        footer?: Snippet;
    }

    let { open, title = '', width = 420, onclose, children, footer }: Props = $props();

    function handleKey(event: KeyboardEvent) {
        if (event.key === 'Escape' && open) {
            event.preventDefault();
            onclose?.();
        }
    }

    onMount(() => {
        document.addEventListener('keydown', handleKey);
    });
    onDestroy(() => {
        document.removeEventListener('keydown', handleKey);
    });

    function onScrimClick() {
        onclose?.();
    }
</script>

<!-- Scrim sits at a lower z than the sheet so click-outside is a
     single layer.  pointer-events toggled by `open` so a closed
     sheet doesn't intercept anything. -->
<div
    class="ss-scrim"
    class:is-open={open}
    onclick={onScrimClick}
    role="presentation"
></div>

<aside
    class="ss-panel"
    class:is-open={open}
    style="--ss-width: {width}px;"
    aria-hidden={!open}
>
    {#if title}
        <header class="ss-head">
            <h2 class="ss-title">{title}</h2>
            <button
                type="button"
                class="ss-close"
                onclick={() => onclose?.()}
                aria-label="Close"
                title="Close (Esc)"
            >
                <X class="ss-close-ico" />
            </button>
        </header>
    {/if}

    <div class="ss-body">
        {@render children?.()}
    </div>

    {#if footer}
        <footer class="ss-footer">{@render footer()}</footer>
    {/if}
</aside>

<style>
    .ss-scrim {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.42);
        opacity: 0;
        pointer-events: none;
        z-index: 90;
        transition: opacity 220ms var(--ease-out);
    }
    .ss-scrim.is-open {
        opacity: 1;
        pointer-events: auto;
    }

    .ss-panel {
        position: fixed;
        top: 0;
        right: 0;
        height: 100vh;
        width: var(--ss-width, 420px);
        max-width: 92vw;
        background: var(--color-panel);
        border-left: 1px solid var(--color-border);
        box-shadow: var(--shadow-lg);
        display: flex;
        flex-direction: column;
        /* translate3d-only entrance — DesignPro §6 motion rule 6 */
        transform: translate3d(100%, 0, 0);
        transition: transform 380ms var(--ease-out);
        z-index: 91;
        /* Prevent overscroll chain — sheet content scrolling
           shouldn't cascade to the parent page. */
        overscroll-behavior: contain;
    }
    .ss-panel.is-open {
        transform: translate3d(0, 0, 0);
    }

    .ss-head {
        flex: none;
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 16px 18px;
        border-bottom: 1px solid var(--color-border);
    }
    .ss-title {
        margin: 0;
        font-size: 14.5px;
        font-weight: 600;
        color: var(--color-text);
        letter-spacing: -0.006em;
    }
    .ss-close {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 30px;
        height: 30px;
        background: transparent;
        border: 1px solid transparent;
        border-radius: var(--radius-control);
        color: var(--color-text-secondary);
        cursor: pointer;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .ss-close:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .ss-panel :global(.ss-close-ico) {
        width: 15px;
        height: 15px;
    }

    .ss-body {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        padding: 16px 18px;
        scrollbar-gutter: stable;
    }

    .ss-footer {
        flex: none;
        padding: 12px 18px;
        border-top: 1px solid var(--color-border);
        background: var(--color-panel-2);
        display: flex;
        align-items: center;
        justify-content: flex-end;
        gap: 8px;
    }
</style>
