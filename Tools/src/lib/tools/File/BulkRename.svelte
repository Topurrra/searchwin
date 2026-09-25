<script lang="ts">
    import { onMount } from 'svelte';
    import { open } from '@tauri-apps/plugin-dialog';
    import { CheckCircle2, FilePenLine, RefreshCw, Search, Plus, FolderOpen, Trash2, Undo2 } from '@lucide/svelte';
    import DropZone from '$lib/DropZone.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import { ToolPage, ToolPanel, Button, Checkbox } from '$lib/ui';
    import { escToClear } from '$lib/actions/escToClear';
    import {
        addBulkRenameRoots,
        applyBulkRename,
        bulkRenameApplying,
        bulkRenameCaseMode,
        bulkRenameExtensionCase,
        bulkRenameFind,
        bulkRenameIncludeDirs,
        bulkRenameIncludeFiles,
        bulkRenameIncludeHidden,
        bulkRenameLastJob,
        bulkRenameNumberingEnabled,
        bulkRenameNumberingPadding,
        bulkRenameNumberingSeparator,
        bulkRenameNumberingStart,
        bulkRenamePrefix,
        bulkRenamePreview,
        bulkRenamePreviewing,
        bulkRenameRecovery,
        bulkRenameReadyItems,
        bulkRenameRecursive,
        bulkRenameReplace,
        bulkRenameResults,
        bulkRenameRoots,
        bulkRenameSuffix,
        bulkRenameUseRegex,
        clearBulkRenameInputs,
        initBulkRenameRecovery,
        previewBulkRename,
        removeBulkRenameRoot,
        resetBulkRenameRules,
        type RenamePreviewItem,
        undoBulkRename,
        bulkRenamePresets,
        saveBulkRenamePresetFromCurrent,
        loadBulkRenamePreset,
        deleteBulkRenamePreset,
    } from '$lib/stores/bulkRename';

    onMount(() => {
        void initBulkRenameRecovery();
    });

    // Quality Pass Wave 1 / BR-3 (2026-05-29): preset-input draft state.
    let presetNameDraft = $state('');
    function handleSavePreset() {
        const name = presetNameDraft.trim();
        if (!name) return;
        saveBulkRenamePresetFromCurrent(name);
        presetNameDraft = '';
    }

    // Quality Pass Wave 1 user feedback (2026-05-28): the undo banner
    // used to stick around forever, which was startling on the next
    // launch when users had moved on. Cap it at 60 SECONDS and show a
    // live countdown so users understand when the safety net expires.
    const RECOVERY_TTL_MS = 60 * 1000;
    let nowMs = $state(Date.now());
    $effect(() => {
        const interval = setInterval(() => { nowMs = Date.now(); }, 1000);
        return () => clearInterval(interval);
    });
    const recoveryExpiresAt = $derived(
        $bulkRenameRecovery ? $bulkRenameRecovery.created_at + RECOVERY_TTL_MS : 0
    );
    const recoveryRemainingMs = $derived(Math.max(0, recoveryExpiresAt - nowMs));
    const showRecovery = $derived(Boolean($bulkRenameRecovery) && recoveryRemainingMs > 0);
    const recoveryCountdown = $derived.by(() => {
        const totalSec = Math.floor(recoveryRemainingMs / 1000);
        const m = Math.floor(totalSec / 60);
        const s = (totalSec % 60).toString().padStart(2, '0');
        return `${m}:${s}`;
    });

    async function pickFiles() {
        const picked = await open({ directory: false, multiple: true });
        if (Array.isArray(picked)) addBulkRenameRoots(picked);
        else if (typeof picked === 'string') addBulkRenameRoots([picked]);
    }

    async function pickFolder() {
        const picked = await open({ directory: true, multiple: true });
        if (Array.isArray(picked)) addBulkRenameRoots(picked);
        else if (typeof picked === 'string') addBulkRenameRoots([picked]);
    }

    function fileName(path: string) {
        return path.split(/[\\/]/).pop() ?? path;
    }

    function statusBadgeClass(status: RenamePreviewItem['status']) {
        if (status === 'ready') return 'bg-success-soft text-success border border-success-strong';
        if (status === 'conflict') return 'bg-warning-soft text-warning border border-warning-strong';
        if (status === 'invalid') return 'bg-error-soft text-error border border-error-strong';
        return 'bg-panel-2 text-muted border border-border';
    }

    // ── Elegance-pass view helpers (presentation only; no behavior change) ──
    const busy = $derived($bulkRenamePreviewing || $bulkRenameApplying);
    const caseOptions = [
        { value: 'none', label: 'Keep' },
        { value: 'lower', label: 'lowercase' },
        { value: 'upper', label: 'UPPERCASE' },
        { value: 'title', label: 'Title Case' },
    ];
    const extCaseOptions = [
        { value: 'keep', label: 'Keep' },
        { value: 'lower', label: 'lowercase' },
        { value: 'upper', label: 'UPPERCASE' },
    ];
</script>

<DropZone onFiles={addBulkRenameRoots}>
    <ToolPage
        icon={FilePenLine}
        iconTint="#64748b"
        title="Preview every change before a single file moves"
        description="Find/replace, casing, numbering, prefixes and suffixes — all visualized side-by-side with conflict and validity checks. Nothing on disk changes until you apply. One-click undo restores the previous names."
        width="wide"
        fill={false}
    >
        {#snippet actions()}
            <div class="br-page-actions">
            <Button variant="secondary" icon={Plus} onclick={pickFiles} disabled={busy}>Add files</Button>
            <Button variant="secondary" icon={FolderOpen} onclick={pickFolder} disabled={busy}>Add folders</Button>
            {#if $bulkRenameRoots.length}
                <Button variant="ghost" icon={Trash2} onclick={clearBulkRenameInputs} disabled={busy}>
                    Clear ({$bulkRenameRoots.length})
                </Button>
            {/if}
            <span class="br-page-actions-sep" aria-hidden="true"></span>
            <Button
                variant="secondary"
                icon={Search}
                onclick={previewBulkRename}
                loading={$bulkRenamePreviewing}
                disabled={busy || !$bulkRenameRoots.length}
            >
                {$bulkRenamePreviewing ? 'Previewing…' : 'Preview'}
            </Button>
            <Button
                variant="primary"
                icon={CheckCircle2}
                onclick={applyBulkRename}
                loading={$bulkRenameApplying}
                disabled={busy || !$bulkRenameReadyItems.length}
            >
                {$bulkRenameApplying ? 'Applying…' : `Apply (${$bulkRenameReadyItems.length})`}
            </Button>
            </div>
        {/snippet}

        <div class="br-shell">
            <!-- Recovery banner — auto-expires after 60s with live countdown. -->
            {#if showRecovery && $bulkRenameRecovery}
                <div class="br-recovery" role="status">
                    <div class="br-recovery-text">
                        <div class="br-recovery-title">
                            <span>Undo available</span>
                            <span class="br-recovery-count">· Auto-clears in {recoveryCountdown}</span>
                        </div>
                        <div class="br-recovery-desc" title={$bulkRenameRecovery.description}>
                            {$bulkRenameRecovery.description}.
                        </div>
                    </div>
                    <Button variant="secondary" icon={Undo2} onclick={undoBulkRename} disabled={busy}>
                        Undo last rename
                    </Button>
                </div>
            {/if}

            <div class="br-grid">
                <!-- ── Sidebar: inputs + rules ── -->
                <aside class="br-side">
                    <ToolPanel padding="md">
                        <div class="br-side-head">
                            <div class="br-section-title">
                                <span class="br-step" aria-hidden="true">1</span>
                                <h3 class="br-h">Choose items</h3>
                            </div>
                            <span class="br-sub">{$bulkRenameRoots.length} selected</span>
                        </div>
                        <div class="br-sources">
                            {#if $bulkRenameRoots.length}
                                {#each $bulkRenameRoots as root (root)}
                                    <div class="br-source">
                                        <span class="br-source-path" dir="rtl" title={root}>{root}</span>
                                        <button
                                            type="button"
                                            class="br-link-btn"
                                            onclick={() => removeBulkRenameRoot(root)}
                                            disabled={busy}
                                            aria-label="Remove {fileName(root)}"
                                        >Remove</button>
                                    </div>
                                {/each}
                            {:else}
                                <button type="button" class="br-drop-cta" onclick={pickFiles}>
                                    Drop files/folders here or choose files
                                </button>
                            {/if}
                        </div>
                        <div class="br-checks">
                            <Checkbox bind:checked={$bulkRenameRecursive} disabled={busy} label="Recursive" />
                            <Checkbox bind:checked={$bulkRenameIncludeHidden} disabled={busy} label="Hidden" />
                            <Checkbox bind:checked={$bulkRenameIncludeFiles} disabled={busy} label="Files" />
                            <Checkbox bind:checked={$bulkRenameIncludeDirs} disabled={busy} label="Folders" />
                        </div>
                    </ToolPanel>

                    <ToolPanel padding="md">
                        <div class="br-side-head">
                            <div class="br-section-title">
                                <span class="br-step" aria-hidden="true">2</span>
                                <h3 class="br-h">Set rules</h3>
                            </div>
                            <Button size="sm" variant="ghost" icon={RefreshCw} onclick={resetBulkRenameRules} disabled={busy}>
                                Reset
                            </Button>
                        </div>

                        <!-- BR-3: saved presets -->
                        <form class="br-preset-form" onsubmit={(e) => { e.preventDefault(); handleSavePreset(); }}>
                            <input
                                class="br-input"
                                bind:value={presetNameDraft}
                                placeholder="Save current as preset…"
                                disabled={busy}
                                aria-label="Preset name"
                            />
                            <Button size="sm" variant="secondary" type="submit" disabled={!presetNameDraft.trim() || busy}>
                                Save
                            </Button>
                        </form>
                        {#if $bulkRenamePresets.length}
                            <div class="br-presets">
                                {#each $bulkRenamePresets as preset (preset.id)}
                                    <div class="br-preset">
                                        <button
                                            type="button"
                                            class="br-preset-name"
                                            onclick={() => loadBulkRenamePreset(preset)}
                                            disabled={busy}
                                            title="Load preset {preset.name}"
                                        >{preset.name}</button>
                                        <button
                                            type="button"
                                            class="br-preset-del"
                                            onclick={() => deleteBulkRenamePreset(preset.id)}
                                            disabled={busy}
                                            aria-label="Delete preset {preset.name}"
                                        >×</button>
                                    </div>
                                {/each}
                            </div>
                        {/if}

                        <!-- BR-1: regex toggle above find/replace -->
                        <div class="br-regex">
                            <Checkbox bind:checked={$bulkRenameUseRegex} disabled={busy} label="Regex mode" />
                            {#if $bulkRenameUseRegex}
                                <span class="br-regex-hint">$1, $name for captures</span>
                            {/if}
                        </div>

                        <div class="br-rules-grid">
                            <label class="br-field">
                                <span class="br-label">Find{$bulkRenameUseRegex ? ' (regex)' : ''}</span>
                                <input
                                    class="br-input"
                                    class:br-mono={$bulkRenameUseRegex}
                                    bind:value={$bulkRenameFind}
                                    use:escToClear={() => bulkRenameFind.set('')}
                                    disabled={busy}
                                    placeholder={$bulkRenameUseRegex ? '(.+?)-(\\d+)' : ''}
                                    aria-label="Find text"
                                />
                            </label>
                            <label class="br-field">
                                <span class="br-label">Replace{$bulkRenameUseRegex ? ' (substitution)' : ''}</span>
                                <input
                                    class="br-input"
                                    class:br-mono={$bulkRenameUseRegex}
                                    bind:value={$bulkRenameReplace}
                                    use:escToClear={() => bulkRenameReplace.set('')}
                                    disabled={busy}
                                    placeholder={$bulkRenameUseRegex ? '$1_$2' : ''}
                                    aria-label="Replacement text"
                                />
                            </label>
                            <label class="br-field">
                                <span class="br-label">Prefix</span>
                                <input
                                    class="br-input br-mono"
                                    bind:value={$bulkRenamePrefix}
                                    use:escToClear={() => bulkRenamePrefix.set('')}
                                    disabled={busy}
                                    placeholder={'{date:YYYY-MM-DD}_'}
                                    aria-label="Filename prefix"
                                />
                            </label>
                            <label class="br-field">
                                <span class="br-label">Suffix</span>
                                <input
                                    class="br-input br-mono"
                                    bind:value={$bulkRenameSuffix}
                                    use:escToClear={() => bulkRenameSuffix.set('')}
                                    disabled={busy}
                                    placeholder={'_{n:000}'}
                                    aria-label="Filename suffix"
                                />
                            </label>
                        </div>

                        <!-- BR-2: tokens hint -->
                        <div class="br-tokens">
                            <span class="br-tokens-title">Tokens:</span>
                            <code class="br-tok">{'{n}'}</code>,
                            <code class="br-tok">{'{n:000}'}</code>,
                            <code class="br-tok">{'{name}'}</code>,
                            <code class="br-tok">{'{ext}'}</code>,
                            <code class="br-tok">{'{parent}'}</code>,
                            <code class="br-tok">{'{date:YYYY-MM-DD}'}</code>,
                            <code class="br-tok">{'{modified:YYYY-MM-DD}'}</code>
                            <span class="br-sub">— work in prefix, suffix, replace</span>
                        </div>

                        <div class="br-rules-grid">
                            <label class="br-field">
                                <span class="br-label">Case</span>
                                <select class="br-input br-select" bind:value={$bulkRenameCaseMode} disabled={busy} aria-label="Filename case">
                                    {#each caseOptions as opt}<option value={opt.value}>{opt.label}</option>{/each}
                                </select>
                            </label>
                            <label class="br-field">
                                <span class="br-label">Extension</span>
                                <select class="br-input br-select" bind:value={$bulkRenameExtensionCase} disabled={busy} aria-label="Extension case">
                                    {#each extCaseOptions as opt}<option value={opt.value}>{opt.label}</option>{/each}
                                </select>
                            </label>
                        </div>

                        <div class="br-numbering-toggle">
                            <Checkbox bind:checked={$bulkRenameNumberingEnabled} disabled={busy} label="Add numbering" />
                        </div>
                        {#if $bulkRenameNumberingEnabled}
                            <div class="br-numbering">
                                <label class="br-field">
                                    <span class="br-label">Start</span>
                                    <input class="br-input" type="number" bind:value={$bulkRenameNumberingStart} aria-label="Numbering start" />
                                </label>
                                <label class="br-field">
                                    <span class="br-label">Padding</span>
                                    <input class="br-input" type="number" min="1" max="12" bind:value={$bulkRenameNumberingPadding} aria-label="Numbering padding" />
                                </label>
                                <label class="br-field">
                                    <span class="br-label">Sep</span>
                                    <input
                                        class="br-input"
                                        bind:value={$bulkRenameNumberingSeparator}
                                        use:escToClear={() => bulkRenameNumberingSeparator.set('')}
                                        aria-label="Numbering separator"
                                    />
                                </label>
                            </div>
                        {/if}
                    </ToolPanel>
                </aside>

                <!-- ── Main: stats + preview ── -->
                <main class="br-main" aria-label="Rename preview">
                    <div class="br-stats" aria-label="Rename preview summary">
                        <div class="br-stat">
                            <div class="br-stat-label">Scanned</div>
                            <div class="br-stat-val">{$bulkRenamePreview?.scanned_items ?? 0}</div>
                        </div>
                        <div class="br-stat">
                            <div class="br-stat-label">Ready</div>
                            <div class="br-stat-val is-success">{$bulkRenamePreview?.ready_count ?? 0}</div>
                        </div>
                        <div class="br-stat">
                            <div class="br-stat-label">Conflicts</div>
                            <div class="br-stat-val is-warning">{$bulkRenamePreview?.conflict_count ?? 0}</div>
                        </div>
                        <div class="br-stat">
                            <div class="br-stat-label">Invalid</div>
                            <div class="br-stat-val is-error">{$bulkRenamePreview?.invalid_count ?? 0}</div>
                        </div>
                        <div class="br-stat">
                            <div class="br-stat-label">Unchanged</div>
                            <div class="br-stat-val">{$bulkRenamePreview?.unchanged_count ?? 0}</div>
                        </div>
                    </div>

                    <ToolPanel padding="md">
                        <div class="br-preview-head">
                            <div>
                                <div class="br-section-title">
                                    <span class="br-step" aria-hidden="true">3</span>
                                    <h3 class="br-h">Review changes</h3>
                                </div>
                                <div class="br-sub">Existing files and duplicate target names are blocked before anything changes.</div>
                            </div>
                            <span class="br-preview-state">
                                {#if $bulkRenamePreview?.ready_count}
                                    {$bulkRenamePreview.ready_count} safe to apply
                                {:else}
                                    Preview required
                                {/if}
                            </span>
                        </div>

                        {#if $bulkRenamePreviewing || $bulkRenameApplying}
                            <!-- LOADING -->
                            <div class="br-loading">
                                <span class="br-loading-icon" aria-hidden="true">
                                    {#if $bulkRenameApplying}<CheckCircle2 class="br-ico" />{:else}<Search class="br-ico" />{/if}
                                </span>
                                <div class="br-loading-text">
                                    <div class="br-loading-step">{$bulkRenameApplying ? 'Applying safe rename plan…' : 'Building rename preview…'}</div>
                                    <div class="br-sub">{$bulkRenameLastJob ?? 'Checking for conflicts before changing anything.'}</div>
                                </div>
                                <div class="br-bar" role="progressbar" aria-label="Rename progress">
                                    <div class="br-bar-fill br-indeterminate"></div>
                                </div>
                            </div>
                        {:else if $bulkRenamePreview && $bulkRenamePreview.items.length}
                            <div class="br-preview">
                                {#each $bulkRenamePreview.items as item}
                                    <div class="br-row" data-status={item.status}>
                                        <span class="br-badge {statusBadgeClass(item.status)}">{item.status}</span>
                                        <div class="br-row-text">
                                            <div class="br-row-line">
                                                <span class="br-row-tag">Current</span>
                                                <span class="br-row-name" title={item.original_name}>{item.original_name}</span>
                                            </div>
                                            <div class="br-row-path" dir="rtl" title={item.original_path}>{item.original_path}</div>
                                            <div class="br-row-line br-row-line-new">
                                                <span class="br-row-tag">New</span>
                                                <span class="br-row-name" title={item.new_name}>{item.new_name}</span>
                                            </div>
                                            <div class="br-row-path" dir="rtl" title={item.new_path}>{item.new_path}</div>
                                        </div>
                                        <div class="br-row-reason" title={item.reason ?? 'Ready to rename'}>
                                            {item.reason ?? 'Ready to rename'}
                                        </div>
                                    </div>
                                {/each}
                            </div>
                        {:else}
                            <EmptyState
                                icon={Search}
                                title="No preview yet"
                                description="Add inputs and click Preview to see exactly what will change."
                                variant="dashed"
                            />
                        {/if}
                    </ToolPanel>

                    {#if $bulkRenameResults.length}
                        <ToolPanel padding="md">
                            <h3 class="br-h">Last apply result</h3>
                            <div class="br-results">
                                {#each $bulkRenameResults as result}
                                    <div class="br-result">
                                        <div class="br-result-row">
                                            <span class="br-result-name" title={result.original_path}>{fileName(result.original_path)}</span>
                                            <span class={result.success ? 'is-success' : 'is-error'}>{result.success ? 'Done' : 'Failed'}</span>
                                        </div>
                                        <div class="br-row-path" dir="rtl" title={result.success ? result.new_path : result.error}>
                                            {result.success ? result.new_path : result.error}
                                        </div>
                                    </div>
                                {/each}
                            </div>
                        </ToolPanel>
                    {/if}
                </main>
            </div>
        </div>
    </ToolPage>
</DropZone>

<style>
    .br-shell {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .br-page-actions {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 8px;
    }
    .br-page-actions-sep {
        width: 1px;
        height: 20px;
        margin: 0 2px;
        background: var(--color-border);
    }
    .br-grid {
        display: grid;
        grid-template-columns: 1fr;
        gap: 16px;
        align-items: start;
    }
    @media (min-width: 1080px) {
        .br-grid {
            grid-template-columns: 324px minmax(0, 1fr);
        }
        .br-side {
            position: sticky;
            top: 12px;
        }
    }
    .br-side {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .br-main {
        display: flex;
        flex-direction: column;
        gap: 12px;
        min-width: 0;
    }
    .br-h {
        margin: 0;
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
    }
    .br-section-title {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }
    .br-step {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 18px;
        height: 18px;
        flex: none;
        border: 1px solid color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
        border-radius: 999px;
        color: var(--color-accent);
        font-size: 10px;
        font-weight: 700;
        font-variant-numeric: tabular-nums;
    }
    .br-sub {
        font-size: 11.5px;
        color: var(--color-muted);
        line-height: 1.45;
    }
    .br-side-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        margin-bottom: 10px;
    }
    .br-preview-head {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 16px;
    }
    .br-preview-state {
        flex: none;
        padding: 4px 8px;
        border: 1px solid var(--color-border);
        border-radius: 999px;
        color: var(--color-text-secondary);
        font-size: 11px;
        font-variant-numeric: tabular-nums;
        white-space: nowrap;
    }

    /* ── Recovery banner ── */
    .br-recovery {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 12px 14px;
        border-radius: var(--radius-card, 12px);
        border: 1px solid color-mix(in srgb, var(--color-info) 45%, var(--color-border));
        background: color-mix(in srgb, var(--color-info) 10%, transparent);
    }
    .br-recovery-text {
        min-width: 0;
        flex: 1;
    }
    .br-recovery-title {
        display: flex;
        align-items: center;
        gap: 8px;
        font-size: 13px;
        font-weight: 500;
        color: var(--color-info);
    }
    .br-recovery-count {
        font-size: 11px;
        font-weight: 400;
        opacity: 0.75;
        font-variant-numeric: tabular-nums;
    }
    .br-recovery-desc {
        font-size: 11.5px;
        color: color-mix(in srgb, var(--color-info) 80%, var(--color-text));
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    /* ── Sources ── */
    .br-sources {
        display: flex;
        flex-direction: column;
        gap: 6px;
        max-height: 12rem;
        overflow: auto;
        scrollbar-gutter: stable;
        margin-bottom: 12px;
    }
    .br-source {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        padding: 6px 8px;
        font-size: 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }
    .br-source-path {
        min-width: 0;
        flex: 1;
        text-align: left;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .br-link-btn {
        flex: none;
        background: transparent;
        border: none;
        padding: 2px 4px;
        font-size: 12px;
        color: var(--color-muted);
        cursor: pointer;
    }
    .br-link-btn:hover:not(:disabled) {
        color: var(--color-error);
    }
    .br-link-btn:disabled {
        opacity: 0.5;
        cursor: default;
    }
    .br-drop-cta {
        width: 100%;
        padding: 20px;
        text-align: center;
        font-size: 13px;
        color: var(--color-muted);
        background: transparent;
        border: 1px dashed var(--color-border);
        border-radius: var(--radius-control, 8px);
        cursor: pointer;
    }
    .br-drop-cta:hover {
        color: var(--color-text);
        border-color: color-mix(in srgb, var(--color-accent) 60%, var(--color-border));
    }
    .br-checks {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 8px;
    }

    /* ── Rules ── */
    .br-preset-form {
        display: flex;
        align-items: center;
        gap: 8px;
        margin-bottom: 8px;
    }
    .br-presets {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
        margin-bottom: 10px;
    }
    .br-preset {
        display: inline-flex;
        align-items: center;
        gap: 2px;
        overflow: hidden;
        font-size: 11px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: 999px;
    }
    .br-preset-name {
        padding: 4px 4px 4px 10px;
        background: transparent;
        border: none;
        color: var(--color-text);
        cursor: pointer;
    }
    .br-preset-name:hover:not(:disabled) {
        color: var(--color-accent);
    }
    .br-preset-del {
        padding: 4px 8px 4px 2px;
        background: transparent;
        border: none;
        color: var(--color-muted);
        cursor: pointer;
    }
    .br-preset-del:hover:not(:disabled) {
        color: var(--color-error);
    }
    .br-preset-name:disabled,
    .br-preset-del:disabled {
        opacity: 0.5;
        cursor: default;
    }
    .br-regex {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        margin-bottom: 10px;
    }
    .br-regex-hint {
        font-size: 10px;
        color: var(--color-muted);
    }
    .br-rules-grid {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 8px;
        margin-bottom: 10px;
    }
    .br-field {
        display: flex;
        flex-direction: column;
        gap: 5px;
        min-width: 0;
    }
    .br-label {
        font-size: 11.5px;
        color: var(--color-text-secondary);
    }
    .br-input {
        width: 100%;
        height: 32px;
        padding: 0 8px;
        font-size: 13px;
        color: var(--color-text);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }
    .br-input:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: 1px;
    }
    .br-input:disabled {
        opacity: 0.6;
    }
    .br-mono {
        font-family: var(--font-mono, ui-monospace, monospace);
    }
    .br-select {
        cursor: pointer;
    }
    .br-select option {
        background: var(--color-panel);
        color: var(--color-text);
    }
    .br-tokens {
        font-size: 11px;
        line-height: 1.6;
        color: var(--color-muted);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        padding: 8px 10px;
        margin-bottom: 10px;
    }
    .br-tokens-title {
        font-weight: 600;
        color: var(--color-text);
    }
    .br-tok {
        font-family: var(--font-mono, ui-monospace, monospace);
        color: var(--color-accent);
    }
    .br-numbering-toggle {
        margin-bottom: 8px;
    }
    .br-numbering {
        display: grid;
        grid-template-columns: 1fr 1fr 1fr;
        gap: 8px;
    }

    /* ── Stats ── */
    .br-stats {
        display: grid;
        grid-template-columns: repeat(5, minmax(0, 1fr));
        margin: 2px 0 4px;
        border-top: 1px solid var(--color-divider, var(--color-border));
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    .br-stat {
        min-width: 0;
        padding: 10px 12px;
    }
    .br-stat + .br-stat {
        border-left: 1px solid var(--color-divider, var(--color-border));
    }
    .br-stat-label {
        font-size: 11px;
        color: var(--color-muted);
    }
    .br-stat-val {
        margin-top: 2px;
        font-size: 18px;
        font-weight: 600;
        color: var(--color-text);
    }
    .br-stat-val.is-success { color: var(--color-success); }
    .br-stat-val.is-warning { color: var(--color-warning); }
    .br-stat-val.is-error { color: var(--color-error); }

    @media (max-width: 1079px) {
        .br-stats {
            grid-template-columns: repeat(2, minmax(0, 1fr));
        }
        .br-stat:nth-child(odd) {
            border-left: none;
        }
        .br-stat:nth-child(n + 3) {
            border-top: 1px solid var(--color-divider, var(--color-border));
        }
    }

    /* ── Loading ── */
    .br-loading {
        margin-top: 12px;
        display: flex;
        flex-direction: column;
        align-items: center;
        text-align: center;
        gap: 10px;
        padding: 24px;
        border: 1px dashed var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: color-mix(in srgb, var(--color-panel-2) 40%, transparent);
    }
    .br-loading-icon {
        width: 34px;
        height: 34px;
        border-radius: 999px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border: 1px solid color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
        background: var(--color-accent-soft, color-mix(in srgb, var(--color-accent) 12%, transparent));
        animation: br-pulse 1.6s ease-in-out infinite;
    }
    .br-loading-icon :global(.br-ico) {
        width: 16px;
        height: 16px;
        color: var(--color-accent);
    }
    .br-loading-text {
        min-width: 0;
    }
    .br-loading-step {
        font-size: 13px;
        font-weight: 500;
        color: var(--color-text);
    }
    .br-bar {
        width: 100%;
        height: 6px;
        border-radius: 999px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        overflow: hidden;
    }
    .br-bar-fill {
        height: 100%;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .br-indeterminate {
        width: 33%;
        animation: br-slide 1.15s ease-in-out infinite;
    }
    @keyframes br-slide {
        0% { transform: translate3d(-120%, 0, 0); }
        50% { transform: translate3d(115%, 0, 0); }
        100% { transform: translate3d(260%, 0, 0); }
    }
    @keyframes br-pulse {
        0%, 100% { opacity: 1; }
        50% { opacity: 0.6; }
    }

    /* ── Preview rows (status-strip indicator) ── */
    .br-preview {
        margin-top: 12px;
        max-height: 60vh;
        overflow: auto;
        scrollbar-gutter: stable;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }
    .br-row {
        position: relative;
        display: grid;
        grid-template-columns: auto 1fr auto;
        gap: 12px;
        align-items: start;
        padding: 8px 10px 8px 12px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
    }
    .br-row:first-child {
        border-top: none;
    }
    .br-row::before {
        content: '';
        position: absolute;
        left: 2px;
        top: 8px;
        bottom: 8px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-border);
    }
    .br-row[data-status='ready']::before { background: var(--color-success); }
    .br-row[data-status='conflict']::before { background: var(--color-warning); }
    .br-row[data-status='invalid']::before { background: var(--color-error); }
    .br-badge {
        align-self: start;
        margin-top: 2px;
        display: inline-flex;
        align-items: center;
        border-radius: 6px;
        padding: 1px 6px;
        font-size: 10px;
        font-weight: 600;
        text-transform: uppercase;
    }
    .br-row-text {
        min-width: 0;
        font-size: 12px;
        display: flex;
        flex-direction: column;
        gap: 2px;
    }
    .br-row-line {
        display: flex;
        align-items: baseline;
        gap: 8px;
    }
    .br-row-line-new {
        padding-top: 4px;
    }
    .br-row-tag {
        flex: none;
        color: var(--color-muted);
    }
    .br-row-name {
        font-weight: 500;
        color: var(--color-text);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .br-row-path {
        text-align: left;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .br-row-reason {
        max-width: 180px;
        font-size: 11px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    /* ── Apply results ── */
    .br-results {
        display: flex;
        flex-direction: column;
        gap: 4px;
        max-height: 12rem;
        overflow: auto;
        scrollbar-gutter: stable;
        margin-top: 8px;
    }
    .br-result {
        padding: 6px 8px;
        font-size: 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }
    .br-result-row {
        display: flex;
        justify-content: space-between;
        gap: 8px;
    }
    .br-result-name {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .is-success { color: var(--color-success); }
    .is-error { color: var(--color-error); }

    @media (max-width: 720px) {
        .br-page-actions-sep {
            display: none;
        }
        .br-preview-head {
            flex-direction: column;
            gap: 8px;
        }
    }

    @media (prefers-reduced-motion: reduce) {
        .br-indeterminate {
            animation: none;
            width: 100%;
            opacity: 0.5;
        }
        .br-loading-icon {
            animation: none;
        }
    }
</style>
