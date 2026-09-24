<script lang="ts">
    /*
      Select — a styled native <select>. Native is kept for reliability
      and accessibility; only the chrome is restyled, with a custom
      chevron. `value` is bindable.
    */
    import type { HTMLSelectAttributes } from 'svelte/elements';
    import { ChevronDown } from '@lucide/svelte';

    interface Props extends Omit<HTMLSelectAttributes, 'size'> {
        value?: string;
        size?: 'sm' | 'md';
        options: { value: string; label: string; disabled?: boolean }[];
        /** Optional placeholder, shown as a disabled first option. */
        placeholder?: string;
    }

    let {
        value = $bindable(''),
        size = 'md',
        options,
        placeholder,
        class: className = '',
        ...rest
    }: Props = $props();
</script>

<div class="sel s-{size} {className}">
    <select class="sel-el" bind:value {...rest}>
        {#if placeholder}
            <option value="" disabled>{placeholder}</option>
        {/if}
        {#each options as opt}
            <option value={opt.value} disabled={opt.disabled}>{opt.label}</option>
        {/each}
    </select>
    <ChevronDown class="sel-chevron" />
</div>

<style>
    .sel {
        position: relative;
        display: inline-block;
        width: 100%;
    }
    .sel-el {
        width: 100%;
        appearance: none;
        -webkit-appearance: none;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-size: 13px;
        cursor: pointer;
    }
    .sel-el option {
        background: var(--color-panel);
        color: var(--color-text);
    }
    .s-md .sel-el {
        height: 34px;
        padding: 0 32px 0 11px;
    }
    .s-sm .sel-el {
        height: 28px;
        padding: 0 30px 0 9px;
        font-size: 12px;
    }
    .sel-el:hover:not(:disabled):not(:focus) {
        border-color: var(--color-border-strong);
    }
    .sel-el:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }
    .sel :global(.sel-chevron) {
        position: absolute;
        right: 10px;
        top: 50%;
        transform: translateY(-50%);
        width: 15px;
        height: 15px;
        color: var(--color-muted);
        pointer-events: none;
    }
</style>
