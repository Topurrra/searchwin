<script lang="ts">
    /*
      Component-kit style guide. A dev / design surface — not part of the
      app's navigation. View it by pointing the running webview at /kit
      (in the dev devtools console: `location.assign('/kit')`).

      Every component renders here in its variants and states, across all
      four themes — the surface for judging the design and catching
      regressions as the kit evolves.
    */
    import { onMount } from 'svelte';
    import {
        Button,
        Card,
        SectionHeader,
        TextInput,
        Textarea,
        Select,
        Toggle,
        Checkbox,
        Tabs,
        Kbd,
        ListRow,
        EmptyState,
        LoadingState,
        // Phase 1 — UI/UX upgrade track primitives
        ToolPage,
        ToolToolbar,
        ToolPanel,
        ResultList,
        ResultRow,
        ErrorState,
        SideSheet,
    } from '$lib/ui';
    import {
        Search,
        Plus,
        Trash2,
        Download,
        Settings,
        FileText,
        Folder,
        Star,
        Inbox,
        ArrowRight,
        Copy,
        Shield,
        FileSearch,
        Clipboard,
        Sparkles,
        Mic,
        Wrench,
    } from '@lucide/svelte';

    const themes = ['dark', 'light', 'dracula', 'nord'];
    let theme = $state('dark');

    function setTheme(t: string) {
        theme = t;
        document.documentElement.dataset.theme = t;
    }

    onMount(() => {
        const current = document.documentElement.dataset.theme;
        if (current) theme = current;
        else document.documentElement.dataset.theme = theme;
    });

    // Live demo state
    let query = $state('');
    let note = $state('');
    let format = $state('json');
    let notifications = $state(false);
    let agree = $state(true);
    let beta = $state(false);
    let activeTab = $state('convert');

    // Phase 1 demo state
    let resultListMode = $state<'rows' | 'loading' | 'empty' | 'error'>('rows');
    let selectedRow = $state<string>('clipboard');
    let sheetOpen = $state(false);

    /** Pack-color tints for demoing the icon-tile color story.
     *  These match (or stand in for) the eventual per-pack token set. */
    const PACK_TINTS = {
        pdf: '#a78bfa',
        image: '#22d3ee',
        document: '#f472b6',
        development: '#34d399',
        archive: '#fbbf24',
        privacy: '#fb7185',
    } as const;

    const formatOptions = [
        { value: 'json', label: 'JSON' },
        { value: 'yaml', label: 'YAML' },
        { value: 'toml', label: 'TOML' },
        { value: 'xml', label: 'XML' },
    ];
    const demoTabs = [
        { id: 'convert', label: 'Convert', icon: FileText },
        { id: 'tree', label: 'Tree', icon: Folder },
        { id: 'settings', label: 'Settings', icon: Settings },
    ];
</script>

<div class="kit">
    <header class="kit-head">
        <div>
            <h1>KeepItLocal — Component Kit</h1>
            <p>
                The canonical Svelte 5 kit, built to <code>DESIGN.md</code>.
                Every surface composes from this.
            </p>
        </div>
        <div class="theme-switch" role="group" aria-label="Theme">
            {#each themes as t}
                <button
                    type="button"
                    class="theme-chip {theme === t ? 'is-on' : ''}"
                    onclick={() => setTheme(t)}>{t}</button
                >
            {/each}
        </div>
    </header>

    <section>
        <SectionHeader
            title="Buttons"
            description="One primary action per view — secondary is the default."
            icon={Star}
            divider
        />
        <div class="demo">
            <div class="row">
                <Button variant="primary" icon={Plus}>Primary</Button>
                <Button variant="secondary">Secondary</Button>
                <Button variant="ghost">Ghost</Button>
                <Button variant="danger" icon={Trash2}>Danger</Button>
            </div>
            <div class="row">
                <Button variant="primary" size="sm">Small</Button>
                <Button variant="secondary" size="sm" icon={Download}>Small</Button>
                <Button variant="secondary" loading>Loading</Button>
                <Button variant="secondary" disabled>Disabled</Button>
                <Button variant="secondary" iconOnly icon={Settings} title="Settings" />
                <Button variant="primary" iconTrailing={ArrowRight}>Continue</Button>
            </div>
        </div>
    </section>

    <section>
        <SectionHeader
            title="Inputs"
            description="Text fields, selects, and a multi-line area."
            icon={Search}
            divider
        />
        <div class="demo grid2">
            <TextInput bind:value={query} placeholder="Search…" icon={Search} />
            <Select bind:value={format} options={formatOptions} />
            <TextInput value="" placeholder="Invalid field" invalid />
            <TextInput value="Disabled" disabled />
        </div>
        <div class="demo">
            <Textarea bind:value={note} placeholder="A multi-line note…" rows={3} />
        </div>
    </section>

    <section>
        <SectionHeader
            title="Selection"
            description="Toggles and checkboxes."
            icon={Shield}
            divider
        />
        <div class="demo">
            <div class="row">
                <Toggle bind:checked={notifications} ariaLabel="Notifications" />
                <span class="lbl">Notifications {notifications ? 'on' : 'off'}</span>
            </div>
            <div class="row">
                <Checkbox bind:checked={agree} label="I agree to keep it local" />
                <Checkbox bind:checked={beta} label="Join the beta" />
                <Checkbox checked={false} disabled label="Disabled" />
            </div>
        </div>
    </section>

    <section>
        <SectionHeader
            title="Tabs"
            description="A macOS-style segmented control."
            icon={FileText}
            divider
        />
        <div class="demo">
            <Tabs tabs={demoTabs} bind:active={activeTab} ariaLabel="Demo tabs" />
            <p class="lbl">Active tab: {activeTab}</p>
        </div>
    </section>

    <section>
        <SectionHeader
            title="Cards & rows"
            description="Surfaces, list rows, and key caps."
            icon={Folder}
            divider
        />
        <div class="demo grid2">
            <Card>
                <h3>Quiet card</h3>
                <p>A hairline border, no shadow. The content is the hero.</p>
            </Card>
            <Card elevated>
                <h3>Elevated card</h3>
                <p>A soft layered shadow lifts it off the page.</p>
            </Card>
        </div>
        <div class="demo">
            <Card padding="sm">
                <ListRow
                    icon={FileText}
                    title="report-2026.pdf"
                    subtitle="2.4 MB · PDF"
                    onclick={() => {}}
                >
                    {#snippet trailing()}<Kbd keys="Enter" />{/snippet}
                </ListRow>
                <ListRow
                    icon={Folder}
                    title="Invoices"
                    subtitle="18 items"
                    selected
                    onclick={() => {}}
                >
                    {#snippet trailing()}
                        <Button size="sm" variant="ghost" iconOnly icon={Copy} title="Copy" />
                    {/snippet}
                </ListRow>
                <ListRow icon={Star} title="Starred" subtitle="3 items" onclick={() => {}} />
            </Card>
        </div>
        <div class="demo row">
            <span class="lbl">Shortcuts:</span>
            <Kbd keys="Ctrl+K" />
            <Kbd keys="Ctrl+Shift+P" />
            <Kbd keys="Esc" />
        </div>
    </section>

    <section>
        <SectionHeader
            title="States"
            description="Empty and loading states are designed, not afterthoughts."
            icon={Inbox}
            divider
        />
        <div class="demo grid2">
            <Card padding="none">
                <EmptyState
                    icon={Inbox}
                    title="Nothing here yet"
                    description="Drop a file to get started."
                />
            </Card>
            <Card>
                <LoadingState label="Working…" subLabel="Converting 3 files" />
            </Card>
        </div>
        <div class="demo">
            <ErrorState
                title="Couldn't reach the file index"
                description="The Tantivy index responded but the query failed to parse. Try a simpler search term."
                retry={() => {}}
                retryLabel="Retry"
            />
        </div>
    </section>

    <!-- ─── Phase 1: UI/UX upgrade track primitives ─────────────── -->
    <section class="phase-flag">
        <SectionHeader
            title="ToolPage — the new page header"
            description="Replaces the radial-gradient hero. Compact, calm, accent reserved for actions."
            icon={Wrench}
            divider
        />
        <div class="demo phase-frame">
            <ToolPage
                icon={FileSearch}
                iconTint={PACK_TINTS.development}
                title="Find anything on your machine"
                description="Multi-pass, typo-tolerant, frecency-ranked search. Indexes locally, nothing leaves your device."
                width="wide"
                fill={false}
            >
                {#snippet actions()}
                    <Button variant="ghost" icon={Settings}>Manage index</Button>
                    <Button variant="primary" icon={Search}>Search</Button>
                {/snippet}
                <ToolToolbar>
                    {#snippet left()}
                        <TextInput
                            bind:value={query}
                            placeholder="Search files, folders, apps…"
                            icon={Search}
                        />
                    {/snippet}
                    {#snippet right()}
                        <Button variant="secondary" size="sm">Filters</Button>
                        <Kbd keys="Ctrl+K" />
                    {/snippet}
                </ToolToolbar>
                <ToolPanel padding="sm">
                    <p class="dim-note">
                        The page body lives here. Pack-color tint on the icon, neutral surface everywhere else.
                    </p>
                </ToolPanel>
                {#snippet footer()}
                    <span><Kbd keys="Enter" /> Run</span>
                    <span><Kbd keys="Ctrl+,"/> Settings</span>
                    <span><Kbd keys="Esc" /> Close</span>
                {/snippet}
            </ToolPage>
        </div>
    </section>

    <section>
        <SectionHeader
            title="ToolPanel — work surface"
            description="The card that holds the work. Padding, tone, and elevation are tokenized."
            icon={Folder}
            divider
        />
        <div class="demo grid2">
            <ToolPanel padding="md" tone="panel">
                <h3>Default tone</h3>
                <p>bg-panel, hairline border, no elevation.</p>
            </ToolPanel>
            <ToolPanel padding="md" tone="panel-2" elevated>
                <h3>Recessed + elevated</h3>
                <p>bg-panel-2 with a soft shadow.</p>
            </ToolPanel>
            <ToolPanel padding="md" tone="accent">
                <h3>Accent-tinted</h3>
                <p>For "active / featured" surfaces only.</p>
            </ToolPanel>
            <ToolPanel padding="none">
                <ListRow
                    icon={FileText}
                    title="Compact list panel"
                    subtitle="padding=none lets list rows go edge-to-edge"
                />
                <ListRow icon={Folder} title="A second row" subtitle="No gutter" />
            </ToolPanel>
        </div>
    </section>

    <section>
        <SectionHeader
            title="ResultList + ResultRow — the Raycast-style list"
            description="One primitive for the four states: rows, loading, empty, error."
            icon={Search}
            divider
        />
        <div class="demo">
            <div class="phase-state-switch" role="group" aria-label="Demo state">
                {#each [
                    { id: 'rows', label: 'Rows' },
                    { id: 'loading', label: 'Loading' },
                    { id: 'empty', label: 'Empty' },
                    { id: 'error', label: 'Error' },
                ] as opt}
                    <button
                        type="button"
                        class="state-chip"
                        class:is-on={resultListMode === opt.id}
                        onclick={() => (resultListMode = opt.id as typeof resultListMode)}
                    >
                        {opt.label}
                    </button>
                {/each}
            </div>
            <ToolPanel padding="sm">
                <ResultList
                    groupLabel="Suggested Tools"
                    loading={resultListMode === 'loading'}
                    empty={resultListMode === 'empty'}
                    emptyIcon={Inbox}
                    emptyTitle="No matches"
                    emptyDescription="Try a different search term or clear filters."
                    error={resultListMode === 'error' ? 'Connection lost — local index is offline.' : null}
                    retry={() => (resultListMode = 'rows')}
                >
                    <ResultRow
                        icon={Clipboard}
                        iconTint={PACK_TINTS.development}
                        title="Clipboard History"
                        subtitle="324 entries · 18 pinned"
                        meta="Just now"
                        selected={selectedRow === 'clipboard'}
                        onclick={() => (selectedRow = 'clipboard')}
                    >
                        {#snippet trailing()}
                            <Kbd keys="Enter" />
                        {/snippet}
                    </ResultRow>
                    <ResultRow
                        icon={Sparkles}
                        iconTint={PACK_TINTS.image}
                        title="Snippets"
                        subtitle={`12 saved · {{date}} expands to today`}
                        meta="2m"
                        selected={selectedRow === 'snippets'}
                        onclick={() => (selectedRow = 'snippets')}
                    />
                    <ResultRow
                        icon={Mic}
                        iconTint={PACK_TINTS.privacy}
                        title="Voice to Text"
                        subtitle="Offline Vosk session ready"
                        meta="Idle"
                        selected={selectedRow === 'voice'}
                        onclick={() => (selectedRow = 'voice')}
                    />
                    <ResultRow
                        icon={FileSearch}
                        title="File Search"
                        subtitle="292,108 files indexed"
                        meta="Active"
                        selected={selectedRow === 'search'}
                        onclick={() => (selectedRow = 'search')}
                    />
                </ResultList>
            </ToolPanel>
        </div>
    </section>

    <section>
        <SectionHeader
            title="SideSheet — tool-local settings panel"
            description="Slide-in from the right at 420px. Esc / scrim / close button all dismiss."
            icon={Settings}
            divider
        />
        <div class="demo">
            <Button variant="primary" icon={Settings} onclick={() => (sheetOpen = true)}>
                Open SideSheet
            </Button>
            <SideSheet
                open={sheetOpen}
                title="Clipboard settings"
                onclose={() => (sheetOpen = false)}
            >
                <p class="dim-note">
                    SideSheet body — typically the tool's settings live here. Wire forms / toggles
                    using the existing kit primitives.
                </p>
                <Toggle bind:checked={notifications} ariaLabel="Pause capture" />
                <span class="lbl">Pause capture {notifications ? 'on' : 'off'}</span>
                <hr class="dim-divider" />
                <p class="dim-note">
                    Footer slot is for primary/secondary actions ("Reset", "Save").
                </p>
                {#snippet footer()}
                    <Button variant="ghost" onclick={() => (sheetOpen = false)}>Cancel</Button>
                    <Button variant="primary" onclick={() => (sheetOpen = false)}>Save</Button>
                {/snippet}
            </SideSheet>
        </div>
    </section>
</div>

<style>
    .kit {
        max-width: 880px;
        margin: 0 auto;
        padding: 40px 32px 80px;
        height: 100%;
        overflow-y: auto;
    }
    .kit-head {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 24px;
    }
    .kit-head h1 {
        margin: 0;
        font-size: 22px;
        font-weight: 600;
    }
    .kit-head p {
        margin: 6px 0 0;
        max-width: 420px;
        font-size: 13px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }
    .kit-head code {
        font-size: 12px;
        padding: 1px 5px;
        border-radius: 5px;
        background: var(--color-panel-2);
    }
    .theme-switch {
        flex: none;
        display: inline-flex;
        gap: 2px;
        padding: 3px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
    }
    .theme-chip {
        height: 26px;
        padding: 0 10px;
        border: none;
        background: transparent;
        border-radius: 6px;
        font-size: 12px;
        font-weight: 500;
        text-transform: capitalize;
        color: var(--color-text-secondary);
    }
    .theme-chip.is-on {
        background: var(--color-panel);
        color: var(--color-text);
        box-shadow: var(--shadow-sm);
    }
    section {
        margin-top: 40px;
    }
    .demo {
        margin-top: 16px;
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .demo.grid2 {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 12px;
    }
    .row {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 10px;
    }
    .lbl {
        font-size: 13px;
        color: var(--color-text-secondary);
    }
    .demo :global(h3) {
        margin: 0 0 4px;
        font-size: 14px;
        font-weight: 600;
    }
    .demo :global(.card p) {
        margin: 0;
        font-size: 13px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }

    /* ─── Phase 1 demo helpers ───────────────────────────────── */
    /* Visual flag so we can spot the new primitives at a glance
       while iterating. Subtle accent left-strip. */
    .phase-flag :global(h2) {
        position: relative;
    }
    .phase-flag :global(h2)::before {
        content: '';
        position: absolute;
        left: -10px;
        top: 4px;
        width: 3px;
        height: calc(100% - 8px);
        border-radius: 999px;
        background: var(--color-accent);
        opacity: 0.7;
    }
    /* Inset frame so the demo'd ToolPage doesn't fill the whole
       kit canvas — gives a visible "edge" to judge its layout. */
    .phase-frame {
        border: 1px dashed var(--color-divider, var(--color-border));
        border-radius: var(--radius-card);
        padding: 4px;
        background: var(--color-bg);
        min-height: 260px;
    }
    .phase-frame :global(.tool-page) {
        height: auto;
    }
    .dim-note {
        margin: 0;
        font-size: 13px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }
    .dim-divider {
        border: none;
        border-top: 1px solid var(--color-divider, var(--color-border));
        margin: 16px 0;
    }
    .phase-state-switch {
        display: inline-flex;
        gap: 2px;
        padding: 3px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        margin-bottom: 12px;
    }
    .state-chip {
        height: 26px;
        padding: 0 11px;
        border: none;
        background: transparent;
        border-radius: 6px;
        font-size: 12px;
        font-weight: 500;
        color: var(--color-text-secondary);
        cursor: pointer;
        transition:
            background-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out);
    }
    .state-chip:hover:not(.is-on) {
        color: var(--color-text);
    }
    .state-chip.is-on {
        background: var(--color-panel);
        color: var(--color-text);
        box-shadow: var(--shadow-sm);
    }
</style>
