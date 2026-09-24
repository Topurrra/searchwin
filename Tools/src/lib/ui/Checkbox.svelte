<script lang="ts">
    /*
      Checkbox — a real <input type="checkbox"> with a styled box, so
      form semantics and keyboard behaviour stay native. `checked` is
      bindable. Pass `label` for a visible label, or `ariaLabel`.
    */
    import { Check } from '@lucide/svelte';

    interface Props {
        checked?: boolean;
        disabled?: boolean;
        /** Visible label text. */
        label?: string;
        ariaLabel?: string;
        onchange?: (checked: boolean) => void;
        id?: string;
    }

    let {
        checked = $bindable(false),
        disabled = false,
        label,
        ariaLabel,
        onchange,
        id,
    }: Props = $props();
</script>

<label class="cb {disabled ? 'is-disabled' : ''}">
    <input
        {id}
        type="checkbox"
        class="cb-input"
        bind:checked
        {disabled}
        aria-label={ariaLabel}
        onchange={(e) => onchange?.(e.currentTarget.checked)}
    />
    <span class="cb-box" aria-hidden="true">
        <Check class="cb-tick" />
    </span>
    {#if label}
        <span class="cb-label">{label}</span>
    {/if}
</label>

<style>
    .cb {
        position: relative;
        display: inline-flex;
        align-items: center;
        gap: 8px;
        cursor: pointer;
    }
    .cb.is-disabled {
        cursor: not-allowed;
        opacity: 0.5;
    }
    .cb-input {
        position: absolute;
        width: 1px;
        height: 1px;
        margin: 0;
        opacity: 0;
    }
    .cb-box {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 17px;
        height: 17px;
        border: 1px solid var(--color-border-strong);
        border-radius: 5px;
        background: var(--color-panel-2);
        transition:
            background-color var(--dur-micro) var(--ease-out),
            border-color var(--dur-micro) var(--ease-out);
    }
    .cb-input:checked + .cb-box {
        background: var(--color-accent);
        border-color: var(--color-accent);
    }
    .cb-input:focus-visible + .cb-box {
        outline: none;
        box-shadow:
            0 0 0 2px var(--color-bg),
            0 0 0 4px color-mix(in srgb, var(--color-accent) 55%, transparent);
    }
    .cb-box :global(.cb-tick) {
        width: 12px;
        height: 12px;
        color: var(--color-accent-contrast);
        opacity: 0;
        transition: opacity var(--dur-micro) var(--ease-out);
    }
    .cb-input:checked + .cb-box :global(.cb-tick) {
        opacity: 1;
    }
    .cb-label {
        font-size: 13px;
        color: var(--color-text);
        line-height: 1;
    }
</style>
