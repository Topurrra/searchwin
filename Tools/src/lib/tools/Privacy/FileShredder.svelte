<script lang="ts">
    import {open} from '@tauri-apps/plugin-dialog';
    import { get } from 'svelte/store';
    import DropZone from '$lib/DropZone.svelte';
    import {
        shredFiles, shredderTab, shredMethod, shredRandomize, shredRecursive,
        shredResults, shredProcessing, shredProgress,
        wipeDir, wipeProcessing, wipeProgress, wipeResult, wipePattern,
        runShred, runWipe, cancelOperation,
        clearShredHistory, clearWipeHistory, clearShredFiles,
        type Method, type WipePattern, type ShredderTab,
    } from '$lib/stores/shredder';
    import {File, Flame, Plus, FolderOpen, Trash2, AlertTriangle, Eraser} from '@lucide/svelte';
    import LoadingState from '$lib/components/LoadingState.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import ToolCancelButton from '$lib/components/ToolCancelButton.svelte';
    import { ToolPage } from '$lib/ui';


    let tab = $state<ShredderTab>(get(shredderTab));
    let confirmText = $state('');
    let showConfirm = $state(false);
    let wipeConfirmText = $state('');
    let showWipeConfirm = $state(false);

    const methods: { id: Method; name: string; passes: number; description: string }[] = [
        {
            id: 'quick',
            name: 'Quick',
            passes: 1,
            description: '1 pass random — fast, sufficient for SSDs and modern drives'
        },
        {id: 'dod3', name: 'Thorough (3-pass)', passes: 3, description: 'Three passes over the data — old-school hard-drive standard.'},
        {id: 'dod7', name: 'Paranoid (7-pass)', passes: 7, description: 'Seven passes — very slow. Only worth it for old magnetic hard drives.'},
    ];

    const wipePatterns: { id: WipePattern; label: string; sub: string; description: string }[] = [
        { id: 'zeros', label: 'Zeros', sub: 'Disk speed', description: 'Writes all zeros — fastest, fully overwrites previous data. Recommended for routine wipes.' },
        { id: 'ones', label: 'Ones', sub: 'Disk speed', description: 'Writes all-ones — fastest, alternative to zeros.' },
        { id: 'fast', label: 'Fast random', sub: 'Non-crypto', description: 'Pseudo-random data — much faster than secure random, indistinguishable for forensic purposes.' },
        { id: 'random', label: 'Secure random', sub: 'Slowest', description: 'Cryptographic random — slowest. Only needed against state-level forensics. Overkill for most users.' },
    ];

    function handleDrop(paths: string[]) {
        shredFiles.update(files => {
            const newOnes = paths.filter(p => !files.includes(p));
            return [...files, ...newOnes];
        });
    }

    async function pickFiles() {
        const selected = await open({multiple: true, directory: false});
        if (Array.isArray(selected)) handleDrop(selected);
        else if (typeof selected === 'string') handleDrop([selected]);
    }

    async function pickFolder() {
        const selected = await open({multiple: false, directory: true});
        if (typeof selected === 'string') handleDrop([selected]);
    }

    function removeFile(path: string) {
        shredFiles.update(files => files.filter(f => f !== path));
    }


    function selectTab(next: ShredderTab) {
        tab = next;
        shredderTab.set(next);
    }
    async function startShred() {
        if ($shredFiles.length === 0 || confirmText !== 'SHRED') return;
        showConfirm = false;
        confirmText = '';
        await runShred({
            paths: $shredFiles,
            method: $shredMethod,
            randomize_names: $shredRandomize,
            recursive: $shredRecursive,
        });
    }

    async function pickWipeDir() {
        const selected = await open({multiple: false, directory: true});
        if (typeof selected === 'string') wipeDir.set(selected);
    }

    async function startWipe() {
        if (!$wipeDir || wipeConfirmText !== 'WIPE') return;
        showWipeConfirm = false;
        wipeConfirmText = '';
        await runWipe($wipeDir, $wipePattern);
    }

    function fileName(path: string): string {
        return path.split(/[\\/]/).pop() || path;
    }

    function formatBytes(bytes: number): string {
        if (bytes < 1024) return `${bytes} B`;
        if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
        if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
        return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
    }

    let successCount = $derived($shredResults.filter(r => r.success).length);
    let failCount = $derived($shredResults.filter(r => !r.success).length);
    let currentMethod = $derived(methods.find(m => m.id === $shredMethod)!);
    let currentPattern = $derived(wipePatterns.find(p => p.id === $wipePattern)!);

    let progressPercent = $derived(
        $shredProgress && $shredProgress.bytes_total > 0
            ? Math.min(100, ($shredProgress.bytes_done / $shredProgress.bytes_total) * 100)
            : 0
    );
    let overallPercent = $derived(
        $shredProgress && $shredProgress.files_total > 0
            ? Math.min(100, (($shredProgress.files_done + progressPercent / 100) / $shredProgress.files_total) * 100)
            : 0
    );
    let wipePercent = $derived(
        $wipeProgress && $wipeProgress.estimated_total > 0
            ? Math.min(100, ($wipeProgress.bytes_written / $wipeProgress.estimated_total) * 100)
            : 0
    );
</script>

<ToolPage
    icon={Flame}
    iconTint="#ef4444"
    title="Permanently destroy files — and the traces they left behind"
    description="Shred individual files and folders with 1, 3, or 7 overwrite passes, or wipe an entire drive's free space to obliterate deleted-file remnants. Operations are local and irreversible — type to confirm before anything runs."
    width="wide"
    fill={false}
>
    <div class="shredder-warning" role="note">
        <AlertTriangle class="shredder-warning-icon" aria-hidden="true" />
        <div>
            <strong>No undo.</strong>
            <p>Shredded files are unrecoverable. On SSDs, wear-leveling means physical erasure is not guaranteed; full-disk encryption is the safer starting point.</p>
        </div>
    </div>

    <div class="shredder-tabs" role="tablist" aria-label="Destruction mode">
        <button
            type="button"
            role="tab"
            aria-selected={tab === 'shred'}
            onclick={() => selectTab('shred')}
            class="shredder-tab"
            class:is-active={tab === 'shred'}
        >
            <Flame class="shredder-tab-icon" aria-hidden="true" />
            <span>Shred files &amp; folders</span>
            {#if $shredProcessing}
                <span class="shredder-live" aria-label="Shredding in progress"></span>
            {/if}
        </button>
        <button
            type="button"
            role="tab"
            aria-selected={tab === 'wipe'}
            onclick={() => selectTab('wipe')}
            class="shredder-tab"
            class:is-active={tab === 'wipe'}
        >
            <Eraser class="shredder-tab-icon" aria-hidden="true" />
            <span>Wipe free space</span>
            {#if $wipeProcessing}
                <span class="shredder-live" aria-label="Free-space wipe in progress"></span>
            {/if}
        </button>
    </div>
    {#if tab === 'shred'}
        <div class="shredder-toolbar">
            <div class="shredder-toolbar-group">
                <button
                    type="button"
                    onclick={pickFiles}
                    disabled={$shredProcessing}
                    class="shredder-button"
                >
                    <Plus class="shredder-button-icon" aria-hidden="true" />
                    Add files
                </button>
                <button
                    type="button"
                    onclick={pickFolder}
                    disabled={$shredProcessing}
                    class="shredder-button"
                >
                    <FolderOpen class="shredder-button-icon" aria-hidden="true" />
                    Add folder
                </button>
                {#if $shredFiles.length > 0 && !$shredProcessing}
                    <button
                        type="button"
                        onclick={clearShredFiles}
                        class="shredder-button is-quiet-danger"
                    >
                        <Trash2 class="shredder-button-icon" aria-hidden="true" />
                        Clear {$shredFiles.length}
                    </button>
                {/if}
            </div>

            <div class="shredder-toolbar-actions">
                <ToolCancelButton running={$shredProcessing} onCancel={cancelOperation} />
                {#if !showConfirm}
                    <button
                        type="button"
                        onclick={() => { showConfirm = true; confirmText = ''; }}
                        disabled={$shredFiles.length === 0 || $shredProcessing}
                        class="shredder-button is-destructive"
                    >
                        {#if $shredProcessing}
                            <LoadingState variant="inline" label="Shredding…" />
                        {:else}
                            <Flame class="shredder-button-icon" aria-hidden="true" />
                            Shred {$shredFiles.length || ''}
                        {/if}
                    </button>
                {/if}
            </div>
        </div>

        {#if showConfirm}
            <section class="shredder-confirm" aria-labelledby="shred-confirm-heading">
                <div>
                    <div id="shred-confirm-heading" class="shredder-confirm-title">Confirm irreversible deletion</div>
                    <p>Type <kbd>SHRED</kbd> to permanently delete {$shredFiles.length} item{$shredFiles.length === 1 ? '' : 's'}.</p>
                </div>
                <div class="shredder-confirm-controls">
                    <input
                        type="text"
                        bind:value={confirmText}
                        placeholder="SHRED"
                        aria-label="Type SHRED to confirm"
                        class="shredder-confirm-input"
                    />
                    <button type="button" onclick={startShred} disabled={confirmText !== 'SHRED'} class="shredder-button is-destructive">Confirm shred</button>
                    <button type="button" onclick={() => { showConfirm = false; confirmText = ''; }} class="shredder-button">Cancel</button>
                </div>
            </section>
        {/if}

        <div class="shredder-workspace">
            <section class="shredder-panel shredder-target-panel" aria-labelledby="shred-targets-heading">
                <div class="shredder-panel-heading">
                    <div>
                        <div class="shredder-eyebrow">Source</div>
                        <h2 id="shred-targets-heading">Targets</h2>
                    </div>
                    <span class="shredder-count">{$shredFiles.length} selected</span>
                </div>

                <DropZone onFiles={handleDrop}>
                    {#snippet children()}
                        {#if $shredFiles.length === 0}
                            <EmptyState
                                icon={File}
                                title="Drop files or folders to shred"
                                description="Every byte is overwritten before the file is unlinked. Folders use the recursive setting below."
                                variant="dashed"
                            />
                        {:else}
                            <div class="shredder-target-list">
                                {#each $shredFiles as path (path)}
                                    <div class="shredder-target-row">
                                        <File class="shredder-target-icon" aria-hidden="true" />
                                        <div class="shredder-target-copy">
                                            <div class="shredder-target-name" title={path}>{fileName(path)}</div>
                                            <div class="shredder-target-path" title={path}>{path}</div>
                                        </div>
                                        {#if !$shredProcessing}
                                            <button
                                                type="button"
                                                onclick={() => removeFile(path)}
                                                class="shredder-remove"
                                                aria-label={`Remove ${fileName(path)}`}
                                            >
                                                Remove
                                            </button>
                                        {/if}
                                    </div>
                                {/each}
                            </div>
                        {/if}
                    {/snippet}
                </DropZone>
            </section>

            <aside class="shredder-panel shredder-options-panel" aria-labelledby="shred-options-heading">
                <div class="shredder-panel-heading">
                    <div>
                        <div class="shredder-eyebrow">Overwrite policy</div>
                        <h2 id="shred-options-heading">Method</h2>
                    </div>
                </div>

                <div class="shredder-choice-list">
                    {#each methods as m}
                        {@const active = $shredMethod === m.id}
                        <button
                            type="button"
                            onclick={() => shredMethod.set(m.id)}
                            disabled={$shredProcessing}
                            class="shredder-choice"
                            class:is-active={active}
                            aria-pressed={active}
                        >
                            <span class="shredder-choice-main">
                                <span class="shredder-choice-name">{m.name}</span>
                                <span class="shredder-choice-meta">{m.passes} pass{m.passes === 1 ? '' : 'es'}</span>
                            </span>
                        </button>
                    {/each}
                </div>
                <p class="shredder-choice-note">{currentMethod.description}</p>

                <div class="shredder-toggle-list">
                    <label class="shredder-toggle">
                        <input type="checkbox" checked={$shredRecursive} onchange={(e) => shredRecursive.set(e.currentTarget.checked)} disabled={$shredProcessing} />
                        <span><strong>Recursive folders</strong><small>Process subdirectories of selected folders.</small></span>
                    </label>
                    <label class="shredder-toggle">
                        <input type="checkbox" checked={$shredRandomize} onchange={(e) => shredRandomize.set(e.currentTarget.checked)} disabled={$shredProcessing} />
                        <span><strong>Randomize filenames</strong><small>Renames between passes to reduce journal traces.</small></span>
                    </label>
                </div>
            </aside>
        </div>

        {#if $shredProgress && $shredProcessing}
            <section class="shredder-progress" aria-live="polite">
                <div class="shredder-progress-head">
                    <div>
                        <div class="shredder-eyebrow">In progress</div>
                        <div class="shredder-progress-file" title={$shredProgress.current_file}>{fileName($shredProgress.current_file)}</div>
                    </div>
                    <div class="shredder-progress-meta">
                        <span>File {$shredProgress.files_done + 1} of {$shredProgress.files_total}</span>
                        <span>Pass {$shredProgress.current_pass}/{$shredProgress.total_passes}</span>
                    </div>
                </div>
                <div class="shredder-progress-label">
                    <span>{formatBytes($shredProgress.bytes_done)} / {formatBytes($shredProgress.bytes_total)}</span>
                    <span>{progressPercent.toFixed(1)}%</span>
                </div>
                <div class="shredder-progress-track"><div class="shredder-progress-bar" style={`width: ${progressPercent}%`}></div></div>
                <div class="shredder-progress-label is-overall">
                    <span>Overall</span>
                    <span>{overallPercent.toFixed(1)}%</span>
                </div>
                <div class="shredder-progress-track"><div class="shredder-progress-bar is-overall" style={`width: ${overallPercent}%`}></div></div>
            </section>
        {/if}

        {#if $shredResults.length > 0}
            <section class="shredder-results" aria-labelledby="shred-results-heading">
                <div class="shredder-panel-heading">
                    <div>
                        <div class="shredder-eyebrow">Operation history</div>
                        <h2 id="shred-results-heading">Results</h2>
                    </div>
                    <div class="shredder-results-summary">
                        <span class="is-success">{successCount} done</span>
                        {#if failCount > 0}<span class="is-error">{failCount} failed</span>{/if}
                        <button type="button" onclick={clearShredHistory} class="shredder-clear-history">Clear</button>
                    </div>
                </div>
                <div class="shredder-result-list">
                    {#each $shredResults as r}
                        <article class="shredder-result" class:is-error={!r.success}>
                            <div class="shredder-result-main">
                                <div class="shredder-result-title" title={r.path}>
                                    <span class="shredder-result-kind">{r.kind}</span>
                                    {fileName(r.path)}
                                </div>
                                <div class="shredder-result-path" title={r.path}>{r.path}</div>
                            </div>
                            <span class="shredder-status" class:is-error={!r.success}>{r.success ? 'Done' : 'Failed'}</span>
                            {#if r.kind === 'file'}
                                <div class="shredder-result-meta">{formatBytes(r.size)} · {r.passes_completed} pass{r.passes_completed === 1 ? '' : 'es'}</div>
                            {/if}
                            {#if r.error}<div class="shredder-result-error">{r.error}</div>{/if}
                        </article>
                    {/each}
                </div>
            </section>
        {/if}


    {:else}
        <div class="shredder-warning is-wipe" role="note">
            <AlertTriangle class="shredder-warning-icon" aria-hidden="true" />
            <div>
                <strong>Free-space wipes are heavy operations.</strong>
                <p>KeepItLocal fills the drive containing the selected folder with temporary data, then removes it. Do not run this on SSDs.</p>
            </div>
        </div>

        <div class="shredder-toolbar">
            <div class="shredder-toolbar-group">
                {#if $wipeDir && !$wipeProcessing}
                    <button
                        type="button"
                        onclick={() => wipeDir.set(null)}
                        class="shredder-button is-quiet-danger"
                    >
                        <Trash2 class="shredder-button-icon" aria-hidden="true" />
                        Clear target
                    </button>
                {/if}
            </div>
            <div class="shredder-toolbar-actions">
                <ToolCancelButton running={$wipeProcessing} onCancel={cancelOperation} />
                {#if !showWipeConfirm}
                    <button
                        type="button"
                        onclick={() => { showWipeConfirm = true; wipeConfirmText = ''; }}
                        disabled={!$wipeDir || $wipeProcessing}
                        class="shredder-button is-destructive"
                    >
                        {#if $wipeProcessing}
                            <LoadingState variant="inline" label="Wiping…" />
                        {:else}
                            <Eraser class="shredder-button-icon" aria-hidden="true" />
                            Wipe free space
                        {/if}
                    </button>
                {/if}
            </div>
        </div>

        {#if showWipeConfirm}
            <section class="shredder-confirm" aria-labelledby="wipe-confirm-heading">
                <div>
                    <div id="wipe-confirm-heading" class="shredder-confirm-title">Confirm free-space wipe</div>
                    <p>Type <kbd>WIPE</kbd> to fill the selected drive temporarily.</p>
                </div>
                <div class="shredder-confirm-controls">
                    <input
                        type="text"
                        bind:value={wipeConfirmText}
                        placeholder="WIPE"
                        aria-label="Type WIPE to confirm"
                        class="shredder-confirm-input"
                    />
                    <button
                        type="button"
                        onclick={startWipe}
                        disabled={wipeConfirmText !== 'WIPE'}
                        class="shredder-button is-destructive"
                    >
                        Confirm wipe
                    </button>
                    <button
                        type="button"
                        onclick={() => { showWipeConfirm = false; wipeConfirmText = ''; }}
                        class="shredder-button"
                    >
                        Cancel
                    </button>
                </div>
            </section>
        {/if}

        <div class="shredder-wipe-workspace">
            <section class="shredder-panel shredder-wipe-target" aria-labelledby="wipe-target-heading">
                <div class="shredder-panel-heading">
                    <div>
                        <div class="shredder-eyebrow">Destination</div>
                        <h2 id="wipe-target-heading">Drive target</h2>
                    </div>
                </div>
                <div class="shredder-drive-target" class:is-empty={!$wipeDir}>
                    <FolderOpen class="shredder-drive-icon" aria-hidden="true" />
                    <div class="shredder-drive-copy">
                        <span>{$wipeDir ? 'Selected folder' : 'No folder selected'}</span>
                        <strong title={$wipeDir ?? 'Choose a folder on the drive you want to wipe'}>{$wipeDir ?? 'Choose a target drive folder'}</strong>
                    </div>
                    <button
                        type="button"
                        onclick={pickWipeDir}
                        disabled={$wipeProcessing}
                        class="shredder-button"
                    >
                        {$wipeDir ? 'Change' : 'Choose'} target
                    </button>
                </div>
                <p class="shredder-choice-note">The selected folder only identifies the drive. KeepItLocal creates and removes a temporary file in that location.</p>
            </section>

            <section class="shredder-panel shredder-pattern-panel" aria-labelledby="wipe-pattern-heading">
                <div class="shredder-panel-heading">
                    <div>
                        <div class="shredder-eyebrow">Write strategy</div>
                        <h2 id="wipe-pattern-heading">Pattern</h2>
                    </div>
                </div>
                <div class="shredder-pattern-grid">
                    {#each wipePatterns as p}
                        {@const active = $wipePattern === p.id}
                        <button
                            type="button"
                            onclick={() => wipePattern.set(p.id)}
                            disabled={$wipeProcessing}
                            class="shredder-choice"
                            class:is-active={active}
                            aria-pressed={active}
                        >
                            <span class="shredder-choice-main">
                                <span class="shredder-choice-name">{p.label}</span>
                                <span class="shredder-choice-meta">{p.sub}</span>
                            </span>
                        </button>
                    {/each}
                </div>
                <p class="shredder-choice-note">{currentPattern.description}</p>
            </section>
        </div>

        {#if $wipeProgress && $wipeProcessing}
            <section class="shredder-progress" aria-live="polite">
                <div class="shredder-progress-head">
                    <div>
                        <div class="shredder-eyebrow">In progress</div>
                        <div class="shredder-progress-file">Writing temporary data</div>
                    </div>
                </div>
                <div class="shredder-progress-label">
                    <span>{formatBytes($wipeProgress.bytes_written)}{#if $wipeProgress.estimated_total > 0} / ~{formatBytes($wipeProgress.estimated_total)}{/if}</span>
                    {#if $wipeProgress.estimated_total > 0}<span>{wipePercent.toFixed(1)}%</span>{/if}
                </div>
                {#if $wipeProgress.estimated_total > 0}
                    <div class="shredder-progress-track"><div class="shredder-progress-bar" style={`width: ${wipePercent}%`}></div></div>
                {/if}
            </section>
        {/if}

        {#if $wipeResult}
            <section class="shredder-wipe-result" class:is-error={Boolean($wipeResult.error)}>
                <div>
                    {#if $wipeResult.error}
                        <div class="shredder-result-error">{$wipeResult.error}</div>
                    {:else}
                        <div class="shredder-wipe-result-title">Free space wiped</div>
                        <div class="shredder-result-meta">{formatBytes($wipeResult.bytes)} of temporary data written and removed.</div>
                    {/if}
                </div>
                <button type="button" onclick={clearWipeHistory} class="shredder-clear-history">Clear</button>
            </section>
        {/if}
    {/if}
</ToolPage>

<style>
    .shredder-warning {
        display: grid;
        grid-template-columns: auto minmax(0, 1fr);
        gap: 10px;
        padding: 12px 14px;
        border: 1px solid color-mix(in srgb, var(--color-error) 36%, var(--color-border));
        border-left: 3px solid var(--color-error);
        border-radius: var(--radius-control, 10px);
        color: var(--color-text-secondary);
        font-size: 12.5px;
        line-height: 1.5;
    }
    .shredder-warning.is-wipe {
        border-color: color-mix(in srgb, var(--color-warning) 42%, var(--color-border));
        border-left-color: var(--color-warning);
    }
    .shredder-warning :global(.shredder-warning-icon) {
        width: 17px;
        height: 17px;
        margin-top: 1px;
        color: var(--color-error);
    }
    .shredder-warning.is-wipe :global(.shredder-warning-icon) { color: var(--color-warning); }
    .shredder-warning strong { color: var(--color-text); }
    .shredder-warning p { margin: 2px 0 0; }

    .shredder-tabs {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: 3px;
        padding: 3px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 10px);
        background: var(--color-panel);
    }
    .shredder-tab {
        position: relative;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 8px;
        min-height: 38px;
        padding: 0 14px 0 17px;
        border: 0;
        border-radius: calc(var(--radius-control, 10px) - 3px);
        background: transparent;
        color: var(--color-muted);
        font: inherit;
        font-size: 13px;
        font-weight: 600;
        cursor: pointer;
        transition: background-color var(--dur-micro, 130ms) var(--ease-out, ease), color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .shredder-tab:hover { color: var(--color-text); background: var(--color-panel-2); }
    .shredder-tab.is-active { background: var(--color-panel-2); color: var(--color-text); }
    .shredder-tab.is-active::before {
        content: '';
        position: absolute;
        left: 7px;
        top: 9px;
        bottom: 9px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .shredder-tab :global(.shredder-tab-icon) { width: 15px; height: 15px; color: var(--color-muted); }
    .shredder-tab.is-active :global(.shredder-tab-icon) { color: var(--color-accent); }
    .shredder-live {
        width: 6px;
        height: 6px;
        border-radius: 50%;
        background: var(--color-success);
        animation: shredder-pulse 1.15s ease-in-out infinite;
    }
    @keyframes shredder-pulse { 50% { opacity: .35; transform: scale(.78); } }

    .shredder-toolbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        flex-wrap: wrap;
        gap: 10px;
        padding: 0 0 14px;
        border-bottom: 1px solid var(--color-border);
    }
    .shredder-toolbar-group,
    .shredder-toolbar-actions {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 8px;
    }
    .shredder-button {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 7px;
        min-height: 36px;
        padding: 0 11px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
        color: var(--color-text);
        font: inherit;
        font-size: 12.5px;
        font-weight: 600;
        cursor: pointer;
        transition: border-color var(--dur-micro, 130ms) var(--ease-out, ease), background-color var(--dur-micro, 130ms) var(--ease-out, ease), color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .shredder-button:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-accent) 48%, var(--color-border)); background: var(--color-panel); }
    .shredder-button:disabled { opacity: .48; cursor: not-allowed; }
    .shredder-button.is-quiet-danger:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-error) 55%, var(--color-border)); color: var(--color-error); }
    .shredder-button.is-destructive { border-color: var(--color-error); background: var(--color-error); color: white; }
    .shredder-button.is-destructive:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-error) 82%, black); background: color-mix(in srgb, var(--color-error) 88%, black); }
    .shredder-button :global(.shredder-button-icon) { width: 15px; height: 15px; }

    .shredder-confirm {
        display: flex;
        align-items: end;
        justify-content: space-between;
        flex-wrap: wrap;
        gap: 14px;
        padding: 14px;
        border: 1px solid color-mix(in srgb, var(--color-error) 42%, var(--color-border));
        border-left: 3px solid var(--color-error);
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
    }
    .shredder-confirm-title { color: var(--color-text); font-size: 13px; font-weight: 700; }
    .shredder-confirm p { margin: 3px 0 0; color: var(--color-text-secondary); font-size: 12.5px; }
    .shredder-confirm kbd {
        padding: 1px 5px;
        border: 1px solid var(--color-border);
        border-radius: 4px;
        background: var(--color-panel-2);
        color: var(--color-text);
        font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
        font-size: 11px;
        font-weight: 700;
    }
    .shredder-confirm-controls { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
    .shredder-confirm-input {
        width: 104px;
        min-height: 36px;
        padding: 0 10px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
        font-size: 12px;
        outline: none;
    }
    .shredder-confirm-input:focus { border-color: var(--color-error); box-shadow: 0 0 0 3px color-mix(in srgb, var(--color-error) 14%, transparent); }

    .shredder-workspace,
    .shredder-wipe-workspace {
        display: grid;
        grid-template-columns: minmax(0, 1fr);
        gap: 12px;
    }
    @media (min-width: 880px) {
        .shredder-workspace { grid-template-columns: minmax(0, 1fr) 310px; }
        .shredder-wipe-workspace { grid-template-columns: minmax(0, .9fr) minmax(0, 1.1fr); }
    }
    .shredder-panel,
    .shredder-results,
    .shredder-progress,
    .shredder-wipe-result {
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
    }
    .shredder-panel { padding: 15px; }
    .shredder-panel-heading {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 12px;
        margin-bottom: 12px;
    }
    .shredder-eyebrow {
        color: var(--color-muted);
        font-size: 10.5px;
        font-weight: 700;
        letter-spacing: .08em;
        text-transform: uppercase;
    }
    .shredder-panel h2,
    .shredder-results h2 {
        margin: 2px 0 0;
        color: var(--color-text);
        font-size: 14px;
        font-weight: 700;
    }
    .shredder-count {
        padding: 3px 8px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-pill, 999px);
        color: var(--color-muted);
        font-size: 11px;
        white-space: nowrap;
    }
    .shredder-target-list {
        max-height: 340px;
        overflow: auto;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    .shredder-target-row {
        display: flex;
        align-items: center;
        gap: 10px;
        min-width: 0;
        padding: 10px 11px;
        border-bottom: 1px solid var(--color-border);
    }
    .shredder-target-row:last-child { border-bottom: 0; }
    .shredder-target-row:hover { background: var(--color-panel); }
    :global(.shredder-target-icon) { flex: none; width: 16px; height: 16px; color: var(--color-muted); }
    .shredder-target-copy { min-width: 0; flex: 1; }
    .shredder-target-name,
    .shredder-target-path,
    .shredder-result-title,
    .shredder-result-path,
    .shredder-drive-copy strong {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }
    .shredder-target-name { color: var(--color-text); font-size: 12.5px; font-weight: 600; }
    .shredder-target-path { margin-top: 2px; color: var(--color-muted); font-size: 11px; }
    .shredder-remove,
    .shredder-clear-history {
        border: 0;
        background: transparent;
        color: var(--color-muted);
        font: inherit;
        font-size: 11.5px;
        font-weight: 600;
        cursor: pointer;
    }
    .shredder-remove:hover { color: var(--color-error); }
    .shredder-clear-history:hover { color: var(--color-text); }

    .shredder-choice-list { display: grid; gap: 7px; }
    .shredder-pattern-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 7px; }
    .shredder-choice {
        position: relative;
        display: flex;
        width: 100%;
        min-height: 58px;
        padding: 10px 12px 10px 15px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel);
        color: var(--color-text);
        font: inherit;
        text-align: left;
        cursor: pointer;
        transition: border-color var(--dur-micro, 130ms) var(--ease-out, ease), background-color var(--dur-micro, 130ms) var(--ease-out, ease);
    }
    .shredder-choice:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-accent) 46%, var(--color-border)); }
    .shredder-choice:disabled { opacity: .5; cursor: not-allowed; }
    .shredder-choice.is-active { border-color: color-mix(in srgb, var(--color-accent) 48%, var(--color-border)); background: var(--color-panel-2); }
    .shredder-choice.is-active::before {
        content: '';
        position: absolute;
        left: 6px;
        top: 10px;
        bottom: 10px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .shredder-choice-main { display: grid; gap: 2px; min-width: 0; }
    .shredder-choice-name { color: var(--color-text); font-size: 12.5px; font-weight: 700; }
    .shredder-choice-meta { color: var(--color-muted); font-size: 11px; }
    .shredder-choice-note { margin: 10px 0 0; color: var(--color-text-secondary); font-size: 11.5px; line-height: 1.5; }

    .shredder-toggle-list { display: grid; gap: 9px; margin-top: 16px; padding-top: 14px; border-top: 1px solid var(--color-border); }
    .shredder-toggle { display: flex; align-items: flex-start; gap: 9px; color: var(--color-text-secondary); cursor: pointer; }
    .shredder-toggle input { margin: 2px 0 0; accent-color: var(--color-accent); }
    .shredder-toggle input:disabled + span { opacity: .55; cursor: not-allowed; }
    .shredder-toggle strong,
    .shredder-toggle small { display: block; }
    .shredder-toggle strong { color: var(--color-text); font-size: 12px; font-weight: 600; }
    .shredder-toggle small { margin-top: 2px; font-size: 11px; line-height: 1.35; }

    .shredder-progress { padding: 14px; }
    .shredder-progress-head,
    .shredder-progress-label { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
    .shredder-progress-file { margin-top: 2px; overflow: hidden; color: var(--color-text); font-size: 13px; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
    .shredder-progress-meta { display: flex; align-items: center; flex-wrap: wrap; justify-content: flex-end; gap: 8px; color: var(--color-muted); font-size: 11px; }
    .shredder-progress-label { margin-top: 13px; color: var(--color-text-secondary); font-size: 11.5px; }
    .shredder-progress-label.is-overall { margin-top: 10px; }
    .shredder-progress-track { height: 5px; margin-top: 5px; overflow: hidden; border-radius: 999px; background: var(--color-panel-2); }
    .shredder-progress-bar { height: 100%; border-radius: inherit; background: var(--color-accent); transition: width var(--dur-micro, 130ms) var(--ease-out, ease); }
    .shredder-progress-bar.is-overall { background: var(--color-success); }
    .shredder-results { padding: 15px; }
    .shredder-results-summary { display: flex; align-items: center; flex-wrap: wrap; justify-content: flex-end; gap: 8px; color: var(--color-muted); font-size: 11.5px; }
    .shredder-results-summary .is-success { color: var(--color-success); }
    .shredder-results-summary .is-error { color: var(--color-error); }
    .shredder-result-list { display: grid; gap: 7px; max-height: 460px; overflow: auto; }
    .shredder-result {
        display: grid;
        grid-template-columns: minmax(0, 1fr) auto;
        gap: 5px 12px;
        padding: 10px 11px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    .shredder-result.is-error { border-color: color-mix(in srgb, var(--color-error) 45%, var(--color-border)); }
    .shredder-result-main { min-width: 0; }
    .shredder-result-title { color: var(--color-text); font-size: 12.5px; font-weight: 600; }
    .shredder-result-kind { margin-right: 7px; color: var(--color-muted); font-size: 10px; font-weight: 700; letter-spacing: .05em; text-transform: uppercase; }
    .shredder-result-path { margin-top: 2px; color: var(--color-muted); font-size: 11px; }
    .shredder-status { align-self: start; padding: 2px 7px; border: 1px solid color-mix(in srgb, var(--color-success) 45%, var(--color-border)); border-radius: var(--radius-pill, 999px); color: var(--color-success); font-size: 10px; font-weight: 700; letter-spacing: .04em; text-transform: uppercase; }
    .shredder-status.is-error { border-color: color-mix(in srgb, var(--color-error) 45%, var(--color-border)); color: var(--color-error); }
    .shredder-result-meta,
    .shredder-result-error { grid-column: 1 / -1; font-size: 11px; }
    .shredder-result-meta { color: var(--color-muted); }
    .shredder-result-error { color: var(--color-error); }

    .shredder-drive-target { display: flex; align-items: center; gap: 10px; padding: 12px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); background: var(--color-panel-2); }
    .shredder-drive-target.is-empty { border-style: dashed; }
    :global(.shredder-drive-icon) { flex: none; width: 18px; height: 18px; color: var(--color-muted); }
    .shredder-drive-copy { display: grid; min-width: 0; flex: 1; gap: 2px; }
    .shredder-drive-copy span { color: var(--color-muted); font-size: 11px; }
    .shredder-drive-copy strong { color: var(--color-text); font-size: 12.5px; }

    .shredder-wipe-result { display: flex; align-items: flex-start; justify-content: space-between; gap: 14px; padding: 14px; border-left: 3px solid var(--color-success); }
    .shredder-wipe-result.is-error { border-color: color-mix(in srgb, var(--color-error) 45%, var(--color-border)); border-left-color: var(--color-error); }
    .shredder-wipe-result-title { color: var(--color-success); font-size: 13px; font-weight: 700; }

    .shredder-tab:focus-visible,
    .shredder-button:focus-visible,
    .shredder-choice:focus-visible,
    .shredder-remove:focus-visible,
    .shredder-clear-history:focus-visible {
        outline: 2px solid var(--color-accent);
        outline-offset: 2px;
    }
    @media (max-width: 560px) {
        .shredder-tabs { grid-template-columns: 1fr; }
        .shredder-toolbar-actions { width: 100%; justify-content: flex-end; }
        .shredder-confirm-controls { width: 100%; }
        .shredder-confirm-input { flex: 1; }
        .shredder-drive-target { align-items: flex-start; flex-wrap: wrap; }
        .shredder-drive-target .shredder-button { margin-left: 28px; }
        .shredder-progress-head { align-items: flex-start; flex-direction: column; }
        .shredder-progress-meta { justify-content: flex-start; }
    }
    @media (prefers-reduced-motion: reduce) {
        .shredder-live { animation: none; }
        .shredder-tab,
        .shredder-button,
        .shredder-choice,
        .shredder-progress-bar { transition: none; }
    }
</style>
