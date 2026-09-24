<script lang="ts">
    import { Copy, ShieldCheck, ShieldOff, Trash2, Plus, Play, RotateCcw, FileCheck2 } from '@lucide/svelte';
    import { open } from '@tauri-apps/plugin-dialog';
    import { toast } from '$lib/stores/toasts';
    import { _ } from 'svelte-i18n';
    import ToolCancelButton from '$lib/components/ToolCancelButton.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import { ToolPage } from '$lib/ui';
    import {
        addHashFiles,
        cancelHashRun,
        clearHashState,
        hashError,
        hashOperationId,
        hashProcessing,
        hashRows,
        hashSummary,
        hashVerifyAlgorithm,
        removeHashFile,
        resetHashRows,
        runHashCheck,
        setHashExpected,
        type HashAlgorithm,
        type HashRow,
    } from '$lib/stores/hashStore';

    const algorithmOptions: HashAlgorithm[] = ['md5', 'sha1', 'sha256', 'blake3'];
    const algorithmDisplay: Record<HashAlgorithm, string> = {
        md5: 'MD5',
        sha1: 'SHA-1',
        sha256: 'SHA-256',
        blake3: 'BLAKE3',
    };

    const rowCount = $derived.by(() => $hashRows.length);
    const canRun = $derived.by(() => rowCount > 0 && !$hashProcessing);
    const progressText = $derived.by(() => `${$hashSummary.done}/${$hashSummary.total}`);

    function statusClass(status: HashRow['status']) {
        if (status === 'pass') return 'bg-success-soft text-success border border-success-strong';
        if (status === 'fail') return 'bg-error-soft text-error border border-error-strong';
        if (status === 'running') return 'bg-warning-soft text-warning border border-warning-strong';
        if (status === 'cancelled') return 'bg-warning-soft text-warning border border-warning-strong';
        if (status === 'error') return 'bg-error-soft text-error border border-error-strong';
        if (status === 'done') return 'bg-success-soft text-success border border-success-strong';
        return 'bg-panel-2 text-muted border border-border';
    }

    /** Returns an i18n key for the row's status badge. */
    function statusText(status: HashRow['status']) {
        if (status === 'pass') return 'tool.hashCheck.statusPass';
        if (status === 'fail') return 'tool.hashCheck.statusFail';
        if (status === 'running') return 'tool.hashCheck.statusRunning';
        if (status === 'cancelled') return 'tool.hashCheck.statusCancelled';
        if (status === 'error') return 'tool.hashCheck.statusError';
        if (status === 'done') return 'tool.hashCheck.statusDone';
        return 'tool.hashCheck.statusWaiting';
    }

    function resultForRow(row: HashRow): string | null {
        if (!row.hashes) return null;
        return row.hashes[$hashVerifyAlgorithm];
    }

    /** Returns an i18n key (or empty string) for the match-result note. */
    function resultMatchText(row: HashRow) {
        if (row.status === 'pass') return 'tool.hashCheck.matchedExpected';
        if (row.status === 'fail') return 'tool.hashCheck.didNotMatch';
        return '';
    }

    function onAlgorithmChange(event: Event) {
        const value = (event.currentTarget as HTMLSelectElement).value as HashAlgorithm;
        hashVerifyAlgorithm.set(value);
    }

    async function pickFiles() {
        const selected = await open({
            multiple: true,
            directory: false,
        });
        if (!selected) return;

        const paths = Array.isArray(selected) ? selected : [selected];
        addHashFiles(paths);
    }

    async function run() {
        await runHashCheck();
    }

    async function cancel() {
        await cancelHashRun();
    }

    async function copyRowHash(hash: string, fileName: string) {
        await navigator.clipboard.writeText(hash);
        toast($_('tool.hashCheck.copiedHash', { values: { fileName } }), 'success');
    }

    async function copyReport() {
        const payload = $hashRows.map((row) => ({
            file: row.path,
            file_name: row.name,
            status: row.status,
            expected: row.expected,
            hashes: row.hashes,
            elapsed_ms: row.elapsed,
            hash_error: row.error,
        }));

        await navigator.clipboard.writeText(JSON.stringify(payload, null, 2));
        toast($_('tool.hashCheck.reportCopied'), 'success');
    }
</script>

<ToolPage
    icon={ShieldCheck}
    iconTint="#f59e0b"
    title={$_('tool.hashCheck.heroTitle')}
    description={$_('tool.hashCheck.heroDescription')}
    width="wide"
    fill={false}
>
    <!-- Toolbar -->
    <div class="hash-toolbar flex flex-wrap items-center gap-3 rounded-2xl border border-border bg-panel p-3 md:p-4">
        <button
            onclick={pickFiles}
            class="hash-action inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
            disabled={$hashProcessing}
        >
            <Plus class="h-4 w-4" />
            {$_('tool.hashCheck.addFiles')}
        </button>

        <button
            onclick={() => resetHashRows()}
            class="hash-action inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
            disabled={$hashProcessing || !rowCount}
        >
            <RotateCcw class="h-4 w-4" />
            {$_('tool.hashCheck.resetStatus')}
        </button>

        <button
            onclick={() => clearHashState()}
            class="hash-action hash-clear inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm text-muted hover:text-text hover:border-error-strong disabled:opacity-50"
            disabled={$hashProcessing || !rowCount}
        >
            {$_('tool.hashCheck.clearAll')}
        </button>

        <button
            onclick={() => void copyReport()}
            class="hash-action inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm text-muted hover:text-text hover:border-accent/70 disabled:opacity-50"
            disabled={!rowCount}
        >
            <Copy class="h-4 w-4" />
            {$_('tool.hashCheck.copyReport')}
        </button>

        <div class="flex-1"></div>

        <label class="hash-algorithm inline-flex items-center gap-2 text-xs text-muted">
            <span>{$_('tool.hashCheck.algorithm')}</span>
            <select
                value={$hashVerifyAlgorithm}
                onchange={onAlgorithmChange}
                class="hash-select h-9 rounded-lg border border-border bg-panel-2 px-2 text-xs text-text"
                disabled={$hashProcessing}
            >
                {#each algorithmOptions as algo}
                    <option value={algo}>{algorithmDisplay[algo]}</option>
                {/each}
            </select>
        </label>

        <ToolCancelButton
            running={$hashProcessing}
            cancelling={false}
            onCancel={() => void cancel()}
        />

        {#if !$hashProcessing}
            <button
                onclick={() => void run()}
                class="hash-primary inline-flex h-10 items-center gap-1.5 rounded-xl bg-accent px-4 text-sm font-medium text-accent-contrast hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed"
                disabled={!canRun}
            >
                <Play class="h-4 w-4" />
                {$_('tool.hashCheck.runIntegrityCheck')}
            </button>
        {/if}
    </div>

    <!-- Summary -->
    <div class="hash-summary rounded-2xl border border-border bg-panel p-3 md:p-4">
        <div class="hash-summary-metrics flex flex-wrap items-center gap-x-4 gap-y-2 text-xs text-muted">
            <div>{$_('tool.hashCheck.progress')} <span class="text-text font-medium">{progressText}</span></div>
            <div class="flex items-center gap-1"><span class="text-success">{$_('tool.hashCheck.statusPass')}</span> {$hashSummary.pass}</div>
            <div class="flex items-center gap-1"><span class="text-error">{$_('tool.hashCheck.statusFail')}</span> {$hashSummary.fail}</div>
            <div class="flex items-center gap-1"><span class="text-error">{$_('tool.hashCheck.statusError')}</span> {$hashSummary.error}</div>
            <div class="flex items-center gap-1"><span class="text-warning">{$_('tool.hashCheck.statusCancelled')}</span> {$hashSummary.cancelled}</div>
            <div>{$_('tool.hashCheck.total', { values: { count: $hashSummary.total } })}</div>
            <div class="ml-auto">{$hashProcessing ? $_('tool.hashCheck.processing') : $_('tool.hashCheck.ready')}</div>
        </div>

        {#if $hashError}
            <div class="hash-error mt-3 rounded-xl border border-error-strong bg-error-soft px-3 py-2 text-sm text-error">{$hashError}</div>
        {/if}

        {#if $hashProcessing && $hashOperationId}
            <div class="hash-operation mt-2 text-xs text-muted">{$_('tool.hashCheck.runningOperation', { values: { id: $hashOperationId } })}</div>
        {/if}
    </div>

    <!-- Table -->
    <div class="hash-workspace rounded-2xl border border-border bg-panel p-4 md:p-5">
        {#if $hashRows.length === 0}
            <EmptyState
                icon={FileCheck2}
                title={$_('tool.hashCheck.noFilesTitle')}
                description={$_('tool.hashCheck.noFilesDescription')}
                variant="dashed"
            />
        {:else}
            <div class="hash-table-shell overflow-auto rounded-xl border border-border bg-panel-2 max-h-[60vh]">
                <table class="hash-table w-full text-xs">
                    <thead class="bg-bg/40 border-b border-border sticky top-0">
                        <tr class="text-left text-muted">
                            <th class="px-3 py-2 font-semibold">{$_('tool.hashCheck.colFile')}</th>
                            <th class="px-3 py-2 font-semibold">{$_('tool.hashCheck.colExpected')}</th>
                            <th class="px-3 py-2 font-semibold">{$_('tool.hashCheck.colStatus')}</th>
                            <th class="px-3 py-2 font-semibold">{$_('tool.hashCheck.colResult')}</th>
                            <th class="px-3 py-2 font-semibold">{$_('tool.hashCheck.colActions')}</th>
                        </tr>
                    </thead>
                    <tbody>
                        {#each $hashRows as row}
                            <tr class="border-t border-border align-top">
                                <td class="px-3 py-2">
                                    <div class="tool-filename font-semibold text-text" title={row.path}>{row.name}</div>
                                    <div class="path-wrap text-muted" title={row.path}>{row.path}</div>
                                </td>
                                <td class="px-3 py-2">
                                    <textarea
                                        oninput={(event) => setHashExpected(row.id, (event.currentTarget as HTMLTextAreaElement).value)}
                                        rows="2"
                                        value={row.expected}
                                        placeholder={$_('tool.hashCheck.expectedPlaceholder', { values: { algorithm: algorithmDisplay[$hashVerifyAlgorithm] } })}
                                        class="hash-expected w-full border border-border bg-panel rounded-lg p-1.5 font-mono text-xs focus:outline-none focus:border-accent"
                                    ></textarea>
                                </td>
                                <td class="px-3 py-2">
                                    <span class={`hash-status rounded-md px-2 py-1 inline-block text-[10px] font-semibold ${statusClass(row.status)}`}>{$_(statusText(row.status))}</span>
                                    {#if row.elapsed > 0}
                                        <div class="text-muted mt-1">{row.elapsed}ms</div>
                                    {/if}
                                </td>
                                <td class="px-3 py-2">
                                    {#if resultForRow(row)}
                                        <div class="font-mono break-all">{resultForRow(row)}</div>
                                        {#if row.expected.trim()}
                                            <div class="mt-1 {row.status === 'pass' ? 'text-success' : row.status === 'fail' ? 'text-error' : 'text-muted'}">
                                                {$_(resultMatchText(row))}
                                            </div>
                                        {/if}
                                    {:else if row.error}
                                        <div class="text-error break-all">{row.error}</div>
                                    {:else if row.status === 'running'}
                                        <div class="text-muted">{$_('tool.hashCheck.computingHashes')}</div>
                                    {:else}
                                        <div class="text-muted">{$_('tool.hashCheck.noHashYet')}</div>
                                    {/if}
                                </td>
                                <td class="px-3 py-2">
                                    <div class="flex flex-wrap items-center gap-1">
                                        {#if resultForRow(row)}
                                            <button
                                                onclick={() => void copyRowHash(resultForRow(row) as string, row.name)}
                                                class="hash-icon-button rounded-lg border border-border bg-panel p-2 hover:border-accent/70"
                                                title={$_('tool.hashCheck.copyHash')}
                                            >
                                                <Copy class="w-3 h-3" />
                                            </button>
                                        {/if}
                                        <button
                                            onclick={() => removeHashFile(row.id)}
                                            class="hash-icon-button is-danger rounded-lg border border-border bg-panel p-2 text-error hover:border-error-strong disabled:opacity-50"
                                            disabled={$hashProcessing}
                                            title={$_('tool.hashCheck.remove')}
                                        >
                                            <Trash2 class="w-3 h-3" />
                                        </button>
                                    </div>
                                </td>
                            </tr>
                        {/each}
                    </tbody>
                </table>
            </div>
        {/if}

        {#if $hashRows.some((row) => row.hashes)}
            <div class="hash-complete mt-4 flex flex-wrap items-center gap-3">
                {#if $hashRows.some((row) => row.status === 'pass' || row.status === 'fail')}
                    <div class="text-xs text-success flex items-center gap-1">
                        <ShieldCheck class="w-3.5 h-3.5" />
                        {$_('tool.hashCheck.integrityCompleted')}
                    </div>
                {/if}
                {#if $hashRows.some((row) => row.status === 'fail')}
                    <div class="text-xs text-error flex items-center gap-1">
                        <ShieldOff class="w-3.5 h-3.5" />
                        {$_('tool.hashCheck.digestsMismatch')}
                    </div>
                {/if}
            </div>
        {/if}
    </div>
</ToolPage>
<style>
    .hash-toolbar {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 8px;
        padding: 0 0 12px;
        border: 0;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        border-radius: 0;
        background: transparent;
    }
    .hash-toolbar > :global(.flex-1) { flex: 1; }
    .hash-action, .hash-primary {
        min-height: 34px;
        padding: 0 11px;
        border-radius: var(--radius-control, 8px);
        background: transparent;
        font-size: 12px;
    }
    .hash-action { color: var(--color-text-secondary); }
    .hash-action:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-text) 24%, var(--color-border)); color: var(--color-text); }
    .hash-clear:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-error) 44%, var(--color-border)); color: var(--color-error); }
    .hash-primary { border-color: var(--color-accent); background: var(--color-accent); color: var(--color-accent-contrast); }
    .hash-primary:hover:not(:disabled) { background: var(--color-accent-hover, var(--color-accent)); }
    .hash-action:focus-visible, .hash-primary:focus-visible, .hash-select:focus-visible, .hash-expected:focus-visible,
    .hash-icon-button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
    .hash-algorithm { min-height: 34px; padding-left: 8px; border-left: 1px solid var(--color-divider, var(--color-border)); }
    .hash-select { min-height: 30px; border-radius: var(--radius-control, 8px); background: var(--color-panel); color: var(--color-text); }

    .hash-summary {
        padding: 11px 0 11px 12px;
        border: 0;
        border-left: 2px solid var(--color-accent);
        border-radius: 0;
        background: transparent;
    }
    .hash-summary-metrics { gap: 8px 16px; }
    .hash-summary-metrics > div { display: inline-flex; align-items: center; min-height: 22px; }
    .hash-summary-metrics > :global(.ml-auto) { margin-left: auto; }
    .hash-error {
        padding: 8px 0;
        border: 0;
        border-left: 2px solid var(--color-error);
        border-radius: 0;
        background: transparent;
        color: var(--color-error);
        font-size: 12px;
    }
    .hash-operation { font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace); }

    .hash-workspace { padding: 4px 0 0; border: 0; border-radius: 0; background: transparent; }
    .hash-table-shell {
        max-height: min(60vh, 42rem);
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
    }
    .hash-table { min-width: 760px; border-collapse: separate; border-spacing: 0; }
    .hash-table thead { background: var(--color-panel-2); }
    .hash-table th { height: 38px; color: var(--color-muted); font-size: 10.5px; letter-spacing: .04em; text-transform: uppercase; }
    .hash-table td { background: var(--color-panel); }
    .hash-table tbody tr:hover td { background: color-mix(in srgb, var(--color-text) 2.5%, var(--color-panel)); }
    .hash-expected {
        min-width: 13rem;
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
        color: var(--color-text);
        line-height: 1.4;
    }
    .hash-status { border-radius: 999px; }
    .hash-icon-button {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 30px;
        height: 30px;
        padding: 0;
        border-radius: var(--radius-control, 8px);
        background: transparent;
        color: var(--color-muted);
        cursor: pointer;
    }
    .hash-icon-button:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-text) 24%, var(--color-border)); color: var(--color-text); }
    .hash-icon-button.is-danger:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-error) 44%, var(--color-border)); color: var(--color-error); }
    .hash-complete { padding: 10px 12px; border-top: 1px solid var(--color-divider, var(--color-border)); }
    @media (max-width: 760px) {
        .hash-toolbar > :global(.flex-1) { display: none; }
        .hash-algorithm { width: 100%; padding: 0; border-left: 0; }
        .hash-summary-metrics > :global(.ml-auto) { width: 100%; margin-left: 0; }
    }
</style>
