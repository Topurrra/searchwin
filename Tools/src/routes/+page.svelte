<script lang="ts">
    // search://tools: every tool Search has, by pack, each a link to its own
    // page. A tool opens in a tab like any page, so it can be pinned, kept in
    // a space and bookmarked. Typing narrows the list; Enter opens the first.
    import { LayoutGrid } from '@lucide/svelte';
    import { searchTools, toolsByPack } from '$lib/searchTools';
    import { packLabels } from '$lib/appScreens';
    import { wearIcon } from '$lib/toolIcon';

    let query = $state('');

    const visible = $derived.by(() => {
        const words = query.toLowerCase().split(/\s+/).filter(Boolean);
        return searchTools.filter((tool) => {
            const text = `${tool.name} ${tool.description} ${packLabels[tool.pack]}`.toLowerCase();
            return words.every((word) => text.includes(word));
        });
    });

    const groups = $derived(toolsByPack(visible));

    $effect(() => {
        document.title = 'Tools';
        wearIcon(LayoutGrid);
    });

    let list = $state<HTMLElement>();

    // Through the first card's own link, so the router takes it: setting
    // location.hash reads as an edit to the address and reloads the page.
    function openFirst(event: KeyboardEvent) {
        if (event.key !== 'Enter') return;
        event.preventDefault();
        list?.querySelector<HTMLAnchorElement>('a.tool')?.click();
    }
</script>

<div class="scroller">
    <main class="tools-index" bind:this={list}>
        <header>
            <h1>Tools</h1>
            <p class="lede">Everything here runs on this computer. Nothing you open, convert or check leaves it.</p>
            <!-- svelte-ignore a11y_autofocus -->
            <input
                class="filter"
                type="search"
                placeholder="Find a tool"
                aria-label="Find a tool"
                bind:value={query}
                onkeydown={openFirst}
                autofocus
            />
        </header>

        {#each groups as group (group.pack)}
            <section aria-label={group.name}>
                <h2>{group.name}</h2>
                <div class="grid">
                    {#each group.tools as tool (tool.id)}
                        {@const ToolIcon = tool.icon}
                        <a class="tool" href="#/tool/{tool.id}">
                            <ToolIcon size={18} strokeWidth={1.75} />
                            <span class="name">{tool.name}</span>
                            <span class="what">{tool.description}</span>
                        </a>
                    {/each}
                </div>
            </section>
        {:else}
            <p class="lede">No tool matches “{query}”.</p>
        {/each}
    </main>
</div>

<style>
    .scroller {
        height: 100%;
        overflow-y: auto;
    }
    .tools-index {
        max-width: 960px;
        margin: 0 auto;
        padding: 40px 24px 64px;
    }
    h1 {
        font-size: 24px;
        font-weight: 600;
        margin: 0 0 4px;
    }
    .lede {
        color: var(--search-muted);
        margin: 0 0 20px;
        font-size: 13px;
    }
    .filter {
        width: 100%;
        max-width: 420px;
        padding: 9px 12px;
        border-radius: var(--search-radius-card);
        border: 1px solid transparent;
        background: var(--search-hover);
        color: inherit;
        font: inherit;
        outline: none;
    }
    .filter:focus-visible {
        border-color: var(--search-faint);
        box-shadow: none;
    }
    section {
        margin-top: 28px;
    }
    h2 {
        font-size: 11px;
        font-weight: 500;
        color: var(--search-muted);
        margin: 0 0 8px 2px;
        letter-spacing: 0;
    }
    .grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
        gap: 6px;
    }
    .tool {
        display: grid;
        grid-template-columns: 22px 1fr;
        grid-template-rows: auto auto;
        column-gap: 10px;
        row-gap: 2px;
        padding: 12px;
        border-radius: var(--search-radius-card);
        background: var(--search-hover);
        color: inherit;
        text-decoration: none;
        transition: background-color var(--search-quick) var(--search-ease);
    }
    .tool:hover,
    .tool:focus-visible {
        background: var(--search-wash);
        outline: none;
        box-shadow: none;
    }
    .tool :global(svg) {
        grid-row: span 2;
        margin-top: 1px;
        color: var(--search-muted);
    }
    .name {
        font-size: 14px;
        font-weight: 500;
    }
    .what {
        font-size: 12px;
        color: var(--search-muted);
        line-height: 1.35;
    }
</style>
