<script lang="ts">
    /*
      Screenshot Redact — refreshed UX.

      The core drawing logic (mouse-driven rectangle capture, mode-specific
      visual preview, backend export via redact_image) is preserved from
      the previous version. Visual chrome is rebuilt to match the modern
      hero/toolbar pattern used by ClipboardHistory + Snippets.

      New in this revision:
        - Hero + Privacy badge so the screen reads as a deliberate Privacy
          Pack tool rather than a side utility
        - Undo / redo stack — every region add / remove pushes a snapshot;
          Ctrl+Z / Ctrl+Shift+Z navigate
        - Shared EmptyState for the "no image yet" case
        - Keyboard shortcuts: Del / Backspace removes most-recent region,
          Esc cancels in-progress draw, Ctrl+Z / Ctrl+Y for undo/redo
        - Region sidebar listing every drawn region with mode badge + delete
        - Activity log entry on successful export
    */
    import { onMount } from 'svelte';
    import { get } from 'svelte/store';
    import { invoke, convertFileSrc } from '@tauri-apps/api/core';
    import { open, save } from '@tauri-apps/plugin-dialog';
    import {
        ImageOff,
        Undo2,
        Redo2,
        Eraser,
        Trash2,
        Save,
        ImageIcon,
        Square,
        EyeOff,
        Layers,
        FolderOpen,
        ShieldAlert,
    } from '@lucide/svelte';
    import DropZone from '$lib/DropZone.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import LoadingState from '$lib/components/LoadingState.svelte';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import { recordActivity } from '$lib/stores/activityLog';
    import {
        redactSourcePath,
        redactImageUrl,
        redactRegions,
        redactNextId,
        redactUndoStack,
        redactRedoStack,
        redactMode,
        redactStrength,
        redactLastSaved,
        redactSaving,
        redactError,
        type Mode,
        type Region,
    } from '$lib/stores/screenshotRedact';
    import { ToolPage } from '$lib/ui';
    import { subscribeToToolLaunchTarget } from '$lib/stores/toolLaunchTarget';

    // Persisted editor state — hydrate from the store on mount, mirror back
    // via $effect, so a loaded image + drawn regions + undo/redo history
    // survive navigation (the app remounts tools via {#key}, wiping local
    // $state). See screenshotRedact.ts for what stays transient.
    let sourcePath = $state<string | null>(get(redactSourcePath));
    $effect(() => { redactSourcePath.set(sourcePath); });
    let imageUrl = $state<string | null>(get(redactImageUrl));
    $effect(() => { redactImageUrl.set(imageUrl); });
    // Recomputed from the <img> onload when the persisted URL reloads.
    let imageNaturalWidth = $state(0);
    let imageNaturalHeight = $state(0);

    let regions = $state<Region[]>(get(redactRegions));
    $effect(() => { redactRegions.set(regions); });
    // Mirror the id counter through the store so ids stay unique across a
    // remount. Read live via get(redactNextId) at allocation time.
    function allocRegionId(): number {
        const id = get(redactNextId);
        redactNextId.set(id + 1);
        return id;
    }

    /** Undo/redo stack — snapshots of the regions array at each
     * mutation point. Past stack holds states we can undo INTO; future
     * stack holds states we can redo INTO. New mutations clear the
     * future stack. Capped at 50 entries to bound memory. */
    let undoStack = $state<Region[][]>(get(redactUndoStack));
    $effect(() => { redactUndoStack.set(undoStack); });
    let redoStack = $state<Region[][]>(get(redactRedoStack));
    $effect(() => { redactRedoStack.set(redoStack); });
    const UNDO_LIMIT = 50;

    let mode = $state<Mode>(get(redactMode));
    $effect(() => { redactMode.set(mode); });
    let strength = $state(get(redactStrength));
    $effect(() => { redactStrength.set(strength); });

    // Drawing state
    let drawing = $state(false);
    let drawStart = $state<{ x: number; y: number } | null>(null);
    let currentRect = $state<{ x: number; y: number; w: number; h: number } | null>(null);

    let imgEl = $state<HTMLImageElement | null>(null);
    let containerEl = $state<HTMLDivElement | null>(null);

    let saving = $derived($redactSaving);
    let lastSaved = $derived($redactLastSaved);
    let error = $derived($redactError);

    /** Visual config per mode — used in the toolbar mode buttons + the
     * region list to give each mode an instantly-recognizable color. */
    const MODE_META: Record<Mode, { label: string; description: string; icon: any }> = {
        black: { label: 'Black box', description: 'Hardest redaction. Sensitive info disappears completely.', icon: Square },
        white: { label: 'White box', description: 'Clean look on light-themed screenshots.', icon: Square },
        blur: { label: 'Blur', description: 'Softer redaction — content vaguely visible. Use for less-sensitive cases.', icon: EyeOff },
        pixelate: { label: 'Pixelate', description: 'Mosaic effect — recognizable shape, illegible content.', icon: Layers },
    };

    async function pickFile() {
        const selected = await open({
            multiple: false,
            directory: false,
            filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp'] }],
        });
        if (typeof selected === 'string') loadImage(selected);
    }

    function handleDrop(paths: string[]) {
        if (paths.length > 0) loadImage(paths[0]);
    }

    function loadImage(path: string) {
        sourcePath = path;
        imageUrl = convertFileSrc(path);
        regions = [];
        undoStack = [];
        redoStack = [];
        redactLastSaved.set(null);
        redactError.set(null);
    }

    function unloadImage() {
        imageUrl = null;
        sourcePath = null;
        regions = [];
        undoStack = [];
        redoStack = [];
        redactLastSaved.set(null);
    }

    function onImageLoad() {
        if (!imgEl) return;
        imageNaturalWidth = imgEl.naturalWidth;
        imageNaturalHeight = imgEl.naturalHeight;
    }

    /** Convert mouse coords (relative to displayed image) to natural
     * image coords. We always work in natural pixel space internally so
     * regions stay correct regardless of the on-screen image size. */
    function mouseToImage(e: MouseEvent): { x: number; y: number } {
        if (!imgEl) return { x: 0, y: 0 };
        const rect = imgEl.getBoundingClientRect();
        const scaleX = imageNaturalWidth / rect.width;
        const scaleY = imageNaturalHeight / rect.height;
        return {
            x: Math.max(0, Math.min(imageNaturalWidth, (e.clientX - rect.left) * scaleX)),
            y: Math.max(0, Math.min(imageNaturalHeight, (e.clientY - rect.top) * scaleY)),
        };
    }

    function startDraw(e: MouseEvent) {
        if (!imageUrl) return;
        e.preventDefault();
        const p = mouseToImage(e);
        drawStart = p;
        currentRect = { x: p.x, y: p.y, w: 0, h: 0 };
        drawing = true;
    }

    function moveDraw(e: MouseEvent) {
        if (!drawing || !drawStart) return;
        const p = mouseToImage(e);
        currentRect = {
            x: Math.min(drawStart.x, p.x),
            y: Math.min(drawStart.y, p.y),
            w: Math.abs(p.x - drawStart.x),
            h: Math.abs(p.y - drawStart.y),
        };
    }

    function endDraw() {
        if (!drawing || !currentRect) {
            cancelDraw();
            return;
        }
        if (currentRect.w >= 5 && currentRect.h >= 5) {
            pushUndoSnapshot();
            regions = [
                ...regions,
                {
                    id: allocRegionId(),
                    x: Math.round(currentRect.x),
                    y: Math.round(currentRect.y),
                    width: Math.round(currentRect.w),
                    height: Math.round(currentRect.h),
                    mode,
                    strength,
                },
            ];
        }
        drawing = false;
        currentRect = null;
        drawStart = null;
    }

    function cancelDraw() {
        drawing = false;
        currentRect = null;
        drawStart = null;
    }

    function removeRegion(id: number) {
        pushUndoSnapshot();
        regions = regions.filter((r) => r.id !== id);
    }

    function clearRegions() {
        if (regions.length === 0) return;
        pushUndoSnapshot();
        regions = [];
    }

    /** Snapshot the current regions array onto the undo stack and clear
     * the redo stack. Called BEFORE each mutation so the snapshot
     * represents the state we're transitioning AWAY from. */
    function pushUndoSnapshot() {
        undoStack = [...undoStack, regions.map((r) => ({ ...r }))];
        if (undoStack.length > UNDO_LIMIT) {
            undoStack = undoStack.slice(undoStack.length - UNDO_LIMIT);
        }
        redoStack = [];
    }

    function undo() {
        if (undoStack.length === 0) return;
        const previous = undoStack[undoStack.length - 1];
        redoStack = [...redoStack, regions.map((r) => ({ ...r }))];
        regions = previous;
        undoStack = undoStack.slice(0, undoStack.length - 1);
    }

    function redo() {
        if (redoStack.length === 0) return;
        const next = redoStack[redoStack.length - 1];
        undoStack = [...undoStack, regions.map((r) => ({ ...r }))];
        regions = next;
        redoStack = redoStack.slice(0, redoStack.length - 1);
    }

    /** Keyboard shortcuts — only fire when the user isn't typing in
     * an input/textarea, since Del/Backspace would otherwise eat
     * characters in unrelated fields. */
    function onKeydown(event: KeyboardEvent) {
        const target = event.target as HTMLElement | null;
        if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)) {
            return;
        }
        if (!imageUrl) return;
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'z' && !event.shiftKey) {
            event.preventDefault();
            undo();
            return;
        }
        if ((event.ctrlKey || event.metaKey) && (event.key.toLowerCase() === 'y' || (event.shiftKey && event.key.toLowerCase() === 'z'))) {
            event.preventDefault();
            redo();
            return;
        }
        if (event.key === 'Escape' && drawing) {
            event.preventDefault();
            cancelDraw();
            return;
        }
        if ((event.key === 'Delete' || event.key === 'Backspace') && regions.length > 0) {
            event.preventDefault();
            removeRegion(regions[regions.length - 1].id);
        }
    }

    onMount(() => {
        const stopTarget = subscribeToToolLaunchTarget('screenshot-redact', ({ targetFile }) => {
            loadImage(targetFile);
        });
        document.addEventListener('keydown', onKeydown);
        return () => {
            stopTarget();
            document.removeEventListener('keydown', onKeydown);
        };
    });
    async function exportImage() {
        if (!sourcePath || regions.length === 0 || get(redactSaving)) return;

        const ext = sourcePath.split('.').pop()?.toLowerCase() || 'png';
        const baseName = sourcePath.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, '') || 'image';

        const outPath = await save({
            defaultPath: `${baseName}_redacted.${ext}`,
            filters: [
                { name: 'PNG', extensions: ['png'] },
                { name: 'JPEG', extensions: ['jpg', 'jpeg'] },
                { name: 'WebP', extensions: ['webp'] },
            ],
        });
        if (!outPath) return;

        redactSaving.set(true);
        redactError.set(null);
        try {
            const result = await invoke<{ output_path: string; width: number; height: number; regions_applied: number }>(
                'redact_image',
                {
                    input: {
                        source_path: sourcePath,
                        output_path: outPath,
                        regions: regions.map((r) => ({
                            x: r.x,
                            y: r.y,
                            width: r.width,
                            height: r.height,
                            mode: r.mode,
                            strength: r.strength,
                        })),
                        strip_metadata: true,
                    },
                },
            );
            redactLastSaved.set(result.output_path);
            toast('Image redacted and saved', 'success');
            void recordActivity({
                toolId: 'screenshot-redact',
                summary: `Redacted screenshot · ${result.regions_applied} region${result.regions_applied === 1 ? '' : 's'}`,
                details: `Source: ${baseName}.${ext} · Output: ${outPath}`,
                outcome: 'success',
            });
        } catch (e) {
            redactError.set(String(e));
            // UxAudit UX-F-01: was `toast('Save failed', 'error')` — two
            // words for a destructive failure (the user's redacted image
            // is potentially lost). Now: situation + recovery path.
            errorToast("Couldn't save the redacted image", e, {
                hint: 'Try a different folder — the destination may be read-only or full.',
                durationMs: 7000,
            });
            void recordActivity({
                toolId: 'screenshot-redact',
                summary: 'Screenshot redact failed',
                details: String(e),
                outcome: 'failed',
            });
        } finally {
            redactSaving.set(false);
        }
    }

    function fileName(path: string): string {
        return path.split(/[\\/]/).pop() || path;
    }

    /** Style helper for region preview overlay. Computes display
     * coordinates from the natural-pixel region values. */
    function regionStyle(r: { x: number; y: number; width: number; height: number; mode: string; strength?: number }) {
        if (!imgEl) return '';
        const rect = imgEl.getBoundingClientRect();
        const scaleX = rect.width / imageNaturalWidth;
        const scaleY = rect.height / imageNaturalHeight;

        let bg = 'rgba(0,0,0,0.85)';
        let extra = '';
        if (r.mode === 'black') bg = 'rgba(0,0,0,0.95)';
        else if (r.mode === 'white') bg = 'rgba(255,255,255,0.95)';
        else if (r.mode === 'blur') {
            bg = 'transparent';
            extra = `backdrop-filter: blur(${(r.strength || 15) / 2}px); -webkit-backdrop-filter: blur(${(r.strength || 15) / 2}px);`;
        } else if (r.mode === 'pixelate') {
            bg = 'rgba(50,50,50,0.6)';
        }

        return `
            left: ${r.x * scaleX}px;
            top: ${r.y * scaleY}px;
            width: ${r.width * scaleX}px;
            height: ${r.height * scaleY}px;
            background: ${bg};
            ${extra}
        `;
    }
</script>

<ToolPage
    icon={ShieldAlert}
    iconTint="#ef4444"
    title="Black out sensitive parts of any screenshot"
    description="Drag rectangles over names, account numbers, tokens — anything you don't want shared. The exported file has the regions actually replaced, not just covered. Originals are never modified."
    width="wide"
    fill={false}
>
    {#if !imageUrl}
        <DropZone onFiles={handleDrop} accept={['png', 'jpg', 'jpeg', 'webp']}>
            {#snippet children()}
                <EmptyState
                    icon={ImageIcon}
                    title="Drop a screenshot to redact"
                    description="PNG, JPEG, or WebP. Drag and drop, or click below to browse."
                    variant="dashed"
                >
                    {#snippet actions()}
                        <button
                            type="button"
                            onclick={pickFile}
                            class="redact-empty-action"
                        >
                            <FolderOpen class="h-4 w-4" />
                            Browse files
                        </button>
                    {/snippet}
                </EmptyState>
            {/snippet}
        </DropZone>
    {:else}
        <!-- Toolbar -->
        <div class="redact-toolbar">
            <!-- Mode picker -->
            <div class="redact-mode-picker" role="group" aria-label="Redaction mode">
                {#each Object.entries(MODE_META) as [m, meta]}
                    {@const Icon = meta.icon}
                    {@const active = mode === m}
                    <button
                        type="button"
                        onclick={() => (mode = m as Mode)}
                        title={meta.description}
                        class="redact-mode inline-flex h-9 items-center gap-1.5 rounded-lg border px-3 text-xs font-medium transition-colors
                            {active
                            ? 'border-accent bg-accent text-accent-contrast'
                            : 'border-border bg-panel-2 text-text hover:border-accent/60'}"
                        aria-pressed={active}
                    >
                        <Icon class="h-3.5 w-3.5" />
                        {meta.label}
                    </button>
                {/each}
            </div>

            <!-- Strength slider — only for blur/pixelate -->
            {#if mode === 'blur' || mode === 'pixelate'}
                <div class="redact-strength">
                    <span class="text-[10px] uppercase tracking-wider text-muted">Strength</span>
                    <input
                        type="range"
                        min={mode === 'blur' ? 2 : 4}
                        max={mode === 'blur' ? 50 : 60}
                        bind:value={strength}
                        class="redact-strength-range w-24"
                    />
                    <span class="text-xs text-text font-mono w-6 text-right">{strength}</span>
                </div>
            {/if}

            <div class="redact-toolbar-spacer"></div>

            <!-- Undo/redo -->
            <div class="redact-history-actions">
                <button
                    type="button"
                    onclick={undo}
                    disabled={undoStack.length === 0}
                    title="Undo (Ctrl+Z)"
                    aria-label="Undo"
                    class="redact-icon-button"
                >
                    <Undo2 class="h-4 w-4" />
                </button>
                <button
                    type="button"
                    onclick={redo}
                    disabled={redoStack.length === 0}
                    title="Redo (Ctrl+Y)"
                    aria-label="Redo"
                    class="redact-icon-button"
                >
                    <Redo2 class="h-4 w-4" />
                </button>
            </div>

            <!-- Clear all -->
            <button
                type="button"
                onclick={clearRegions}
                disabled={regions.length === 0}
                class="redact-secondary-button is-danger"
            >
                <Eraser class="h-3.5 w-3.5" />
                Clear all
            </button>

            <!-- Load different -->
            <button
                type="button"
                onclick={unloadImage}
                class="redact-secondary-button"
            >
                <ImageOff class="h-3.5 w-3.5" />
                Replace image
            </button>

            <!-- Export -->
            <button
                type="button"
                onclick={exportImage}
                disabled={regions.length === 0 || saving}
                class="redact-primary-button"
            >
                {#if saving}
                    <LoadingState variant="inline" label="Saving…" />
                {:else}
                    <Save class="h-4 w-4" />
                    Export redacted
                {/if}
            </button>
        </div>

        <!-- Canvas + region list -->
        <div class="redact-workspace">
            <div
                bind:this={containerEl}
                class="redact-canvas relative overflow-auto"
                style="max-height: 70vh;"
            >
                <div class="redact-image-stage relative inline-block" style="user-select: none;">
                    <img
                        bind:this={imgEl}
                        src={imageUrl}
                        alt="Source"
                        onload={onImageLoad}
                        class="redact-source-image block max-w-full"
                        style="user-drag: none; -webkit-user-drag: none; pointer-events: none;"
                    />

                    <!-- Overlay layer for drawing + existing regions -->
                    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                    <!-- Pointer-driven draw surface for redaction regions; role="application"
                         signals the custom mouse-only interaction model to assistive tech. -->
                    <div
                        class="redact-draw-surface absolute inset-0 cursor-crosshair"
                        onmousedown={startDraw}
                        onmousemove={moveDraw}
                        onmouseup={endDraw}
                        onmouseleave={endDraw}
                        role="application"
                    >
                        {#each regions as r (r.id)}
                            <div
                                class="redact-region absolute pointer-events-auto group rounded-sm"
                                style={regionStyle(r)}
                            >
                                <button
                                    onclick={(e) => {
                                        e.stopPropagation();
                                        removeRegion(r.id);
                                    }}
                                    class="redact-region-delete absolute -top-2 -right-2 inline-flex h-5 w-5 items-center justify-center rounded-full bg-error text-white text-[10px] opacity-0 group-hover:opacity-100 transition-opacity shadow-md"
                                    title="Remove this region"
                                    aria-label="Remove region"
                                >
                                    <Trash2 class="h-3 w-3" />
                                </button>
                            </div>
                        {/each}

                        <!-- In-progress draw rect -->
                        {#if currentRect && imgEl}
                            <div
                                class="redact-current-region absolute border-2 border-accent border-dashed bg-accent/20 pointer-events-none rounded-sm"
                                style="
                                    left: {currentRect.x * (imgEl.getBoundingClientRect().width / imageNaturalWidth)}px;
                                    top: {currentRect.y * (imgEl.getBoundingClientRect().height / imageNaturalHeight)}px;
                                    width: {currentRect.w * (imgEl.getBoundingClientRect().width / imageNaturalWidth)}px;
                                    height: {currentRect.h * (imgEl.getBoundingClientRect().height / imageNaturalHeight)}px;
                                "
                            ></div>
                        {/if}
                    </div>
                </div>
            </div>

            <!-- Region sidebar -->
            <aside class="redact-regions">
                <div class="redact-regions-head flex items-center justify-between">
                    <h3 class="text-xs font-semibold uppercase tracking-wider text-muted">
                        Regions ({regions.length})
                    </h3>
                    {#if regions.length > 0}
                        <span class="text-[10px] text-muted">Hover to highlight</span>
                    {/if}
                </div>
                {#if regions.length === 0}
                    <div class="redact-empty-regions text-center text-xs text-muted">
                        Drag on the image to draw your first redaction.
                    </div>
                {:else}
                    <ul class="redact-region-list max-h-[60vh] overflow-y-auto pr-1">
                        {#each regions as r (r.id)}
                            {@const meta = MODE_META[r.mode]}
                            {@const Icon = meta.icon}
                            <li class="redact-region-row flex items-center gap-2 text-xs">
                                <Icon class="h-3 w-3 text-muted shrink-0" />
                                <div class="min-w-0 flex-1">
                                    <div class="font-medium text-text truncate">{meta.label}</div>
                                    <div class="text-[10px] text-muted">
                                        {r.width}×{r.height} px @ ({r.x}, {r.y})
                                    </div>
                                </div>
                                <button
                                    type="button"
                                    onclick={() => removeRegion(r.id)}
                                    class="redact-region-remove"
                                    aria-label="Remove region"
                                >
                                    <Trash2 class="h-3 w-3" />
                                </button>
                            </li>
                        {/each}
                    </ul>
                {/if}

                <!-- Keyboard hints -->
                <div class="redact-shortcuts text-[10px] leading-relaxed text-muted space-y-1">
                    <div><kbd class="rounded border border-border bg-panel px-1 py-0.5 font-mono">Ctrl+Z</kbd> undo · <kbd class="rounded border border-border bg-panel px-1 py-0.5 font-mono">Ctrl+Y</kbd> redo</div>
                    <div><kbd class="rounded border border-border bg-panel px-1 py-0.5 font-mono">Del</kbd> remove last · <kbd class="rounded border border-border bg-panel px-1 py-0.5 font-mono">Esc</kbd> cancel draw</div>
                </div>
            </aside>
        </div>

        <!-- Footer / image meta -->
        <div class="redact-meta text-xs text-muted flex flex-wrap items-center gap-3">
            <span class="text-text font-medium" title={sourcePath || ''}>{fileName(sourcePath || '')}</span>
            <span>·</span>
            <span>{imageNaturalWidth}×{imageNaturalHeight}px</span>
            <span>·</span>
            <span>Click and drag on the image to draw a redaction box.</span>
        </div>

        {#if error}
            <div class="redact-message is-error text-sm">
                {error}
            </div>
        {/if}

        {#if lastSaved}
            <div class="redact-message is-success text-sm">
                <span class="text-success font-medium">Saved:</span>
                <span class="ml-2 font-mono text-xs text-text break-all" title={lastSaved}>{lastSaved}</span>
            </div>
        {/if}
    {/if}
</ToolPage>
<style>
    .redact-empty-action,
    .redact-primary-button,
    .redact-secondary-button,
    .redact-icon-button,
    .redact-mode,
    .redact-region-remove {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 7px;
        min-height: 36px;
        border-radius: var(--radius-control);
        font: inherit;
        line-height: 1;
        transition:
            border-color var(--dur-micro) var(--ease-out),
            background-color var(--dur-micro) var(--ease-out),
            color var(--dur-micro) var(--ease-out),
            opacity var(--dur-micro) var(--ease-out);
    }

    .redact-empty-action,
    .redact-primary-button {
        padding: 0 13px;
        border: 1px solid var(--color-accent);
        background: var(--color-accent);
        color: var(--color-accent-contrast);
        font-size: 13px;
        font-weight: 600;
    }

    .redact-empty-action:hover,
    .redact-primary-button:hover {
        border-color: var(--color-accent-hover);
        background: var(--color-accent-hover);
    }

    .redact-toolbar {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 10px;
        padding: 0 0 12px;
        border-bottom: 1px solid var(--color-border);
    }

    .redact-mode-picker,
    .redact-history-actions {
        display: inline-flex;
        align-items: center;
        gap: 3px;
    }

    .redact-mode {
        position: relative;
        padding: 0 10px;
        border-color: transparent;
        background: transparent;
        color: var(--color-text-secondary);
    }

    .redact-mode:hover {
        border-color: color-mix(in srgb, var(--color-text) 10%, transparent);
        background: color-mix(in srgb, var(--color-panel-2) 72%, transparent);
        color: var(--color-text);
    }

    .redact-mode[aria-pressed='true'] {
        padding-left: 16px;
        border-color: color-mix(in srgb, var(--color-text) 10%, transparent);
        background: var(--color-panel-2);
        color: var(--color-text);
    }

    .redact-mode[aria-pressed='true']::before {
        content: '';
        position: absolute;
        top: 8px;
        bottom: 8px;
        left: 5px;
        width: 2px;
        border-radius: 999px;
        background: var(--color-accent);
    }

    .redact-strength {
        display: inline-flex;
        align-items: center;
        gap: 8px;
        min-height: 34px;
        padding-left: 10px;
        border-left: 1px solid var(--color-border);
    }

    .redact-strength-range {
        accent-color: var(--color-accent);
    }

    .redact-toolbar-spacer {
        flex: 1 1 12px;
        min-width: 12px;
    }

    .redact-history-actions {
        padding-right: 10px;
        border-right: 1px solid var(--color-border);
    }

    .redact-icon-button,
    .redact-secondary-button {
        border: 1px solid color-mix(in srgb, var(--color-text) 10%, transparent);
        background: transparent;
        color: var(--color-text-secondary);
    }

    .redact-icon-button {
        width: 36px;
        padding: 0;
    }

    .redact-secondary-button {
        padding: 0 11px;
        font-size: 12px;
        font-weight: 500;
    }

    .redact-icon-button:hover,
    .redact-secondary-button:hover {
        border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
        color: var(--color-text);
    }

    .redact-secondary-button.is-danger:hover {
        border-color: color-mix(in srgb, var(--color-error) 60%, var(--color-border));
        color: var(--color-error);
    }

    .redact-empty-action:disabled,
    .redact-primary-button:disabled,
    .redact-secondary-button:disabled,
    .redact-icon-button:disabled {
        cursor: not-allowed;
        opacity: 0.45;
    }

    .redact-workspace {
        display: grid;
        grid-template-columns: minmax(0, 1fr) minmax(220px, 280px);
        gap: 24px;
        min-height: 0;
    }

    .redact-canvas {
        min-width: 0;
        min-height: 300px;
        padding: 12px;
        border: 1px solid color-mix(in srgb, var(--color-text) 10%, transparent);
        border-radius: var(--radius-control);
        background: color-mix(in srgb, var(--color-panel) 90%, var(--color-panel-2));
    }

    .redact-image-stage {
        line-height: 0;
    }

    .redact-source-image {
        border-radius: calc(var(--radius-control) - 2px);
    }

    .redact-draw-surface {
        touch-action: none;
    }

    .redact-region {
        outline: 1px dashed color-mix(in srgb, var(--color-text) 46%, transparent);
        outline-offset: -1px;
    }

    .redact-region-delete {
        border: 1px solid color-mix(in srgb, var(--color-text) 18%, transparent);
        background: var(--color-error);
        color: var(--color-accent-contrast);
    }

    .redact-region-delete:focus-visible,
    .redact-region:hover .redact-region-delete {
        opacity: 1;
    }

    .redact-current-region {
        background: color-mix(in srgb, var(--color-accent) 14%, transparent);
    }

    .redact-regions {
        min-width: 0;
        padding-left: 20px;
        border-left: 1px solid var(--color-border);
    }

    .redact-regions-head {
        min-height: 28px;
        margin-bottom: 6px;
    }

    .redact-empty-regions {
        padding: 11px 0 11px 12px;
        border-left: 2px dashed color-mix(in srgb, var(--color-text) 16%, transparent);
        text-align: left;
    }

    .redact-region-list {
        margin: 0;
        padding: 0;
        list-style: none;
    }

    .redact-region-row {
        min-width: 0;
        padding: 9px 0;
        border-bottom: 1px solid color-mix(in srgb, var(--color-text) 7%, transparent);
    }

    .redact-region-row:first-child {
        border-top: 1px solid color-mix(in srgb, var(--color-text) 7%, transparent);
    }

    .redact-region-remove {
        flex: none;
        min-height: 28px;
        width: 28px;
        padding: 0;
        border: 1px solid transparent;
        background: transparent;
        color: var(--color-muted);
    }

    .redact-region-remove:hover {
        border-color: color-mix(in srgb, var(--color-error) 36%, transparent);
        color: var(--color-error);
    }

    .redact-shortcuts {
        margin-top: 14px;
        padding-top: 12px;
        border-top: 1px solid var(--color-border);
    }

    .redact-shortcuts kbd {
        border-color: color-mix(in srgb, var(--color-text) 13%, transparent);
        background: transparent;
        color: var(--color-text-secondary);
    }

    .redact-meta {
        padding-top: 2px;
        color: var(--color-muted);
    }

    .redact-meta::before {
        content: '';
        flex: 0 0 100%;
        height: 1px;
        margin-bottom: 8px;
        background: var(--color-border);
    }

    .redact-message {
        padding: 9px 0 9px 12px;
        border-left: 2px solid currentColor;
    }

    .redact-message.is-error {
        color: var(--color-error);
    }

    .redact-message.is-success {
        color: var(--color-success);
    }

    .redact-mode:focus-visible,
    .redact-empty-action:focus-visible,
    .redact-primary-button:focus-visible,
    .redact-secondary-button:focus-visible,
    .redact-icon-button:focus-visible,
    .redact-region-remove:focus-visible {
        outline: 2px solid color-mix(in srgb, var(--color-accent) 70%, transparent);
        outline-offset: 2px;
    }

    @media (max-width: 900px) {
        .redact-workspace {
            grid-template-columns: minmax(0, 1fr);
        }

        .redact-regions {
            padding-top: 16px;
            padding-left: 0;
            border-top: 1px solid var(--color-border);
            border-left: 0;
        }

        .redact-region-list {
            max-height: none;
        }
    }

    @media (max-width: 640px) {
        .redact-toolbar-spacer {
            flex-basis: 100%;
            height: 0;
        }

        .redact-mode-picker {
            width: 100%;
        }

        .redact-mode {
            flex: 1 1 auto;
        }

        .redact-strength {
            padding-left: 0;
            border-left: 0;
        }

        .redact-canvas {
            padding: 8px;
        }
    }

    @media (prefers-reduced-motion: reduce) {
        .redact-empty-action,
        .redact-primary-button,
        .redact-secondary-button,
        .redact-icon-button,
        .redact-mode,
        .redact-region-remove {
            transition: none;
        }
    }
</style>
