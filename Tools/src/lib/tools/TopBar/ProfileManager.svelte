<script lang="ts">
    import { categoryIcons, categoryOrder, type Category } from '$lib/tools';
    import {
        profiles, activeProfile, activeProfileId,
        createProfile, updateProfile, duplicateProfile, deleteProfile, setActiveProfile,
        type Profile,
    } from '$lib/stores/profiles';
    import { installedTools } from '$lib/stores/toolPacks';
    import { toast } from '$lib/stores/toasts';
    import { ToolPage } from '$lib/ui';
    import { _ } from 'svelte-i18n';
    import { t } from '$lib/i18n';
    import { Plus, Copy, Trash2, Check, Layers, Pencil, X, AlertCircle } from '@lucide/svelte';

    type Mode = 'list' | 'edit' | 'create';
    let mode = $state<Mode>('list');

    let editingId = $state<string | null>(null);
    let draftName = $state('');
    let draftDescription = $state('');
    let draftToolIds = $state<Set<string>>(new Set());

    let confirmDeleteId = $state<string | null>(null);

    const categories: Category[] = categoryOrder;

    function startCreate() {
        mode = 'create';
        editingId = null;
        draftName = '';
        draftDescription = '';
        draftToolIds = new Set($installedTools.filter(t => t.available && t.id !== 'file-search' && !t.hidden).map(t => t.id));
    }

    function startEdit(p: Profile) {
        if (p.builtIn) {
            // Built-in: offer to duplicate
            const dup = duplicateProfile(p.id);
            if (dup) {
                toast(t('page.profiles.toast.createdEditable', { name: dup.name }), 'info');
                startEdit(dup);
            }
            return;
        }
        mode = 'edit';
        editingId = p.id;
        draftName = p.name;
        draftDescription = p.description ?? '';
        const profileToolIds = $installedTools.filter(t => t.id !== 'file-search' && !t.hidden).map(t => t.id);
        const allowedIds = new Set(profileToolIds);
        draftToolIds = new Set(p.toolIds.length === 0 ? profileToolIds : p.toolIds.filter((id) => allowedIds.has(id)));
    }

    function cancelEdit() {
        mode = 'list';
        editingId = null;
    }

    function save() {
        const trimmed = draftName.trim();
        if (!trimmed) {
            toast(t('page.profiles.toast.nameEmpty'), 'error');
            return;
        }

        if (mode === 'create') {
            try {
                const created = createProfile(trimmed, [...draftToolIds], draftDescription.trim() || undefined);
                toast(t('page.profiles.toast.created', { name: created.name }), 'success');
                setActiveProfile(created.id);
            } catch (e) {
                toast(String(e), 'error');
                return;
            }
        } else if (mode === 'edit' && editingId) {
            updateProfile(editingId, {
                name: trimmed,
                description: draftDescription.trim() || undefined,
                toolIds: [...draftToolIds],
            });
            toast(t('page.profiles.toast.saved'), 'success');
        }

        mode = 'list';
        editingId = null;
    }

    function toggleTool(id: string) {
        if (draftToolIds.has(id)) {
            draftToolIds.delete(id);
        } else {
            draftToolIds.add(id);
        }
        draftToolIds = new Set(draftToolIds);
    }

    function selectAllInCategory(cat: Category, select: boolean) {
        const ids = $installedTools.filter(t => t.category === cat && t.available && t.id !== 'file-search' && !t.hidden).map(t => t.id);
        if (select) {
            ids.forEach(id => draftToolIds.add(id));
        } else {
            ids.forEach(id => draftToolIds.delete(id));
        }
        draftToolIds = new Set(draftToolIds);
    }

    function handleDuplicate(p: Profile) {
        const dup = duplicateProfile(p.id);
        if (dup) toast(t('page.profiles.toast.duplicated', { name: dup.name }), 'success');
    }

    function handleDelete(p: Profile) {
        if (p.builtIn) {
            toast(t('page.profiles.toast.builtInCannotDelete'), 'info');
            return;
        }
        confirmDeleteId = p.id;
    }

    function confirmDelete() {
        if (!confirmDeleteId) return;
        const target = $profiles.find(p => p.id === confirmDeleteId);
        if (target && deleteProfile(confirmDeleteId)) {
            toast(t('page.profiles.toast.deleted', { name: target.name }), 'success');
        }
        confirmDeleteId = null;
    }

    let selectedCount = $derived(draftToolIds.size);
    let availableTotal = $derived($installedTools.filter(t => t.available && t.id !== 'file-search' && !t.hidden).length);
</script>

<ToolPage
    icon={Layers}
    iconTint="var(--color-accent)"
    title={mode === 'list' ? $_('page.profiles.title') : mode === 'create' ? $_('page.profiles.newProfile') : $_('page.profiles.editProfile')}
    description={mode === 'list' ? $_('page.profiles.intro') : $_('page.profiles.editIntro')}
    width="medium"
    fill={false}
>
<div class="profile-manager">
    {#if mode === 'list'}
        <div class="pm-toolbar">
            <button
                type="button"
                onclick={startCreate}
                class="pm-primary-action"
            >
                <Plus class="w-4 h-4" />
                {$_('page.profiles.newProfile')}
            </button>
        </div>

        <div class="pm-profile-list">
            {#each $profiles as p (p.id)}
                <div class="pm-profile bg-panel border {p.id === $activeProfileId ? 'border-accent/50' : 'border-border'} rounded p-4" class:is-active={p.id === $activeProfileId}>
                    <div class="pm-profile-layout flex items-start justify-between gap-4">
                        <div class="pm-profile-copy flex-1 min-w-0">
                            <div class="flex items-center gap-2 mb-1">
                                <Layers class="w-3.5 h-3.5 text-muted" />
                                <span class="font-medium text-text">{p.name}</span>
                                {#if p.builtIn}
                                    <span class="text-[9px] text-muted uppercase font-semibold tracking-wider">{$_('page.profiles.builtIn')}</span>
                                {/if}
                                {#if p.id === $activeProfileId}
                                    <span class="text-[9px] text-accent uppercase font-semibold tracking-wider">{$_('page.profiles.active')}</span>
                                {/if}
                            </div>
                            {#if p.description}
                                <div class="text-xs text-muted mb-2">{p.description}</div>
                            {/if}
                            <div class="text-xs text-text-secondary">
                                {p.toolIds.length === 0
                                    ? $_('page.profiles.allInstalledTools', { values: { count: $installedTools.filter(t => t.id !== 'file-search' && !t.hidden).length } })
                                    : $_('page.profiles.toolCount', { values: { count: p.toolIds.filter((id) => id !== 'file-search').length } })}
                            </div>
                        </div>

                        <div class="pm-profile-actions flex items-center gap-1 shrink-0">
                            {#if p.id !== $activeProfileId}
                                <button
                                        onclick={() => setActiveProfile(p.id)}
                                        class="pm-activate"
                                >
                                    {$_('page.profiles.activate')}
                                </button>
                            {/if}

                            <button
                                    onclick={() => startEdit(p)}
                                    class="pm-icon-action"
                                    title={p.builtIn ? $_('page.profiles.duplicateToEdit') : $_('page.profiles.edit')}
                                    aria-label={p.builtIn ? $_('page.profiles.duplicateToEdit') : $_('page.profiles.editProfile')}
                            >
                                <Pencil class="w-4 h-4" />
                            </button>

                            <button
                                    onclick={() => handleDuplicate(p)}
                                    class="pm-icon-action"
                                    title={$_('page.profiles.duplicate')}
                                    aria-label={$_('page.profiles.duplicateProfile')}
                            >
                                <Copy class="w-4 h-4" />
                            </button>

                            {#if !p.builtIn}
                                <button
                                        onclick={() => handleDelete(p)}
                                        class="pm-icon-action is-danger"
                                        title={$_('page.profiles.delete')}
                                        aria-label={$_('page.profiles.deleteProfile')}
                                >
                                    <Trash2 class="w-4 h-4" />
                                </button>
                            {/if}
                        </div>
                    </div>
                </div>
            {/each}
        </div>

        {#if confirmDeleteId}
            {@const target = $profiles.find(p => p.id === confirmDeleteId)}
            {#if target}
                <div class="pm-modal-backdrop" role="presentation">
                    <div class="pm-delete-dialog" role="dialog" aria-modal="true">
                        <div class="pm-delete-copy flex items-start gap-3 mb-4">
                            <AlertCircle class="pm-delete-icon w-5 h-5" />
                            <div>
                                <div class="pm-delete-title font-medium text-text mb-1">{$_('page.profiles.deleteDialog.title')}</div>
                                <div class="pm-delete-body text-sm text-text-secondary">
                                    {$_('page.profiles.deleteDialog.body', { values: { name: target.name } })}
                                </div>
                            </div>
                        </div>
                        <div class="pm-dialog-actions flex justify-end gap-2">
                            <button
                                    onclick={() => (confirmDeleteId = null)}
                                    class="pm-secondary-action"
                            >
                                {$_('page.profiles.deleteDialog.cancel')}
                            </button>
                            <button
                                    onclick={confirmDelete}
                                    class="pm-danger-action"
                            >
                                {$_('page.profiles.deleteDialog.confirm')}
                            </button>
                        </div>
                    </div>
                </div>
            {/if}
        {/if}

    {:else}
        <!-- Edit / Create -->
        <div class="pm-editor-toolbar flex items-start justify-between mb-6">
            <div class="pm-editor-copy">
                <h1 class="text-2xl font-semibold mb-1">
                    {mode === 'create' ? $_('page.profiles.newProfile') : $_('page.profiles.editProfile')}
                </h1>
                <p class="text-muted text-sm">
                    {$_('page.profiles.editIntro')}
                </p>
            </div>
            <div class="pm-editor-actions flex gap-2">
                <button
                        onclick={cancelEdit}
                        class="pm-secondary-action"
                >
                    {$_('page.profiles.deleteDialog.cancel')}
                </button>
                <button
                        onclick={save}
                        disabled={!draftName.trim()}
                        class="pm-primary-action"
                >
                    <Check class="w-4 h-4" />
                    {$_('page.profiles.save')}
                </button>
            </div>
        </div>

        <div class="pm-details-grid grid grid-cols-2 gap-4 mb-6">
            <div class="pm-field">
                <label for="prof-name" class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">
                    {$_('page.profiles.nameLabel')}
                </label>
                <input
                        id="prof-name"
                        type="text"
                        bind:value={draftName}
                        maxlength="50"
                        placeholder={$_('page.profiles.namePlaceholder')}
                        class="w-full px-3 py-2 bg-panel border border-border rounded text-sm focus:outline-none focus:border-accent transition-colors"
                />
                <div class="pm-field-count">{draftName.length}/50</div>
            </div>
            <div class="pm-field">
                <label for="prof-desc" class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">
                    {$_('page.profiles.descriptionLabel')}
                </label>
                <input
                        id="prof-desc"
                        type="text"
                        bind:value={draftDescription}
                        maxlength="200"
                        placeholder={$_('page.profiles.descriptionPlaceholder')}
                        class="w-full px-3 py-2 bg-panel border border-border rounded text-sm focus:outline-none focus:border-accent transition-colors"
                />
                <div class="pm-field-count">{draftDescription.length}/200</div>
            </div>
        </div>

        <div class="pm-selection-summary mb-3 flex items-center justify-between">
      <span class="text-xs uppercase tracking-wider text-muted font-semibold">
        {$_('page.profiles.toolsSelected', { values: { selected: selectedCount, total: availableTotal } })}
      </span>
        </div>

        <div class="pm-category-list">
            {#each categories as cat}
                {@const Icon = categoryIcons[cat]}
                {@const items = $installedTools.filter(t => t.category === cat && t.id !== 'file-search' && !t.hidden)}
                {#if items.length > 0}
                    {@const availableInCat = items.filter(t => t.available)}
                    {@const allChecked = availableInCat.every(t => draftToolIds.has(t.id))}
                    {@const noneChecked = availableInCat.every(t => !draftToolIds.has(t.id))}

                    <div class="pm-category bg-panel border border-border rounded">
                        <div class="pm-category-header flex items-center justify-between px-3 py-2 border-b border-border">
                            <div class="pm-category-title flex items-center gap-2">
                                <Icon class="w-3.5 h-3.5 text-text-secondary" />
                                <span class="text-xs font-bold uppercase tracking-wider text-text-secondary">{cat}</span>
                                <span class="text-[10px] text-muted">
                  {availableInCat.filter(t => draftToolIds.has(t.id)).length}/{availableInCat.length}
                </span>
                            </div>
                            <div class="pm-category-actions flex gap-1 text-xs">
                                <button
                                        class="text-muted hover:text-text"
                                        onclick={() => selectAllInCategory(cat, true)}
                                        disabled={allChecked}
                                >
                                    {$_('page.profiles.selectAll')}
                                </button>
                                <span class="text-muted">·</span>
                                <button
                                        class="text-muted hover:text-text"
                                        onclick={() => selectAllInCategory(cat, false)}
                                        disabled={noneChecked}
                                >
                                    {$_('page.profiles.selectNone')}
                                </button>
                            </div>
                        </div>

                        <div class="pm-tool-list divide-y divide-border">
                            {#each items as tool (tool.id)}
                                {@const ToolIcon = tool.icon}
                                <label class="pm-tool-row flex items-center gap-3 px-3 py-2 hover:bg-bg/50 cursor-pointer {!tool.available ? 'opacity-50' : ''}">
                                    <input
                                            type="checkbox"
                                            checked={draftToolIds.has(tool.id)}
                                            onchange={() => toggleTool(tool.id)}
                                            disabled={!tool.available}
                                            class="pm-tool-checkbox"
                                    />
                                    <ToolIcon class="w-3.5 h-3.5 text-text-secondary shrink-0" />
                                    <div class="flex-1 min-w-0">
                                        <div class="text-sm text-text">{tool.name}</div>
                                        <div class="text-xs text-muted truncate">{tool.description}</div>
                                    </div>
                                    {#if !tool.available}
                                        <span class="text-[9px] text-muted uppercase">{$_('page.profiles.soon')}</span>
                                    {/if}
                                </label>
                            {/each}
                        </div>
                    </div>
                {/if}
            {/each}
        </div>
    {/if}
</div>
</ToolPage>

<style>
    .profile-manager {
        display: grid;
        gap: 14px;
    }

    .pm-toolbar,
    .pm-editor-toolbar {
        display: flex;
        align-items: center;
        justify-content: flex-end;
        min-height: 36px;
    }

    .pm-editor-copy {
        display: none;
    }

    .pm-primary-action,
    .pm-secondary-action,
    .pm-danger-action,
    .pm-activate,
    .pm-icon-action {
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        transition: border-color var(--dur-micro) var(--ease-out), background-color var(--dur-micro) var(--ease-out), color var(--dur-micro) var(--ease-out);
    }

    .pm-primary-action,
    .pm-danger-action,
    .pm-activate {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 7px;
        min-height: 36px;
        padding: 0 13px;
        font-size: 13px;
        font-weight: 600;
    }

    .pm-primary-action {
        border-color: var(--color-accent);
        background: var(--color-accent);
        color: var(--color-accent-contrast);
    }

    .pm-primary-action:hover:not(:disabled) {
        background: var(--color-accent-hover, var(--color-accent));
    }

    .pm-primary-action:disabled {
        cursor: not-allowed;
        opacity: 0.45;
    }

    .pm-secondary-action,
    .pm-activate,
    .pm-icon-action {
        background: var(--color-panel-2);
        color: var(--color-text);
    }

    .pm-secondary-action,
    .pm-activate {
        min-height: 36px;
        padding: 0 12px;
        font-size: 13px;
    }

    .pm-secondary-action:hover,
    .pm-activate:hover,
    .pm-icon-action:hover {
        border-color: color-mix(in srgb, var(--color-accent) 58%, var(--color-border));
    }

    .pm-danger-action {
        border-color: color-mix(in srgb, var(--color-error) 42%, var(--color-border));
        background: var(--color-error);
        color: #fff;
    }

    .pm-danger-action:hover {
        border-color: var(--color-error);
    }

    .pm-profile-list,
    .pm-category-list {
        display: grid;
        gap: 10px;
    }

    .pm-profile {
        position: relative;
        overflow: hidden;
        border-color: var(--color-border);
        border-radius: calc(var(--radius-control) + 4px);
        background: var(--color-panel);
        transition: border-color var(--dur-micro) var(--ease-out), background-color var(--dur-micro) var(--ease-out);
    }

    .pm-profile:hover {
        border-color: color-mix(in srgb, var(--color-text) 18%, var(--color-border));
    }

    .pm-profile.is-active {
        border-color: color-mix(in srgb, var(--color-accent) 38%, var(--color-border));
        background: var(--color-panel-2);
    }

    .pm-profile.is-active::before {
        position: absolute;
        top: 13px;
        bottom: 13px;
        left: 0;
        width: 3px;
        border-radius: 0 99px 99px 0;
        background: var(--color-accent);
        content: '';
    }

    .pm-profile-layout {
        align-items: flex-start;
    }

    .pm-profile-copy > :first-child {
        margin-bottom: 5px;
    }

    .pm-profile-copy > :first-child :global(svg) {
        color: var(--color-text-secondary);
    }

    .pm-profile-copy :global(.font-medium) {
        font-size: 14px;
        font-weight: 620;
    }

    .pm-profile-copy :global(.text-accent) {
        color: var(--color-accent);
    }

    .pm-profile-copy :global(.text-muted),
    .pm-profile-copy :global(.text-text-secondary) {
        font-size: 12px;
        line-height: 1.45;
    }

    .pm-profile-actions,
    .pm-editor-actions,
    .pm-dialog-actions {
        gap: 7px;
    }

    .pm-icon-action {
        display: inline-flex;
        width: 34px;
        height: 34px;
        align-items: center;
        justify-content: center;
        color: var(--color-muted);
    }

    .pm-icon-action:hover {
        color: var(--color-text);
    }

    .pm-icon-action.is-danger:hover {
        border-color: color-mix(in srgb, var(--color-error) 58%, var(--color-border));
        color: var(--color-error);
    }

    .pm-details-grid {
        padding: 14px;
        border: 1px solid var(--color-border);
        border-radius: calc(var(--radius-control) + 4px);
        background: var(--color-panel);
    }

    .pm-field label {
        display: block;
        margin-bottom: 7px;
        color: var(--color-muted);
        font-size: 11px;
        font-weight: 650;
        letter-spacing: 0.06em;
        text-transform: uppercase;
    }

    .pm-field input {
        width: 100%;
        min-height: 38px;
        padding: 0 11px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        outline: none;
        background: var(--color-panel-2);
        color: var(--color-text);
        font-size: 13px;
        transition: border-color var(--dur-micro) var(--ease-out);
    }

    .pm-field input:focus {
        border-color: var(--color-accent);
    }

    .pm-field-count {
        margin-top: 5px;
        color: var(--color-muted);
        font-size: 11px;
        text-align: right;
    }

    .pm-selection-summary {
        min-height: 34px;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        color: var(--color-muted);
        font-size: 11px;
        font-weight: 650;
        letter-spacing: 0.06em;
        text-transform: uppercase;
    }

    .pm-category {
        overflow: hidden;
        border-color: var(--color-border);
        border-radius: calc(var(--radius-control) + 4px);
        background: var(--color-panel);
    }

    .pm-category-header {
        min-height: 48px;
        padding-block: 0;
        border-color: var(--color-divider, var(--color-border));
    }

    .pm-category-title {
        color: var(--color-text-secondary);
        font-size: 11px;
        font-weight: 650;
        letter-spacing: 0.06em;
        text-transform: uppercase;
    }

    .pm-category-title :global(svg) {
        color: var(--color-accent);
    }

    .pm-category-title :global(.text-muted) {
        font-size: 11px;
        font-weight: 500;
        letter-spacing: 0;
    }

    .pm-category-actions {
        color: var(--color-muted);
    }

    .pm-category-actions button {
        border: 0;
        padding: 0;
        background: transparent;
        color: inherit;
        font: inherit;
    }

    .pm-category-actions button:hover:not(:disabled) {
        color: var(--color-text);
    }

    .pm-category-actions button:disabled {
        cursor: default;
        opacity: 0.45;
    }

    .pm-tool-list {
        border-color: var(--color-divider, var(--color-border));
    }

    .pm-tool-row {
        min-height: 54px;
        transition: background-color var(--dur-micro) var(--ease-out);
    }

    .pm-tool-row:hover {
        background: color-mix(in srgb, var(--color-text) 2.5%, transparent);
    }

    .pm-tool-checkbox {
        width: 15px;
        height: 15px;
        accent-color: var(--color-accent);
    }

    .pm-tool-row :global(.text-sm) {
        font-size: 13px;
    }

    .pm-tool-row :global(.text-xs) {
        font-size: 12px;
    }

    .pm-modal-backdrop {
        position: fixed;
        z-index: 50;
        inset: 0;
        display: grid;
        place-items: center;
        padding: 20px;
        background: color-mix(in srgb, black 52%, transparent);
    }

    .pm-delete-dialog {
        width: min(100%, 420px);
        padding: 20px;
        border: 1px solid var(--color-border);
        border-radius: calc(var(--radius-control) + 6px);
        background: var(--color-panel);
        box-shadow: 0 18px 60px color-mix(in srgb, black 35%, transparent);
    }

    .pm-delete-copy :global(.pm-delete-icon) {
        flex: 0 0 auto;
        color: var(--color-warning);
    }

    .pm-delete-title {
        font-size: 15px;
        font-weight: 620;
    }

    .pm-delete-body {
        line-height: 1.5;
    }

    .pm-dialog-actions {
        margin-top: 20px;
    }

    .profile-manager button:focus-visible,
    .profile-manager input:focus-visible {
        outline: 2px solid color-mix(in srgb, var(--color-accent) 76%, transparent);
        outline-offset: 2px;
    }

    @media (max-width: 640px) {
        .pm-profile-layout,
        .pm-editor-toolbar {
            align-items: stretch;
            flex-direction: column;
        }

        .pm-profile-actions,
        .pm-editor-actions {
            justify-content: flex-end;
        }

        .pm-details-grid {
            grid-template-columns: 1fr;
        }

        .pm-category-header {
            align-items: flex-start;
            flex-direction: column;
            gap: 8px;
            padding-block: 12px;
        }
    }

    @media (prefers-reduced-motion: reduce) {
        .pm-primary-action,
        .pm-secondary-action,
        .pm-danger-action,
        .pm-activate,
        .pm-icon-action,
        .pm-profile,
        .pm-field input,
        .pm-tool-row {
            transition: none;
        }
    }
</style>
