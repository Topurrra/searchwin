<script lang="ts">
    import { setContext } from 'svelte';
    import { ArrowLeft } from '@lucide/svelte';
    import Skeleton from '$lib/Skeleton.svelte';
    import ToolErrorBoundary from '$lib/components/ToolErrorBoundary.svelte';
    import { Button } from '$lib/ui';
    import {
        toolScreens,
        toolPackIdForScreen,
        categoryIcons,
        packColors,
        packLabels,
        packIdForCategoryScreen,
        getScreen,
        type ToolPackId,
    } from '$lib/appScreens';
    import { categoryActiveTool, commandTabForId, libraryActivePack } from '$lib/stores/categoryNav';
    import { activeProfile } from '$lib/stores/profiles';

    let { selected = $bindable() }: { selected: string } = $props();

    setContext('kil:category-workspace', true);

    let activeToolId = $state('');

    const packId = $derived(packIdForCategoryScreen(selected) as ToolPackId | null);

    const allPackTools = $derived(
        packId
            ? toolScreens.filter(
                  (tool) =>
                      toolPackIdForScreen(tool) === packId &&
                      // `hidden` tools keep their code but leave every surface —
                      // filtering here also keeps them out of `orderedToolIds`, so a
                      // hidden tool can never become the category's default pick.
                      !tool.hidden &&
                      commandTabForId(tool.id) === null,
              )
            : [],
    );

    const profileToolSet = $derived(
        $activeProfile && $activeProfile.toolIds.length > 0
            ? new Set($activeProfile.toolIds)
            : null,
    );
    const packTools = $derived(
        !profileToolSet
            ? allPackTools
            : allPackTools.filter(
                  (tool) => profileToolSet.has(tool.id) || tool.id === activeToolId,
              ),
    );
    const category = $derived(packTools[0]?.category);
    const tint = $derived(packId ? packColors[packId] : 'var(--color-accent)');
    const HeaderIcon = $derived(category ? categoryIcons[category] : undefined);
    const headerLabel = $derived(packId ? packLabels[packId] : '');
    const orderedToolIds = $derived(packTools.map((tool) => tool.id));

    $effect(() => {
        const pending = $categoryActiveTool;
        if (pending && allPackTools.some((tool) => tool.id === pending)) {
            activeToolId = pending;
            categoryActiveTool.set(null);
        } else if (!activeToolId && orderedToolIds.length) {
            activeToolId = orderedToolIds[0];
        }
    });

    // ─── Inner tool loading (mirrors the router's dynamic-import path) ──
    let activeComponent = $state<any>(null);
    let toolLoading = $state(false);
    let toolError = $state<string | null>(null);
    const componentCache = new Map<string, any>();
    let loadingId = '';

    async function loadTool(id: string) {
        const screen = getScreen(id);
        if (!screen || screen.kind !== 'tool') {
            activeComponent = null;
            toolError = `Unknown tool: ${id}`;
            return;
        }
        if (componentCache.has(id)) {
            activeComponent = componentCache.get(id) ?? null;
            toolLoading = false;
            toolError = null;
            return;
        }
        loadingId = id;
        toolLoading = true;
        activeComponent = null;
        toolError = null;
        try {
            const module = await screen.loader();
            componentCache.set(id, (module as { default: unknown }).default);
            if (activeToolId === id) activeComponent = componentCache.get(id);
        } catch (error) {
            if (activeToolId === id) toolError = String(error);
        } finally {
            if (activeToolId === id) toolLoading = false;
        }
    }

    $effect(() => {
        if (activeToolId) void loadTool(activeToolId);
    });

    function backToLibrary() {
        if (packId) libraryActivePack.set(packId);
        selected = 'tool-packs';
    }

    const activeScreen = $derived(activeToolId ? getScreen(activeToolId) : undefined);
</script>

<section class="cat" data-screen-kind="category" style={`--cat-tint: ${tint};`}>
    <header class="cat-head">
        <Button
            variant="ghost"
            size="sm"
            icon={ArrowLeft}
            onclick={backToLibrary}
            title="Back to Library"
        >
            Back to Library
        </Button>
        {#if HeaderIcon}
            {@const Icon = HeaderIcon}
            <span class="cat-icon" aria-hidden="true"><Icon class="cat-icon-svg" /></span>
        {/if}
        <div class="cat-head-text">
            {#if activeScreen}<p class="cat-pack-label">{headerLabel}</p>{/if}
            <h1 class="cat-title">{activeScreen?.name ?? headerLabel}</h1>
        </div>
    </header>

    <div class="cat-body">
        {#key activeToolId}
            {#if toolError}
                <div class="cat-error">{toolError}</div>
            {:else if toolLoading}
                <div class="cat-loading">
                    <Skeleton height="1.5rem" width="12rem" />
                    <Skeleton height="1rem" width="22rem" />
                    <div class="cat-loading-card">
                        <Skeleton height="1rem" />
                        <Skeleton height="1rem" />
                        <Skeleton height="8rem" />
                    </div>
                </div>
            {:else if activeComponent}
                {@const ToolComponent = activeComponent}
                <svelte:boundary>
                    {#snippet failed(error, reset)}
                        <ToolErrorBoundary
                            {error}
                            {reset}
                            screenId={activeToolId}
                            screenName={activeScreen?.name}
                        />
                    {/snippet}
                    <ToolComponent />
                </svelte:boundary>
            {/if}
        {/key}
    </div>
</section>

<style>
    .cat {
        display: flex;
        flex-direction: column;
        gap: 16px;
        max-width: 1280px;
        margin: 0 auto;
        padding: 28px 32px 32px;
        width: 100%;
        min-height: 100%;
    }

    /* ─── Pack header ──────────────────────────────────────────────── */
    .cat-head {
        display: flex;
        align-items: center;
        gap: 12px;
    }
    /* The ONLY place the pack tint survives (2026-07-29): the category's identity
       mark. One tinted 40px square per screen tells you which pack you're in at a
       glance, without competing with the theme accent that now owns every
       interactive/selected state. Do not reintroduce `--cat-tint` on buttons,
       borders, or any other selection affordance. */
    .cat-icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 40px;
        height: 40px;
        border-radius: 10px;
        flex: none;
        color: var(--cat-tint);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .cat :global(.cat-icon-svg) {
        width: 20px;
        height: 20px;
    }
    .cat-head-text {
        min-width: 0;
    }
    .cat-pack-label {
        margin: 0 0 2px;
        font-size: 11px;
        font-weight: 600;
        letter-spacing: 0.05em;
        text-transform: uppercase;
        color: var(--color-muted);
    }
    .cat-title {
        margin: 0;
        font-size: 22px;
        font-weight: 600;
        line-height: 1.2;
        letter-spacing: -0.018em;
        color: var(--color-text);
    }

    /* ─── Active tool body ─────────────────────────────────────────── */
    .cat-body {
        min-width: 0;
    }
    .cat-loading {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .cat-loading-card {
        display: flex;
        flex-direction: column;
        gap: 12px;
        padding: 16px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
    }
    .cat-error {
        padding: 12px 14px;
        border: 1px solid color-mix(in srgb, var(--color-error) 40%, var(--color-border));
        background: color-mix(in srgb, var(--color-error) 8%, transparent);
        border-radius: var(--radius-control, 8px);
        color: var(--color-error);
        font-size: 13px;
    }

    @media (max-width: 720px) {
        .cat {
            padding: 20px 16px 24px;
        }
    }
</style>
