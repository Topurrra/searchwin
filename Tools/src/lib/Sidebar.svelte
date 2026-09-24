<script lang="ts">
    import { invoke } from '@tauri-apps/api/core';
    import { getScreen } from '$lib/appScreens';
    import { settings } from '$lib/stores/settings';
    import { pinnedToolIds } from '$lib/stores/toolPacks';
    import {
        Activity,
        BookOpen,
        Command,
        Clock3,
        NotebookPen,
        Pin,
        ShieldCheck,
        Settings as SettingsIcon,
        ChevronsLeft,
        ChevronsRight,
        Zap,
    } from '@lucide/svelte';

    interface NavItem {
        id: string;
        label: string;
        icon: any;
    }

    let { selected = $bindable() }: { selected: string } = $props();

    let collapsed = $derived($settings.sidebarCollapsed);
    const continuityScreenIds = new Set(['home', 'pinned', 'command', 'tool-packs']);

    /**
     * Navigate to a screen by id. Frecency is recorded so the overlay's
     * Suggested Tools learns the user's workflow even when they navigate
     * from here instead of the command palette.
     */
    function selectScreen(id: string) {
        selected = id;
        if (!continuityScreenIds.has(id)) {
            void invoke('record_frecency_launch', { kind: 'tool', path: id }).catch(() => {});
        }
    }

    function toggleCollapse() {
        settings.update((s) => ({ ...s, sidebarCollapsed: !s.sidebarCollapsed }));
    }

    const continuity: NavItem[] = [
        { id: 'pinned', label: 'Pinned', icon: Pin },
        { id: 'tool-packs', label: 'Library', icon: BookOpen },
    ];

    const workspace: NavItem[] = [
        { id: 'home', label: 'Recent', icon: Clock3 },
        { id: 'command', label: 'Command', icon: Command },
        { id: 'notes', label: 'Notes', icon: NotebookPen },
        { id: 'time-tracker', label: 'Time Tracker', icon: Activity },
        { id: 'my-commands', label: 'My Commands', icon: Zap },
        { id: 'privacy-audit', label: 'Privacy Audit', icon: ShieldCheck },
    ];

    const pinnedTools = $derived.by<NavItem[]>(() =>
        $pinnedToolIds.flatMap((id) => {
            const screen = getScreen(id);
            if (!screen) return [];
            return [{ id: screen.id, label: screen.name, icon: screen.kind === 'tool' ? screen.icon : Pin }];
        }),
    );

</script>

<aside class="sidebar" class:collapsed>
    <!-- Brand lives in the TitleBar. Command button + Local-Only badge
         were removed at user request; the sidebar is now navigation only. -->

    <nav class="nav">
        {#if !collapsed}
            <div class="section-label">Workspaces</div>
        {/if}
        {#each workspace as item (item.id)}
            {@const Icon = item.icon}
            <button
                type="button"
                class="nav-item"
                class:is-active={selected === item.id}
                onclick={() => selectScreen(item.id)}
                title={item.label}
                aria-current={selected === item.id ? 'page' : undefined}
            >
                <Icon class="nav-ico" />
                {#if !collapsed}<span class="nav-label">{item.label}</span>{/if}
            </button>
        {/each}
        {#if !collapsed}
            <div class="section-label">Continuity</div>
        {/if}
        {#each continuity as item (item.id)}
            {@const Icon = item.icon}
            <button
                type="button"
                class="nav-item"
                class:is-active={selected === item.id}
                onclick={() => selectScreen(item.id)}
                title={item.label}
                aria-current={selected === item.id ? 'page' : undefined}
            >
                <Icon class="nav-ico" />
                {#if !collapsed}<span class="nav-label">{item.label}</span>{/if}
            </button>
        {/each}
        {#if pinnedTools.length}
            {#if !collapsed}
                <div class="section-label">Pinned tools</div>
            {/if}
            {#each pinnedTools as item (item.id)}
                {@const Icon = item.icon}
                <button
                    type="button"
                    class="nav-item"
                    class:is-active={selected === item.id}
                    onclick={() => selectScreen(item.id)}
                    title={item.label}
                    aria-current={selected === item.id ? 'page' : undefined}
                >
                    <Icon class="nav-ico" />
                    {#if !collapsed}<span class="nav-label">{item.label}</span>{/if}
                </button>
            {/each}
        {/if}
    </nav>

    <button
        type="button"
        class="nav-item nav-settings"
        class:is-active={selected === 'settings'}
        onclick={() => selectScreen('settings')}
        title="Settings"
        aria-current={selected === 'settings' ? 'page' : undefined}
    >
        <SettingsIcon class="nav-ico" />
        {#if !collapsed}<span class="nav-label">Settings</span>{/if}
    </button>
    <button
        type="button"
        class="collapse-btn"
        onclick={toggleCollapse}
        title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
    >
        {#if collapsed}
            <ChevronsRight class="collapse-ico" />
        {:else}
            <ChevronsLeft class="collapse-ico" />
            <span class="collapse-label">Collapse</span>
        {/if}
    </button>
</aside>

<style>
    .sidebar {
        display: flex;
        flex-direction: column;
        height: 100%;
        padding: 12px 10px 8px;
        background: var(--color-panel);
        border-right: 1px solid var(--color-border);
        transition: width var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .sidebar.collapsed {
        width: 56px;
        padding: 12px 8px 8px;
    }

    /* ─── Nav ──────────────────────────────────────────────────── */
    .nav {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        display: flex;
        flex-direction: column;
        gap: 1px;
    }
    .section-label {
        margin: 12px 6px 4px;
        font-size: 10px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }
    .section-label:first-child {
        margin-top: 2px;
    }
    .nav-item {
        /* `position: relative` is required so the `.is-active::before`
           pill indicator (drawn below) can absolute-position against
           the item, not against an ancestor. */
        position: relative;
        display: flex;
        align-items: center;
        gap: 10px;
        width: 100%;
        height: 30px;
        padding: 0 8px;
        background: transparent;
        border: none;
        border-radius: var(--radius-control, 8px);
        color: var(--color-text);
        font-size: 13px;
        font-weight: 500;
        text-align: left;
        cursor: pointer;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .nav-item:hover:not(.is-active) {
        background: var(--color-panel-2);
    }
    /* Selected/active item — the canonical pattern (see
       feedback_selected_item_pattern.md):
         - background: neutral panel-2 (NOT accent-tinted)
         - left strip: accent, rounded as a pill, inset top/bottom
           via a ::before pseudo (not full-height box-shadow)
         - icon: turns accent (handled by the `:global(.nav-ico)` rule
           further down — kept as-is)
         - text: stays at the regular `--color-text`
       Accent lives in the indicators, never on the surface itself. */
    .nav-item.is-active {
        background: var(--color-panel-2);
    }
    .nav-item.is-active::before {
        content: '';
        position: absolute;
        left: 2px;
        top: 6px;
        bottom: 6px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .sidebar :global(.nav-ico) {
        flex: none;
        width: 16px;
        height: 16px;
        color: var(--color-text-secondary);
    }
    .nav-item.is-active :global(.nav-ico) {
        color: var(--color-accent);
    }
    .nav-label {
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .sidebar.collapsed .nav-item {
        justify-content: center;
        padding: 0;
    }

    /* ─── Settings + collapse pinned to the bottom ─────────────── */
    .nav-settings {
        margin-top: 4px;
    }
    /* Mirror the nav-item geometry exactly in expanded mode so the
       collapse row reads as part of the same vertical rail (icon at
       8px from the left edge, label flowing right of it). In collapsed
       mode we override to icon-centered. */
    .collapse-btn {
        margin-top: 6px;
        display: flex;
        align-items: center;
        gap: 10px;
        width: 100%;
        height: 30px;
        padding: 0 8px;
        background: transparent;
        border: none;
        border-radius: var(--radius-control, 8px);
        color: var(--color-muted);
        font-size: 12px;
        font-weight: 500;
        text-align: left;
        cursor: pointer;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .collapse-btn:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .collapse-label {
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .sidebar :global(.collapse-ico) {
        flex: none;
        width: 16px;
        height: 16px;
    }
    /* Collapsed sidebar: shrink the button to icon-only, centered, no
       label. Matches the .sidebar.collapsed .nav-item rule above so all
       bottom-rail items (Settings + Collapse) keep alignment in both
       expanded and collapsed states. */
    .sidebar.collapsed .collapse-btn {
        justify-content: center;
        padding: 0;
        gap: 0;
    }
</style>
