<script lang="ts">
    /*
      My Commands — standalone management surface for user-defined palette
      commands (quicklinks, custom bangs, file/folder/app launchers, and
      trusted shell commands).

      Promoted out of Settings → My Commands into a first-class sidebar tool
      (2026-06-21). The UI is the SAME surface that lived in Settings, moved
      verbatim onto the ToolPage kit; the `myCommands` store still owns all
      CRUD, persistence (localStorage), cross-window sync, and the seeded
      bang defaults — this component is purely a view over it.
    */
    import {
        Link as LinkIcon,
        Globe,
        AppWindow,
        FileText,
        Folder as FolderIcon,
        Terminal,
        Plus,
        X,
        Trash2,
        Search as SearchIcon,
        Zap,
        FolderOpen,
        Save as SaveIcon,
        AlertTriangle,
    } from '@lucide/svelte';
    import { open as openFileDialog } from '@tauri-apps/plugin-dialog';
    import { ToolPage, Button, Pager } from '$lib/ui';
    import { errorToast } from '$lib/stores/errorToast';
    import {
        myCommands,
        addCommand,
        updateCommand,
        removeCommand,
        type MyCommand,
        type MyCommandType,
        type MyShellKind,
    } from '$lib/stores/myCommands';

    let mcModalOpen = $state(false);
    let mcEditingId = $state<string | null>(null);
    /** Ref to the modal's keyword input. We focus it on open via a
     *  user-initiated $effect rather than an autofocus attribute —
     *  the a11y guideline: focus only on user action, never on render. */
    let mcKeywordInput = $state<HTMLInputElement | null>(null);
    let mcKeyword = $state('');
    let mcLabel = $state('');
    let mcType = $state<MyCommandType>('url');
    let mcTarget = $state('');
    let mcDescription = $state('');
    // Wave 4.1b-2 (2026-05-27): which shell to use for type === 'shell'.
    // Defaults to 'powershell' (matches Wave 4.1 behavior). The picker
    // is only rendered when mcType === 'shell'.
    let mcShellKind = $state<MyShellKind>('powershell');
    /** Search filter applied to the visible list. Matches against
     *  keyword, label, target, and description. */
    let mcSearchQuery = $state('');
    /** Type filter chip selection — 'all' = no filter. */
    let mcTypeFilter = $state<'all' | MyCommandType>('all');

    let mcTargetPlaceholder = $derived(
        mcType === 'bang'
            ? 'https://www.google.com/search?q={query}'
            : mcType === 'url'
              ? 'https://github.com/me/repo'
              : mcType === 'app'
                ? 'C:\\Path\\To\\App.exe'
                : mcType === 'folder'
                  ? 'C:\\Users\\me\\Projects'
                  : mcType === 'shell'
                    ? 'git status -s'
                    : 'C:\\Users\\me\\notes.txt',
    );
    /** Field label text per type — shell needs different framing
     *  (it's a command string, not a path or URL). */
    let mcTargetFieldLabel = $derived(
        mcType === 'bang'
            ? 'URL template — put {query} where the search text goes'
            : mcType === 'url'
              ? 'URL'
              : mcType === 'shell'
                ? 'Shell command — runs via PowerShell; you trust what you wrote'
                : 'Path',
    );
    /** Lucide icon component for a given type — used in the card list. */
    function mcTypeIcon(type: MyCommandType) {
        switch (type) {
            case 'url':
                return LinkIcon;
            case 'bang':
                return Globe;
            case 'app':
                return AppWindow;
            case 'file':
                return FileText;
            case 'folder':
                return FolderIcon;
            case 'shell':
                return Terminal;
        }
    }
    /** Human label for type-filter chips. */
    const MC_TYPE_LABELS: Record<MyCommandType, string> = {
        url: 'URL',
        bang: 'Bang',
        app: 'App',
        file: 'File',
        folder: 'Folder',
        shell: 'Shell',
    };
    /** Visible-commands derived: applies search + type filter. */
    let mcFilteredCommands = $derived.by(() => {
        const all = $myCommands;
        const q = mcSearchQuery.trim().toLowerCase();
        return all.filter((cmd) => {
            if (mcTypeFilter !== 'all' && cmd.type !== mcTypeFilter) return false;
            if (!q) return true;
            return (
                cmd.keyword.toLowerCase().includes(q) ||
                cmd.label.toLowerCase().includes(q) ||
                cmd.target.toLowerCase().includes(q) ||
                (cmd.description ?? '').toLowerCase().includes(q)
            );
        });
    });

    // Paginate the filtered command list so a large custom-command set stays
    // browsable. The Pager clamps the page when the set shrinks (deletes/filter);
    // we also jump back to page 1 whenever the search or type filter changes.
    const MC_PAGE_SIZE = 10;
    let mcPage = $state(1);
    let mcPagedCommands = $derived(
        mcFilteredCommands.slice((mcPage - 1) * MC_PAGE_SIZE, mcPage * MC_PAGE_SIZE),
    );
    $effect(() => {
        void mcSearchQuery;
        void mcTypeFilter;
        mcPage = 1;
    });

    function mcReset() {
        mcEditingId = null;
        mcKeyword = '';
        mcLabel = '';
        mcType = 'url';
        mcTarget = '';
        mcDescription = '';
        mcShellKind = 'powershell';
    }
    function mcOpenAddModal() {
        mcReset();
        mcModalOpen = true;
    }

    /** Focus the keyword input on each modal open (add OR edit). $effect
     *  fires after the modal mounts, so the ref is wired up. requestAnim-
     *  ationFrame defers one tick so Svelte's bind:this has populated. */
    $effect(() => {
        if (mcModalOpen) {
            requestAnimationFrame(() => mcKeywordInput?.focus());
        }
    });
    function mcStartEdit(cmd: MyCommand) {
        mcEditingId = cmd.id;
        mcKeyword = cmd.keyword;
        mcLabel = cmd.label;
        mcType = cmd.type;
        mcTarget = cmd.target;
        mcDescription = cmd.description ?? '';
        mcShellKind = cmd.shellKind ?? 'powershell';
        mcModalOpen = true;
    }
    function mcCloseModal() {
        mcModalOpen = false;
        mcReset();
    }
    function mcSubmit() {
        if (!mcKeyword.trim() || !mcTarget.trim()) return;
        if (mcEditingId) {
            updateCommand(mcEditingId, {
                keyword: mcKeyword,
                label: mcLabel,
                type: mcType,
                target: mcTarget,
                description: mcDescription,
                shellKind: mcType === 'shell' ? mcShellKind : undefined,
            });
        } else {
            addCommand({
                keyword: mcKeyword,
                label: mcLabel,
                type: mcType,
                target: mcTarget,
                description: mcDescription,
                shellKind: mcType === 'shell' ? mcShellKind : undefined,
            });
        }
        mcCloseModal();
    }
    function mcDelete(id: string) {
        removeCommand(id);
        if (mcEditingId === id) mcCloseModal();
    }
    // Native file/folder picker for app/file/folder command types — typing
    // a full Windows path by hand is error-prone, so let the user point at
    // the real thing. URL/bang types keep manual entry (they're not paths).
    async function mcBrowse() {
        try {
            const picked = await openFileDialog(
                mcType === 'folder'
                    ? { directory: true, multiple: false }
                    : {
                          directory: false,
                          multiple: false,
                          ...(mcType === 'app'
                              ? {
                                    filters: [
                                        {
                                            name: 'Applications',
                                            extensions: ['exe', 'lnk', 'bat', 'cmd'],
                                        },
                                    ],
                                }
                              : {}),
                      },
            );
            if (typeof picked === 'string' && picked.length) {
                mcTarget = picked;
                // Auto-fill the label from the basename if the user hasn't
                // typed one yet — saves a step for the common case.
                if (!mcLabel.trim()) {
                    const base = picked.split(/[\\/]/).pop() ?? picked;
                    mcLabel = base.replace(/\.(exe|lnk|bat|cmd)$/i, '');
                }
            }
        } catch (error) {
            errorToast("Couldn't open the file picker", error, {
                hint: 'Try again — Windows occasionally refuses dialog focus during heavy activity.',
            });
        }
    }
</script>

<ToolPage
    icon={Zap}
    title="My Commands"
    description="Keyword shortcuts you run from the command palette — quicklinks, search bangs, files, folders, apps, and trusted shell commands. Saved locally on this device."
    width="wide"
>
    {#snippet actions()}
        <Button variant="primary" icon={Plus} onclick={mcOpenAddModal}>
            Add command
        </Button>
    {/snippet}

    <!-- Toolbar: search + type filter chips -->
    <div class="mc-toolbar">
        <div class="mc-search">
            <SearchIcon class="mc-search-ico" />
            <input
                class="mc-search-input"
                bind:value={mcSearchQuery}
                placeholder="Search commands…"
                spellcheck="false"
            />
            {#if mcSearchQuery}
                <button
                    type="button"
                    class="mc-search-clear"
                    aria-label="Clear search"
                    onclick={() => (mcSearchQuery = '')}
                >
                    <X class="mc-search-clear-ico" />
                </button>
            {/if}
        </div>
        <div class="mc-type-chips" role="group" aria-label="Filter by type">
            <button
                type="button"
                class="mc-type-chip"
                class:is-on={mcTypeFilter === 'all'}
                onclick={() => (mcTypeFilter = 'all')}
            >
                All
                <span class="mc-type-chip-count">{$myCommands.length}</span>
            </button>
            {#each ['url', 'bang', 'app', 'file', 'folder', 'shell'] as t (t)}
                {@const tt = t as MyCommandType}
                {@const TypeIco = mcTypeIcon(tt)}
                {@const count = $myCommands.filter((c) => c.type === tt).length}
                {#if count > 0 || mcTypeFilter === tt}
                    <button
                        type="button"
                        class="mc-type-chip"
                        class:is-on={mcTypeFilter === tt}
                        onclick={() => (mcTypeFilter = tt)}
                    >
                        <TypeIco class="mc-type-chip-ico" />
                        {MC_TYPE_LABELS[tt]}
                        <span class="mc-type-chip-count">{count}</span>
                    </button>
                {/if}
            {/each}
        </div>
    </div>

    {#if $myCommands.length === 0}
        <div class="mc-empty">
            <Zap class="mc-empty-ico" />
            <p class="mc-empty-title">No commands yet</p>
            <p class="mc-empty-desc">
                Add one above, then run it from the command palette by typing its keyword.
            </p>
        </div>
    {:else if mcFilteredCommands.length === 0}
        <div class="mc-empty">
            <p class="mc-empty-title">No matches</p>
            <p class="mc-empty-desc">
                Nothing matches "{mcSearchQuery}"{mcTypeFilter !== 'all' ? ` in ${MC_TYPE_LABELS[mcTypeFilter]}` : ''}. Try a different filter.
            </p>
        </div>
    {:else}
        <div class="mc-list">
            {#each mcPagedCommands as cmd (cmd.id)}
                {@const TypeIco = mcTypeIcon(cmd.type)}
                <div class="mc-card" class:is-editing={mcEditingId === cmd.id}>
                    <div class="mc-card-icon mc-icon-{cmd.type}" aria-hidden="true">
                        <TypeIco class="mc-card-icon-svg" />
                    </div>
                    <div class="mc-card-text">
                        <div class="mc-card-row">
                            <span class="mc-kw">{cmd.keyword}</span>
                            <span class="mc-card-label">{cmd.label}</span>
                        </div>
                        <div class="mc-card-target">{cmd.target}</div>
                        {#if cmd.description}
                            <div class="mc-card-desc">{cmd.description}</div>
                        {/if}
                    </div>
                    <div class="mc-card-actions">
                        <Button
                            variant="secondary"
                            size="sm"
                            onclick={() => mcStartEdit(cmd)}
                        >
                            Edit
                        </Button>
                        <Button
                            variant="danger"
                            size="sm"
                            icon={Trash2}
                            onclick={() => mcDelete(cmd.id)}
                        >
                            Delete
                        </Button>
                    </div>
                </div>
            {/each}
        </div>
        <Pager
            total={mcFilteredCommands.length}
            bind:page={mcPage}
            pageSize={MC_PAGE_SIZE}
            label="commands"
        />
    {/if}

    <!-- Add/Edit modal — replaces the inline form for a more focused flow.
         Backdrop click + Esc close; Enter submits when both required fields
         are filled. -->
    {#if mcModalOpen}
        <div
            class="mc-modal-backdrop"
            role="presentation"
            onclick={(event) => {
                if (event.target === event.currentTarget) mcCloseModal();
            }}
            onkeydown={(event) => {
                if (event.key === 'Escape') mcCloseModal();
            }}
        >
            <div
                class="mc-modal"
                role="dialog"
                aria-modal="true"
                aria-labelledby="mc-modal-title"
            >
                <header class="mc-modal-head">
                    <h3 id="mc-modal-title">
                        {mcEditingId ? 'Edit command' : 'Add command'}
                    </h3>
                    <button
                        type="button"
                        class="mc-modal-close"
                        aria-label="Close"
                        onclick={mcCloseModal}
                    >
                        <X class="mc-modal-close-ico" />
                    </button>
                </header>

                <div class="mc-modal-body">
                    <div class="mc-grid">
                        <label class="mc-field">
                            <span class="mc-flabel">Keyword</span>
                            <input
                                class="mc-input"
                                bind:this={mcKeywordInput}
                                bind:value={mcKeyword}
                                placeholder="gh"
                                spellcheck="false"
                            />
                        </label>
                        <label class="mc-field">
                            <span class="mc-flabel">Label</span>
                            <input
                                class="mc-input"
                                bind:value={mcLabel}
                                placeholder="My GitHub repo"
                            />
                        </label>
                        <label class="mc-field">
                            <span class="mc-flabel">Type</span>
                            <select class="mc-input" bind:value={mcType}>
                                <option value="url">URL (quicklink)</option>
                                <option value="bang">Bang ({'{query}'})</option>
                                <option value="app">App</option>
                                <option value="file">File</option>
                                <option value="folder">Folder</option>
                                <option value="shell">Shell command</option>
                            </select>
                        </label>
                    </div>
                    <label class="mc-field mc-field-full">
                        <span class="mc-flabel">{mcTargetFieldLabel}</span>
                        {#if mcType === 'app' || mcType === 'file' || mcType === 'folder'}
                            <div class="mc-target-row">
                                <input
                                    class="mc-input"
                                    bind:value={mcTarget}
                                    placeholder={mcTargetPlaceholder}
                                    spellcheck="false"
                                />
                                <Button
                                    variant="secondary"
                                    icon={FolderOpen}
                                    onclick={mcBrowse}
                                >
                                    Browse…
                                </Button>
                            </div>
                        {:else if mcType === 'shell'}
                            <textarea
                                class="mc-input mc-input-textarea"
                                bind:value={mcTarget}
                                placeholder={mcTargetPlaceholder}
                                spellcheck="false"
                                rows="3"
                            ></textarea>
                            <!-- Wave 4.1b-2 (2026-05-27):
                                 shell kind picker.
                                 Defaults to PowerShell —
                                 the only one guaranteed
                                 installed on every modern
                                 Windows. cmd is for
                                 `for /f` / .bat quirks;
                                 pwsh for users who have
                                 PS 7+. -->
                            <div class="mc-shellkind">
                                <span class="mc-shellkind-label">Shell:</span>
                                <div class="mc-shellkind-pills">
                                    {#each [
                                        { value: 'powershell', label: 'PowerShell', hint: 'Windows PowerShell 5.1 (default)' },
                                        { value: 'pwsh', label: 'pwsh', hint: 'PowerShell 7+ (Core, if installed)' },
                                        { value: 'cmd', label: 'cmd', hint: 'Legacy cmd.exe — for .bat / for /f recipes' },
                                    ] as opt}
                                        <button
                                            type="button"
                                            class="mc-shellkind-pill"
                                            class:is-on={mcShellKind === opt.value}
                                            onclick={() => (mcShellKind = opt.value as MyShellKind)}
                                            title={opt.hint}
                                        >
                                            {opt.label}
                                        </button>
                                    {/each}
                                </div>
                            </div>
                            <p class="mc-field-hint mc-field-hint-warn">
                                <AlertTriangle class="mc-field-hint-ico" />
                                Runs with your account's permissions. Only commands you trust.
                            </p>
                        {:else}
                            <input
                                class="mc-input"
                                bind:value={mcTarget}
                                placeholder={mcTargetPlaceholder}
                                spellcheck="false"
                            />
                        {/if}
                    </label>
                    <label class="mc-field mc-field-full">
                        <span class="mc-flabel">Description (optional)</span>
                        <input
                            class="mc-input"
                            bind:value={mcDescription}
                            placeholder="Helpful note about what this does"
                        />
                    </label>
                </div>

                <footer class="mc-modal-foot">
                    <Button variant="secondary" onclick={mcCloseModal}>
                        Cancel
                    </Button>
                    <Button
                        variant="primary"
                        icon={SaveIcon}
                        disabled={!mcKeyword.trim() || !mcTarget.trim()}
                        onclick={mcSubmit}
                    >
                        {mcEditingId ? 'Save changes' : 'Add command'}
                    </Button>
                </footer>
            </div>
        </div>
    {/if}
</ToolPage>

<style>
    /* Toolbar: search + type filter chips, both ghost-buttoned. */
    .mc-toolbar {
        display: flex;
        flex-wrap: wrap;
        gap: 12px;
        margin-bottom: 14px;
    }
    .mc-search {
        position: relative;
        flex: 1;
        min-width: 200px;
        display: flex;
        align-items: center;
    }
    .mc-search :global(.mc-search-ico) {
        position: absolute;
        left: 10px;
        width: 14px;
        height: 14px;
        color: var(--color-muted);
        pointer-events: none;
    }
    .mc-search-input {
        width: 100%;
        height: 34px;
        padding: 0 32px 0 32px;
        background: var(--color-bg);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        color: var(--color-text);
        font-size: 13px;
        font-family: inherit;
        transition: border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .mc-search-input:focus {
        outline: none;
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 16%, transparent);
    }
    .mc-search-clear {
        position: absolute;
        right: 8px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 18px;
        height: 18px;
        padding: 0;
        background: transparent;
        border: none;
        color: var(--color-muted);
        cursor: pointer;
        border-radius: 4px;
    }
    .mc-search-clear:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .mc-search :global(.mc-search-clear-ico) {
        width: 12px;
        height: 12px;
    }

    .mc-type-chips {
        display: inline-flex;
        flex-wrap: wrap;
        gap: 6px;
        align-items: center;
    }
    .mc-type-chip {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        height: 30px;
        padding: 0 10px;
        background: transparent;
        border: 1px solid var(--color-border);
        border-radius: 999px;
        color: var(--color-text-secondary);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;
        transition:
            background-color var(--dur-micro, 130ms) var(--ease-out, ease),
            color var(--dur-micro, 130ms) var(--ease-out, ease),
            border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .mc-type-chip:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
    }
    .mc-type-chip.is-on {
        color: var(--color-accent);
    }
    .mc-type-chips :global(.mc-type-chip-ico) {
        width: 13px;
        height: 13px;
    }
    .mc-type-chip-count {
        font-size: 10px;
        color: var(--color-muted);
        background: var(--color-panel-2);
        padding: 1px 6px;
        border-radius: 999px;
        min-width: 18px;
        text-align: center;
    }
    .mc-type-chip.is-on .mc-type-chip-count {
        background: color-mix(in srgb, var(--color-accent) 20%, transparent);
        color: var(--color-accent);
    }

    /* Empty state — used both for "no commands at all" and
       "no commands match the current filter." */
    .mc-empty {
        text-align: center;
        padding: 32px 16px;
        background: var(--color-panel);
        border: 1px dashed var(--color-border);
        border-radius: var(--radius-card, 12px);
    }
    .mc-empty :global(.mc-empty-ico) {
        width: 32px;
        height: 32px;
        color: var(--color-muted);
        margin-bottom: 8px;
    }
    .mc-empty-title {
        font-size: 14px;
        font-weight: 600;
        color: var(--color-text);
        margin: 0 0 4px;
    }
    .mc-empty-desc {
        font-size: 12px;
        color: var(--color-text-secondary);
        margin: 0;
    }

    .mc-list {
        display: flex;
        flex-direction: column;
        gap: 8px;
    }
    .mc-kw {
        flex: none;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 12px;
        font-weight: 600;
        color: var(--color-accent);
        background: color-mix(in srgb, var(--color-accent) 12%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-accent) 30%, var(--color-border));
        padding: 3px 8px;
        border-radius: 6px;
    }
    .mc-type-pill {
        flex: none;
        font-size: 10px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.05em;
        color: var(--color-muted);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        padding: 2px 8px;
        border-radius: 999px;
    }
    /* Card layout for each command — icon tile left, details middle,
       actions right. */
    .mc-card {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 12px;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        transition: border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .mc-card:hover {
        border-color: color-mix(in srgb, var(--color-accent) 30%, var(--color-border));
    }
    .mc-card.is-editing {
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        box-shadow: 0 0 0 1px color-mix(in srgb, var(--color-accent) 30%, transparent);
    }
    .mc-card-icon {
        flex: none;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 36px;
        height: 36px;
        border-radius: 10px;
        color: var(--color-text-secondary);
    }
    .mc-card-icon :global(.mc-card-icon-svg) {
        width: 18px;
        height: 18px;
    }
    /* Per-type icon tints — each type gets its own subtle color so the
       eye can scan the list by shape AND color, not just by text. */
    .mc-icon-url      { background: color-mix(in srgb, #06b6d4 16%, var(--color-panel-2)); color: #06b6d4; }
    .mc-icon-bang {
        color: var(--color-accent);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .mc-icon-app      { background: color-mix(in srgb, #a855f7 16%, var(--color-panel-2)); color: #a855f7; }
    .mc-icon-file     { background: color-mix(in srgb, #f59e0b 16%, var(--color-panel-2)); color: #f59e0b; }
    .mc-icon-folder   { background: color-mix(in srgb, #f97316 16%, var(--color-panel-2)); color: #f97316; }
    .mc-icon-shell    { background: color-mix(in srgb, #ef4444 16%, var(--color-panel-2)); color: #ef4444; }

    .mc-card-text {
        flex: 1;
        min-width: 0;
    }
    .mc-card-row {
        display: flex;
        align-items: center;
        gap: 10px;
        flex-wrap: wrap;
    }
    .mc-card-label {
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
    }
    .mc-card-target {
        margin-top: 4px;
        font-size: 11.5px;
        color: var(--color-text-secondary);
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .mc-card-desc {
        margin-top: 4px;
        font-size: 12px;
        color: var(--color-muted);
        line-height: 1.4;
    }
    .mc-card-actions {
        flex: none;
        display: inline-flex;
        gap: 6px;
    }

    /* Modal — focused add/edit flow. Backdrop dims + blurs the page;
       the panel floats centered with a card shadow. */
    .mc-modal-backdrop {
        position: fixed;
        inset: 0;
        background: color-mix(in srgb, #000 55%, transparent);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 100;
        padding: 24px;
    }
    .mc-modal {
        width: min(560px, 100%);
        max-height: calc(100vh - 80px);
        overflow-y: auto;
        background: var(--color-panel);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-overlay, 16px);
        box-shadow: 0 24px 60px rgba(0, 0, 0, 0.45);
        display: flex;
        flex-direction: column;
    }
    .mc-modal-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 16px 20px;
        border-bottom: 1px solid var(--color-border);
    }
    .mc-modal-head h3 {
        font-size: 15px;
        font-weight: 600;
        color: var(--color-text);
        margin: 0;
    }
    .mc-modal-close {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 28px;
        height: 28px;
        padding: 0;
        background: transparent;
        border: 1px solid transparent;
        color: var(--color-text-secondary);
        border-radius: 8px;
        cursor: pointer;
    }
    .mc-modal-close:hover {
        background: var(--color-panel-2);
        color: var(--color-text);
        border-color: var(--color-border);
    }
    .mc-modal-close :global(.mc-modal-close-ico) {
        width: 14px;
        height: 14px;
    }
    .mc-modal-body {
        padding: 20px;
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .mc-modal-foot {
        display: flex;
        justify-content: flex-end;
        gap: 8px;
        padding: 14px 20px;
        border-top: 1px solid var(--color-border);
    }

    .mc-grid {
        display: grid;
        grid-template-columns: 1fr 1fr 1fr;
        gap: 10px;
    }
    @media (max-width: 640px) {
        .mc-grid {
            grid-template-columns: 1fr;
        }
    }
    .mc-field {
        display: flex;
        flex-direction: column;
        gap: 5px;
        min-width: 0;
    }
    .mc-flabel {
        font-size: 11px;
        font-weight: 600;
        color: var(--color-text-secondary);
    }
    .mc-input {
        width: 100%;
        height: 34px;
        padding: 0 10px;
        background: var(--color-bg);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        color: var(--color-text);
        font-size: 13px;
        font-family: inherit;
        transition: border-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    select.mc-input {
        cursor: pointer;
    }
    .mc-input:focus {
        outline: none;
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-accent) 16%, transparent);
    }
    .mc-target-row {
        display: flex;
        gap: 8px;
        align-items: center;
    }
    .mc-target-row .mc-input {
        flex: 1;
        min-width: 0;
    }
    .mc-input-textarea {
        height: auto;
        min-height: 72px;
        padding: 8px 10px;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        font-size: 12px;
        line-height: 1.4;
        resize: vertical;
    }

    .mc-field-hint {
        display: inline-flex;
        align-items: flex-start;
        gap: 6px;
        margin: 0;
        font-size: 11.5px;
        color: var(--color-text-secondary);
    }
    .mc-field-hint :global(.mc-field-hint-ico) {
        width: 14px;
        height: 14px;
        flex-shrink: 0;
        margin-top: 1px;
    }
    .mc-field-hint-warn {
        color: #f59e0b;
    }
    .mc-field-hint-warn :global(.mc-field-hint-ico) {
        color: #f59e0b;
    }

    /* ─── Wave 4.1b-2: shell kind picker ─────────────────────────────
       Compact pill row beneath the shell-command textarea. Visually
       quieter than radios — three crisp pills, accent ring on active.
    */
    .mc-shellkind {
        display: flex;
        align-items: center;
        gap: 10px;
        margin-top: 8px;
        flex-wrap: wrap;
    }
    .mc-shellkind-label {
        font-size: 11.5px;
        color: var(--color-text-secondary);
        text-transform: uppercase;
        letter-spacing: 0.5px;
    }
    .mc-shellkind-pills {
        display: inline-flex;
        gap: 6px;
        padding: 3px;
        background: var(--color-panel-2);
        border-radius: 8px;
    }
    .mc-shellkind-pill {
        appearance: none;
        border: 0;
        background: transparent;
        color: var(--color-text-secondary);
        padding: 4px 10px;
        font-size: 12px;
        font-family: 'JetBrains Mono', ui-monospace, monospace;
        border-radius: 6px;
        cursor: pointer;
        transition: background 120ms ease, color 120ms ease;
    }
    .mc-shellkind-pill:hover {
        color: var(--color-text-primary);
        background: var(--color-panel-3);
    }
    .mc-shellkind-pill.is-on {
        background: var(--color-accent);
        color: var(--color-on-accent, #fff);
    }
</style>
