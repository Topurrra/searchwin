<script lang="ts">
    /*
      Textarea — a multi-line text field. `mono` switches to the code
      typeface for data / code input. `value` is bindable.
    */
    import type { HTMLTextareaAttributes } from 'svelte/elements';

    interface Props extends HTMLTextareaAttributes {
        value?: string;
        /** Render an error-toned border. */
        invalid?: boolean;
        /** Monospace (code / data) text. */
        mono?: boolean;
        resize?: 'none' | 'vertical';
    }

    let {
        value = $bindable(''),
        invalid = false,
        mono = false,
        resize = 'vertical',
        rows = 4,
        class: className = '',
        ...rest
    }: Props = $props();
</script>

<textarea
    class="ta resize-{resize} {mono ? 'mono' : ''} {invalid ? 'is-invalid' : ''} {className}"
    {rows}
    bind:value
    {...rest}
></textarea>

<style>
    .ta {
        width: 100%;
        padding: 9px 11px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-size: 13px;
        line-height: 1.6;
    }
    .ta::placeholder {
        color: var(--color-muted);
    }
    .ta:hover:not(:disabled):not(:focus) {
        border-color: var(--color-border-strong);
    }
    .ta:disabled {
        opacity: 0.5;
        cursor: not-allowed;
    }
    .ta.is-invalid {
        border-color: var(--color-error);
    }
    .resize-none {
        resize: none;
    }
    .resize-vertical {
        resize: vertical;
    }
</style>
