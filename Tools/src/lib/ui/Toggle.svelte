<script lang="ts">
    /*
      Toggle — an on/off switch. A real button with role="switch";
      `checked` is bindable. Always pass `ariaLabel` — the switch has
      no visible label of its own.
    */
    interface Props {
        checked?: boolean;
        disabled?: boolean;
        size?: 'sm' | 'md';
        /** Accessible name for the switch. */
        ariaLabel?: string;
        onchange?: (checked: boolean) => void;
        id?: string;
    }

    let {
        checked = $bindable(false),
        disabled = false,
        size = 'md',
        ariaLabel,
        onchange,
        id,
    }: Props = $props();

    function toggle() {
        checked = !checked;
        onchange?.(checked);
    }
</script>

<button
    {id}
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={ariaLabel}
    {disabled}
    class="tg tg-{size} {checked ? 'is-on' : ''}"
    onclick={toggle}
>
    <span class="tg-knob"></span>
</button>

<style>
    .tg {
        flex: none;
        display: inline-flex;
        align-items: center;
        padding: 0;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-pill);
        background: var(--color-panel-3);
    }
    .tg-md {
        width: 38px;
        height: 22px;
    }
    .tg-sm {
        width: 32px;
        height: 19px;
    }
    .tg.is-on {
        background: var(--color-accent);
        border-color: var(--color-accent);
    }
    .tg:disabled {
        opacity: 0.45;
        cursor: not-allowed;
    }
    .tg-knob {
        display: block;
        margin: 0 2px;
        border-radius: 50%;
        background: #fff;
        box-shadow: 0 1px 2px rgba(0, 0, 0, 0.35);
        transition: transform var(--dur-micro) var(--ease-out);
    }
    .tg-md .tg-knob {
        width: 16px;
        height: 16px;
    }
    .tg-sm .tg-knob {
        width: 13px;
        height: 13px;
    }
    .tg-md.is-on .tg-knob {
        transform: translateX(16px);
    }
    .tg-sm.is-on .tg-knob {
        transform: translateX(13px);
    }
</style>
