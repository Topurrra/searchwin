<script lang="ts">
    /*
      Standardized cancel button for long-running tool operations.

      Always rendered the same way across tools — same icon, same color
      treatment, same disabled state. Pass the running flag and the
      cancel handler; the component handles the visual states.

      Three states implicit in the button:
        - hidden:  pass `running={false}` and the button doesn't render
                   (component decides — caller doesn't need a wrapper {#if})
        - cancellable: showing "Cancel" with an X icon, hover destructive
        - cancelling: showing a spinner + "Cancelling…" while we wait
                       for the backend to acknowledge
    */
    import { X, Loader2 } from '@lucide/svelte';
    import { _ } from 'svelte-i18n';

    let {
        running,
        onCancel,
        cancelling = false,
        label = undefined,
        cancellingLabel = undefined,
    }: {
        running: boolean;
        onCancel: () => void | Promise<void>;
        cancelling?: boolean;
        label?: string;
        cancellingLabel?: string;
    } = $props();

    /** Localized fallbacks when callers don't pass explicit labels. */
    let resolvedLabel = $derived(label ?? $_('errors.cancel'));
    let resolvedCancellingLabel = $derived(cancellingLabel ?? $_('errors.cancelling'));
</script>

{#if running}
    <button
        type="button"
        class="cancel-btn"
        onclick={() => void onCancel()}
        disabled={cancelling}
        aria-label={cancelling ? resolvedCancellingLabel : resolvedLabel}
    >
        {#if cancelling}
            <Loader2 class="cancel-icon spin" />
            <span>{resolvedCancellingLabel}</span>
        {:else}
            <X class="cancel-icon" />
            <span>{resolvedLabel}</span>
        {/if}
    </button>
{/if}

<style>
    .cancel-btn {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 0 12px;
        height: 32px;
        border-radius: 8px;
        border: 1px solid color-mix(in srgb, var(--color-error) 40%, var(--color-border));
        background: color-mix(in srgb, var(--color-error) 10%, var(--color-panel-2));
        color: var(--color-error);
        font-size: 12.5px;
        font-weight: 500;
        cursor: pointer;
        transition: background-color 140ms ease, border-color 140ms ease;
    }

    .cancel-btn:hover:not(:disabled) {
        background: color-mix(in srgb, var(--color-error) 18%, var(--color-panel-2));
        border-color: color-mix(in srgb, var(--color-error) 60%, var(--color-border));
    }

    .cancel-btn:disabled {
        opacity: 0.7;
        cursor: not-allowed;
    }

    :global(.cancel-icon) {
        width: 14px;
        height: 14px;
    }

    :global(.cancel-icon.spin) {
        animation: cancel-spin 800ms linear infinite;
    }

    @keyframes cancel-spin {
        to {
            transform: rotate(360deg);
        }
    }
</style>
