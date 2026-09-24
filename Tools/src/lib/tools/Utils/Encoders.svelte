<script lang="ts">
    import { invoke } from '@tauri-apps/api/core';
    import { toast } from '$lib/stores/toasts';
    import { Code2, ArrowDownUp, Copy, Eraser } from '@lucide/svelte';
    import { _ } from 'svelte-i18n';
    import { ToolPage } from '$lib/ui';
    import {
        encodersAlgorithm,
        encodersMode,
        encodersInput,
        encodersOutput,
    } from '$lib/stores/encoders';

    type Algorithm = {
        id: string;
        /** i18n key for the algorithm display name. */
        name: string;
        /** i18n key for the algorithm description. */
        description: string;
        supportsDecode: boolean;
    };

    const algorithms: Algorithm[] = [
        { id: 'base64',    name: 'tool.encoders.algoBase64Name',    description: 'tool.encoders.algoBase64Desc',    supportsDecode: true },
        { id: 'base64url', name: 'tool.encoders.algoBase64UrlName', description: 'tool.encoders.algoBase64UrlDesc', supportsDecode: true },
        { id: 'hex',       name: 'tool.encoders.algoHexName',       description: 'tool.encoders.algoHexDesc',       supportsDecode: true },
        { id: 'url',       name: 'tool.encoders.algoUrlName',       description: 'tool.encoders.algoUrlDesc',       supportsDecode: true },
        { id: 'html',      name: 'tool.encoders.algoHtmlName',      description: 'tool.encoders.algoHtmlDesc',      supportsDecode: true },
        { id: 'binary',    name: 'tool.encoders.algoBinaryName',    description: 'tool.encoders.algoBinaryDesc',    supportsDecode: true },
        { id: 'rot13',     name: 'tool.encoders.algoRot13Name',     description: 'tool.encoders.algoRot13Desc',     supportsDecode: false },
    ];

    // Persisted across navigation via module stores (see stores/encoders).
    const algorithm = encodersAlgorithm;
    const mode = encodersMode;
    const input = encodersInput;
    const output = encodersOutput;
    let error = $state<string | null>(null);

    let currentAlgo = $derived(algorithms.find(a => a.id === $algorithm)!);

    async function run() {
        error = null;
        if (!$input) {
            output.set('');
            return;
        }
        try {
            output.set(await invoke('encode_decode', { algorithm: $algorithm, mode: $mode, input: $input }));
        } catch (e) {
            error = String(e);
            output.set('');
        }
    }

    $effect(() => {
        $input;
        $mode;
        $algorithm;
        run();
    });

    function swap() {
        if (!$output) return;
        input.set($output);
        if (currentAlgo.supportsDecode) {
            mode.set($mode === 'encode' ? 'decode' : 'encode');
        }
    }

    async function copyOutput() {
        if ($output) {
            await navigator.clipboard.writeText($output);
            toast($_('tool.encoders.copiedOutput'), 'success');
        }
    }

    function clearAll() {
        input.set('');
        output.set('');
        error = null;
    }
</script>

<ToolPage
    icon={Code2}
    iconTint="#f59e0b"
    title={$_('tool.encoders.heroTitle')}
    description={$_('tool.encoders.heroDescription')}
    width="wide"
    fill={false}
>

    <!-- Algorithm picker -->
    <div class="enc-algorithms rounded-2xl border border-border bg-panel p-4 md:p-5">
        <div class="mb-3 text-xs uppercase tracking-wider text-muted font-semibold">{$_('tool.encoders.algorithm')}</div>
        <div class="enc-algorithm-list flex flex-wrap gap-2">
            {#each algorithms as algo}
                <button type="button" class:is-active={$algorithm === algo.id} aria-pressed={$algorithm === algo.id}
                        class="enc-algorithm px-3 py-1.5 rounded-lg text-sm transition-colors"
                        onclick={() => algorithm.set(algo.id)}
                        title={$_(algo.description)}
                >
                    {$_(algo.name)}
                </button>
            {/each}
        </div>
        <div class="text-xs text-muted mt-2">{$_(currentAlgo.description)}</div>
    </div>

    <!-- Toolbar (mode toggle + actions) -->
    <div class="enc-toolbar flex flex-wrap items-center gap-3 rounded-2xl border border-border bg-panel p-3 md:p-4">
        <button type="button" class:is-active={$mode === 'encode'} aria-pressed={$mode === 'encode'}
                class="enc-mode inline-flex h-10 items-center gap-1.5 rounded-xl px-4 text-sm font-medium transition-colors"
                onclick={() => mode.set('encode')}
        >
            {$_('tool.encoders.encode')}
        </button>
        <button type="button" class:is-active={$mode === 'decode'} aria-pressed={$mode === 'decode'}
                class="enc-mode inline-flex h-10 items-center gap-1.5 rounded-xl px-4 text-sm font-medium transition-colors
             {!currentAlgo.supportsDecode ? 'opacity-40 cursor-not-allowed' : ''}"
                onclick={() => currentAlgo.supportsDecode && mode.set('decode')}
                disabled={!currentAlgo.supportsDecode}
        >
            {$_('tool.encoders.decode')}
        </button>

        <div class="flex-1"></div>

        <button
                onclick={swap}
                disabled={!$output}
                class="enc-action inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-40"
        >
            <ArrowDownUp class="h-4 w-4" /> {$_('tool.encoders.swap')}
        </button>
        <button
                onclick={clearAll}
                class="enc-action inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm text-muted hover:text-text hover:border-accent/70"
        >
            <Eraser class="h-4 w-4" /> {$_('tool.encoders.clear')}
        </button>
        <button
                onclick={copyOutput}
                disabled={!$output}
                class="enc-primary inline-flex h-10 items-center gap-1.5 rounded-xl bg-accent px-4 text-sm font-medium text-accent-contrast hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed"
        >
            <Copy class="h-4 w-4" /> {$_('tool.encoders.copyOutput')}
        </button>
    </div>

    <!-- Input/output -->
    <div class="enc-editor-workspace rounded-2xl border border-border bg-panel p-4 md:p-5">
        <div class="enc-editor-grid grid gap-4 md:grid-cols-2">
            <div class="enc-editor">
                <div class="flex items-center justify-between mb-2">
                    <span class="text-xs uppercase tracking-wider text-muted font-semibold">{$_('tool.encoders.input')}</span>
                    <span class="text-xs text-muted">{$_('tool.encoders.chars', { values: { count: $input.length } })}</span>
                </div>
                <textarea
                        bind:value={$input}
                        placeholder={$_('tool.encoders.inputPlaceholder')}
                        class="enc-input w-full h-72 bg-panel-2 border border-border rounded-lg p-3 font-mono text-sm
               text-text placeholder:text-muted resize-none
               focus:outline-none focus:border-accent transition-colors"
                ></textarea>
            </div>

            <div class="enc-editor">
                <div class="flex items-center justify-between mb-2">
                    <span class="text-xs uppercase tracking-wider text-muted font-semibold">{$_('tool.encoders.output')}</span>
                    <span class="text-xs text-muted">{$_('tool.encoders.chars', { values: { count: $output.length } })}</span>
                </div>
                <textarea
                        value={$output}
                        readonly
                        class="enc-output w-full h-72 bg-panel-2 rounded-lg p-3 font-mono text-sm
               text-text resize-none cursor-default border
               {error ? 'border-error-strong text-error' : 'border-border'}"
                ></textarea>
            </div>
        </div>

        {#if error}
            <div class="enc-error mt-3 rounded-xl border border-error-strong bg-error-soft px-3 py-2 text-sm text-error">
                {error}
            </div>
        {/if}
    </div>
</ToolPage>
<style>
    .enc-algorithms {
        display: grid;
        gap: 8px;
        padding: 0 0 12px;
        border: 0;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        border-radius: 0;
        background: transparent;
    }
    .enc-algorithm-list { gap: 5px; }
    .enc-algorithm {
        position: relative;
        min-height: 32px;
        padding: 0 11px 0 16px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel);
        color: var(--color-text-secondary);
        font-size: 12px;
        cursor: pointer;
    }
    .enc-algorithm:hover { border-color: color-mix(in srgb, var(--color-text) 24%, var(--color-border)); color: var(--color-text); }
    .enc-algorithm.is-active { border-color: transparent; background: var(--color-panel-2); color: var(--color-text); }
    .enc-algorithm.is-active::before {
        position: absolute;
        top: 6px;
        bottom: 6px;
        left: 5px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }
    .enc-toolbar {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 7px;
        padding: 0 0 12px;
        border: 0;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        border-radius: 0;
        background: transparent;
    }
    .enc-toolbar > :global(.flex-1) { flex: 1; }
    .enc-mode {
        position: relative;
        min-height: 34px;
        padding: 0 12px 0 17px;
        border: 1px solid transparent;
        border-radius: var(--radius-control, 8px);
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12px;
    }
    .enc-mode:hover:not(:disabled) { border-color: var(--color-border); color: var(--color-text); }
    .enc-mode.is-active { border-color: var(--color-border); background: var(--color-panel-2); color: var(--color-text); }
    .enc-mode.is-active::before {
        position: absolute;
        top: 7px;
        bottom: 7px;
        left: 5px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }
    .enc-action, .enc-primary {
        min-height: 34px;
        padding: 0 11px;
        border-radius: var(--radius-control, 8px);
        font-size: 12px;
    }
    .enc-action { background: transparent; color: var(--color-text-secondary); }
    .enc-action:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-text) 24%, var(--color-border)); color: var(--color-text); }
    .enc-primary { border-color: var(--color-accent); background: var(--color-accent); color: var(--color-accent-contrast); }
    .enc-primary:hover:not(:disabled) { background: var(--color-accent-hover, var(--color-accent)); }
    .enc-action:focus-visible, .enc-primary:focus-visible, .enc-algorithm:focus-visible, .enc-mode:focus-visible,
    .enc-input:focus-visible, .enc-output:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }

    .enc-editor-workspace {
        padding: 4px 0 0;
        border: 0;
        border-radius: 0;
        background: transparent;
    }
    .enc-editor-grid { gap: 12px; }
    .enc-editor {
        min-width: 0;
        padding: 13px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
    }
    .enc-editor :global(.mb-2) { margin-bottom: 10px; }
    .enc-input, .enc-output {
        box-sizing: border-box;
        min-height: 18rem;
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
        font-size: 12px;
        line-height: 1.55;
    }
    .enc-output { cursor: text; }
    .enc-error {
        padding: 10px 12px;
        border: 0;
        border-left: 2px solid var(--color-error);
        border-radius: 0;
        background: transparent;
        color: var(--color-error);
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
        font-size: 12px;
        line-height: 1.45;
    }
    @media (max-width: 700px) {
        .enc-toolbar > :global(.flex-1) { display: none; }
        .enc-algorithm-list { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); }
        .enc-algorithm { justify-content: center; }
    }
</style>
