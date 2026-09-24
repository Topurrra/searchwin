<script lang="ts">
    import { onMount } from 'svelte';
    import { open } from '@tauri-apps/plugin-dialog';
    import DropZone from '$lib/DropZone.svelte';
    import {
        addAutomationFiles,
        applyRecipeDefaults,
        automationActivity,
        automationCancelling,
        automationCompressEnabled,
        automationCompressQuality,
        automationConvertEnabled,
        automationConvertFormat,
        automationCurrentStep,
        automationFiles,
        automationFinalResults,
        automationImageQuality,
        automationLastError,
        automationOutputDir,
        automationPlainSummary,
        automationProcessing,
        automationProgressByKey,
        automationRecipeName,
        automationResizeEnabled,
        automationResizeLongestSide,
        automationStats,
        automationStripMetadataEnabled,
        automationView,
        builtInAutomationRecipes,
        cancelImageAutomationRecipe,
        clearAutomationActivity,
        clearAutomationFiles,
        clearAutomationRun,
        deleteAutomationRecipe,
        destroyAutomationRecipesStoreListener,
        enabledAutomationSteps,
        fileName,
        fmtBytes,
        initAutomationRecipesStore,
        loadAutomationRecipe,
        readableStepName,
        recipeActionSummary,
        removeAutomationFile,
        saveCurrentAutomationRecipe,
        savedAutomationRecipes,
        selectBuiltInRecipe,
        selectedAutomationRecipe,
        selectedRecipeCard,
        type AutomationRecipeId,
    } from '$lib/stores/automationRecipes';
    import AutomationJobs from './AutomationJobs.svelte';
    import {
        destroyAutomationJobsStoreListener,
        enqueueCurrentAutomationRecipe,
        initAutomationJobsStore,
    } from '$lib/stores/automationJobs';
    import {
        AlertTriangle,
        ArrowRight,
        CheckCircle2,
        ChevronDown,
        Clock3,
        FileImage,
        FolderOpen,
        History,
        Image,
        Loader2,
        Mail,
        PackageCheck,
        Play,
        RotateCcw,
        Save,
        Settings2,
        ShieldCheck,
        Trash2,
        X,
    } from '@lucide/svelte';

    const imageExts = ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'tiff', 'tif', 'gif', 'svg', 'ico'];
    const formats = [
        { id: 'jpg', label: 'JPG' },
        { id: 'png', label: 'PNG' },
        { id: 'webp', label: 'WebP' },
        { id: 'bmp', label: 'BMP' },
        { id: 'tiff', label: 'TIFF' },
    ];

    let showAdvanced = $state(false);
    let showSaved = $state(false);

    onMount(() => {
        initAutomationRecipesStore();
        initAutomationJobsStore();
        return () => {
            destroyAutomationRecipesStoreListener();
            destroyAutomationJobsStoreListener();
        };
    });

    async function pickImages() {
        const picked = await open({
            multiple: true,
            filters: [{ name: 'Images', extensions: imageExts }],
        });
        if (!picked) return;
        addAutomationFiles(Array.isArray(picked) ? picked : [picked]);
    }

    async function pickOutputDir() {
        const picked = await open({ directory: true, multiple: false });
        if (typeof picked === 'string') automationOutputDir.set(picked);
    }

    function iconFor(id: AutomationRecipeId | string) {
        if (id === 'clean-photos') return ShieldCheck;
        if (id === 'email-small') return Mail;
        if (id === 'safe-share-images') return PackageCheck;
        if (id === 'custom-image') return Settings2;
        return Image;
    }

    function timeLabel(value: number) {
        return new Date(value).toLocaleString([], { month: 'short', day: '2-digit', hour: '2-digit', minute: '2-digit' });
    }

    function duration(start: number, end: number) {
        const sec = Math.max(1, Math.round((end - start) / 1000));
        if (sec < 60) return `${sec}s`;
        return `${Math.floor(sec / 60)}m ${sec % 60}s`;
    }

    function resultTone(level: string) {
        if (level === 'success') return 'text-success';
        if (level === 'warning') return 'text-warning';
        if (level === 'error') return 'text-error';
        return 'text-muted';
    }

    function currentStepText() {
        if ($automationCancelling) return 'Cancelling…';
        return readableStepName($automationCurrentStep);
    }

    const progressItems = $derived(Object.values($automationProgressByKey));
    const lastProgress = $derived(progressItems.at(-1));
</script>

<section class="p-4 max-w-7xl mx-auto space-y-3">
    <div class="flex flex-wrap items-center justify-between gap-3">
        <div>
            <h1 class="text-lg font-semibold tracking-tight flex items-center gap-2">
                <Settings2 class="w-4 h-4 text-accent" /> Automation
            </h1>
            <p class="text-xs text-muted">Manual local recipes. KeepItLocal shows the plan before it runs.</p>
        </div>

        <div class="flex items-center gap-1 bg-panel border border-border rounded p-1">
            <button onclick={() => automationView.set('hub')} class="h-8 px-3 rounded text-sm {$automationView === 'hub' ? 'bg-accent text-accent-contrast' : 'text-muted hover:bg-panel-2'}">Recipes</button>
            <button onclick={() => automationView.set('setup')} class="h-8 px-3 rounded text-sm {$automationView === 'setup' ? 'bg-accent text-accent-contrast' : 'text-muted hover:bg-panel-2'}">Setup</button>
            <button onclick={() => automationView.set('jobs')} class="h-8 px-3 rounded text-sm {$automationView === 'jobs' ? 'bg-accent text-accent-contrast' : 'text-muted hover:bg-panel-2'}">Jobs</button>
            <button onclick={() => automationView.set('activity')} class="h-8 px-3 rounded text-sm {$automationView === 'activity' ? 'bg-accent text-accent-contrast' : 'text-muted hover:bg-panel-2'}">Activity</button>
        </div>
    </div>

    {#if $automationProcessing}
        <div class="bg-panel border border-border rounded p-3">
            <div class="flex flex-wrap items-center justify-between gap-3">
                <div class="flex items-center gap-2">
                    <Loader2 class="w-4 h-4 text-accent animate-spin" />
                    <div>
                        <div class="text-sm font-medium">{currentStepText()}</div>
                        <div class="text-xs text-muted">{lastProgress?.file_name ?? `${$automationFiles.length} file${$automationFiles.length === 1 ? '' : 's'} selected`}</div>
                    </div>
                </div>
                <button onclick={cancelImageAutomationRecipe} disabled={$automationCancelling} class="h-8 px-3 rounded bg-error-soft text-error border border-error-strong text-sm hover:bg-error-soft disabled:opacity-50">
                    {$automationCancelling ? 'Cancelling…' : 'Cancel'}
                </button>
            </div>
            <div class="mt-3 h-1.5 bg-bg rounded overflow-hidden">
                <div class="h-full bg-accent transition-all" style="width: {Math.max(8, lastProgress?.progress ?? 35)}%"></div>
            </div>
            <p class="text-xs text-muted mt-2">You can switch tools. This automation stays visible when you return.</p>
        </div>
    {/if}

    {#if $automationView === 'hub'}
        <div class="grid grid-cols-1 lg:grid-cols-[1fr_320px] gap-3">
            <main class="space-y-3">
                <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
                    {#each builtInAutomationRecipes as recipe}
                        {@const Icon = iconFor(recipe.id)}
                        <button
                                onclick={() => selectBuiltInRecipe(recipe.id)}
                                disabled={$automationProcessing}
                                class="text-left bg-panel border rounded p-3 hover:bg-panel-2 transition-colors disabled:opacity-50 {recipe.id === $selectedAutomationRecipe ? 'border-accent/70' : 'border-border'}"
                        >
                            <div class="flex items-start justify-between gap-3">
                                <div class="h-9 w-9 rounded bg-accent-soft flex items-center justify-center shrink-0">
                                    <Icon class="w-4 h-4 text-accent" />
                                </div>
                                <span class="text-[11px] px-2 py-0.5 rounded bg-panel-2 border border-border text-muted">{recipe.badge}</span>
                            </div>
                            <div class="mt-3 text-sm font-semibold">{recipe.title}</div>
                            <div class="mt-1 text-xs text-muted leading-relaxed">{recipe.short}</div>
                            <div class="mt-3 flex items-center justify-between gap-2 text-xs">
                                <span class="text-muted truncate" title={recipe.safeLine}>{recipe.safeLine}</span>
                                <span class="text-accent flex items-center gap-1 shrink-0">Set up <ArrowRight class="w-3 h-3" /></span>
                            </div>
                        </button>
                    {/each}
                </div>
            </main>

            <aside class="bg-panel border border-border rounded p-3 space-y-3">
                <div>
                    <div class="text-sm font-semibold flex items-center gap-2"><ShieldCheck class="w-4 h-4 text-accent" /> Automation rules</div>
                    <p class="text-xs text-muted mt-1">V1 is manual and safe-by-default.</p>
                </div>
                <div class="space-y-2 text-xs">
                    <div class="flex gap-2"><CheckCircle2 class="w-3.5 h-3.5 text-success shrink-0 mt-0.5" /> Originals are not modified.</div>
                    <div class="flex gap-2"><CheckCircle2 class="w-3.5 h-3.5 text-success shrink-0 mt-0.5" /> Files are processed locally.</div>
                    <div class="flex gap-2"><CheckCircle2 class="w-3.5 h-3.5 text-success shrink-0 mt-0.5" /> One input image creates one final output.</div>
                    <div class="flex gap-2"><AlertTriangle class="w-3.5 h-3.5 text-warning shrink-0 mt-0.5" /> No watch folders or auto-delete in V1.</div>
                </div>

                <div class="border-t border-border pt-3">
                    <div class="flex items-center justify-between gap-2 mb-2">
                        <div class="text-sm font-medium">Saved recipes</div>
                        <button onclick={() => (showSaved = !showSaved)} class="text-xs text-muted hover:text-text">{showSaved ? 'Hide' : 'Show'}</button>
                    </div>
                    {#if showSaved}
                        {#if $savedAutomationRecipes.length}
                            <div class="max-h-64 overflow-auto space-y-1.5">
                                {#each $savedAutomationRecipes as recipe}
                                    <div class="bg-panel-2 border border-border rounded p-2">
                                        <div class="flex items-center justify-between gap-2">
                                            <button onclick={() => loadAutomationRecipe(recipe)} class="text-sm font-medium truncate hover:text-accent" title={recipe.name}>{recipe.name}</button>
                                            <button onclick={() => deleteAutomationRecipe(recipe.id)} class="text-muted hover:text-error"><Trash2 class="w-3.5 h-3.5" /></button>
                                        </div>
                                        <div class="text-[11px] text-muted mt-0.5">{timeLabel(recipe.created_at)}</div>
                                    </div>
                                {/each}
                            </div>
                        {:else}
                            <div class="text-xs text-muted p-3 border border-dashed border-border rounded text-center">No saved recipes yet.</div>
                        {/if}
                    {/if}
                </div>
            </aside>
        </div>
    {:else if $automationView === 'setup'}
        <div class="grid grid-cols-1 xl:grid-cols-[1fr_360px] gap-3">
            <main class="space-y-3">
                <section class="grid grid-cols-1 lg:grid-cols-[1fr_1fr] gap-3">
                    <div class="bg-panel border border-border rounded p-3 space-y-3">
                        <div class="flex items-center justify-between gap-2">
                            <div>
                                <div class="text-sm font-semibold flex items-center gap-2"><FileImage class="w-4 h-4 text-accent" /> 1. Choose images</div>
                                <div class="text-xs text-muted">Drop images or choose files.</div>
                            </div>
                            {#if $automationFiles.length && !$automationProcessing}
                                <button onclick={clearAutomationFiles} class="text-xs text-muted hover:text-text">Clear</button>
                            {/if}
                        </div>

                        <DropZone onFiles={addAutomationFiles} accept={imageExts}>
                            {#snippet children()}
                                <div class="border border-border rounded bg-panel-2 overflow-hidden">
                                    {#if $automationFiles.length === 0}
                                        <button onclick={pickImages} class="w-full p-6 text-center text-muted hover:text-text hover:bg-bg/50 transition-colors">
                                            <FileImage class="w-7 h-7 mx-auto mb-2 opacity-50" />
                                            <div class="text-sm font-medium">Drop images or choose files</div>
                                            <div class="text-xs mt-1">Manual recipes only in V1.</div>
                                        </button>
                                    {:else}
                                        <div class="max-h-56 overflow-auto divide-y divide-border">
                                            {#each $automationFiles as path}
                                                <div class="px-3 py-2 text-sm flex items-center justify-between gap-3 hover:bg-bg/50">
                                                    <span class="truncate" title={path}>{fileName(path)}</span>
                                                    {#if !$automationProcessing}
                                                        <button onclick={() => removeAutomationFile(path)} class="text-muted hover:text-error"><X class="w-3.5 h-3.5" /></button>
                                                    {/if}
                                                </div>
                                            {/each}
                                        </div>
                                        {#if !$automationProcessing}
                                            <button onclick={pickImages} class="w-full p-2 text-xs text-muted hover:text-text border-t border-border">+ Add images</button>
                                        {/if}
                                    {/if}
                                </div>
                            {/snippet}
                        </DropZone>
                    </div>

                    <div class="bg-panel border border-border rounded p-3 space-y-3">
                        <div>
                            <div class="text-sm font-semibold">2. Recipe</div>
                            <div class="text-xs text-muted">Choose a goal, then review the plan.</div>
                        </div>

                        <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
                            {#each builtInAutomationRecipes as recipe}
                                <button
                                        onclick={() => applyRecipeDefaults(recipe.id)}
                                        disabled={$automationProcessing}
                                        class="text-left rounded border px-2.5 py-2 transition-colors {recipe.id === $selectedAutomationRecipe ? 'border-accent bg-accent-soft' : 'border-border bg-panel-2 hover:bg-bg/50'} disabled:opacity-50"
                                >
                                    <div class="text-sm font-medium truncate" title={recipe.title}>{recipe.title}</div>
                                    <div class="text-[11px] text-muted truncate" title={recipe.short}>{recipe.short}</div>
                                </button>
                            {/each}
                        </div>

                        <label class="block text-xs text-muted space-y-1">
                            <span>Recipe name</span>
                            <input bind:value={$automationRecipeName} disabled={$automationProcessing} class="w-full h-8 bg-panel-2 border border-border rounded px-2 text-sm text-text disabled:opacity-50" />
                        </label>

                        <div class="rounded border border-border bg-panel-2 p-2.5 space-y-2">
                            <div class="flex items-center justify-between gap-2">
                                <div class="text-xs text-muted">Output folder</div>
                                <button onclick={pickOutputDir} disabled={$automationProcessing} class="h-7 px-2 rounded bg-bg border border-border text-xs hover:bg-panel disabled:opacity-50 flex items-center gap-1">
                                    <FolderOpen class="w-3.5 h-3.5" /> Choose
                                </button>
                            </div>
                            <div class="text-xs truncate" title={$automationOutputDir ?? 'Beside each source image'}>{$automationOutputDir ?? 'Beside each source image'}</div>
                        </div>
                    </div>
                </section>

                <section class="bg-panel border border-border rounded p-3 space-y-3">
                    <button class="w-full flex items-center justify-between gap-2 text-left" onclick={() => (showAdvanced = !showAdvanced)}>
                        <div class="flex items-center gap-2">
                            <Settings2 class="w-4 h-4 text-accent" />
                            <div>
                                <div class="text-sm font-semibold">3. Actions</div>
                                <div class="text-xs text-muted">{recipeActionSummary()}</div>
                            </div>
                        </div>
                        <ChevronDown class="w-4 h-4 text-muted transition-transform {showAdvanced ? 'rotate-180' : ''}" />
                    </button>

                    {#if showAdvanced}
                        <div class="grid grid-cols-1 lg:grid-cols-4 gap-2 pt-1">
                            <div class="rounded border border-border bg-panel-2 p-3 space-y-2">
                                <label class="flex items-center gap-2 text-sm font-medium">
                                    <input type="checkbox" bind:checked={$automationStripMetadataEnabled} disabled={$automationProcessing} />
                                    Clean metadata
                                </label>
                                <p class="text-xs text-muted">Creates re-encoded copies without common image metadata.</p>
                            </div>

                            <div class="rounded border border-border bg-panel-2 p-3 space-y-2">
                                <label class="flex items-center gap-2 text-sm font-medium">
                                    <input type="checkbox" bind:checked={$automationResizeEnabled} disabled={$automationProcessing} />
                                    Resize
                                </label>
                                <label class="block text-xs text-muted space-y-1">
                                    <span>Longest side: {$automationResizeLongestSide}px</span>
                                    <input type="range" bind:value={$automationResizeLongestSide} min="320" max="4096" step="80" disabled={!$automationResizeEnabled || $automationProcessing} class="w-full" />
                                </label>
                            </div>

                            <div class="rounded border border-border bg-panel-2 p-3 space-y-2">
                                <label class="flex items-center gap-2 text-sm font-medium">
                                    <input type="checkbox" bind:checked={$automationConvertEnabled} disabled={$automationProcessing} />
                                    Convert
                                </label>
                                <div class="grid grid-cols-[1fr_1fr] gap-2">
                                    <label class="block text-xs text-muted space-y-1">
                                        <span>Format</span>
                                        <select bind:value={$automationConvertFormat} disabled={!$automationConvertEnabled || $automationProcessing} class="w-full h-8 bg-bg border border-border rounded px-2 text-sm text-text">
                                            {#each formats as format}
                                                <option value={format.id}>{format.label}</option>
                                            {/each}
                                        </select>
                                    </label>
                                    <label class="block text-xs text-muted space-y-1">
                                        <span>Quality {$automationImageQuality}</span>
                                        <input type="range" bind:value={$automationImageQuality} min="1" max="100" disabled={!$automationConvertEnabled || $automationProcessing} class="w-full h-8" />
                                    </label>
                                </div>
                            </div>

                            <div class="rounded border border-border bg-panel-2 p-3 space-y-2">
                                <label class="flex items-center gap-2 text-sm font-medium">
                                    <input type="checkbox" bind:checked={$automationCompressEnabled} disabled={$automationProcessing} />
                                    Compress
                                </label>
                                <label class="block text-xs text-muted space-y-1">
                                    <span>Quality {$automationCompressQuality}</span>
                                    <input type="range" bind:value={$automationCompressQuality} min="1" max="100" disabled={!$automationCompressEnabled || $automationProcessing} class="w-full" />
                                </label>
                            </div>
                        </div>
                    {/if}
                </section>
            </main>

            <aside class="space-y-3">
                <section class="bg-panel border border-border rounded p-3 space-y-3">
                    <div>
                        <div class="text-sm font-semibold flex items-center gap-2"><CheckCircle2 class="w-4 h-4 text-accent" /> Review before running</div>
                        <div class="text-xs text-muted">No assumptions. This is exactly what KeepItLocal will do.</div>
                    </div>

                    <div class="space-y-2">
                        <div class="text-xs font-medium text-muted">KeepItLocal will</div>
                        {#each $automationPlainSummary.will as line}
                            <div class="flex gap-2 text-xs"><CheckCircle2 class="w-3.5 h-3.5 text-success shrink-0 mt-0.5" /><span>{line}</span></div>
                        {/each}
                    </div>

                    <div class="space-y-2 border-t border-border pt-3">
                        <div class="text-xs font-medium text-muted">KeepItLocal will not</div>
                        {#each $automationPlainSummary.wont as line}
                            <div class="flex gap-2 text-xs"><AlertTriangle class="w-3.5 h-3.5 text-warning shrink-0 mt-0.5" /><span>{line}</span></div>
                        {/each}
                    </div>

                    <div class="grid grid-cols-2 gap-2 pt-1">
                        <button onclick={saveCurrentAutomationRecipe} disabled={$automationProcessing || $enabledAutomationSteps.length === 0} class="h-8 px-3 rounded bg-panel-2 border border-border text-sm hover:bg-bg disabled:opacity-50 flex items-center justify-center gap-1.5">
                            <Save class="w-3.5 h-3.5" /> Save
                        </button>
                        {#if $automationProcessing}
                            <button onclick={cancelImageAutomationRecipe} disabled={$automationCancelling} class="h-8 px-3 rounded bg-error-soft text-error border border-error-strong text-sm disabled:opacity-50">
                                {$automationCancelling ? 'Cancelling…' : 'Cancel'}
                            </button>
                        {:else}
                            <button onclick={() => { enqueueCurrentAutomationRecipe(); automationView.set('jobs'); }} disabled={$automationFiles.length === 0 || $enabledAutomationSteps.length === 0} class="h-8 px-3 rounded bg-accent text-accent-contrast text-sm font-medium hover:opacity-90 disabled:opacity-50 flex items-center justify-center gap-1.5">
                                <Play class="w-3.5 h-3.5" /> Queue job
                            </button>
                        {/if}
                    </div>
                </section>

                <section class="bg-panel border border-border rounded p-3 space-y-2">
                    <div class="flex items-center justify-between gap-2">
                        <div class="text-sm font-semibold">Results</div>
                        {#if $automationFinalResults.length && !$automationProcessing}
                            <button onclick={clearAutomationRun} class="text-xs text-muted hover:text-text flex items-center gap-1"><RotateCcw class="w-3 h-3" /> Clear</button>
                        {/if}
                    </div>
                    {#if $automationFinalResults.length}
                        <div class="grid grid-cols-3 gap-2 text-xs">
                            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Done</div><div class="font-semibold text-success">{$automationStats.success}</div></div>
                            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Failed</div><div class="font-semibold text-error">{$automationStats.failed}</div></div>
                            <div class="bg-panel-2 border border-border rounded p-2"><div class="text-muted">Cancelled</div><div class="font-semibold text-warning">{$automationStats.cancelled}</div></div>
                        </div>
                        <div class="max-h-64 overflow-auto space-y-1.5 pt-1">
                            {#each $automationFinalResults as result}
                                <div class="bg-panel-2 border border-border rounded p-2 text-xs">
                                    <div class="flex items-center justify-between gap-2">
                                        <span class="truncate font-medium" title={result.source_path}>{fileName(result.source_path)}</span>
                                        <span class={result.success ? 'text-success' : 'text-error'}>{result.success ? 'Done' : 'Failed'}</span>
                                    </div>
                                    <div class="text-muted truncate mt-0.5" title={result.success ? result.output_path : result.error}>{result.success ? `${fmtBytes(result.original_size)} → ${fmtBytes(result.new_size)} · ${result.output_path}` : result.error}</div>
                                </div>
                            {/each}
                        </div>
                    {:else if $automationLastError}
                        <div class="text-xs text-error bg-error-soft border border-error-strong rounded p-3">{$automationLastError}</div>
                    {:else}
                        <div class="text-xs text-muted p-4 border border-dashed border-border rounded text-center">Results will appear here after running.</div>
                    {/if}
                </section>
            </aside>
        </div>
    {:else if $automationView === 'jobs'}
        <AutomationJobs />
    {:else}
        <div class="bg-panel border border-border rounded p-3 space-y-3">
            <div class="flex flex-wrap items-center justify-between gap-3">
                <div>
                    <div class="text-sm font-semibold flex items-center gap-2"><History class="w-4 h-4 text-accent" /> Activity log</div>
                    <div class="text-xs text-muted">Local summary of automation runs.</div>
                </div>
                {#if $automationActivity.length}
                    <button onclick={clearAutomationActivity} class="h-8 px-3 rounded bg-panel-2 border border-border text-sm hover:bg-bg flex items-center gap-1.5"><Trash2 class="w-3.5 h-3.5" /> Clear</button>
                {/if}
            </div>

            {#if $automationActivity.length}
                <div class="space-y-2">
                    {#each $automationActivity as item}
                        <div class="bg-panel-2 border border-border rounded p-3">
                            <div class="flex flex-wrap items-center justify-between gap-2">
                                <div class="flex items-center gap-2 min-w-0">
                                    {#if item.level === 'success'}
                                        <CheckCircle2 class="w-4 h-4 text-success shrink-0" />
                                    {:else if item.level === 'error'}
                                        <AlertTriangle class="w-4 h-4 text-error shrink-0" />
                                    {:else}
                                        <Clock3 class="w-4 h-4 text-warning shrink-0" />
                                    {/if}
                                    <div class="min-w-0">
                                        <div class="text-sm font-medium truncate" title={item.recipe_name}>{item.recipe_name}</div>
                                        <div class="text-xs text-muted truncate" title={item.message}>{item.message}</div>
                                    </div>
                                </div>
                                <div class="text-xs text-muted text-right">
                                    <div>{timeLabel(item.finished_at)}</div>
                                    <div>{duration(item.started_at, item.finished_at)}</div>
                                </div>
                            </div>
                            <div class="mt-2 grid grid-cols-2 md:grid-cols-4 gap-2 text-xs">
                                <div><span class="text-muted">Input:</span> {item.input_count}</div>
                                <div><span class="text-muted">Done:</span> <span class="text-success">{item.success_count}</span></div>
                                <div><span class="text-muted">Failed:</span> <span class={resultTone(item.level)}>{item.failed_count}</span></div>
                                <div class="truncate" title={item.output_dir ?? 'beside source'}><span class="text-muted">Output:</span> {item.output_dir ?? 'beside source'}</div>
                            </div>
                        </div>
                    {/each}
                </div>
            {:else}
                <div class="text-sm text-muted p-8 border border-dashed border-border rounded text-center">No automation runs yet.</div>
            {/if}
        </div>
    {/if}
</section>
