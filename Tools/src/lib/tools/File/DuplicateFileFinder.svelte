<script lang="ts">
    import { open } from '@tauri-apps/plugin-dialog';
    import { convertFileSrc } from '@tauri-apps/api/core';
    import { escToClear } from '$lib/actions/escToClear';
    import { Ban, Copy, Eye, EyeOff, Files, FolderOpen, Search, ShieldCheck, Trash2, Plus } from '@lucide/svelte';
    import DropZone from '$lib/DropZone.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import DuplicatePreview from './DuplicatePreview.svelte';
    import { ToolPage, ToolPanel, Button, Checkbox } from '$lib/ui';
    import { toast } from '$lib/stores/toasts';
    import {
        addDuplicateRoots,
        cancelDuplicateScan,
        clearDuplicateInputs,
        duplicateCancelRequested,
        duplicateDestinationDir,
        duplicateExpandedGroups,
        duplicatePreviewExpandedPaths,
        duplicateFollowSymlinks,
        duplicateGroups,
        duplicateKeepHeuristic,
        duplicateHashAlgorithm,
        duplicateHashStrategy,
        duplicateSimilarityThreshold,
        duplicateIncludeHidden,
        duplicateLastJob,
        duplicateMinSizeKb,
        duplicatePerformance,
        duplicateMoveResults,
        duplicateMoving,
        duplicateRecursive,
        duplicateResult,
        duplicateRoots,
        duplicateScanning,
        duplicateSelectedBytes,
        duplicateSelectedPaths,
        duplicateExtensionFilter,
        duplicateProgress,
        formatBytes,
        moveSelectedDuplicates,
        removeDuplicateRoot,
        scanDuplicateFiles,
        selectDuplicatesByLocation,
        setDuplicateGroupSelection,
        toggleDuplicateGroup,
        toggleDuplicateSelection,
        type DuplicateGroup,
    } from '$lib/stores/duplicateFinder';

    async function pickFolders() {
        const picked = await open({ directory: true, multiple: true });
        if (Array.isArray(picked)) addDuplicateRoots(picked);
        else if (typeof picked === 'string') addDuplicateRoots([picked]);
    }

    async function pickFiles() {
        const picked = await open({ directory: false, multiple: true });
        if (Array.isArray(picked)) addDuplicateRoots(picked);
        else if (typeof picked === 'string') addDuplicateRoots([picked]);
    }

    async function pickDestination() {
        const picked = await open({ directory: true, multiple: false });
        if (typeof picked === 'string') duplicateDestinationDir.set(picked);
    }

    async function copyReport() {
        const result = $duplicateResult;
        if (!result) return;

        const lines = [
            'KeepItLocal Duplicate File Finder Report',
            `Scanned files: ${result.scanned_files}`,
            `Hashed files: ${result.hashed_files}`,
            `Exact content groups: ${result.duplicate_groups.length}`,
            `Reclaimable: ${formatBytes(result.reclaimable_bytes)}`,
            '',
            ...result.duplicate_groups.flatMap((group, index) => [
                `Group ${index + 1} — ${formatBytes(group.size)} each — ${group.count} files`,
                ...group.files.map((file) => `  - ${file.path}`),
                '',
            ]),
        ];

        await navigator.clipboard.writeText(lines.join('\n'));
        toast('Duplicate report copied', 'success');
    }

    function fileName(path: string) {
        return path.split(/[\\/]/).pop() ?? path;
    }

    // Quality Pass Wave 1 / DF-3: bulk-select-by-location input value
    // (e.g. "Downloads"). Per-session lookup, not persisted.
    let locationFilter = $state('');

    function applyBulkLocationSelection() {
        if (!locationFilter.trim()) return;
        selectDuplicatesByLocation(locationFilter, $duplicateGroups);
    }

    function groupChecked(group: DuplicateGroup) {
        const duplicatePaths = group.files.slice(1).map((file) => file.path);
        return duplicatePaths.length > 0 && duplicatePaths.every((path) => $duplicateSelectedPaths.includes(path));
    }

    // Quality Pass Wave 1 / DF-4 (2026-05-29): inline preview state.
    // A Set of file paths that have their preview pane expanded under
    // the row. The DuplicatePreview component is lazily mounted only
    // when a path appears here, so collapsing it tears down all its
    // resources (image decode, audio element, hljs work).
    let previewExpandedPaths = $derived(new Set($duplicatePreviewExpandedPaths));

    function isImagePreviewable(extension: string): boolean {
        // Matches the backend `classify()` Image arm. Excludes svg
        // from the inline-thumbnail (svg renders fine but at this size
        // is just a black square for most icon files).
        return [
            'png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'tif', 'tiff', 'ico',
        ].includes(extension);
    }

    function togglePreview(path: string) {
        const next = new Set(previewExpandedPaths);
        if (next.has(path)) next.delete(path);
        else next.add(path);
        duplicatePreviewExpandedPaths.set([...next]);
    }

    /** True when every file in the group has its preview pane open.
     *  Used by the group-header "Preview all" toggle so a second click
     *  collapses every preview in the group. */
    function groupAllPreviewsOpen(group: DuplicateGroup): boolean {
        return group.files.every((f) => previewExpandedPaths.has(f.path));
    }

    function toggleGroupPreviewAll(group: DuplicateGroup) {
        const next = new Set(previewExpandedPaths);
        const allOpen = groupAllPreviewsOpen(group);
        for (const file of group.files) {
            if (allOpen) next.delete(file.path);
            else next.add(file.path);
        }
        duplicatePreviewExpandedPaths.set([...next]);
    }

    // ── Elegance-pass view helpers (presentation only; no behavior change) ──
    const busy = $derived($duplicateScanning || $duplicateMoving);
    const hashStrategyOptions = [
        { value: 'exact', label: 'Exact content (same bytes)' },
        { value: 'perceptual', label: 'Similar images (resized, re-saved, lightly edited)' },
    ];
    const hashAlgorithmOptions = [
        { value: 'blake3', label: 'BLAKE3 — fast' },
        { value: 'sha256', label: 'SHA-256 — standard' },
    ];
    const performanceOptions = [
        { value: 'balanced', label: 'Balanced — up to 4 threads' },
        { value: 'fast', label: 'Fast — use more CPU' },
    ];
    const keepHeuristicOptions = [
        { value: 'first', label: 'First found (default)' },
        { value: 'newest', label: 'Newest file' },
        { value: 'oldest', label: 'Oldest file' },
        { value: 'shortest_path', label: 'Shortest path' },
        { value: 'longest_name', label: 'Longest filename' },
    ];
    function similarityLabel(v: number): string {
        if (v === 0) return 'identical only';
        if (v <= 4) return 'near-identical';
        if (v <= 10) return 'resized / re-encoded';
        if (v <= 18) return 'visually similar';
        return 'loose match';
    }
</script>

<DropZone onFiles={addDuplicateRoots}>
    <ToolPage
        icon={Files}
        iconTint="#64748b"
        title="Byte-for-byte duplicates, with nothing deleted"
        description="Size is used as a fast filter; matches are confirmed by full content hash. KeepItLocal never deletes — selected duplicates move to a review folder you choose, and one-click undo puts them back."
        width="wide"
        fill={false}
    >
        {#snippet actions()}
            <Button variant="secondary" icon={Plus} onclick={pickFiles} disabled={busy}>Add files</Button>
            <Button variant="secondary" icon={FolderOpen} onclick={pickFolders} disabled={busy}>Add folders</Button>
            {#if $duplicateRoots.length}
                <Button variant="ghost" icon={Trash2} onclick={clearDuplicateInputs} disabled={busy}>
                    Clear ({$duplicateRoots.length})
                </Button>
            {/if}
            {#if $duplicateScanning}
                <Button
                    variant="danger"
                    icon={Ban}
                    onclick={cancelDuplicateScan}
                    loading={$duplicateCancelRequested}
                    disabled={$duplicateCancelRequested}
                >
                    {$duplicateCancelRequested ? 'Cancelling' : 'Cancel'}
                </Button>
            {:else}
                <Button
                    variant="primary"
                    icon={Search}
                    onclick={scanDuplicateFiles}
                    loading={$duplicateScanning}
                    disabled={busy || !$duplicateRoots.length}
                >
                    Scan
                </Button>
            {/if}
        {/snippet}

        <div class="dupe-shell">
            <div class="dupe-grid">
                <!-- ── Sidebar ── -->
                <aside class="dupe-side">
                    <ToolPanel padding="md">
                        <div class="dupe-side-head">
                            <div class="dupe-section-title">
                                <span class="dupe-step" aria-hidden="true">1</span>
                                <h3 class="dupe-h">Choose scope</h3>
                            </div>
                            <span class="dupe-sub">{$duplicateRoots.length} selected</span>
                        </div>
                        <div class="dupe-sources">
                            {#if $duplicateRoots.length}
                                {#each $duplicateRoots as root (root)}
                                    <div class="dupe-source">
                                        <span class="dupe-source-path" title={root}>{root}</span>
                                        <button
                                            type="button"
                                            class="dupe-link-btn"
                                            onclick={() => removeDuplicateRoot(root)}
                                            disabled={busy}
                                        >Remove</button>
                                    </div>
                                {/each}
                            {:else}
                                <button type="button" class="dupe-drop-cta" onclick={pickFolders}>
                                    Drop files/folders here or choose a folder
                                </button>
                            {/if}
                        </div>
                    </ToolPanel>

                    <ToolPanel padding="md">
                        <div class="dupe-side-head">
                            <div class="dupe-section-title">
                                <span class="dupe-step" aria-hidden="true">2</span>
                                <h3 class="dupe-h">Configure scan</h3>
                            </div>
                        </div>
                        <div class="dupe-checks">
                            <Checkbox bind:checked={$duplicateRecursive} disabled={busy} label="Recursive" />
                            <Checkbox bind:checked={$duplicateIncludeHidden} disabled={busy} label="Hidden" />
                            <Checkbox bind:checked={$duplicateFollowSymlinks} disabled={busy} label="Symlinks" />
                            <label class="dupe-field">
                                <span class="dupe-label">Min KB</span>
                                <input
                                    class="dupe-input"
                                    type="number"
                                    min="0"
                                    bind:value={$duplicateMinSizeKb}
                                    disabled={busy}
                                    aria-label="Minimum file size in kilobytes"
                                />
                            </label>
                        </div>

                        <div class="dupe-stack">
                            <label class="dupe-field">
                                <span class="dupe-label">What counts as a duplicate?</span>
                                <select class="dupe-input dupe-select" bind:value={$duplicateHashStrategy} disabled={busy} aria-label="What counts as a duplicate">
                                    {#each hashStrategyOptions as opt}<option value={opt.value}>{opt.label}</option>{/each}
                                </select>
                            </label>

                            {#if $duplicateHashStrategy === 'perceptual'}
                                <label class="dupe-field">
                                    <span class="dupe-label">
                                        Similarity sensitivity
                                        <span class="dupe-hint-inline">({similarityLabel($duplicateSimilarityThreshold)})</span>
                                    </span>
                                    <div class="dupe-range-row">
                                        <input
                                            class="dupe-range"
                                            type="range"
                                            min="0"
                                            max="24"
                                            step="1"
                                            bind:value={$duplicateSimilarityThreshold}
                                            disabled={busy}
                                            aria-label="Similarity sensitivity"
                                        />
                                        <span class="dupe-range-val">{$duplicateSimilarityThreshold}</span>
                                    </div>
                                </label>
                            {:else}
                                <label class="dupe-field">
                                    <span class="dupe-label">Full content hash</span>
                                    <select class="dupe-input dupe-select" bind:value={$duplicateHashAlgorithm} disabled={busy} aria-label="Full content hash algorithm">
                                        {#each hashAlgorithmOptions as opt}<option value={opt.value}>{opt.label}</option>{/each}
                                    </select>
                                </label>
                            {/if}

                            {#if $duplicateHashStrategy === 'perceptual'}
                                <div class="dupe-banner">
                                    <div class="dupe-banner-title">dHash fingerprint · 100% local</div>
                                    Scans PNG / JPG / WebP / TIFF / BMP / GIF. Catches resized,
                                    re-encoded, lightly cropped, and recolored copies that exact-hash
                                    tools miss. Nothing leaves your machine.
                                </div>
                            {/if}

                            <label class="dupe-field">
                                <span class="dupe-label">Scan speed</span>
                                <select class="dupe-input dupe-select" bind:value={$duplicatePerformance} disabled={busy} aria-label="Scan speed">
                                    {#each performanceOptions as opt}<option value={opt.value}>{opt.label}</option>{/each}
                                </select>
                            </label>

                            <label class="dupe-field">
                                <span class="dupe-label">Auto-pick "keep"</span>
                                <select class="dupe-input dupe-select" bind:value={$duplicateKeepHeuristic} disabled={busy} aria-label="Which copy to keep">
                                    {#each keepHeuristicOptions as opt}<option value={opt.value}>{opt.label}</option>{/each}
                                </select>
                            </label>

                            <label class="dupe-field">
                                <span class="dupe-label">Only these file types (optional)</span>
                                <input
                                    class="dupe-input dupe-mono"
                                    type="text"
                                    placeholder="jpg, png, pdf"
                                    bind:value={$duplicateExtensionFilter}
                                    use:escToClear={() => duplicateExtensionFilter.set('')}
                                    disabled={busy}
                                    aria-label="Restrict scan to these file extensions"
                                />
                            </label>
                        </div>
                    </ToolPanel>

                    <ToolPanel padding="md">
                        <div class="dupe-section-title">
                            <span class="dupe-step" aria-hidden="true">4</span>
                            <h3 class="dupe-h">Review destination</h3>
                        </div>
                        <div class="dupe-stack">
                            <Button variant="secondary" full icon={FolderOpen} onclick={pickDestination} disabled={busy}>
                                Choose folder
                            </Button>
                            <div class="dupe-dest" title={$duplicateDestinationDir ?? 'Selected duplicates will be moved here'}>
                                {$duplicateDestinationDir ?? 'Selected duplicates will be moved here'}
                            </div>
                            <Button
                                variant="primary"
                                full
                                icon={Trash2}
                                onclick={moveSelectedDuplicates}
                                loading={$duplicateMoving}
                                disabled={busy || !$duplicateSelectedPaths.length || !$duplicateDestinationDir}
                            >
                                Move selected ({$duplicateSelectedPaths.length})
                            </Button>
                        </div>
                    </ToolPanel>
                </aside>

                <!-- ── Main ── -->
                <main class="dupe-main">
                    <div class="dupe-stats">
                        <div class="dupe-stat">
                            <div class="dupe-stat-label">Scanned</div>
                            <div class="dupe-stat-val">{$duplicateResult?.scanned_files ?? 0}</div>
                        </div>
                        <div class="dupe-stat">
                            <div class="dupe-stat-label">Hashed</div>
                            <div class="dupe-stat-val">{$duplicateResult?.hashed_files ?? 0}</div>
                        </div>
                        <div class="dupe-stat">
                            <div class="dupe-stat-label">Groups</div>
                            <div class="dupe-stat-val">{$duplicateGroups.length}</div>
                        </div>
                        <div class="dupe-stat">
                            <div class="dupe-stat-label">Reclaimable</div>
                            <div class="dupe-stat-val is-success">{formatBytes($duplicateResult?.reclaimable_bytes ?? 0)}</div>
                        </div>
                        <div class="dupe-stat">
                            <div class="dupe-stat-label">Selected</div>
                            <div class="dupe-stat-val is-accent">{formatBytes($duplicateSelectedBytes)}</div>
                        </div>
                    </div>

                    {#if $duplicateScanning || $duplicateMoving}
                        <!-- LOADING: live progress (phase + tickers); indeterminate bar -->
                        <ToolPanel padding="md">
                            <div class="dupe-prog">
                                <div class="dupe-prog-head">
                                    <span class="dupe-prog-icon" aria-hidden="true">
                                        {#if $duplicateMoving}<Trash2 class="dupe-ico" />{:else}<Search class="dupe-ico" />{/if}
                                    </span>
                                    <div class="dupe-prog-text">
                                        <div class="dupe-prog-step">
                                            {#if $duplicateMoving}
                                                Moving selected duplicates…
                                            {:else if $duplicateCancelRequested}
                                                Cancelling duplicate scan…
                                            {:else if $duplicateProgress.phase === 'perceptual'}
                                                Fingerprinting {$duplicateProgress.hashed.toLocaleString()} images…
                                            {:else if $duplicateProgress.phase === 'hashing'}
                                                Hashing {$duplicateProgress.hashed.toLocaleString()} candidate files…
                                            {:else}
                                                Walking folders · {$duplicateProgress.scanned.toLocaleString()} files seen
                                            {/if}
                                        </div>
                                        <div class="dupe-prog-detail">
                                            {#if $duplicateProgress.phase === 'perceptual'}
                                                dHash 8×8 fingerprint per image. Cached files are skipped.
                                            {:else if $duplicateProgress.phase === 'hashing'}
                                                Confirming exact matches with a full content hash. Cached files are skipped.
                                            {:else if $duplicateProgress.phase === 'walking'}
                                                Listing every file in the chosen folders to find same-size candidates.
                                            {:else}
                                                {$duplicateLastJob ?? 'Working…'}
                                            {/if}
                                        </div>
                                    </div>
                                </div>
                                <div class="dupe-mini">
                                    <div class="dupe-mini-cell">
                                        <div class="dupe-mini-label">Scanned</div>
                                        <div class="dupe-mini-val">{$duplicateProgress.scanned.toLocaleString()}</div>
                                    </div>
                                    <div class="dupe-mini-cell">
                                        <div class="dupe-mini-label">
                                            {$duplicateProgress.phase === 'perceptual' ? 'Fingerprinted' : 'Hashed'}
                                        </div>
                                        <div class="dupe-mini-val">{$duplicateProgress.hashed.toLocaleString()}</div>
                                    </div>
                                </div>
                                <div class="dupe-bar" role="progressbar" aria-label="Scan progress">
                                    <div class="dupe-bar-fill dupe-indeterminate"></div>
                                </div>
                            </div>
                        </ToolPanel>
                    {/if}

                    <ToolPanel padding="md">
                        <div class="dupe-results-head">
                            <div>
                                <div class="dupe-section-title">
                                    <span class="dupe-step" aria-hidden="true">3</span>
                                    <h3 class="dupe-h">Review matches</h3>
                                </div>
                                <div class="dupe-sub">Each group has the same full-file hash. First file in each group is kept by default.</div>
                            </div>
                            <Button
                                size="sm"
                                variant="secondary"
                                icon={Copy}
                                onclick={copyReport}
                                disabled={!$duplicateResult || !$duplicateGroups.length}
                            >Copy report</Button>
                        </div>

                        {#if $duplicateGroups.length}
                            <div class="dupe-bulk">
                                <span class="dupe-sub">Bulk-select dupes in</span>
                                <input
                                    class="dupe-input dupe-mono dupe-bulk-input"
                                    type="text"
                                    placeholder="Downloads, Pictures, …"
                                    bind:value={locationFilter}
                                    use:escToClear={() => (locationFilter = '')}
                                    disabled={busy}
                                    onkeydown={(e) => { if (e.key === 'Enter') applyBulkLocationSelection(); }}
                                    aria-label="Bulk-select duplicates whose path contains this text"
                                />
                                <Button
                                    size="sm"
                                    variant="secondary"
                                    onclick={applyBulkLocationSelection}
                                    disabled={busy || !locationFilter.trim()}
                                >Select</Button>
                            </div>

                            <div class="dupe-groups">
                                {#each $duplicateGroups as group, index}
                                    <div class="dupe-group">
                                        <div class="dupe-group-head">
                                            <button
                                                type="button"
                                                class="dupe-group-toggle"
                                                onclick={() => toggleDuplicateGroup(group.hash)}
                                                aria-expanded={$duplicateExpandedGroups.includes(group.hash)}
                                            >
                                                <div class="dupe-group-title">
                                                    Group {index + 1} · {group.count} files · {formatBytes(group.size)} each
                                                </div>
                                                <div class="dupe-group-sub">
                                                    Reclaimable: {formatBytes(group.wasted_bytes)} · content hash {group.hash}
                                                </div>
                                            </button>
                                            <Button
                                                size="sm"
                                                variant="ghost"
                                                icon={groupAllPreviewsOpen(group) ? EyeOff : Eye}
                                                onclick={() => toggleGroupPreviewAll(group)}
                                                disabled={busy}
                                                title={groupAllPreviewsOpen(group) ? 'Hide all previews in this group' : 'Show all previews in this group'}
                                            >
                                                {groupAllPreviewsOpen(group) ? 'Hide all' : 'Preview all'}
                                            </Button>
                                            <Checkbox
                                                checked={groupChecked(group)}
                                                onchange={(c) => setDuplicateGroupSelection(group, c)}
                                                disabled={busy}
                                                label="Select dupes"
                                            />
                                        </div>

                                        {#if $duplicateExpandedGroups.includes(group.hash)}
                                            <div class="dupe-files">
                                                {#each group.files as file, fileIndex}
                                                    {@const selected = $duplicateSelectedPaths.includes(file.path)}
                                                    <div class="dupe-file" class:is-selected={selected}>
                                                        <Checkbox
                                                            checked={selected}
                                                            disabled={fileIndex === 0 || busy}
                                                            onchange={() => toggleDuplicateSelection(file.path)}
                                                            ariaLabel={`Select duplicate ${file.file_name}`}
                                                        />
                                                        <div class="dupe-file-main">
                                                            {#if isImagePreviewable(file.extension)}
                                                                <img
                                                                    src={convertFileSrc(file.path)}
                                                                    alt=""
                                                                    loading="lazy"
                                                                    class="dupe-thumb"
                                                                />
                                                            {/if}
                                                            <div class="dupe-file-text">
                                                                <div class="dupe-file-name" title={file.path}>
                                                                    {file.file_name} {fileIndex === 0 ? '· kept' : ''}
                                                                </div>
                                                                <div class="dupe-file-path" title={file.path}>{file.path}</div>
                                                            </div>
                                                        </div>
                                                        <div class="dupe-file-size">{formatBytes(file.size)}</div>
                                                        <button
                                                            type="button"
                                                            class="dupe-icon-btn"
                                                            onclick={() => togglePreview(file.path)}
                                                            title={previewExpandedPaths.has(file.path) ? 'Hide preview' : 'Show preview'}
                                                            aria-label={previewExpandedPaths.has(file.path) ? 'Hide preview' : 'Show preview'}
                                                        >
                                                            {#if previewExpandedPaths.has(file.path)}
                                                                <EyeOff class="dupe-act-ico" />
                                                            {:else}
                                                                <Eye class="dupe-act-ico" />
                                                            {/if}
                                                        </button>
                                                    </div>
                                                    {#if previewExpandedPaths.has(file.path)}
                                                        <div class="dupe-preview">
                                                            <DuplicatePreview path={file.path} />
                                                        </div>
                                                    {/if}
                                                {/each}
                                            </div>
                                        {/if}
                                    </div>
                                {/each}
                            </div>
                        {:else if $duplicateResult && !$duplicateScanning}
                            <EmptyState
                                icon={ShieldCheck}
                                title="No exact duplicates found"
                                description="Every file in the scanned scope is unique by content."
                                variant="dashed"
                            />
                        {:else if !$duplicateScanning && !$duplicateMoving}
                            <EmptyState
                                icon={Search}
                                title="Nothing scanned yet"
                                description="Add files or folders and click Scan to find exact duplicates."
                                variant="dashed"
                            />
                        {/if}
                    </ToolPanel>

                    {#if $duplicateMoveResults.length}
                        <ToolPanel padding="md">
                            <h3 class="dupe-h">Last move result</h3>
                            <div class="dupe-move-results">
                                {#each $duplicateMoveResults as item}
                                    <div class="dupe-move-item">
                                        <div class="dupe-move-row">
                                            <span class="dupe-move-name" title={item.source_path}>{fileName(item.source_path)}</span>
                                            <span class={item.success ? 'is-success' : 'is-error'}>{item.success ? 'Moved' : 'Failed'}</span>
                                        </div>
                                        <div class="dupe-file-path" title={item.success ? item.output_path : item.error}>
                                            {item.success ? item.output_path : item.error}
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
    .dupe-shell {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .dupe-grid {
        display: grid;
        grid-template-columns: 1fr;
        gap: 16px;
        align-items: start;
    }
    @media (min-width: 1080px) {
        .dupe-grid {
            grid-template-columns: 320px minmax(0, 1fr);
        }
        .dupe-side {
            position: sticky;
            top: 12px;
        }
    }
    .dupe-side {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .dupe-main {
        display: flex;
        flex-direction: column;
        gap: 12px;
        min-width: 0;
    }

    .dupe-h {
        margin: 0;
        font-size: 13px;
        font-weight: 600;
        color: var(--color-text);
    }
    .dupe-section-title {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }
    .dupe-step {
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
    .dupe-sub {
        font-size: 11.5px;
        color: var(--color-muted);
        line-height: 1.45;
    }
    .dupe-side-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        margin-bottom: 10px;
    }

    /* ── Sources ── */
    .dupe-sources {
        display: flex;
        flex-direction: column;
        gap: 6px;
        max-height: 13rem;
        overflow: auto;
        scrollbar-gutter: stable;
    }
    .dupe-source {
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
    .dupe-source-path {
        min-width: 0;
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .dupe-link-btn {
        flex: none;
        background: transparent;
        border: none;
        padding: 2px 4px;
        font-size: 12px;
        color: var(--color-muted);
        cursor: pointer;
    }
    .dupe-link-btn:hover:not(:disabled) {
        color: var(--color-error);
    }
    .dupe-link-btn:disabled {
        opacity: 0.5;
        cursor: default;
    }
    .dupe-drop-cta {
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
    .dupe-drop-cta:hover {
        color: var(--color-text);
        border-color: color-mix(in srgb, var(--color-accent) 60%, var(--color-border));
    }

    /* ── Option fields ── */
    .dupe-checks {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 8px;
        margin-bottom: 12px;
    }
    .dupe-stack {
        display: flex;
        flex-direction: column;
        gap: 10px;
    }
    .dupe-field {
        display: flex;
        flex-direction: column;
        gap: 5px;
    }
    .dupe-label {
        font-size: 11.5px;
        color: var(--color-text-secondary);
    }
    .dupe-hint-inline {
        font-size: 10px;
        opacity: 0.7;
    }
    .dupe-input {
        width: 100%;
        height: 32px;
        padding: 0 8px;
        font-size: 13px;
        color: var(--color-text);
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }
    .dupe-input:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: 1px;
    }
    .dupe-mono {
        font-family: var(--font-mono, ui-monospace, monospace);
    }
    .dupe-select {
        cursor: pointer;
    }
    .dupe-select option {
        background: var(--color-panel);
        color: var(--color-text);
    }
    .dupe-range-row {
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .dupe-range {
        flex: 1;
        accent-color: var(--color-accent);
    }
    .dupe-range-val {
        width: 2rem;
        text-align: right;
        font-size: 12px;
        font-family: var(--font-mono, ui-monospace, monospace);
        color: var(--color-muted);
    }
    .dupe-banner {
        padding: 8px 12px;
        font-size: 11px;
        line-height: 1.5;
        color: var(--color-muted);
        background: color-mix(in srgb, var(--color-accent) 6%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-accent) 30%, var(--color-border));
        border-radius: var(--radius-control, 8px);
    }
    .dupe-banner-title {
        font-weight: 600;
        color: var(--color-text);
        margin-bottom: 2px;
    }
    .dupe-dest {
        font-size: 11px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    /* ── Stats ── */
    .dupe-stats {
        display: grid;
        grid-template-columns: repeat(2, 1fr);
        gap: 0;
        margin: 2px 0 4px;
        border-top: 1px solid var(--color-divider, var(--color-border));
        border-bottom: 1px solid var(--color-divider, var(--color-border));
    }
    @media (min-width: 1024px) {
        .dupe-stats {
            grid-template-columns: repeat(5, 1fr);
        }
    }
    .dupe-stat {
        min-width: 0;
        padding: 10px 12px;
    }
    .dupe-stat + .dupe-stat {
        border-left: 1px solid var(--color-divider, var(--color-border));
    }
    .dupe-stat-label {
        font-size: 11px;
        color: var(--color-muted);
    }
    .dupe-stat-val {
        margin-top: 2px;
        font-size: 18px;
        font-weight: 600;
        color: var(--color-text);
    }
    .dupe-stat-val.is-success {
        color: var(--color-success);
    }
    .dupe-stat-val.is-accent {
        color: var(--color-accent);
    }

    /* ── Progress ── */
    .dupe-prog {
        display: flex;
        flex-direction: column;
        gap: 12px;
    }
    .dupe-prog-head {
        display: flex;
        align-items: center;
        gap: 12px;
    }
    .dupe-prog-icon {
        flex: none;
        width: 34px;
        height: 34px;
        border-radius: 999px;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border: 1px solid color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
        background: var(--color-accent-soft, color-mix(in srgb, var(--color-accent) 12%, transparent));
        animation: dupe-pulse 1.6s ease-in-out infinite;
    }
    .dupe-prog-icon :global(.dupe-ico) {
        width: 16px;
        height: 16px;
        color: var(--color-accent);
    }
    .dupe-prog-text {
        min-width: 0;
    }
    .dupe-prog-step {
        font-size: 13px;
        font-weight: 500;
        color: var(--color-text);
    }
    .dupe-prog-detail {
        font-size: 11.5px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .dupe-mini {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 8px;
    }
    .dupe-mini-cell {
        padding: 6px 10px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }
    .dupe-mini-label {
        font-size: 11px;
        color: var(--color-muted);
    }
    .dupe-mini-val {
        font-size: 13px;
        font-weight: 600;
        font-variant-numeric: tabular-nums;
    }
    .dupe-bar {
        height: 6px;
        border-radius: 999px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        overflow: hidden;
    }
    .dupe-bar-fill {
        height: 100%;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .dupe-indeterminate {
        width: 33%;
        animation: dupe-slide 1.15s ease-in-out infinite;
    }
    @keyframes dupe-slide {
        0% { transform: translate3d(-120%, 0, 0); }
        50% { transform: translate3d(115%, 0, 0); }
        100% { transform: translate3d(260%, 0, 0); }
    }
    @keyframes dupe-pulse {
        0%, 100% { opacity: 1; }
        50% { opacity: 0.6; }
    }

    /* ── Results ── */
    .dupe-results-head {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 8px;
        margin-bottom: 10px;
    }
    .dupe-bulk {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
        margin-bottom: 10px;
    }
    .dupe-bulk-input {
        flex: 1;
        min-width: 120px;
        width: auto;
    }
    .dupe-groups {
        display: flex;
        flex-direction: column;
        gap: 8px;
        max-height: 60vh;
        overflow: auto;
        scrollbar-gutter: stable;
    }
    .dupe-group {
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        overflow: hidden;
        background: color-mix(in srgb, var(--color-panel-2) 40%, transparent);
    }
    .dupe-group-head {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 8px;
    }
    .dupe-group-toggle {
        min-width: 0;
        flex: 1;
        text-align: left;
        background: transparent;
        border: none;
        padding: 0;
        cursor: pointer;
        color: var(--color-text);
    }
    .dupe-group-title {
        font-size: 13px;
        font-weight: 500;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .dupe-group-sub {
        font-size: 11.5px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    /* ── File rows (canonical selected pattern) ── */
    .dupe-files {
        border-top: 1px solid var(--color-border);
    }
    .dupe-file {
        position: relative;
        display: grid;
        grid-template-columns: auto 1fr auto auto;
        gap: 8px;
        align-items: center;
        padding: 6px 8px;
        font-size: 12px;
        border-top: 1px solid color-mix(in srgb, var(--color-border) 60%, transparent);
    }
    .dupe-file:first-child {
        border-top: none;
    }
    .dupe-file.is-selected {
        background: var(--color-panel-2);
    }
    .dupe-file.is-selected::before {
        content: '';
        position: absolute;
        left: 2px;
        top: 6px;
        bottom: 6px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .dupe-file-main {
        min-width: 0;
        display: flex;
        align-items: center;
        gap: 8px;
    }
    .dupe-thumb {
        width: 28px;
        height: 28px;
        flex: none;
        object-fit: cover;
        border-radius: 6px;
        border: 1px solid var(--color-border);
        background: var(--color-panel);
    }
    .dupe-file-text {
        min-width: 0;
    }
    .dupe-file-name {
        font-size: 13px;
        color: var(--color-text);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .dupe-file-path {
        font-size: 11px;
        color: var(--color-muted);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .dupe-file-size {
        color: var(--color-muted);
        white-space: nowrap;
    }
    .dupe-icon-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 26px;
        height: 26px;
        border-radius: 6px;
        border: 1px solid var(--color-border);
        background: transparent;
        color: var(--color-muted);
        cursor: pointer;
    }
    .dupe-icon-btn:hover {
        color: var(--color-text);
        border-color: color-mix(in srgb, var(--color-accent) 60%, var(--color-border));
    }
    .dupe-icon-btn :global(.dupe-act-ico) {
        width: 14px;
        height: 14px;
    }
    .dupe-preview {
        padding: 0 8px 8px;
    }

    /* ── Move results ── */
    .dupe-move-results {
        display: flex;
        flex-direction: column;
        gap: 4px;
        max-height: 12rem;
        overflow: auto;
        scrollbar-gutter: stable;
        margin-top: 8px;
    }
    .dupe-move-item {
        padding: 6px 8px;
        font-size: 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
    }
    .dupe-move-row {
        display: flex;
        justify-content: space-between;
        gap: 8px;
    }
    .dupe-move-name {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .is-success {
        color: var(--color-success);
    }
    .is-error {
        color: var(--color-error);
    }

    @media (prefers-reduced-motion: reduce) {
        .dupe-indeterminate {
            animation: none;
            width: 100%;
            opacity: 0.5;
        }
        .dupe-prog-icon {
            animation: none;
        }
    }
</style>
