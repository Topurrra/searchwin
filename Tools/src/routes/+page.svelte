<script lang="ts">
    // Search › Tools: every tool, by category, each a link to its own page.
    // Every tool is listed (Search's own packs come later). Opens as a tab of
    // its own (search://tools); a tool opens in a tab too,
    // so tools can be pinned, kept in spaces and bookmarked like any page.
    import { toolScreens, categoryOrder, categoryIcons, type Category } from '$lib/appScreens';

    let query = $state('');

    const visible = $derived(
        toolScreens.filter(
            (tool) =>
                tool.available &&
                !(tool as { hidden?: boolean }).hidden &&
                (query === '' ||
                    `${tool.name} ${tool.description ?? ''}`.toLowerCase().includes(query.toLowerCase())),
        ),
    );

    const groups = $derived(
        categoryOrder
            .map((category) => ({ category, tools: visible.filter((tool) => tool.category === category) }))
            .filter((group) => group.tools.length > 0),
    );

    $effect(() => {
        document.title = 'Tools';
    });

    const total = toolScreens.length;
</script>

<main class="tools-index">
    <header>
        <h1>Tools</h1>
        <p class="lede">Everything here runs on this computer. Nothing you open, convert or check leaves it.</p>
        <!-- svelte-ignore a11y_autofocus -->
        <input class="filter" type="search" placeholder="Filter {total} tools" bind:value={query} autofocus />
    </header>

    {#each groups as group (group.category)}
        {@const Icon = categoryIcons[group.category as Category]}
        <section>
            <h2><Icon size={14} /> {group.category}</h2>
            <div class="grid">
                {#each group.tools as tool (tool.id)}
                    {@const ToolIcon = tool.icon}
                    <a class="tool" href="#/tool/{tool.id}">
                        <ToolIcon size={18} />
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

<style>
    .tools-index {
        max-width: 980px;
        margin: 0 auto;
        padding: 40px 24px 64px;
        color: var(--color-text, #e5e5e5);
    }
    h1 {
        font-size: 28px;
        font-weight: 600;
        margin: 0 0 6px;
    }
    .lede {
        color: var(--color-text-muted, #a1a1aa);
        margin: 0 0 20px;
        font-size: 14px;
    }
    .filter {
        width: 100%;
        max-width: 420px;
        padding: 9px 12px;
        border-radius: 10px;
        border: 1px solid var(--color-border, #2a2a2e);
        background: var(--color-panel-1, #18181b);
        color: inherit;
        font: inherit;
        outline: none;
    }
    .filter:focus {
        border-color: var(--color-accent, #6d7cff);
    }
    section {
        margin-top: 28px;
    }
    h2 {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: 12px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-text-muted, #a1a1aa);
        margin: 0 0 10px;
    }
    .grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
        gap: 8px;
    }
    .tool {
        display: grid;
        grid-template-columns: 22px 1fr;
        grid-template-rows: auto auto;
        column-gap: 10px;
        padding: 12px;
        border-radius: 12px;
        background: var(--color-panel-1, #18181b);
        border: 1px solid transparent;
        color: inherit;
        text-decoration: none;
    }
    .tool:hover,
    .tool:focus-visible {
        border-color: var(--color-border, #2a2a2e);
        background: var(--color-panel-2, #1f1f23);
        outline: none;
    }
    .tool :global(svg) {
        grid-row: span 2;
        margin-top: 2px;
        color: var(--color-text-muted, #a1a1aa);
    }
    .name {
        font-size: 14px;
        font-weight: 500;
    }
    .what {
        font-size: 12px;
        color: var(--color-text-muted, #a1a1aa);
        line-height: 1.35;
    }
</style>
