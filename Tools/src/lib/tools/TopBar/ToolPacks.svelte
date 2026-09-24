<script lang="ts">
    import {
        BookOpen,
        Check,
        Clipboard as ClipboardIcon,
        Code,
        Code2,
        Download,
        FileText,
        Files,
        ImageIcon,
        Lock,
        Mic,
        Pin,
        Search,
        ShieldCheck,
        Timer,
        Video,
        Workflow,
        Wrench,
        type Icon as LucideIcon,
    } from '@lucide/svelte';
    import {
        HIDDEN_PACK_IDS,
        packColors,
        packLabels,
        toolPackIdForScreen,
        toolPacks,
        toolScreens,
        type ToolPackId,
    } from '$lib/appScreens';
    import { enabledPackIds, pinnedToolIds, togglePinnedTool } from '$lib/stores/toolPacks';
    import { activeProfile, filterToolsForProfile } from '$lib/stores/profiles';
    import { libraryActivePack } from '$lib/stores/categoryNav';
    import { settingsTarget } from '$lib/stores/settingsTarget';
    import { toast } from '$lib/stores/toasts';
    import { EmptyState, TextInput, ToolPage, ToolToolbar } from '$lib/ui';

    let { selected = $bindable() }: { selected: string } = $props();

    const PACK_ICONS: Record<ToolPackId, typeof LucideIcon> = {
        core: ShieldCheck,
        media: Video,
        utils: Wrench,
        development: Code,
        privacy: Lock,
        document: FileText,
        image: ImageIcon,
        file: Files,
        automation: Workflow,
        'time-focus': Timer,
    };

    interface CoreSurfaceDef {
        id: string;
        name: string;
        description: string;
        icon: typeof LucideIcon;
    }

    const CORE_PAGE_SURFACES: CoreSurfaceDef[] = [
        {
            id: 'clipboard-history',
            name: 'Clipboard History',
            description: 'Searchable history of everything you copy. Pinning, snippets, paste-into-prev-app.',
            icon: ClipboardIcon,
        },
        {
            id: 'snippets',
            name: 'Snippets',
            description: 'Reusable text templates expanded via clipboard-overlay triggers.',
            icon: Code2,
        },
        {
            id: 'voice-to-text',
            name: 'Voice to Text',
            description: 'Offline Vosk dictation. Mic toggle on pages, auto-arms with overlays.',
            icon: Mic,
        },
    ];

    interface PackTool {
        id: string;
        name: string;
        description: string;
        icon: typeof LucideIcon;
    }

    interface PackBucket {
        id: ToolPackId;
        label: string;
        tagline: string;
        icon: typeof LucideIcon;
        color: string;
        installed: boolean;
        tools: PackTool[];
    }

    let buckets = $derived.by<PackBucket[]>(() => {
        const out: Record<string, PackBucket> = {};

        for (const pack of toolPacks) {
            if (HIDDEN_PACK_IDS.has(pack.id)) continue;
            out[pack.id] = {
                id: pack.id,
                label: packLabels[pack.id] ?? pack.name,
                tagline: pack.description,
                icon: PACK_ICONS[pack.id] ?? Wrench,
                color: packColors[pack.id] ?? 'var(--color-accent)',
                installed: pack.id === 'core' || $enabledPackIds.includes(pack.id),
                tools: [],
            };
        }

        for (const screen of filterToolsForProfile($activeProfile, toolScreens)) {
            if (screen.hidden) continue;
            const packId = toolPackIdForScreen(screen);
            if (out[packId]) {
                out[packId].tools.push({
                    id: screen.id,
                    name: screen.name,
                    description: screen.description,
                    icon: screen.icon,
                });
            }
        }

        if (out.core) {
            for (const surface of CORE_PAGE_SURFACES) {
                if (!out.core.tools.some((tool) => tool.id === surface.id)) {
                    out.core.tools.push(surface);
                }
            }
        }

        return toolPacks
            .map((pack) => out[pack.id])
            .filter((bucket): bucket is PackBucket => Boolean(bucket) && bucket.tools.length > 0);
    });

    let query = $state('');
    let filteredBuckets = $derived.by<PackBucket[]>(() => {
        const normalizedQuery = query.trim().toLowerCase();
        if (!normalizedQuery) return buckets;

        return buckets
            .map((bucket) => {
                const packMatches =
                    bucket.label.toLowerCase().includes(normalizedQuery) ||
                    bucket.tagline.toLowerCase().includes(normalizedQuery);
                const matchingTools = bucket.tools.filter((tool) => {
                    return (
                        tool.name.toLowerCase().includes(normalizedQuery) ||
                        tool.description.toLowerCase().includes(normalizedQuery)
                    );
                });

                if (packMatches) return bucket;
                if (matchingTools.length === 0) return null;
                return { ...bucket, tools: matchingTools };
            })
            .filter((bucket): bucket is PackBucket => bucket !== null);
    });

    let toolCount = $derived(buckets.reduce((count, bucket) => count + bucket.tools.length, 0));
    let installedPackCount = $derived(buckets.filter((bucket) => bucket.installed).length);
    let activePackId = $state<ToolPackId>($libraryActivePack ?? 'core');

    let activeBucket = $derived(
        buckets.find((bucket) => bucket.id === activePackId) ?? buckets[0] ?? null,
    );
    let displayedBuckets = $derived(
        query.trim() ? filteredBuckets : activeBucket ? [activeBucket] : [],
    );

    function openTool(bucket: PackBucket, tool: PackTool) {
        if (bucket.installed) {
            selected = tool.id;
            return;
        }

        settingsTarget.set('toolpacks');
        toast(
            tool.name +
                ' is part of the ' +
                bucket.label +
                ' pack. Enable it in Settings, save, and restart KeepItLocal to use this tool.',
            'info',
            6500,
        );
        selected = 'settings';
    }

    function openSettingsForPack(bucket: PackBucket) {
        settingsTarget.set('toolpacks');
        toast(
            'Enable the ' +
                bucket.label +
                ' pack in Settings, save, and restart KeepItLocal to use its tools.',
            'info',
            6500,
        );
        selected = 'settings';
    }

    function selectPack(bucket: PackBucket) {
        activePackId = bucket.id;
        query = '';
        libraryActivePack.set(bucket.id);
    }
</script>

<ToolPage
    icon={BookOpen}
    iconTint="var(--color-accent)"
    title="Library"
    description="Browse every local tool, then open the one that fits the job."
    width="wide"
    fill={false}
>
    <ToolToolbar variant="panel">
        <div class="library-toolbar">
            <div class="library-search">
                <TextInput
                    type="search"
                    placeholder="Search tools, packs, or capabilities"
                    bind:value={query}
                    icon={Search}
                    clearOnEscape
                    aria-label="Search library"
                />
            </div>
            <div class="library-counts" aria-label="Library summary">
                <span><strong>{toolCount}</strong> tools</span>
                <span><strong>{installedPackCount}</strong> active packs</span>
            </div>
        </div>
    </ToolToolbar>

    <div class="library-workspace">
        <aside class="pack-rail" aria-label="Tool packs">
            <div class="rail-heading">Tool packs</div>
            {#each buckets as bucket (bucket.id)}
                {@const PackIcon = bucket.icon}
                <button
                    type="button"
                    class="pack-nav"
                    class:is-active={activeBucket?.id === bucket.id}
                    style="--pack-color: {bucket.color};"
                    onclick={() => selectPack(bucket)}
                    aria-current={activeBucket?.id === bucket.id ? 'page' : undefined}
                >
                    <span class="pack-nav-icon" aria-hidden="true">
                        <PackIcon class="pack-nav-ico" />
                    </span>
                    <span class="pack-nav-copy">
                        <span class="pack-nav-name">{bucket.label}</span>
                        <span class="pack-nav-meta">{bucket.tools.length} tools</span>
                    </span>
                    <span class="pack-nav-state" class:is-installed={bucket.installed}>
                        {bucket.installed ? 'Active' : 'Available'}
                    </span>
                </button>
            {/each}
        </aside>

        <section class="library-content" aria-label={query.trim() ? 'Search results' : 'Selected tool pack'}>
            {#if displayedBuckets.length === 0}
                <EmptyState
                    icon={Search}
                    title="No tools found"
                    description={'No tools match "' + query.trim() + '". Try a different search.'}
                    variant="compact"
                />
            {:else}
                {#each displayedBuckets as bucket (bucket.id)}
                    {@const PackIcon = bucket.icon}
                    <section class="pack-detail" style="--pack-color: {bucket.color};">
                        <header class="pack-detail-head">
                            <div class="pack-detail-identity">
                                <span class="pack-detail-icon" aria-hidden="true">
                                    <PackIcon class="pack-detail-ico" />
                                </span>
                                <div>
                                    <span class="pack-detail-kicker">
                                        {query.trim()
                                            ? 'Search result'
                                            : bucket.installed
                                              ? 'Installed pack'
                                              : 'Available pack'}
                                    </span>
                                    <h2>{bucket.label}</h2>
                                    <p>{bucket.tagline}</p>
                                </div>
                            </div>
                            <div class="pack-detail-actions">
                                <span class="tool-count">{bucket.tools.length} tools</span>
                                {#if bucket.installed}
                                    <span class="status-pill is-installed" title="This pack is enabled">
                                        <Check class="status-ico" />
                                        ACTIVE
                                    </span>
                                {:else}
                                    <button
                                        type="button"
                                        class="status-pill is-available"
                                        onclick={() => openSettingsForPack(bucket)}
                                        title="Open Settings to enable this pack"
                                    >
                                        <Download class="status-ico" />
                                        ENABLE
                                    </button>
                                {/if}
                            </div>
                        </header>

                        <div class="tool-grid">
                            {#each bucket.tools as tool (tool.id)}
                                {@const ToolIcon = tool.icon}
                                <div class="tool-card-wrap">
                                    <button
                                        type="button"
                                        class="tool-card"
                                        class:is-locked={!bucket.installed}
                                        onclick={() => openTool(bucket, tool)}
                                        title={bucket.installed
                                            ? 'Open ' + tool.name
                                            : tool.name +
                                              ' is not enabled. Click to open Settings.'}
                                    >
                                        <span class="tool-card-icon" aria-hidden="true">
                                            <ToolIcon class="tool-ico" />
                                        </span>
                                        <span class="tool-card-body">
                                            <span class="tool-card-name">{tool.name}</span>
                                            <span class="tool-card-desc">{tool.description}</span>
                                        </span>
                                    </button>
                                    <button
                                        type="button"
                                        class="tool-pin"
                                        class:is-pinned={$pinnedToolIds.includes(tool.id)}
                                        aria-label={$pinnedToolIds.includes(tool.id)
                                            ? 'Unpin ' + tool.name
                                            : 'Pin ' + tool.name}
                                        aria-pressed={$pinnedToolIds.includes(tool.id)}
                                        title={$pinnedToolIds.includes(tool.id)
                                            ? 'Unpin ' + tool.name
                                            : 'Pin ' + tool.name}
                                        onclick={() => togglePinnedTool(tool.id)}
                                    >
                                        <Pin class="tool-pin-ico" />
                                    </button>
                                </div>
                            {/each}
                        </div>
                    </section>
                {/each}
            {/if}
        </section>
    </div>
</ToolPage>

<style>
    .library-toolbar {
        display: flex;
        align-items: center;
        gap: 16px;
        width: 100%;
    }

    .library-search {
        flex: 1;
        min-width: 0;
    }

    .library-counts {
        display: flex;
        align-items: center;
        gap: 10px;
        flex-shrink: 0;
        color: var(--color-text-secondary);
        font-size: 12px;
        white-space: nowrap;
    }

    .library-counts span {
        padding: 6px 9px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 999px;
    }

    .library-counts strong {
        color: var(--color-text);
        font-variant-numeric: tabular-nums;
    }

    .library-workspace {
        display: grid;
        grid-template-columns: 246px minmax(0, 1fr);
        gap: 12px;
    }

    .pack-rail,
    .library-content {
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
    }

    .pack-rail {
        align-self: start;
        padding: 8px;
    }

    .rail-heading {
        padding: 8px 10px 10px;
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.08em;
        text-transform: uppercase;
    }

    .pack-nav {
        display: grid;
        grid-template-columns: 32px minmax(0, 1fr) auto;
        align-items: center;
        gap: 10px;
        width: 100%;
        padding: 9px 10px;
        color: var(--color-text-secondary);
        background: transparent;
        border: 1px solid transparent;
        border-radius: 10px;
        cursor: pointer;
        text-align: left;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }

    .pack-nav:hover {
        background: color-mix(in srgb, var(--color-panel-2) 82%, transparent);
        color: var(--color-text);
    }

    .pack-nav.is-active {
        background: var(--color-panel-2);
        border-color: color-mix(in srgb, var(--color-accent) 30%, var(--color-border));
        color: var(--color-text);
    }

    .pack-nav:focus-visible,
    .tool-card:focus-visible,
    .status-pill.is-available:focus-visible {
        outline: none;
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 24%, transparent);
    }

    .pack-nav-icon,
    .pack-detail-icon,
    .tool-card-icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: none;
        color: var(--pack-color);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }

    .pack-nav-icon {
        width: 32px;
        height: 32px;
        border-radius: 8px;
    }

    .pack-nav-copy,
    .tool-card-body {
        display: flex;
        flex-direction: column;
        min-width: 0;
    }

    .pack-nav-name {
        color: inherit;
        font-size: 12.5px;
        font-weight: 600;
        line-height: 1.2;
    }

    .pack-nav-meta {
        margin-top: 2px;
        color: var(--color-muted);
        font-size: 11px;
        line-height: 1.2;
    }

    .pack-nav-state {
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.04em;
        text-transform: uppercase;
    }

    .pack-nav-state.is-installed {
        color: var(--color-accent);
    }

    .library-content {
        min-width: 0;
        padding: 22px;
    }

    .pack-detail + .pack-detail {
        margin-top: 24px;
        padding-top: 24px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 70%, transparent);
    }

    .pack-detail-head {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 20px;
        margin-bottom: 18px;
    }

    .pack-detail-identity {
        display: flex;
        align-items: flex-start;
        gap: 12px;
        min-width: 0;
    }

    .pack-detail-icon {
        width: 42px;
        height: 42px;
        border-radius: 11px;
    }

    .pack-detail-kicker {
        display: block;
        margin-bottom: 3px;
        color: var(--color-muted);
        font-size: 10px;
        font-weight: 700;
        letter-spacing: 0.08em;
        text-transform: uppercase;
    }

    .pack-detail h2 {
        margin: 0;
        color: var(--color-text);
        font-size: 17px;
        font-weight: 650;
        letter-spacing: -0.012em;
        line-height: 1.2;
    }

    .pack-detail p {
        max-width: 620px;
        margin: 4px 0 0;
        color: var(--color-text-secondary);
        font-size: 12.5px;
        line-height: 1.45;
    }

    .pack-detail-actions {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        flex-shrink: 0;
    }

    .tool-count,
    .status-pill {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        white-space: nowrap;
        border-radius: 999px;
    }

    .tool-count {
        height: 28px;
        padding: 0 10px;
        color: var(--color-muted);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        font-size: 11px;
        font-weight: 600;
    }

    .status-pill {
        gap: 6px;
        height: 28px;
        padding: 0 11px;
        border: 1px solid transparent;
        font-size: 10px;
        font-weight: 750;
        letter-spacing: 0.06em;
    }

    .status-pill.is-installed {
        color: var(--color-accent);
        border-color: color-mix(in srgb, var(--color-accent) 32%, var(--color-border));
    }

    .status-pill.is-available {
        color: var(--color-text-secondary);
        background: var(--color-panel-2);
        border-color: var(--color-border);
        cursor: pointer;
        transition:
            color var(--dur-micro, 130ms) var(--ease-out, ease),
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }

    .status-pill.is-available:hover {
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 10%, transparent);
        border-color: color-mix(in srgb, var(--color-accent) 32%, var(--color-border));
    }

    .tool-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
        gap: 10px;
    }

    .tool-card-wrap {
        position: relative;
        min-width: 0;
    }

    .tool-card {
        display: flex;
        align-items: flex-start;
        gap: 12px;
        width: 100%;
        height: 100%;
        min-width: 0;
        padding: 14px 44px 14px 14px;
        color: var(--color-text);
        background: color-mix(in srgb, var(--color-panel-2) 34%, transparent);
        border: 1px solid var(--color-border);
        border-radius: 10px;
        cursor: pointer;
        text-align: left;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            border-color var(--dur-micro, 130ms) var(--ease-out, ease),
            transform var(--dur-micro, 130ms) var(--ease-out, ease);
    }

    .tool-card:hover {
        background: var(--color-panel-2);
        border-color: color-mix(in srgb, var(--color-accent) 48%, var(--color-border));
        transform: translateY(-1px);
    }

    .tool-card:active {
        transform: translateY(0);
    }

    .tool-card.is-locked {
        opacity: 0.62;
    }

    .tool-card.is-locked:hover {
        opacity: 0.9;
    }

    .tool-pin {
        position: absolute;
        top: 10px;
        right: 10px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 28px;
        height: 28px;
        padding: 0;
        color: var(--color-muted);
        background: transparent;
        border: 0;
        border-radius: var(--radius-control, 8px);
        cursor: pointer;
    }

    .tool-pin:hover,
    .tool-pin.is-pinned {
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 10%, transparent);
    }

    .tool-pin:focus-visible {
        outline: none;
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 24%, transparent);
    }

    .tool-card-icon {
        width: 36px;
        height: 36px;
        border-radius: 9px;
    }

    .tool-card-name {
        color: var(--color-text);
        font-size: 13px;
        font-weight: 650;
        line-height: 1.25;
    }

    .tool-card-desc {
        display: -webkit-box;
        margin-top: 4px;
        overflow: hidden;
        color: var(--color-text-secondary);
        font-size: 11.5px;
        line-height: 1.42;
        -webkit-box-orient: vertical;
        -webkit-line-clamp: 2;
        line-clamp: 2;
    }

    .library-workspace :global(.pack-nav-ico) {
        width: 15px;
        height: 15px;
    }

    .library-workspace :global(.pack-detail-ico) {
        width: 19px;
        height: 19px;
    }

    .library-workspace :global(.tool-ico) {
        width: 16px;
        height: 16px;
    }

    .library-workspace :global(.tool-pin-ico) {
        width: 15px;
        height: 15px;
    }

    .library-workspace :global(.status-ico) {
        width: 12px;
        height: 12px;
    }

    @media (max-width: 820px) {
        .library-workspace {
            grid-template-columns: 1fr;
        }

        .pack-rail {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
            gap: 4px;
        }

        .rail-heading {
            grid-column: 1 / -1;
        }
    }

    @media (max-width: 620px) {
        .library-toolbar,
        .pack-detail-head {
            align-items: stretch;
            flex-direction: column;
        }

        .library-counts {
            flex-wrap: wrap;
        }

        .pack-rail {
            grid-template-columns: 1fr;
        }

        .library-content {
            padding: 16px;
        }

        .pack-detail-actions {
            align-self: flex-start;
        }

        .tool-grid {
            grid-template-columns: 1fr;
        }
    }
</style>
