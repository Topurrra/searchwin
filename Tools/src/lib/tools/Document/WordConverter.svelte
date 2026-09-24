<script lang="ts">
    /*
      Unified Word → X converter. Replaces the three separate tools
      (Word → PDF / Word → Markdown / Word → Text) with one screen + a
      target-format selector. All file management + progress display is
      shared; only the backend store dispatched to differs.

      Implementation note: rather than rewrite the three stores into one,
      we keep them as-is (battle-tested) and switch which store-set the
      component reads from based on `target`. A future refactor can DRY
      the stores into a single one keyed by target — that's a separate
      cleanup.
    */
    import { open } from '@tauri-apps/plugin-dialog';
    import { onMount } from 'svelte';
    import { get } from 'svelte/store';
    import DropZone from '$lib/DropZone.svelte';
    import LoadingState from '$lib/components/LoadingState.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import ToolCancelButton from '$lib/components/ToolCancelButton.svelte';
    import {
        FileText,
        FileType,
        FileCode,
        Plus,
        FolderOpen,
        Play,
    } from '@lucide/svelte';
    import { ToolPage } from '$lib/ui';
    import { subscribeToToolLaunchTarget } from '$lib/stores/toolLaunchTarget';
    import { wordConverterTarget, type WordConverterTarget } from '$lib/stores/wordConverter';

    import {
        addWordMarkdownFiles,
        cancelWordMarkdown,
        clearWordMarkdownState,
        initWordMarkdownListeners,
        removeWordMarkdownFile,
        runWordMarkdown,
        wordMarkdownCancelling,
        wordMarkdownError,
        wordMarkdownFiles,
        wordMarkdownOutputDir,
        wordMarkdownProcessing,
        wordMarkdownProgress,
        wordMarkdownResults,
    } from '$lib/stores/wordMarkdown';
    import {
        addWordTextFiles,
        cancelWordText,
        clearWordTextState,
        initWordTextListeners,
        removeWordTextFile,
        runWordText,
        wordTextCancelling,
        wordTextError,
        wordTextFiles,
        wordTextOutputDir,
        wordTextProcessing,
        wordTextProgress,
        wordTextResults,
    } from '$lib/stores/wordText';

    // SB-5 (2026-05-29): DOCX→PDF was dropped (everyone already has
    // "Save as PDF" in Word/LibreOffice; we couldn't beat it). The tool
    // is now focused on the genuinely useful, underserved conversion —
    // DOCX → clean Markdown (and plain text) — done well, fully local.
    type Target = WordConverterTarget;

    /** Active target format. Markdown is the headline conversion. */
    let target = $state<Target>(get(wordConverterTarget));

    const TARGETS: Array<{ id: Target; label: string; ext: string; description: string; icon: any }> = [
        {
            id: 'markdown',
            label: 'Markdown',
            ext: '.md',
            description: 'Headings, bold/italic, links, lists, tables, and images → clean GitHub-Flavored Markdown. Great for notes, git, and static sites.',
            icon: FileCode,
        },
        {
            id: 'text',
            label: 'Plain text',
            ext: '.txt',
            description: 'Stripped to body text only. Reliable for scraping content out of styled docs.',
            icon: FileText,
        },
    ];

    let activeTarget = $derived(TARGETS.find((t) => t.id === target)!);

    // Listener init — run all three so backend events route regardless of
    // which target the user picks. Idempotent inits handle re-mounts.
    onMount(() => {
        const stopTarget = subscribeToToolLaunchTarget('word-converter', ({ targetFile }) => {
            target = 'markdown';
            wordConverterTarget.set('markdown');
            void addWordMarkdownFiles([targetFile]);
        });
        initWordMarkdownListeners();
        initWordTextListeners();
        return stopTarget;
    });

    // Pick the right store-set based on `target`. Returns a thin façade
    // so the template stays uniform.
    let bundle = $derived.by(() => {
        switch (target) {
            case 'markdown':
                return {
                    files: wordMarkdownFiles,
                    outputDir: wordMarkdownOutputDir,
                    processing: wordMarkdownProcessing,
                    cancelling: wordMarkdownCancelling,
                    progress: wordMarkdownProgress,
                    results: wordMarkdownResults,
                    error: wordMarkdownError,
                    add: addWordMarkdownFiles,
                    remove: removeWordMarkdownFile,
                    clear: clearWordMarkdownState,
                    run: runWordMarkdown,
                    cancel: cancelWordMarkdown,
                };
            case 'text':
            default:
                return {
                    files: wordTextFiles,
                    outputDir: wordTextOutputDir,
                    processing: wordTextProcessing,
                    cancelling: wordTextCancelling,
                    progress: wordTextProgress,
                    results: wordTextResults,
                    error: wordTextError,
                    add: addWordTextFiles,
                    remove: removeWordTextFile,
                    clear: clearWordTextState,
                    run: runWordText,
                    cancel: cancelWordText,
                };
        }
    });

    async function pickFiles() {
        const sel = await open({
            multiple: true,
            directory: false,
            filters: [{ name: 'Word documents', extensions: ['docx'] }],
        });
        const list = Array.isArray(sel) ? sel : typeof sel === 'string' ? [sel] : [];
        if (list.length > 0) bundle.add(list);
    }

    async function pickOutputDir() {
        const sel = await open({ multiple: false, directory: true });
        if (typeof sel === 'string') bundle.outputDir.set(sel);
    }

    function basename(path: string): string {
        return path.split(/[\\/]/).pop() ?? path;
    }

    /** Progress percentage from the active target's progress event. The
     * three backends emit slightly different shapes so we compute
     * defensively from `files_done / files_total`. */
    let progressPercent = $derived.by(() => {
        const p = $bundle_progress;
        if (!p || !p.files_total) return 0;
        return Math.round((p.files_done / p.files_total) * 100);
    });
    // Convenience re-derives so the template doesn't have to do $bundle.X — Svelte 5 doesn't
    // unwrap stores out of objects automatically. We pull the active stores via $derived.
    let bundle_files = $derived(bundle.files);
    let bundle_outputDir = $derived(bundle.outputDir);
    let bundle_processing = $derived(bundle.processing);
    let bundle_cancelling = $derived(bundle.cancelling);
    let bundle_progress = $derived(bundle.progress);
    let bundle_results = $derived(bundle.results);
    let bundle_error = $derived(bundle.error);
</script>

<ToolPage
    icon={FileType}
    iconTint="#fb7185"
    title="Convert Word to clean Markdown"
    description="Drop in .docx files and KeepItLocal converts them to GitHub-Flavored Markdown (or plain text) — preserving headings, formatting, links, lists, tables, and images. Originals are never touched; everything runs on your machine."
    width="wide"
    fill={false}
>
    <!-- Target format selector -->
    <div class="rounded-2xl border border-border bg-panel p-4">
        <div class="mb-3 text-xs uppercase tracking-wider text-muted font-semibold">Target format</div>
        <div class="grid gap-2 sm:grid-cols-3">
            {#each TARGETS as t (t.id)}
                {@const Icon = t.icon}
                {@const active = target === t.id}
                <button
                    type="button"
                    onclick={() => {
                        target = t.id;
                        wordConverterTarget.set(t.id);
                    }}
                    disabled={$bundle_processing}
                    class="group rounded-xl border p-3 text-left transition-colors
                           {active
                        ? 'border-accent bg-accent-soft'
                        : 'border-border bg-panel-2 hover:border-accent/60'}
                           disabled:opacity-60 disabled:cursor-not-allowed"
                    aria-pressed={active}
                >
                    <div class="flex items-center gap-2">
                        <Icon class="h-4 w-4 {active ? 'text-accent' : 'text-text-secondary'}" />
                        <span class="text-sm font-medium">{t.label}</span>
                        <span class="ml-auto rounded bg-panel px-1.5 py-0.5 font-mono text-[10px] text-muted">{t.ext}</span>
                    </div>
                    <p class="mt-1 text-[11px] leading-snug text-muted">{t.description}</p>
                </button>
            {/each}
        </div>
    </div>

    <!-- Toolbar -->
    <div class="flex flex-wrap items-center gap-3 rounded-2xl border border-border bg-panel p-3 md:p-4">
        <button
            type="button"
            onclick={pickFiles}
            disabled={$bundle_processing}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
        >
            <Plus class="h-4 w-4" />
            Add .docx files
        </button>

        <button
            type="button"
            onclick={pickOutputDir}
            disabled={$bundle_processing}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
            title={$bundle_outputDir ?? 'Save alongside originals'}
        >
            <FolderOpen class="h-4 w-4" />
            {$bundle_outputDir ? `Output: ${basename($bundle_outputDir)}` : 'Output: alongside originals'}
        </button>

        {#if $bundle_files.length > 0 && !$bundle_processing}
            <button
                type="button"
                onclick={() => bundle.clear()}
                class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-error-strong"
            >
                Clear all ({$bundle_files.length})
            </button>
        {/if}

        <div class="ml-auto flex items-center gap-2">
            <ToolCancelButton
                running={$bundle_processing}
                cancelling={$bundle_cancelling}
                onCancel={() => bundle.cancel()}
            />
            <button
                type="button"
                onclick={() => bundle.run()}
                disabled={$bundle_files.length === 0 || $bundle_processing}
                class="inline-flex h-10 items-center gap-1.5 rounded-xl bg-accent px-4 text-sm font-medium text-accent-contrast hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed"
            >
                {#if $bundle_processing}
                    <LoadingState variant="inline" label="Converting…" />
                {:else}
                    <Play class="h-4 w-4" />
                    Convert {$bundle_files.length} → {activeTarget.label}
                {/if}
            </button>
        </div>
    </div>

    <!-- File list -->
    <div class="rounded-2xl border border-border bg-panel p-4 md:p-5">
        <DropZone onFiles={(paths) => bundle.add(paths)} accept={['docx']}>
            {#snippet children()}
                {#if $bundle_files.length === 0}
                    <EmptyState
                        icon={FileType}
                        title="Drop .docx files here"
                        description="Or click 'Add .docx files' above. KeepItLocal only reads files — your originals are never modified."
                        variant="dashed"
                    />
                {:else}
                    <div class="max-h-72 overflow-y-auto divide-y divide-border rounded-lg border border-border bg-panel-2">
                        {#each $bundle_files as path (path)}
                            <div class="flex items-center justify-between px-3 py-2 text-sm hover:bg-bg/40">
                                <div class="flex-1 min-w-0">
                                    <div class="truncate text-text" title={basename(path)}>{basename(path)}</div>
                                    <div class="path-wrap text-xs text-muted" title={path}>{path}</div>
                                </div>
                                {#if !$bundle_processing}
                                    <button
                                        onclick={() => bundle.remove(path)}
                                        class="ml-3 text-muted hover:text-error text-xs"
                                        aria-label="Remove {basename(path)}"
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
    </div>

    <!-- Progress -->
    {#if $bundle_processing || $bundle_progress}
        <div class="bg-panel border border-border rounded-2xl p-4 space-y-2">
            <div class="flex items-center justify-between gap-3 text-sm">
                <div class="flex items-center gap-2 min-w-0">
                    {#if $bundle_processing}
                        <LoadingState variant="inline" label={$bundle_cancelling ? 'Cancelling…' : `Converting to ${activeTarget.label}…`} />
                    {:else}
                        <span class="font-medium">Last conversion</span>
                    {/if}
                    {#if $bundle_progress?.current_file}
                        <span class="text-muted truncate" title={$bundle_progress.current_file}>{basename($bundle_progress.current_file)}</span>
                    {/if}
                </div>
                <span class="text-xs text-muted">
                    {$bundle_progress?.files_done ?? 0}/{$bundle_progress?.files_total ?? $bundle_files.length}
                </span>
            </div>
            <div class="h-1.5 rounded bg-panel-2 overflow-hidden">
                <div class="h-full bg-accent transition-all" style="width: {progressPercent}%"></div>
            </div>
        </div>
    {/if}

    <!-- Error -->
    {#if $bundle_error}
        <div class="rounded-xl border border-error-strong bg-error-soft px-3 py-2 text-sm text-error">
            {$bundle_error}
        </div>
    {/if}

    <!-- Results -->
    {#if $bundle_results.length > 0}
        {@const successCount = $bundle_results.filter((r: any) => r.success).length}
        {@const failCount = $bundle_results.filter((r: any) => !r.success).length}
        <div class="rounded-2xl border border-border bg-panel p-4 md:p-5">
            <div class="mb-3 flex items-center justify-between">
                <h2 class="text-sm uppercase tracking-wider text-muted font-semibold">Results</h2>
                <div class="text-xs">
                    <span class="text-success">{successCount} converted</span>
                    {#if failCount > 0}
                        <span class="text-muted mx-2">·</span>
                        <span class="text-error">{failCount} failed</span>
                    {/if}
                </div>
            </div>
            <div class="space-y-2 max-h-80 overflow-y-auto">
                {#each $bundle_results as r}
                    <div class="rounded-lg border {r.success ? 'border-border bg-panel-2' : 'border-error-strong bg-error-soft'} p-3">
                        <div class="flex items-start justify-between gap-3">
                            <div class="min-w-0 flex-1">
                                <div class="text-sm text-text truncate" title={r.source_path}>{basename(r.source_path)}</div>
                                {#if r.success && r.output_path}
                                    <div class="text-xs text-muted path-wrap" title={r.output_path}>→ {r.output_path}</div>
                                {/if}
                            </div>
                            <span class="text-xs px-2 py-0.5 rounded uppercase font-semibold
                                         {r.success ? 'bg-success-soft text-success border border-success-strong' : 'bg-error-soft text-error border border-error-strong'}">
                                {r.success ? 'OK' : 'FAIL'}
                            </span>
                        </div>
                        {#if !r.success && r.error}
                            <div class="text-xs text-error mt-1">{r.error}</div>
                        {/if}
                    </div>
                {/each}
            </div>
        </div>
    {/if}
</ToolPage>
