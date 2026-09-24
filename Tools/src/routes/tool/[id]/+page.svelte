<script lang="ts">
    // One tool, alone in a tab: https://tools.search/#/tool/<id>. The same
    // screen Workspace showed inside its app shell, with nothing around it.
    import { page } from '$app/state';
    import { getScreen } from '$lib/appScreens';
    import ToolErrorBoundary from '$lib/components/ToolErrorBoundary.svelte';
    import ToastContainer from '$lib/ToastContainer.svelte';

    const id = $derived(page.params.id ?? '');
    const screen = $derived(getScreen(id));
    let component = $state<any>(null);
    let failed = $state<string | null>(null);

    $effect(() => {
        const wanted = screen;
        component = null;
        failed = null;
        document.title = wanted?.name ?? 'Tool';
        if (!wanted || !('loader' in wanted) || !wanted.loader) {
            failed = `There's no tool called “${id}”.`;
            return;
        }
        wanted
            .loader()
            .then((module) => {
                if (screen === wanted) component = module.default;
            })
            .catch((error) => {
                failed = `This tool didn't load: ${error}`;
            });
    });
</script>

<div class="tool-page">
    {#if failed}
        <p class="missing">{failed} <a href="#/">All tools</a></p>
    {:else if component}
        {@const ScreenComponent = component}
        {#key id}
            <svelte:boundary>
                {#snippet failed(error, reset)}
                    <ToolErrorBoundary {error} {reset} screenId={id} screenName={screen?.name} />
                {/snippet}
                <ScreenComponent />
            </svelte:boundary>
        {/key}
    {/if}
</div>

<ToastContainer />

<style>
    .tool-page {
        min-height: 100vh;
        padding: 24px;
        box-sizing: border-box;
    }
    .missing {
        color: var(--color-text-muted, #a1a1aa);
    }
    .missing a {
        color: var(--color-accent, #6d7cff);
    }
</style>
