<script lang="ts">
    // One tool, alone in a tab: https://tools.search/#/tool/<id>
    // (search://tools/<id>), with its own title and icon. A tool Search leaves
    // out says why instead (searchTools.ts).
    import { page } from '$app/state';
    import { openTool } from '$lib/searchTools';
    import { wearIcon } from '$lib/toolIcon';
    import ToolErrorBoundary from '$lib/components/ToolErrorBoundary.svelte';
    import ToastContainer from '$lib/ToastContainer.svelte';

    const id = $derived(page.params.id ?? '');
    const wanted = $derived(openTool(id));
    let component = $state<any>(null);
    let failed = $state<string | null>(null);

    $effect(() => {
        const tool = wanted;
        component = null;
        failed = null;
        if ('refused' in tool) {
            document.title = 'Tools';
            wearIcon(undefined);
            failed = tool.refused;
            return;
        }
        document.title = tool.name;
        wearIcon(tool.icon);
        tool.loader()
            .then((module) => {
                if (wanted === tool) component = module.default;
            })
            .catch((error) => {
                failed = `This tool didn't load: ${error}`;
            });
    });
</script>

<div class="tool-page">
    {#if failed}
        <div class="missing">
            <p>{failed}</p>
            <a href="#/">All tools</a>
        </div>
    {:else if component}
        {@const ScreenComponent = component}
        {#key id}
            <svelte:boundary>
                {#snippet failed(error, reset)}
                    <ToolErrorBoundary {error} {reset} screenId={id} screenName={'name' in wanted ? wanted.name : id} />
                {/snippet}
                <ScreenComponent />
            </svelte:boundary>
        {/key}
    {/if}
</div>

<ToastContainer />

<style>
    .tool-page {
        height: 100%;
        overflow: auto;
        padding: 24px;
        box-sizing: border-box;
    }
    .missing {
        max-width: 420px;
        margin: 18vh auto 0;
        text-align: center;
        color: var(--search-muted);
    }
    .missing p {
        margin: 0 0 12px;
    }
    .missing a {
        display: inline-block;
        padding: 6px 12px;
        border-radius: var(--search-radius-control);
        background: var(--search-hover);
        color: var(--search-ink);
        text-decoration: none;
    }
    .missing a:hover {
        background: var(--search-wash);
    }
</style>
