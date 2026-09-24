<script lang="ts">
    // One logo everywhere, every theme: the original dark mark (logo-mark.png,
    // == workspace.png). The theme-conditional light variant was retired
    // 2026-07-16 by founder decision — the mark reads on all seven themes and a
    // single asset is one fewer thing to keep in sync. logo-mark-light.png is
    // kept on disk but no longer referenced.
    import mark from './assets/brand/logo-mark.png';

    let {
        size = 24,
        class: className = '',
    }: { size?: number | string; class?: string } = $props();
</script>

<img
    src={mark}
    alt="KeepItLocal"
    class={className}
    width={size}
    height={size}
    style="object-fit:contain;display:block;"
    draggable="false"
/>
