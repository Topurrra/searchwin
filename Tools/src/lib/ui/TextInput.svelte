<script lang="ts">
    /*
      TextInput — a single-line text field with an optional leading icon.
      The focus ring is the app-wide accent ring (styles.css). `value`
      is bindable.
    */
    import type { HTMLInputAttributes } from 'svelte/elements';

    interface Props extends Omit<HTMLInputAttributes, 'size'> {
        value?: string;
        size?: 'sm' | 'md';
        /** Leading Lucide icon component. */
        icon?: any;
        /** Render an error-toned border. */
        invalid?: boolean;
        /** Quality Pass Wave 1 / Esc-Sweep (2026-05-29): when true,
         *  pressing Esc on a non-empty input clears it (and stops the
         *  event from bubbling). Empty input passes Esc through so
         *  ancestor dialogs / overlays can still use Esc to close.
         *  Mirrors the `use:escToClear` action that's applied to bare
         *  `<input>` elements elsewhere — same UX, the prop form is
         *  needed here because Svelte's `use:` directive can't attach
         *  to a component, only a DOM node. */
        clearOnEscape?: boolean;
    }

    let {
        value = $bindable(''),
        size = 'md',
        icon: Icon,
        invalid = false,
        clearOnEscape = false,
        class: className = '',
        ...rest
    }: Props = $props();

    function handleKeyDown(event: KeyboardEvent) {
        if (!clearOnEscape) return;
        if (event.key !== 'Escape') return;
        // Empty → let the Esc bubble so a wrapping dialog can close.
        if (!value) return;
        event.preventDefault();
        event.stopPropagation();
        value = '';
    }
</script>

<div class="field f-{size} {className}">
    {#if Icon}
        <Icon class="field-ico" />
    {/if}
    <input
        class="field-input {Icon ? 'has-ico' : ''} {invalid ? 'is-invalid' : ''}"
        bind:value
        onkeydown={handleKeyDown}
        {...rest}
    />
</div>

<style>
    .field {
        position: relative;
        display: block;
        width: 100%;
    }
    .field :global(.field-ico) {
        position: absolute;
        left: 10px;
        top: 50%;
        transform: translateY(-50%);
        width: 15px;
        height: 15px;
        color: var(--color-muted);
        pointer-events: none;
    }
    .field-input {
        width: 100%;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-size: 13px;
    }
    .f-md .field-input {
        height: 34px;
        padding: 0 11px;
    }
    .f-sm .field-input {
        height: 28px;
        padding: 0 9px;
        font-size: 12px;
    }
    .f-md .field-input.has-ico {
        padding-left: 32px;
    }
    .f-sm .field-input.has-ico {
        padding-left: 29px;
    }
    .field-input::placeholder {
        color: var(--color-muted);
    }
    .field-input:hover:not(:disabled):not(:focus) {
        border-color: var(--color-border-strong);
    }
    .field-input:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }
    .field-input.is-invalid {
        border-color: var(--color-error);
    }
</style>
