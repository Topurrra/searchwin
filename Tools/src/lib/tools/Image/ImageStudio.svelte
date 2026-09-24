<script lang="ts">
    /*
      Image Studio — the unified image pipeline. One surface that resizes
      (visually in Single mode, or in Batch by pixels/percent/side), converts
      between formats (incl. WebP), and compresses — in a single pass. Absorbs
      the former Image Resizer, Converter, and Compressor.
    */
    import { onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { emitTo } from '@tauri-apps/api/event';
    import { open } from '@tauri-apps/plugin-dialog';
    import { Crop, ImageIcon, Plus, FolderOpen, Play, Maximize2, WandSparkles, X } from '@lucide/svelte';
    import DropZone from '$lib/DropZone.svelte';
    import LoadingState from '$lib/components/LoadingState.svelte';
    import EmptyState from '$lib/components/EmptyState.svelte';
    import ToolCancelButton from '$lib/components/ToolCancelButton.svelte';
    import { ToolPage } from '$lib/ui';
    import {
        addStudioBatchFiles,
        applyStudioSinglePercent,
        cancelImageStudio,
        clampDimension,
        clearStudioBatch,
        clearStudioSingle,
        initImageStudioStore,
        removeStudioSingleBackground,
        removeStudioBatchFile,
        resetStudioSingleCrop,
        resetStudioSingleSize,
        runStudioBatch,
        runStudioSingleCrop,
        runStudioSingle,
        setStudioSingleCrop,
        setStudioSingleHeight,
        setStudioSingleOriginalDimensions,
        setStudioSinglePath,
        setStudioSingleWidth,
        studioActiveTab,
        studioBatchFiles,
        studioBackgroundQuality,
        studioCancelling,
        studioFilter,
        studioHeight,
        studioLongestSide,
        studioMode,
        studioOperationKind,
        studioOutputDir,
        studioOutputFormat,
        studioPercent,
        studioPreserveAspect,
        studioProcessing,
        studioProgressByPath,
        studioQuality,
        studioResultLabel,
        studioResults,
        studioSingleCrop,
        studioSingleCropEnabled,
        studioShortestSide,
        studioSingleHeight,
        studioSingleOriginalH,
        studioSingleOriginalW,
        studioSinglePath,
        studioSinglePreserveAspect,
        studioSinglePreview,
        studioSingleWidth,
        studioSuffix,
        studioWidth,
        type ImageStudioFilter,
        type ImageStudioFormat,
        type ImageStudioCrop,
        type ImageStudioMode,
    } from '$lib/stores/imageStudio';
    import {
        settingsBackgroundRemovalModelDownloadBusy,
        settingsBackgroundRemovalModelStatus,
        type BackgroundRemovalModelStatus,
    } from '$lib/stores/settings';
    import { fileName, fmtBytes } from '$lib/stores/imageBatchCommon';
    import { subscribeToToolLaunchTarget } from '$lib/stores/toolLaunchTarget';
    import { escToClear } from '$lib/actions/escToClear';

    // Input accepts SVG (rasterized on load); output formats exclude SVG.
    const supportedExts = ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'tiff', 'tif', 'gif', 'svg', 'ico'];
    const PREVIEW_MAX_W = 520;
    const PREVIEW_MAX_H = 340;

    let dragStartX = 0;
    let dragStartY = 0;
    let dragStartW = 0;
    let dragStartH = 0;
    let dragging = $state(false);
    type CropHandle = 'move' | 'nw' | 'ne' | 'se' | 'sw';
    type CropDrag = { handle: CropHandle; startX: number; startY: number; crop: ImageStudioCrop };
    let cropDrag = $state<CropDrag | null>(null);
    const highQualityModelAvailable = $derived($settingsBackgroundRemovalModelStatus?.available === true);

    async function refreshHighQualityModelAvailability() {
        try {
            const status = await invoke<BackgroundRemovalModelStatus>('background_removal_model_status');
            settingsBackgroundRemovalModelStatus.set(status);
            if (!status.available) studioBackgroundQuality.set('fast');
        } catch {
            settingsBackgroundRemovalModelStatus.set(null);
            studioBackgroundQuality.set('fast');
        }
    }

    function selectBackgroundQuality(quality: 'fast' | 'high') {
        if (quality === 'high' && !highQualityModelAvailable) return;
        studioBackgroundQuality.set(quality);
    }

    function openImageModels() {
        void emitTo('main', 'navigate-tool', {
            toolId: 'settings',
            settingsSection: 'imageModels',
        }).catch((error) => {
            console.error('Could not open Image models settings', error);
        });
    }

    onMount(() => {
        const stopTarget = subscribeToToolLaunchTarget('image-studio', ({ targetFile }) => {
            studioActiveTab.set('single');
            setStudioSinglePath(targetFile);
        });
        initImageStudioStore();
        if (!$settingsBackgroundRemovalModelDownloadBusy) {
            void refreshHighQualityModelAvailability();
        }
        return () => {
            stopTarget();
            stopResizeDrag();
            stopCropDrag();
        };
    });

    async function pickSingleFile() {
        const selected = await open({ multiple: false, directory: false, filters: [{ name: 'Images', extensions: supportedExts }] });
        if (typeof selected === 'string') setStudioSinglePath(selected);
    }

    async function pickBatchFiles() {
        const selected = await open({ multiple: true, directory: false, filters: [{ name: 'Images', extensions: supportedExts }] });
        if (Array.isArray(selected)) addStudioBatchFiles(selected);
        else if (typeof selected === 'string') addStudioBatchFiles([selected]);
    }

    async function pickOutputDir() {
        const selected = await open({ multiple: false, directory: true });
        if (typeof selected === 'string') studioOutputDir.set(selected);
    }

    function handleSingleDrop(paths: string[]) {
        const first = paths[0];
        if (first) setStudioSinglePath(first);
    }

    function onSingleImageLoad(event: Event) {
        const img = event.currentTarget as HTMLImageElement;
        setStudioSingleOriginalDimensions(img.naturalWidth, img.naturalHeight);
    }

    function basename(path: string): string {
        return path.split(/[\\/]/).pop() ?? path;
    }

    const singleAspect = $derived($studioSingleOriginalW > 0 && $studioSingleOriginalH > 0 ? $studioSingleOriginalW / $studioSingleOriginalH : 1);
    const basePreviewScale = $derived(
        $studioSingleOriginalW > 0 && $studioSingleOriginalH > 0
            ? Math.min(PREVIEW_MAX_W / $studioSingleOriginalW, PREVIEW_MAX_H / $studioSingleOriginalH, 1)
            : 1,
    );
    const originalPreviewW = $derived(Math.max(1, Math.round($studioSingleOriginalW * basePreviewScale)));
    const originalPreviewH = $derived(Math.max(1, Math.round($studioSingleOriginalH * basePreviewScale)));
    const visualPreviewW = $derived(Math.max(24, Math.round($studioSingleWidth * basePreviewScale)));
    const visualPreviewH = $derived(Math.max(24, Math.round($studioSingleHeight * basePreviewScale)));
    const cropPreviewActive = $derived($studioSingleCropEnabled && $studioSingleCrop !== null);
    const currentPreviewW = $derived(cropPreviewActive ? originalPreviewW : visualPreviewW);
    const currentPreviewH = $derived(cropPreviewActive ? originalPreviewH : visualPreviewH);
    const previewFrameW = $derived(cropPreviewActive ? originalPreviewW : Math.max(originalPreviewW, visualPreviewW));
    const previewFrameH = $derived(cropPreviewActive ? originalPreviewH : Math.max(originalPreviewH, visualPreviewH));
    const cropStyle = $derived(
        $studioSingleCrop
            ? `left: ${Math.round($studioSingleCrop.x * basePreviewScale)}px; top: ${Math.round($studioSingleCrop.y * basePreviewScale)}px; width: ${Math.max(1, Math.round($studioSingleCrop.width * basePreviewScale))}px; height: ${Math.max(1, Math.round($studioSingleCrop.height * basePreviewScale))}px;`
            : 'display: none;',
    );
    const singlePercent = $derived($studioSingleOriginalW > 0 ? Math.round(($studioSingleWidth / $studioSingleOriginalW) * 100) : 100);
    const singleProgress = $derived($studioSinglePath ? $studioProgressByPath[$studioSinglePath] ?? null : null);
    const successCount = $derived($studioResults.filter((r) => r.success).length);
    const failCount = $derived($studioResults.filter((r) => !r.success && r.error !== 'Cancelled').length);
    const activeCount = $derived(Object.values($studioProgressByPath).filter((p) => !['queued', 'done', 'error', 'cancelled'].includes(p.stage)).length);
    // Quality only matters for lossy outputs. 'keep' may map to a lossy source, so keep it enabled.
    const qualityApplies = $derived(['keep', 'jpg', 'webp'].includes($studioOutputFormat));

    function startResizeDrag(event: PointerEvent) {
        if (!$studioSinglePath || $studioProcessing) return;
        event.preventDefault();
        dragging = true;
        dragStartX = event.clientX;
        dragStartY = event.clientY;
        dragStartW = $studioSingleWidth;
        dragStartH = $studioSingleHeight;
        window.addEventListener('pointermove', onResizeDrag);
        window.addEventListener('pointerup', stopResizeDrag, { once: true });
        window.addEventListener('pointercancel', stopResizeDrag, { once: true });
        window.addEventListener('blur', stopResizeDrag, { once: true });
    }

    function onResizeDrag(event: PointerEvent) {
        if (!dragging) return;
        const scale = basePreviewScale || 1;
        const dx = (event.clientX - dragStartX) / scale;
        const dy = (event.clientY - dragStartY) / scale;

        if ($studioSinglePreserveAspect) {
            const rawW = Math.max(1, dragStartW + dx);
            const rawH = Math.max(1, dragStartH + dy);
            const factor = Math.max(rawW / dragStartW, rawH / dragStartH);
            const nextW = clampDimension(dragStartW * factor);
            studioSingleWidth.set(nextW);
            studioSingleHeight.set(clampDimension(nextW / singleAspect));
        } else {
            studioSingleWidth.set(clampDimension(dragStartW + dx));
            studioSingleHeight.set(clampDimension(dragStartH + dy));
        }
    }

    function stopResizeDrag() {
        dragging = false;
        window.removeEventListener('pointermove', onResizeDrag);
        window.removeEventListener('pointerup', stopResizeDrag);
        window.removeEventListener('pointercancel', stopResizeDrag);
        window.removeEventListener('blur', stopResizeDrag);
    }

    function updateStudioCrop(patch: Partial<ImageStudioCrop>) {
        const crop = $studioSingleCrop;
        if (!crop) return;
        setStudioSingleCrop({ ...crop, ...patch });
    }

    function startCropDrag(handle: CropHandle, event: PointerEvent) {
        const crop = $studioSingleCrop;
        if (!crop || !$studioSinglePath || $studioProcessing) return;
        event.preventDefault();
        event.stopPropagation();
        cropDrag = { handle, startX: event.clientX, startY: event.clientY, crop: { ...crop } };
        window.addEventListener('pointermove', onCropDrag);
        window.addEventListener('pointerup', stopCropDrag, { once: true });
        window.addEventListener('pointercancel', stopCropDrag, { once: true });
        window.addEventListener('blur', stopCropDrag, { once: true });
    }

    function onCropDrag(event: PointerEvent) {
        const drag = cropDrag;
        const originalW = $studioSingleOriginalW;
        const originalH = $studioSingleOriginalH;
        if (!drag || !originalW || !originalH) return;

        const scale = basePreviewScale || 1;
        const dx = Math.round((event.clientX - drag.startX) / scale);
        const dy = Math.round((event.clientY - drag.startY) / scale);
        const right = drag.crop.x + drag.crop.width;
        const bottom = drag.crop.y + drag.crop.height;
        if (drag.handle === 'move') {
            setStudioSingleCrop({
                ...drag.crop,
                x: Math.max(0, Math.min(originalW - drag.crop.width, drag.crop.x + dx)),
                y: Math.max(0, Math.min(originalH - drag.crop.height, drag.crop.y + dy)),
            });
            return;
        }
        let left = drag.crop.x;
        let top = drag.crop.y;
        let nextRight = right;
        let nextBottom = bottom;

        if (drag.handle === 'nw' || drag.handle === 'sw') left = Math.max(0, Math.min(right - 1, drag.crop.x + dx));
        if (drag.handle === 'ne' || drag.handle === 'se') nextRight = Math.max(drag.crop.x + 1, Math.min(originalW, right + dx));
        if (drag.handle === 'nw' || drag.handle === 'ne') top = Math.max(0, Math.min(bottom - 1, drag.crop.y + dy));
        if (drag.handle === 'sw' || drag.handle === 'se') nextBottom = Math.max(drag.crop.y + 1, Math.min(originalH, bottom + dy));

        setStudioSingleCrop({ x: left, y: top, width: nextRight - left, height: nextBottom - top });
    }

    function stopCropDrag() {
        cropDrag = null;
        window.removeEventListener('pointermove', onCropDrag);
        window.removeEventListener('pointerup', stopCropDrag);
        window.removeEventListener('pointercancel', stopCropDrag);
        window.removeEventListener('blur', stopCropDrag);
    }

    const filters: { id: ImageStudioFilter; label: string }[] = [
        { id: 'lanczos', label: 'Lanczos' },
        { id: 'cubic', label: 'Cubic' },
        { id: 'linear', label: 'Linear' },
        { id: 'nearest', label: 'Nearest' },
    ];

    const formats: { id: ImageStudioFormat; label: string }[] = [
        { id: 'keep', label: 'Keep original' },
        { id: 'jpg', label: 'JPEG' },
        { id: 'png', label: 'PNG' },
        { id: 'webp', label: 'WebP' },
        { id: 'avif', label: 'AVIF' },
        { id: 'bmp', label: 'BMP' },
        { id: 'tiff', label: 'TIFF' },
        { id: 'ico', label: 'ICO' },
        { id: 'gif', label: 'GIF' },
    ];

    const resizeModes: { id: ImageStudioMode; label: string }[] = [
        { id: 'none', label: 'Keep size' },
        { id: 'px', label: 'Pixels' },
        { id: 'percent', label: 'Percent' },
        { id: 'longest_side', label: 'Longest' },
        { id: 'shortest_side', label: 'Shortest' },
    ];
</script>

<ToolPage
    icon={ImageIcon}
    iconTint="#06b6d4"
    title="Image Studio: resize, crop, convert, compress and remove backgrounds"
    description="Resize, crop, convert, compress, and remove a background to transparent PNG, all locally. Originals stay untouched."
    width="wide"
    fill={false}
>
    <!-- Toolbar -->
    <div class="studio-toolbar flex flex-wrap items-center gap-3 rounded-2xl border border-border bg-panel p-3 md:p-4">
        <div class="studio-mode-switcher inline-flex rounded-xl border border-border bg-panel-2 p-1">
            <button class="studio-mode-tab rounded-lg px-3 py-1.5 text-sm transition-colors text-muted hover:text-text" class:is-active={$studioActiveTab === 'single'} onclick={() => studioActiveTab.set('single')} disabled={$studioProcessing}>Single</button>
            <button class="studio-mode-tab rounded-lg px-3 py-1.5 text-sm transition-colors text-muted hover:text-text" class:is-active={$studioActiveTab === 'batch'} onclick={() => studioActiveTab.set('batch')} disabled={$studioProcessing}>Batch</button>
        </div>

        {#if $studioActiveTab === 'single'}
            <button
                type="button"
                onclick={pickSingleFile}
                disabled={$studioProcessing}
                class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
            >
                <Plus class="h-4 w-4" />
                Browse image
            </button>
            {#if $studioSinglePath && !$studioProcessing}
                <button
                    type="button"
                    onclick={clearStudioSingle}
                    class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-error-strong"
                >
                    Clear
                </button>
            {/if}
        {:else}
            <button
                type="button"
                onclick={pickBatchFiles}
                disabled={$studioProcessing}
                class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
            >
                <Plus class="h-4 w-4" />
                Add images
            </button>
            {#if $studioBatchFiles.length > 0 && !$studioProcessing}
                <button
                    type="button"
                    onclick={clearStudioBatch}
                    class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-error-strong"
                >
                    Clear all ({$studioBatchFiles.length})
                </button>
            {/if}
        {/if}

        <button
            type="button"
            onclick={pickOutputDir}
            disabled={$studioProcessing}
            class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50"
            title={$studioOutputDir ?? 'Save alongside originals'}
        >
            <FolderOpen class="h-4 w-4" />
            {$studioOutputDir ? `Output: ${basename($studioOutputDir)}` : 'Output: alongside originals'}
        </button>

        <div class="flex-1"></div>

        <ToolCancelButton running={$studioProcessing} onCancel={cancelImageStudio} cancelling={$studioCancelling} />

        {#if $studioActiveTab === 'single'}
            <button
                onclick={$studioSingleCropEnabled ? runStudioSingleCrop : runStudioSingle}
                disabled={!$studioSinglePath || ($studioSingleCropEnabled ? !$studioSingleCrop : (!$studioSingleWidth || !$studioSingleHeight)) || $studioProcessing}
                class="inline-flex h-10 items-center gap-1.5 rounded-xl bg-accent px-4 text-sm font-medium text-accent-contrast hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed"
            >
                {#if $studioProcessing}
                    <LoadingState variant="inline" label={$studioOperationKind === 'crop' ? 'Cropping…' : $studioOperationKind === 'background' ? 'Removing background…' : 'Processing…'} />
                {:else if $studioSingleCropEnabled}
                    <Crop class="h-4 w-4" />
                    Crop image
                {:else}
                    <Play class="h-4 w-4" />
                    Process image
                {/if}
            </button>
            <button
                type="button"
                onclick={removeStudioSingleBackground}
                disabled={!$studioSinglePath || $studioProcessing}
                class="inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-50 disabled:cursor-not-allowed"
                title="Create a PNG with a transparent background"
            >
                {#if $studioOperationKind === 'background'}
                    <LoadingState variant="inline" label="Removing…" />
                {:else}
                    <WandSparkles class="h-4 w-4" />
                    Remove background (PNG)
                {/if}
            </button>
            <div
                class="inline-flex h-10 items-center gap-1 rounded-xl border border-border bg-panel-2 p-1 text-xs"
                role="group"
                aria-label="Background removal quality"
            >
                <span class="px-1.5 text-muted">Quality</span>
                <button
                    type="button"
                    class="rounded-lg px-2 py-1 transition-colors {$studioBackgroundQuality === 'fast' ? 'bg-accent text-accent-contrast font-medium' : 'text-muted hover:text-text'}"
                    aria-pressed={$studioBackgroundQuality === 'fast'}
                    onclick={() => selectBackgroundQuality('fast')}
                    disabled={$studioProcessing}
                >
                    Fast
                </button>
                {#if highQualityModelAvailable}
                    <button
                        type="button"
                        class="rounded-lg px-2 py-1 transition-colors {$studioBackgroundQuality === 'high' ? 'bg-accent text-accent-contrast font-medium' : 'text-muted hover:text-text'}"
                        aria-pressed={$studioBackgroundQuality === 'high'}
                        onclick={() => selectBackgroundQuality('high')}
                        disabled={$studioProcessing}
                    >
                        High
                    </button>
                {:else}
                    <span class="px-1 text-muted" title="Full U²-Net is not installed">High unavailable</span>
                    <button
                        type="button"
                        class="rounded-lg px-2 py-1 text-accent transition-colors hover:text-accent-hover disabled:opacity-50"
                        onclick={openImageModels}
                        disabled={$studioProcessing}
                    >
                        Get high quality
                    </button>
                {/if}
            </div>
        {:else}
            <button
                onclick={runStudioBatch}
                disabled={$studioBatchFiles.length === 0 || $studioProcessing}
                class="inline-flex h-10 items-center gap-1.5 rounded-xl bg-accent px-4 text-sm font-medium text-accent-contrast hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed"
            >
                {#if $studioProcessing}
                    <LoadingState variant="inline" label="Processing…" />
                {:else}
                    <Play class="h-4 w-4" />
                    Process {$studioBatchFiles.length} image{$studioBatchFiles.length === 1 ? '' : 's'}
                {/if}
            </button>
        {/if}
    </div>

    <!-- Progress -->
    {#if $studioProcessing}
        <div class="studio-progress bg-panel border border-border rounded-2xl p-4 space-y-2">
            <div class="flex items-center justify-between text-sm">
                <LoadingState variant="inline" label={$studioCancelling ? 'Cancelling…' : $studioOperationKind === 'crop' ? 'Cropping image…' : $studioOperationKind === 'background' ? 'Removing background…' : 'Processing images…'} />
                <span class="text-muted text-xs">{activeCount || 1} active</span>
            </div>
            <div class="h-1.5 bg-panel-2 rounded overflow-hidden"><div class="h-full w-1/2 bg-accent animate-pulse"></div></div>
            <p class="text-xs text-muted">You can switch tools. Progress and results will stay here.</p>
        </div>
    {/if}

    <!-- Work area: single or batch -->
    {#if $studioActiveTab === 'single'}
        <div class="studio-workspace grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]">
            <div class="studio-canvas rounded-2xl border border-border bg-panel p-4 md:p-5">
                <DropZone onFiles={handleSingleDrop} accept={supportedExts}>
                    {#snippet children()}
                        {#if !$studioSinglePath}
                            <EmptyState
                                icon={Maximize2}
                                title="Drop one image to edit"
                                description="Drag and drop or click below to browse."
                                variant="dashed"
                            >
                                {#snippet actions()}
                                    <button onclick={pickSingleFile} class="inline-flex items-center gap-1.5 rounded-lg border border-accent/40 bg-accent/10 px-3 py-1.5 text-sm text-accent hover:bg-accent/20">
                                        <FolderOpen class="h-4 w-4" />
                                        Browse files
                                    </button>
                                {/snippet}
                            </EmptyState>
                        {:else}
                            <div class="min-h-[360px]">
                                <div class="mb-2 flex flex-wrap items-center justify-between gap-2 text-xs text-muted">
                                    <span class="font-mono">{$studioSingleOriginalW || '—'}×{$studioSingleOriginalH || '—'} → {cropPreviewActive && $studioSingleCrop ? `${$studioSingleCrop.width}×${$studioSingleCrop.height}` : `${$studioSingleWidth || '—'}×${$studioSingleHeight || '—'}`} px</span>
                                    <span class="font-mono">{cropPreviewActive ? 'Crop' : `${singlePercent}%`}</span>
                                </div>

                                <div class="studio-preview-stage flex min-h-[340px] max-h-[60vh] items-center justify-center overflow-auto rounded-xl border border-border bg-panel-2 p-3">
                                    <div class="relative flex select-none items-center justify-center" style={`width: ${previewFrameW}px; height: ${previewFrameH}px;`}>
                                        {#if $studioSingleOriginalW > 0 && $studioSingleOriginalH > 0}
                                            <div class="pointer-events-none absolute rounded border border-dashed border-muted/40" style={`width: ${originalPreviewW}px; height: ${originalPreviewH}px;`}></div>
                                        {/if}

                                        <div class="relative shrink-0" style={`width: ${currentPreviewW}px; height: ${currentPreviewH}px;`}>
                                            <img src={$studioSinglePreview} alt="Selected preview" draggable="false" onload={onSingleImageLoad} class="h-full w-full rounded-lg object-fill shadow-lg" />
                                            {#if cropPreviewActive && $studioSingleCrop}
                                                <div class="studio-crop-overlay absolute inset-0 overflow-hidden rounded-lg">
                                                    <!-- svelte-ignore a11y_no_static_element_interactions -->
                                                    <div class="studio-crop-selection" style={cropStyle} onpointerdown={(event) => startCropDrag('move', event)}>
                                                        <button type="button" aria-label="Resize crop from top left" class="studio-crop-handle is-nw" onpointerdown={(event) => startCropDrag('nw', event)} disabled={$studioProcessing}></button>
                                                        <button type="button" aria-label="Resize crop from top right" class="studio-crop-handle is-ne" onpointerdown={(event) => startCropDrag('ne', event)} disabled={$studioProcessing}></button>
                                                        <button type="button" aria-label="Resize crop from bottom right" class="studio-crop-handle is-se" onpointerdown={(event) => startCropDrag('se', event)} disabled={$studioProcessing}></button>
                                                        <button type="button" aria-label="Resize crop from bottom left" class="studio-crop-handle is-sw" onpointerdown={(event) => startCropDrag('sw', event)} disabled={$studioProcessing}></button>
                                                    </div>
                                                </div>
                                            {:else}
                                                <div class="pointer-events-none absolute inset-0 rounded-lg border-2 border-accent/80"></div>
                                                <button aria-label="Drag to resize" title="Drag to resize" class="absolute -bottom-2 -right-2 h-5 w-5 cursor-nwse-resize rounded-full border-2 border-bg bg-accent shadow disabled:cursor-not-allowed disabled:opacity-50" onpointerdown={startResizeDrag} disabled={$studioProcessing}></button>
                                            {/if}
                                        </div>
                                    </div>
                                </div>

                                <div class="mt-2 text-center text-[11px] text-muted">{cropPreviewActive ? 'Drag crop corners, or use exact source-pixel fields on the right.' : 'Drag the bottom-right handle to resize. Use exact fields on the right when needed.'}</div>
                                <div class="mt-1 truncate text-center text-xs text-muted" title={$studioSinglePath}>{fileName($studioSinglePath)}</div>
                            </div>
                        {/if}
                    {/snippet}
                </DropZone>
            </div>

            <aside class="studio-inspector space-y-3">
                {#if !$studioSingleCropEnabled}
                    <div class="studio-inspector-section rounded-2xl border border-border bg-panel p-4">
                        <div class="mb-2 text-xs font-semibold uppercase tracking-wider text-muted">Size</div>
                        <div class="mb-2 grid grid-cols-2 gap-2">
                            <label class="block"><span class="mb-1 block text-[11px] text-muted">Width</span><input type="number" min="1" value={$studioSingleWidth || ''} oninput={(e) => setStudioSingleWidth(Number((e.currentTarget as HTMLInputElement).value))} disabled={!$studioSinglePath || $studioProcessing} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-50" /></label>
                            <label class="block"><span class="mb-1 block text-[11px] text-muted">Height</span><input type="number" min="1" value={$studioSingleHeight || ''} oninput={(e) => setStudioSingleHeight(Number((e.currentTarget as HTMLInputElement).value))} disabled={!$studioSinglePath || $studioProcessing} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-50" /></label>
                        </div>

                        <label class="mb-2 flex items-center gap-2 text-xs text-text"><input type="checkbox" bind:checked={$studioSinglePreserveAspect} disabled={!$studioSinglePath || $studioProcessing} /> Preserve aspect ratio</label>
                        <div class="grid grid-cols-4 gap-1.5">
                            {#each [25, 50, 75, 100] as pct}
                                <button onclick={() => applyStudioSinglePercent(pct)} disabled={!$studioSinglePath || $studioProcessing} class="rounded-lg border border-border px-2 py-1 text-xs text-muted hover:border-accent hover:text-text disabled:opacity-50">{pct}%</button>
                            {/each}
                        </div>
                        <button onclick={resetStudioSingleSize} disabled={!$studioSinglePath || $studioProcessing} class="mt-2 w-full rounded-lg border border-border px-2 py-1.5 text-xs text-muted hover:border-accent hover:text-text disabled:opacity-50">Reset original size</button>
                    </div>
                {/if}

                <div class="studio-inspector-section rounded-2xl border border-border bg-panel p-4">
                    <div class="mb-2 text-xs font-semibold uppercase tracking-wider text-muted">Crop</div>
                    <label class="mb-3 flex items-center gap-2 text-xs text-text"><input type="checkbox" bind:checked={$studioSingleCropEnabled} disabled={!$studioSinglePath || $studioProcessing} /> Crop source image</label>
                    {#if $studioSingleCrop}
                        <div class="grid grid-cols-2 gap-2">
                            <label class="block"><span class="mb-1 block text-[11px] text-muted">Left</span><input type="number" min="0" max={Math.max(0, $studioSingleOriginalW - 1)} value={$studioSingleCrop.x} oninput={(event) => updateStudioCrop({ x: Number((event.currentTarget as HTMLInputElement).value) })} disabled={!$studioSingleCropEnabled || $studioProcessing} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-50" /></label>
                            <label class="block"><span class="mb-1 block text-[11px] text-muted">Top</span><input type="number" min="0" max={Math.max(0, $studioSingleOriginalH - 1)} value={$studioSingleCrop.y} oninput={(event) => updateStudioCrop({ y: Number((event.currentTarget as HTMLInputElement).value) })} disabled={!$studioSingleCropEnabled || $studioProcessing} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-50" /></label>
                            <label class="block"><span class="mb-1 block text-[11px] text-muted">Width</span><input type="number" min="1" max={Math.max(1, $studioSingleOriginalW - $studioSingleCrop.x)} value={$studioSingleCrop.width} oninput={(event) => updateStudioCrop({ width: Number((event.currentTarget as HTMLInputElement).value) })} disabled={!$studioSingleCropEnabled || $studioProcessing} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-50" /></label>
                            <label class="block"><span class="mb-1 block text-[11px] text-muted">Height</span><input type="number" min="1" max={Math.max(1, $studioSingleOriginalH - $studioSingleCrop.y)} value={$studioSingleCrop.height} oninput={(event) => updateStudioCrop({ height: Number((event.currentTarget as HTMLInputElement).value) })} disabled={!$studioSingleCropEnabled || $studioProcessing} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-50" /></label>
                        </div>
                        <div class="mt-2 flex items-center justify-between gap-2 text-[11px] text-muted"><span>{$studioSingleCrop.width}×{$studioSingleCrop.height}px</span><button type="button" onclick={resetStudioSingleCrop} disabled={!$studioSingleCropEnabled || $studioProcessing} class="text-accent hover:text-accent-hover disabled:opacity-50">Reset crop</button></div>
                    {/if}
                </div>

                <div class="studio-inspector-section rounded-2xl border border-border bg-panel p-4">
                    <div class="mb-2 text-xs font-semibold uppercase tracking-wider text-muted">Output</div>
                    <div class="space-y-2">
                        <label class="block"><span class="mb-1 block text-[11px] text-muted">Format</span><select bind:value={$studioOutputFormat} disabled={$studioProcessing} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-50">{#each formats as f}<option value={f.id}>{f.label}</option>{/each}</select></label>
                        {#if !$studioSingleCropEnabled}
                            <label class="block"><span class="mb-1 block text-[11px] text-muted">Resampling</span><select bind:value={$studioFilter} disabled={$studioProcessing} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-50">{#each filters as f}<option value={f.id}>{f.label}</option>{/each}</select></label>
                        {/if}
                        <label class="block"><span class="mb-1 block text-[11px] text-muted">Quality {qualityApplies ? `(${$studioQuality}%)` : '(unused)'}</span><input type="range" min="1" max="100" bind:value={$studioQuality} class="w-full accent-emerald-500" disabled={$studioProcessing || !qualityApplies} /></label>
                        <label class="block"><span class="mb-1 block text-[11px] text-muted">Suffix</span><input type="text" bind:value={$studioSuffix} use:escToClear={() => studioSuffix.set('')} disabled={$studioProcessing} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 font-mono text-sm outline-none focus:border-accent disabled:opacity-50" /></label>
                    </div>
                </div>

                {#if singleProgress}
                    <div class="rounded-2xl border border-border bg-panel p-4">
                        <div class="mb-1 flex items-center justify-between text-xs text-muted"><span class="uppercase">{singleProgress.stage}</span><span>{singleProgress.progress}%</span></div>
                        <div class="h-1.5 overflow-hidden rounded bg-panel-2"><div class="h-full bg-accent transition-all" style={`width: ${singleProgress.progress}%`}></div></div>
                        {#if singleProgress.message}<div class="mt-1 truncate text-[11px] text-error" title={singleProgress.message}>{singleProgress.message}</div>{/if}
                    </div>
                {/if}
            </aside>
        </div>
    {:else}
        <div class="studio-workspace grid gap-3 lg:grid-cols-[minmax(0,1fr)_320px]">
            <div class="studio-canvas rounded-2xl border border-border bg-panel p-4 md:p-5">
                <DropZone onFiles={addStudioBatchFiles} accept={supportedExts}>
                    {#snippet children()}
                        {#if $studioBatchFiles.length === 0}
                            <EmptyState
                                icon={ImageIcon}
                                title="Drop images to process"
                                description="JPEG, PNG, WebP, BMP, TIFF, GIF, SVG, ICO."
                                variant="dashed"
                            >
                                {#snippet actions()}
                                    <button onclick={pickBatchFiles} class="inline-flex items-center gap-1.5 rounded-lg border border-accent/40 bg-accent/10 px-3 py-1.5 text-sm text-accent hover:bg-accent/20">
                                        <FolderOpen class="h-4 w-4" />
                                        Browse files
                                    </button>
                                {/snippet}
                            </EmptyState>
                        {:else}
                            <div class="studio-queue max-h-[60vh] overflow-y-auto divide-y divide-border rounded-lg border border-border bg-panel-2">
                                {#each $studioBatchFiles as path}
                                    {@const p = $studioProgressByPath[path]}
                                    <div class="px-3 py-2 text-sm hover:bg-bg/40">
                                        <div class="flex items-center justify-between gap-3">
                                            <span class="min-w-0 flex-1 truncate text-text" title={path}>{fileName(path)}</span>
                                            {#if $studioProcessing && p}
                                                <span class="shrink-0 text-[10px] uppercase text-muted">{p.stage} · {p.progress}%</span>
                                            {:else if !$studioProcessing}
                                                <button onclick={() => removeStudioBatchFile(path)} class="text-muted hover:text-error text-xs shrink-0" aria-label="Remove file"><X class="w-3.5 h-3.5" /></button>
                                            {/if}
                                        </div>
                                        {#if $studioProcessing && p}
                                            <div class="mt-2 h-1.5 overflow-hidden rounded bg-bg"><div class="h-full bg-accent transition-all" style={`width: ${p.progress}%`}></div></div>
                                            {#if p.message}<div class="mt-1 truncate text-[10px] text-error" title={p.message}>{p.message}</div>{/if}
                                        {/if}
                                    </div>
                                {/each}
                            </div>
                        {/if}
                    {/snippet}
                </DropZone>
            </div>

            <aside class="studio-inspector space-y-3">
                <div class="studio-inspector-section rounded-2xl border border-border bg-panel p-4">
                    <div class="mb-2 text-xs font-semibold uppercase tracking-wider text-muted">Resize mode</div>
                    <div class="grid grid-cols-2 gap-1.5">
                        {#each resizeModes as m}
                            <button class="rounded-lg px-2 py-1.5 text-xs transition-colors {$studioMode === m.id ? 'bg-accent text-accent-contrast font-medium' : 'border border-border text-muted hover:border-accent hover:text-text'}" onclick={() => studioMode.set(m.id)} disabled={$studioProcessing}>{m.label}</button>
                        {/each}
                    </div>

                    <div class="mt-3">
                        {#if $studioMode === 'none'}
                            <p class="text-[11px] text-muted">Dimensions stay the same — only format and quality are applied.</p>
                        {:else if $studioMode === 'px'}
                            <div class="grid grid-cols-2 gap-2">
                                <label class="block"><span class="mb-1 block text-[11px] text-muted">Width</span><input type="number" min="1" bind:value={$studioWidth} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent" disabled={$studioProcessing} /></label>
                                <label class="block"><span class="mb-1 block text-[11px] text-muted">Height</span><input type="number" min="1" bind:value={$studioHeight} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent" disabled={$studioProcessing} /></label>
                            </div>
                            <label class="mt-2 flex items-center gap-2 text-xs text-text"><input type="checkbox" bind:checked={$studioPreserveAspect} disabled={$studioProcessing} /> Preserve aspect</label>
                        {:else if $studioMode === 'percent'}
                            <label class="block">
                                <span class="mb-1 block text-[11px] text-muted">Percent: {$studioPercent}%</span>
                                <input type="range" min="1" max="300" bind:value={$studioPercent} class="w-full accent-emerald-500" disabled={$studioProcessing} />
                            </label>
                        {:else if $studioMode === 'longest_side'}
                            <label class="block">
                                <span class="mb-1 block text-[11px] text-muted">Longest side</span>
                                <input type="number" min="1" bind:value={$studioLongestSide} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent" disabled={$studioProcessing} />
                            </label>
                        {:else if $studioMode === 'shortest_side'}
                            <label class="block">
                                <span class="mb-1 block text-[11px] text-muted">Shortest side</span>
                                <input type="number" min="1" bind:value={$studioShortestSide} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent" disabled={$studioProcessing} />
                            </label>
                        {/if}
                    </div>
                </div>

                <div class="studio-inspector-section rounded-2xl border border-border bg-panel p-4">
                    <div class="mb-2 text-xs font-semibold uppercase tracking-wider text-muted">Output</div>
                    <div class="space-y-2">
                        <label class="block"><span class="mb-1 block text-[11px] text-muted">Format</span><select bind:value={$studioOutputFormat} disabled={$studioProcessing} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent">{#each formats as f}<option value={f.id}>{f.label}</option>{/each}</select></label>
                        <label class="block"><span class="mb-1 block text-[11px] text-muted">Resampling</span><select bind:value={$studioFilter} disabled={$studioProcessing} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 text-sm outline-none focus:border-accent">{#each filters as f}<option value={f.id}>{f.label}</option>{/each}</select></label>
                        <label class="block"><span class="mb-1 block text-[11px] text-muted">Quality {qualityApplies ? `(${$studioQuality}%)` : '(unused)'}</span><input type="range" min="1" max="100" bind:value={$studioQuality} class="w-full accent-emerald-500" disabled={$studioProcessing || !qualityApplies} /></label>
                        <label class="block"><span class="mb-1 block text-[11px] text-muted">Suffix</span><input type="text" bind:value={$studioSuffix} use:escToClear={() => studioSuffix.set('')} class="w-full rounded-lg border border-border bg-panel-2 px-2 py-1.5 font-mono text-sm outline-none focus:border-accent" disabled={$studioProcessing} /></label>
                    </div>
                </div>
            </aside>
        </div>
    {/if}

    {#if $studioResults.length > 0}
        <div class="studio-results rounded-2xl border border-border bg-panel p-4 md:p-5">
            <div class="mb-3 flex items-center justify-between">
                <h2 class="text-sm uppercase tracking-wider text-muted font-semibold">Results</h2>
                <div class="flex items-center gap-2 text-xs">
                    {#if $studioResultLabel}<span class="text-muted">{$studioResultLabel}</span>{/if}
                    <span class="text-success">{successCount} done</span>
                    {#if failCount > 0}
                        <span class="text-muted mx-2">·</span>
                        <span class="text-error">{failCount} failed</span>
                    {/if}
                </div>
            </div>

            <div class="max-h-80 space-y-2 overflow-y-auto">
                {#each $studioResults as r}
                    <div class="rounded-lg border {r.success ? 'border-border bg-panel-2' : 'border-error-strong bg-error-soft'} p-3">
                        <div class="mb-1 flex items-start justify-between gap-3">
                            <div class="min-w-0 flex-1">
                                <div class="truncate text-sm text-text" title={r.source_path}>{fileName(r.source_path)}</div>
                                {#if r.success}<div class="truncate text-xs text-muted" title={r.output_path}>→ {fileName(r.output_path)}</div>{/if}
                            </div>
                            <span class="text-xs px-2 py-0.5 rounded uppercase font-semibold
                                         {r.success ? 'bg-success-soft text-success border border-success-strong' : 'bg-error-soft text-error border border-error-strong'}">
                                {r.success ? 'OK' : r.error === 'Cancelled' ? 'CANCELLED' : 'FAIL'}
                            </span>
                        </div>

                        {#if r.success}
                            <div class="text-xs text-muted">{r.original_dims[0]}×{r.original_dims[1]} → {r.new_dims[0]}×{r.new_dims[1]} · {fmtBytes(r.original_size)} → {fmtBytes(r.new_size)}</div>
                            <div class="mt-1 truncate text-xs text-muted" title={r.output_path}>{r.output_path}</div>
                        {:else}
                            <div class="text-xs text-error">{r.error}</div>
                        {/if}
                    </div>
                {/each}
            </div>
        </div>
    {/if}
</ToolPage>
<style>
    .studio-toolbar {
        gap: 8px 10px;
        padding: 0 0 12px;
        border: 0;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        border-radius: 0;
        background: transparent;
    }

    .studio-mode-switcher {
        gap: 2px;
        padding: 2px;
        border-radius: var(--radius-control);
        border-color: var(--color-border);
        background: transparent;
    }

    .studio-mode-tab {
        position: relative;
        min-height: 30px;
        border: 0;
        background: transparent;
        color: var(--color-text-secondary);
    }

    .studio-mode-tab:hover:not(:disabled) {
        background: var(--color-panel-2);
        color: var(--color-text);
    }

    .studio-mode-tab.is-active {
        padding-left: 15px;
        background: var(--color-panel-2);
        color: var(--color-text);
        font-weight: 500;
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }

    .studio-mode-tab.is-active::before {
        content: '';
        position: absolute;
        left: 5px;
        top: 7px;
        bottom: 7px;
        width: 2px;
        border-radius: 999px;
        background: var(--color-accent);
    }

    .studio-mode-tab:disabled {
        cursor: not-allowed;
        opacity: 0.5;
    }

    .studio-toolbar > button {
        border-radius: var(--radius-control);
    }

    .studio-progress {
        display: grid;
        gap: 8px;
        padding: 10px 12px;
        border-radius: var(--radius-control);
        border-color: color-mix(in srgb, var(--color-accent) 28%, var(--color-border));
        background: color-mix(in srgb, var(--color-accent) 4%, var(--color-panel));
    }

    .studio-workspace {
        align-items: start;
    }

    .studio-canvas {
        min-width: 0;
        border-color: var(--color-border);
        background: var(--color-panel);
        box-shadow: inset 0 1px 0 color-mix(in srgb, var(--color-text) 5%, transparent);
    }

    .studio-preview-stage {
        border-color: color-mix(in srgb, var(--color-text) 8%, var(--color-border));
        background: color-mix(in srgb, var(--color-panel-2) 92%, transparent);
    }

    .studio-crop-selection {
        position: absolute;
        min-width: 1px;
        min-height: 1px;
        border: 2px solid var(--color-accent);
        box-shadow: 0 0 0 9999px color-mix(in srgb, var(--color-bg) 52%, transparent);
        cursor: move;
        touch-action: none;
    }

    .studio-crop-handle {
        position: absolute;
        z-index: 1;
        width: 11px;
        height: 11px;
        padding: 0;
        border: 2px solid var(--color-bg);
        border-radius: 999px;
        background: var(--color-accent);
        cursor: nwse-resize;
    }

    .studio-crop-handle.is-nw { left: -5px; top: -5px; }
    .studio-crop-handle.is-ne { right: -5px; top: -5px; cursor: nesw-resize; }
    .studio-crop-handle.is-se { right: -5px; bottom: -5px; }
    .studio-crop-handle.is-sw { left: -5px; bottom: -5px; cursor: nesw-resize; }

    .studio-crop-handle:focus-visible {
        outline: 2px solid var(--color-text);
        outline-offset: 2px;
    }

    .studio-crop-handle:disabled {
        cursor: not-allowed;
        opacity: 0.5;
    }

    .studio-inspector {
        position: sticky;
        top: 0;
        align-self: start;
    }

    .studio-inspector-section {
        border-radius: var(--radius-card);
        border-color: var(--color-border);
        background: var(--color-panel);
        box-shadow: inset 0 1px 0 color-mix(in srgb, var(--color-text) 4%, transparent);
    }

    .studio-inspector-section :global(input[type='checkbox']) {
        accent-color: var(--color-accent);
    }

    .studio-inspector-section :global(input[type='range']) {
        accent-color: var(--color-accent);
    }

    .studio-queue {
        border-color: color-mix(in srgb, var(--color-text) 8%, var(--color-border));
        background: var(--color-panel-2);
    }

    .studio-queue :global(> div) {
        transition: background-color var(--dur-micro) var(--ease-out);
    }

    .studio-queue :global(> div:hover) {
        background: color-mix(in srgb, var(--color-text) 4%, transparent);
    }

    .studio-results {
        border-color: var(--color-border);
        background: var(--color-panel);
    }

    @media (max-width: 1023px) {
        .studio-inspector {
            position: static;
        }
    }

    @media (max-width: 640px) {
        .studio-toolbar {
            align-items: stretch;
        }

        .studio-toolbar > button {
            justify-content: center;
        }

        .studio-mode-switcher {
            width: 100%;
        }

        .studio-mode-tab {
            flex: 1;
        }
    }
</style>
