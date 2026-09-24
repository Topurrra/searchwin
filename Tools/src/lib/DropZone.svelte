<script lang="ts">
    import { getCurrentWebview } from '@tauri-apps/api/webview';
    import { onMount, onDestroy } from 'svelte';
    import { _ } from 'svelte-i18n';

    let {
        onFiles,
        accept = [] as string[],   // lowercase extensions, e.g. ['jpg', 'png']. Empty = accept all.
        children,
    }: {
        onFiles: (paths: string[]) => void | Promise<void>;
        accept?: string[];
        children: () => any;
    } = $props();

    let isDragging = $state(false);
    let unlisten: (() => void) | null = null;

    function filterAccepted(paths: string[]): string[] {
        if (accept.length === 0) return paths;
        return paths.filter(p => {
            const ext = p.split('.').pop()?.toLowerCase() || '';
            return accept.includes(ext);
        });
    }

    onMount(async () => {
        const webview = getCurrentWebview();
        unlisten = await webview.onDragDropEvent(event => {
            if (event.payload.type === 'enter' || event.payload.type === 'over') {
                isDragging = true;
            } else if (event.payload.type === 'drop') {
                isDragging = false;
                const paths = event.payload.paths || [];
                const accepted = filterAccepted(paths);
                if (accepted.length > 0) onFiles(accepted);
            } else if (event.payload.type === 'leave') {
                isDragging = false;
            }
        });
    });

    onDestroy(() => {
        if (unlisten) unlisten();
    });
</script>

<div class="relative">
    {@render children()}

    {#if isDragging}
        <div class="absolute inset-0 z-50 bg-accent/10 backdrop-blur-sm border-2 border-dashed border-accent rounded flex items-center justify-center pointer-events-none">
            <div class="text-center">
                <div class="text-4xl mb-2">⬇</div>
                <div class="text-accent font-semibold">{$_('nav.dropFilesHere')}</div>
                {#if accept.length > 0}
                    <div class="text-xs text-muted mt-1">
                        {$_('nav.acceptedTypes', { values: { types: accept.map(e => '.' + e).join(', ') } })}
                    </div>
                {/if}
            </div>
        </div>
    {/if}
</div>
