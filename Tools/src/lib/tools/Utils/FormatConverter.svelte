<script lang="ts">
    import {invoke} from '@tauri-apps/api/core';
    import { toast } from '$lib/stores/toasts';
    import { ArrowLeftRight, Eraser, Copy, FileJson } from '@lucide/svelte';
    import { _ } from 'svelte-i18n';
    import { ToolPage } from '$lib/ui';
    import {
        formatConverterFrom,
        formatConverterTo,
        formatConverterInput,
        formatConverterOutput,
        formatConverterView,
        type FormatConverterFormat,
    } from '$lib/stores/formatConverter';

    type Format = FormatConverterFormat;

    const formats: { id: Format; name: string }[] = [
        {id: 'json', name: 'JSON'},
        {id: 'yaml', name: 'YAML'},
        {id: 'toml', name: 'TOML'},
        {id: 'xml', name: 'XML'},
    ];

    // Persisted across navigation via module stores (see stores/formatConverter).
    const from = formatConverterFrom;
    const to = formatConverterTo;
    const input = formatConverterInput;
    const output = formatConverterOutput;
    const view = formatConverterView;
    let error = $state<string | null>(null);

    async function run() {
        error = null;
        if (!$input.trim()) {
            output.set('');
            return;
        }
        try {
            output.set(await invoke('convert_format', { from: $from, to: $to, input: $input }));
        } catch (e) {
            error = String(e);
            output.set('');
        }
    }

    let timeout: ReturnType<typeof setTimeout>;
    $effect(() => {
        $input;
        $from;
        $to;
        clearTimeout(timeout);
        timeout = setTimeout(run, 150);
    });

    function swap() {
        if (!$output) return;
        const oldFrom = $from;
        from.set($to);
        to.set(oldFrom);
        input.set($output);
    }

    async function copyOutput() {
        if ($output) {
            await navigator.clipboard.writeText($output);
            toast($_('tool.formatConverter.copiedOutput'), 'success');
        }
    }

    function clearAll() {
        input.set('');
        output.set('');
        error = null;
    }

    // ─── JSON tree explorer ──────────────────────────────────────────────
    // Folded in from the former standalone JSON Beautifier: a clickable
    // tree view of the input parsed as JSON. The conversion side already
    // pretty-prints JSON (JSON → JSON), so the tree is the piece worth
    // carrying over — explore structure, click any node to copy its path.
    type TreeNode = { key: string | null; value: unknown; path: string; depth: number };

    /** The input flattened to a clickable node list. Empty when the input
     *  is not valid JSON — the tree is a JSON view (switch `from` to JSON). */
    let treeNodes = $derived.by((): TreeNode[] => {
        if (!$input.trim()) return [];
        try {
            const parsed: unknown = JSON.parse($input);
            const nodes: TreeNode[] = [];
            flattenJson(parsed, null, '$', 0, nodes);
            return nodes;
        } catch {
            return [];
        }
    });

    function flattenJson(
        val: unknown,
        key: string | null,
        path: string,
        depth: number,
        nodes: TreeNode[],
    ): void {
        nodes.push({ key, value: val, path, depth });
        if (Array.isArray(val)) {
            val.slice(0, 50).forEach((v, i) =>
                flattenJson(v, String(i), `${path}[${i}]`, depth + 1, nodes),
            );
            if (val.length > 50) {
                nodes.push({
                    key: `… ${val.length - 50} more`,
                    value: null,
                    path,
                    depth: depth + 1,
                });
            }
        } else if (val !== null && typeof val === 'object') {
            const keys = Object.keys(val as Record<string, unknown>);
            keys.slice(0, 50).forEach((k) =>
                flattenJson(
                    (val as Record<string, unknown>)[k],
                    k,
                    `${path}.${k}`,
                    depth + 1,
                    nodes,
                ),
            );
            if (keys.length > 50) {
                nodes.push({
                    key: `… ${keys.length - 50} more`,
                    value: null,
                    path,
                    depth: depth + 1,
                });
            }
        }
    }

    function valueColor(val: unknown): string {
        if (val === null) return 'text-muted';
        if (typeof val === 'boolean') return 'text-warning';
        if (typeof val === 'number') return 'text-info';
        if (typeof val === 'string') return 'text-success';
        return 'text-muted';
    }

    function valuePreview(val: unknown): string {
        if (val === null) return 'null';
        if (Array.isArray(val)) return `[${val.length}]`;
        if (typeof val === 'object') return `{${Object.keys(val as object).length}}`;
        if (typeof val === 'string') {
            return `"${val.length > 40 ? val.slice(0, 40) + '…' : val}"`;
        }
        return String(val);
    }

    async function copyPath(path: string): Promise<void> {
        await navigator.clipboard.writeText(path);
        toast($_('tool.formatConverter.treeCopiedPath'), 'success');
    }
</script>

<ToolPage
    icon={FileJson}
    iconTint="#f59e0b"
    title={$_('tool.formatConverter.heroTitle')}
    description={$_('tool.formatConverter.heroDescription')}
    width="wide"
    fill={false}
>

    <!-- Toolbar (format pickers + actions) -->
    <div class="fc-toolbar rounded-2xl border border-border bg-panel p-3 md:p-4 space-y-3">
        <div class="fc-format-layout flex flex-wrap items-end gap-3">
            <div class="fc-format-group flex-1 min-w-[260px]">
                <span id="fc-from-label" class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">{$_('tool.formatConverter.from')}</span>
                <div class="fc-format-list flex flex-wrap gap-2" role="group" aria-labelledby="fc-from-label">
                    {#each formats as f}
                        <button type="button" class:is-active={$from === f.id} aria-pressed={$from === f.id}
                                class="fc-format-button px-3 py-1.5 rounded-lg text-sm transition-colors"
                                onclick={() => from.set(f.id)}
                        >
                            {f.name}
                        </button>
                    {/each}
                </div>
            </div>

            <button
                    onclick={swap}
                    disabled={!$output}
                    class="fc-action inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-40"
                    title={$_('tool.formatConverter.swapTitle')}
            >
                <ArrowLeftRight class="h-4 w-4" />
                {$_('tool.formatConverter.swap')}
            </button>

            <div class="fc-format-group flex-1 min-w-[260px]">
                <span id="fc-to-label" class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">{$_('tool.formatConverter.to')}</span>
                <div class="fc-format-list flex flex-wrap gap-2" role="group" aria-labelledby="fc-to-label">
                    {#each formats as f}
                        <button type="button" class:is-active={$to === f.id} aria-pressed={$to === f.id}
                                class="fc-format-button px-3 py-1.5 rounded-lg text-sm transition-colors
                       {f.id === $from ? 'opacity-50' : ''}"
                                onclick={() => to.set(f.id)}
                        >
                            {f.name}
                        </button>
                    {/each}
                </div>
            </div>
        </div>

        <div class="fc-action-row flex items-center gap-2">
            <button
                    onclick={clearAll}
                    class="fc-action inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm text-muted hover:text-text hover:border-accent/70"
            >
                <Eraser class="h-4 w-4" /> {$_('tool.formatConverter.clear')}
            </button>
            <div class="flex-1"></div>
            <button
                    onclick={copyOutput}
                    disabled={!$output}
                    class="fc-primary inline-flex h-10 items-center gap-1.5 rounded-xl bg-accent px-4 text-sm font-medium text-accent-contrast hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed"
            >
                <Copy class="h-4 w-4" /> {$_('tool.formatConverter.copyOutput')}
            </button>
        </div>
    </div>

    <!-- Editors -->
    <div class="fc-editor-workspace rounded-2xl border border-border bg-panel p-4 md:p-5">
        <div class="fc-editor-grid grid gap-4 md:grid-cols-2">
            <div class="fc-editor">
                <div class="flex items-center justify-between mb-2">
                    <span class="text-xs uppercase tracking-wider text-muted font-semibold">
                        {$_('tool.formatConverter.inputLabel', { values: { format: formats.find(f => f.id === $from)?.name ?? '' } })}
                    </span>
                    <span class="text-xs text-muted">{$_('tool.formatConverter.chars', { values: { count: $input.length } })}</span>
                </div>
                <textarea
                        bind:value={$input}
                        spellcheck="false"
                        class="fc-input w-full h-[60vh] bg-panel-2 border border-border rounded-lg p-3 font-mono text-sm
                   text-text placeholder:text-muted resize-none
                   focus:outline-none focus:border-accent transition-colors"
                ></textarea>
            </div>

            <div class="fc-editor">
                <div class="flex items-center justify-between mb-2 gap-2">
                    <div class="fc-view-segment flex gap-1" role="group" aria-labelledby="fc-output-view-label">
                        <span id="fc-output-view-label" class="sr-only">{$_('tool.formatConverter.outputLabel', { values: { format: formats.find(f => f.id === $to)?.name ?? '' } })}</span>
                        <button type="button" class:is-active={$view === 'convert'} aria-pressed={$view === 'convert'}
                                class="fc-view-button px-2.5 py-1 rounded-lg text-xs transition-colors"
                                onclick={() => view.set('convert')}
                        >
                            {$_('tool.formatConverter.viewConvert')}
                        </button>
                        <button type="button" class:is-active={$view === 'tree'} aria-pressed={$view === 'tree'}
                                class="fc-view-button px-2.5 py-1 rounded-lg text-xs transition-colors"
                                onclick={() => view.set('tree')}
                        >
                            {$_('tool.formatConverter.viewTree')}
                        </button>
                    </div>
                    {#if $view === 'convert'}
                        <span class="text-xs text-muted">{$_('tool.formatConverter.chars', { values: { count: $output.length } })}</span>
                    {/if}
                </div>

                {#if $view === 'convert'}
                    <textarea
                            value={$output}
                            readonly
                            spellcheck="false"
                            class="fc-output w-full h-[60vh] bg-panel-2 rounded-lg p-3 font-mono text-sm
                       text-text resize-none cursor-default border
                       {error ? 'border-error-strong text-error' : 'border-border'}"
                    ></textarea>
                {:else}
                    <!-- JSON tree explorer — click any node to copy its path. -->
                    <div class="fc-tree w-full h-[60vh] bg-panel-2 border border-border rounded-lg overflow-y-auto">
                        {#if treeNodes.length === 0}
                            <div class="p-3 text-sm text-muted">{$_('tool.formatConverter.treeEmpty')}</div>
                        {:else}
                            {#each treeNodes as node}
                                <button type="button"
                                        class="fc-tree-row w-full text-left px-3 py-0.5 font-mono text-xs hover:bg-panel flex items-baseline gap-1.5 group"
                                        style="padding-left: {(node.depth * 16) + 12}px"
                                        onclick={() => copyPath(node.path)}
                                        title={node.path}
                                >
                                    {#if node.key !== null}
                                        <span class="text-accent shrink-0">{node.key}</span>
                                        <span class="text-muted shrink-0">:</span>
                                    {/if}
                                    <span class={valueColor(node.value)}>{valuePreview(node.value)}</span>
                                    <span class="text-[10px] text-muted opacity-0 group-hover:opacity-100 ml-auto shrink-0">{node.path}</span>
                                </button>
                            {/each}
                        {/if}
                    </div>
                {/if}
            </div>
        </div>

        {#if error}
            <div class="fc-error mt-3 rounded-xl border border-error-strong bg-error-soft px-3 py-2 text-sm text-error font-mono">
                {error}
            </div>
        {/if}
    </div>
</ToolPage>
<style>
    .fc-toolbar {
        display: flex;
        flex-direction: column;
        gap: 10px;
        padding: 0 0 12px;
        border: 0;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        border-radius: 0;
        background: transparent;
    }
    .fc-format-layout { align-items: end; gap: 12px; width: 100%; }
    .fc-format-group { min-width: min(100%, 16rem); }
    .fc-format-list { gap: 4px; }
    .fc-format-button {
        position: relative;
        min-height: 32px;
        padding: 0 10px 0 15px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel);
        color: var(--color-text-secondary);
        font-size: 12px;
        cursor: pointer;
    }
    .fc-format-button:hover { border-color: color-mix(in srgb, var(--color-text) 24%, var(--color-border)); color: var(--color-text); }
    .fc-format-button.is-active { border-color: transparent; background: var(--color-panel-2); color: var(--color-text); }
    .fc-format-button.is-active::before {
        position: absolute;
        top: 6px;
        bottom: 6px;
        left: 5px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }
    .fc-action, .fc-primary {
        min-height: 34px;
        padding: 0 11px;
        border-radius: var(--radius-control, 8px);
        background: transparent;
        font-size: 12px;
    }
    .fc-action { color: var(--color-text-secondary); }
    .fc-action:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-text) 24%, var(--color-border)); color: var(--color-text); }
    .fc-primary { border-color: var(--color-accent); background: var(--color-accent); color: var(--color-accent-contrast); }
    .fc-primary:hover:not(:disabled) { background: var(--color-accent-hover, var(--color-accent)); }
    .fc-action-row { width: 100%; }
    .fc-action-row > :global(.flex-1) { flex: 1; }
    .fc-format-button:focus-visible, .fc-action:focus-visible, .fc-primary:focus-visible, .fc-input:focus-visible,
    .fc-output:focus-visible, .fc-view-button:focus-visible, .fc-tree-row:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }

    .fc-editor-workspace { padding: 4px 0 0; border: 0; border-radius: 0; background: transparent; }
    .fc-editor-grid { gap: 12px; }
    .fc-editor {
        min-width: 0;
        padding: 13px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
    }
    .fc-editor :global(.mb-2) { margin-bottom: 10px; }
    .fc-input, .fc-output {
        box-sizing: border-box;
        min-height: 28rem;
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
        font-size: 12px;
        line-height: 1.55;
    }
    .fc-output { cursor: text; }
    .fc-view-segment {
        display: inline-flex;
        gap: 3px;
        padding: 3px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel);
    }
    .fc-view-button {
        position: relative;
        min-height: 27px;
        padding: 0 8px 0 13px;
        border: 0;
        border-radius: calc(var(--radius-control, 8px) - 2px);
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 11px;
        cursor: pointer;
    }
    .fc-view-button:hover { color: var(--color-text); }
    .fc-view-button.is-active { background: var(--color-panel-2); color: var(--color-text); }
    .fc-view-button.is-active::before {
        position: absolute;
        top: 6px;
        bottom: 6px;
        left: 4px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }
    .fc-tree {
        min-height: 28rem;
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    .fc-tree-row {
        min-height: 24px;
        border: 0;
        background: transparent;
        color: var(--color-text-secondary);
        cursor: pointer;
    }
    .fc-tree-row:hover { background: color-mix(in srgb, var(--color-text) 4%, transparent); }
    .fc-error {
        padding: 10px 12px;
        border: 0;
        border-left: 2px solid var(--color-error);
        border-radius: 0;
        background: transparent;
        color: var(--color-error);
        font-size: 12px;
        line-height: 1.45;
    }
    @media (max-width: 720px) {
        .fc-format-layout { flex-direction: column; align-items: stretch; }
        .fc-format-group { min-width: 0; }
        .fc-format-layout > .fc-action { align-self: center; }
    }
</style>
