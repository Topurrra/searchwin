<script lang="ts">
    /*
      ErrorState — inline error surface (DESIGN.md §10: errors are
      inline, never an OS alert). Soft error-toned card with a title,
      optional description, and an optional retry CTA.

      Use inside a result/list area when an async task failed; the
      surrounding chrome stays intact so the user can recover without
      losing context. For full-page render-time crashes use the
      ToolErrorBoundary instead.
    */
    import type { Snippet } from 'svelte';
    import { AlertTriangle, RotateCcw } from '@lucide/svelte';

    interface Props {
        title?: string;
        description?: string;
        /** Optional retry handler — renders a "Retry" button. */
        retry?: () => void;
        retryLabel?: string;
        /** Extra controls rendered alongside the retry button. */
        actions?: Snippet;
    }

    let {
        title = 'Something went wrong',
        description = '',
        retry,
        retryLabel = 'Retry',
        actions,
    }: Props = $props();
</script>

<div class="error-state" role="alert">
    <span class="error-icon" aria-hidden="true">
        <AlertTriangle class="error-ico" />
    </span>
    <div class="error-text">
        <div class="error-title">{title}</div>
        {#if description}
            <div class="error-desc">{description}</div>
        {/if}
    </div>
    {#if retry || actions}
        <div class="error-actions">
            {#if actions}{@render actions()}{/if}
            {#if retry}
                <button type="button" class="error-retry" onclick={retry}>
                    <RotateCcw class="error-retry-ico" />
                    <span>{retryLabel}</span>
                </button>
            {/if}
        </div>
    {/if}
</div>

<style>
    .error-state {
        display: flex;
        align-items: flex-start;
        gap: 12px;
        padding: 14px 16px;
        background: var(--color-error-soft);
        border: 1px solid var(--color-error-strong);
        border-radius: var(--radius-card);
        color: var(--color-text);
    }
    .error-icon {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 32px;
        height: 32px;
        border-radius: var(--radius-control);
        background: color-mix(in srgb, var(--color-error) 18%, transparent);
        color: var(--color-error);
    }
    .error-state :global(.error-ico) {
        width: 16px;
        height: 16px;
    }
    .error-text {
        flex: 1;
        min-width: 0;
    }
    .error-title {
        font-size: 13.5px;
        font-weight: 600;
        color: var(--color-text);
        letter-spacing: -0.006em;
    }
    .error-desc {
        margin-top: 3px;
        font-size: 12.5px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }
    .error-actions {
        flex: none;
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .error-retry {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        height: 28px;
        padding: 0 11px;
        background: var(--color-panel);
        color: var(--color-text);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out);
    }
    .error-retry:hover {
        background: var(--color-panel-2);
        border-color: var(--color-border-strong);
    }
    .error-state :global(.error-retry-ico) {
        width: 13px;
        height: 13px;
    }
</style>
