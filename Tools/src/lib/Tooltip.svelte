<script lang="ts">
    let { text, children, position = 'top' }: {
        text: string;
        children: () => any;
        position?: 'top' | 'bottom' | 'left' | 'right';
    } = $props();

    let visible = $state(false);
    let timer: ReturnType<typeof setTimeout>;

    function show() {
        timer = setTimeout(() => (visible = true), 400);
    }

    function hide() {
        clearTimeout(timer);
        visible = false;
    }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- Tooltip-trigger wrapper: keyboard users are served by the focusin/
     focusout handlers; mouseenter/mouseleave are a pure hover enhancement,
     so no ARIA role belongs on this passive wrapper element. -->
<span
        class="relative inline-flex"
        onmouseenter={show}
        onmouseleave={hide}
        onfocusin={show}
        onfocusout={hide}
>
  {@render children()}

    {#if visible}
    <span
            class="absolute z-50 px-2 py-1 text-xs whitespace-nowrap bg-bg border border-border rounded shadow-lg pointer-events-none fade-in
             {position === 'top' ? 'bottom-full mb-1.5 left-1/2 -translate-x-1/2' : ''}
             {position === 'bottom' ? 'top-full mt-1.5 left-1/2 -translate-x-1/2' : ''}
             {position === 'left' ? 'right-full mr-1.5 top-1/2 -translate-y-1/2' : ''}
             {position === 'right' ? 'left-full ml-1.5 top-1/2 -translate-y-1/2' : ''}"
    >
      {text}
    </span>
  {/if}
</span>