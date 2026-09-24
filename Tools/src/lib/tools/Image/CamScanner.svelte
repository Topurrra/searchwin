<script lang="ts">
    /*
      CamScanner — offline document scanner.

      Import a photo of a document → the backend best-effort detects the
      page quad (camscan_detect_corners) → the four corners render as
      DRAGGABLE handles over the photo with a connecting outline. The user
      nudges the corners, then Flatten warps the quad to a clean
      rectangle (camscan_warp) with an enhance mode (Color / Grayscale /
      B&W / Magic). Export writes the chosen file; Extract text runs the
      existing Tesseract OCR over the flattened image. Nothing leaves the
      machine.

      State that must survive the workspace {#key} nav-unmount lives in
      stores/camScanner.ts (source, corners, mode, flattened preview, OCR
      text, busy). Only the ephemeral drag (which handle, pointer offset)
      is component-local $state.

      Corner coordinate handling mirrors ScreenshotRedact.svelte:
      coordinates are stored in NATURAL source-image pixels and converted
      to/from on-screen display pixels via getBoundingClientRect scaling,
      so the overlay stays correct at any rendered image size.
    */
    import { onMount } from 'svelte';
    import { get } from 'svelte/store';
    import { invoke, convertFileSrc } from '@tauri-apps/api/core';
    import { open, save } from '@tauri-apps/plugin-dialog';
    import { writeFile } from '@tauri-apps/plugin-fs';
    import { tempDir, join } from '@tauri-apps/api/path';
    import {
        ScanLine,
        ImageUp,
        ScanSearch,
        RotateCcw,
        Wand2,
        Save,
        FileText,
        ImageOff,
        Copy,
        Loader2,
    } from '@lucide/svelte';
    import DropZone from '$lib/DropZone.svelte';
    import { ToolPage, Button, Select, EmptyState, ErrorState } from '$lib/ui';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import { recordActivity } from '$lib/stores/activityLog';
    import {
        camSourcePath,
        camSourceWidth,
        camSourceHeight,
        camCorners,
        camDetected,
        camEnhance,
        camFlattenedPath,
        camOcrText,
        camBusy,
        camPhase,
        camError,
        camOcrOperationId,
        camOcrCancelRequested,
        clearCamScanner,
        boundsCorners,
        type CamDetect,
        type CamPoint,
        type EnhanceMode,
    } from '$lib/stores/camScanner';

    const IMAGE_EXTS = ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'tif', 'tiff'];
    const ICON_TINT = '#06b6d4'; // Image pack tint.

    // Persisted state (module stores) — survives nav.
    const sourcePath = camSourcePath;
    const sourceWidth = camSourceWidth;
    const sourceHeight = camSourceHeight;
    const corners = camCorners;
    const detected = camDetected;
    const enhance = camEnhance;
    const flattenedPath = camFlattenedPath;
    const ocrText = camOcrText;
    const busy = camBusy;

    const phaseStore = camPhase;
    let phase = $derived($phaseStore);
    const error = camError;
    const ocrOpId = camOcrOperationId;
    const ocrCancelRequested = camOcrCancelRequested;
    // Transient — re-probed each mount.
    let ocrAvailable = $state(false);
    let ocrLanguages = $state<string[]>([]);
    let ocrLang = $state('eng');
    // Cache-bust tokens that defeat the WebView2 image cache (OcrTool.svelte
    // trick). Split so re-warping (flatten / enhance change) reloads ONLY the
    // flattened preview, not the unchanged source photo the overlay sits on.
    let sourceToken = $state(0);
    let flattenToken = $state(0);

    // Ephemeral drag — never persisted.
    let draggingCorner = $state<number | null>(null);
    let imgEl = $state<HTMLImageElement | null>(null);
    let imgNaturalW = $state(0);
    let imgNaturalH = $state(0);
    // Bumped by a ResizeObserver on the source <img> so the corner overlay
    // re-measures when the rendered image size changes (window resize, the
    // 900px grid breakpoint collapse) — getBoundingClientRect is not reactive.
    let layoutTick = $state(0);

    const ENHANCE_OPTIONS: { value: EnhanceMode; label: string }[] = [
        { value: 'color', label: 'Color' },
        { value: 'grayscale', label: 'Grayscale' },
        { value: 'bw', label: 'B&W' },
        { value: 'magic', label: 'Magic' },
    ];

    onMount(async () => {
        try {
            ocrAvailable = await invoke<boolean>('ocr_available');
            if (ocrAvailable) {
                ocrLanguages = await invoke<string[]>('ocr_languages');
                if (ocrLanguages.length && !ocrLanguages.includes(ocrLang)) {
                    ocrLang = ocrLanguages.includes('eng') ? 'eng' : ocrLanguages[0];
                }
            }
        } catch {
            ocrAvailable = false;
        }
    });

    // Re-measure the corner overlay when the source image's rendered size
    // changes (layout reflow, window resize). getBoundingClientRect is read
    // imperatively in handleStyle/quadPoints, so without this they would drift.
    $effect(() => {
        if (!imgEl) return;
        const ro = new ResizeObserver(() => {
            layoutTick += 1;
        });
        ro.observe(imgEl);
        return () => ro.disconnect();
    });

    let langOptions = $derived(
        (ocrLanguages.length ? ocrLanguages : ['eng']).map((value) => ({ value, label: value })),
    );

    // Cache-busted source preview — re-loads on every fresh import.
    let sourceSrc = $derived(
        $sourcePath ? `${convertFileSrc($sourcePath)}?v=${sourceToken}` : '',
    );
    let flattenedSrc = $derived(
        $flattenedPath ? `${convertFileSrc($flattenedPath)}?v=${flattenToken}` : '',
    );

    function basename(path: string): string {
        return path.split(/[\\/]/).pop() ?? path;
    }

    // ─── Import ───────────────────────────────────────────────────────
    async function importPath(path: string) {
        if (get(busy)) return; // ignore paste/drop/pick while an op is in flight
        clearCamScanner();
        error.set(null);
        sourcePath.set(path);
        sourceToken += 1;
        await detectCorners(path);
    }

    async function pickImage() {
        const selected = await open({
            multiple: false,
            directory: false,
            filters: [{ name: 'Images', extensions: IMAGE_EXTS }],
        });
        if (typeof selected === 'string') await importPath(selected);
    }

    function onDropFiles(paths: string[]) {
        if (paths[0]) void importPath(paths[0]);
    }

    /** Clipboard paste — write the pasted image bytes to a temp file so
     *  the backend can read it by path, then run the normal import flow. */
    async function onPaste(event: ClipboardEvent) {
        const items = event.clipboardData?.items;
        if (!items) return;
        for (const item of items) {
            if (item.type.startsWith('image/')) {
                const blob = item.getAsFile();
                if (!blob) continue;
                event.preventDefault();
                try {
                    const bytes = new Uint8Array(await blob.arrayBuffer());
                    const ext = item.type.split('/')[1]?.split('+')[0] || 'png';
                    const dir = await tempDir();
                    const dest = await join(dir, `camscan-paste-${Date.now()}.${ext}`);
                    await writeFile(dest, bytes);
                    await importPath(dest);
                } catch (e) {
                    errorToast("Couldn't read the pasted image", e, {
                        hint: 'Copy the image again, or import a file instead.',
                    });
                }
                return;
            }
        }
    }

    function unload() {
        if (get(busy)) return;
        clearCamScanner();
        error.set(null);
        sourceToken += 1;
    }

    // ─── Detection ────────────────────────────────────────────────────
    async function detectCorners(path: string) {
        busy.set(true);
        phaseStore.set('detecting');
        error.set(null);
        try {
            const result = await invoke<CamDetect>('camscan_detect_corners', { path });
            sourceWidth.set(result.width);
            sourceHeight.set(result.height);
            corners.set(result.corners);
            detected.set(result.detected);
            flattenedPath.set(null);
            ocrText.set('');
        } catch (e) {
            error.set(String(e));
            errorToast("Couldn't analyze this image", e, {
                hint: 'Try a clearer, well-lit photo of the document.',
            });
        } finally {
            busy.set(false);
            phaseStore.set(null);
        }
    }

    async function reDetect() {
        const path = get(sourcePath);
        if (!path || get(busy)) return;
        await detectCorners(path);
    }

    function resetCorners() {
        const w = get(sourceWidth);
        const h = get(sourceHeight);
        if (!w || !h) return;
        corners.set(boundsCorners(w, h));
        detected.set(false);
    }

    // ─── Corner drag (ScreenshotRedact coordinate handling) ───────────
    function onImageLoad() {
        if (!imgEl) return;
        imgNaturalW = imgEl.naturalWidth;
        imgNaturalH = imgEl.naturalHeight;
    }

    /** Mouse → natural image px, clamped to image bounds. */
    function mouseToImage(e: MouseEvent): CamPoint {
        if (!imgEl) return { x: 0, y: 0 };
        const rect = imgEl.getBoundingClientRect();
        const scaleX = imgNaturalW / rect.width;
        const scaleY = imgNaturalH / rect.height;
        return {
            x: Math.max(0, Math.min(imgNaturalW, (e.clientX - rect.left) * scaleX)),
            y: Math.max(0, Math.min(imgNaturalH, (e.clientY - rect.top) * scaleY)),
        };
    }

    function startCornerDrag(index: number, e: MouseEvent) {
        e.preventDefault();
        e.stopPropagation();
        draggingCorner = index;
    }

    function onCanvasMove(e: MouseEvent) {
        if (draggingCorner === null) return;
        const p = mouseToImage(e);
        const next = get(corners).map((c, i) => (i === draggingCorner ? p : c));
        corners.set(next);
    }

    function endCornerDrag() {
        draggingCorner = null;
    }

    /** Display-px position for a natural-px corner (regionStyle approach). */
    function handleStyle(c: CamPoint): string {
        void layoutTick; // re-measure on layout reflow (resize / 900px breakpoint)
        if (!imgEl || !imgNaturalW || !imgNaturalH) return 'display:none;';
        const rect = imgEl.getBoundingClientRect();
        const scaleX = rect.width / imgNaturalW;
        const scaleY = rect.height / imgNaturalH;
        return `left: ${c.x * scaleX}px; top: ${c.y * scaleY}px;`;
    }

    /** SVG polygon points (display px) connecting the 4 corners. */
    let quadPoints = $derived.by(() => {
        void layoutTick; // re-measure on layout reflow (resize / 900px breakpoint)
        if (!imgEl || !imgNaturalW || !imgNaturalH) return '';
        const rect = imgEl.getBoundingClientRect();
        const scaleX = rect.width / imgNaturalW;
        const scaleY = rect.height / imgNaturalH;
        return $corners.map((c) => `${c.x * scaleX},${c.y * scaleY}`).join(' ');
    });

    // ─── Flatten / warp ───────────────────────────────────────────────
    async function flatten() {
        const path = get(sourcePath);
        const pts = get(corners);
        if (!path || pts.length !== 4 || get(busy)) return;
        busy.set(true);
        phaseStore.set('flattening');
        error.set(null);
        try {
            const out = await invoke<string>('camscan_warp', {
                path,
                corners: pts,
                enhance: get(enhance),
                outPath: null,
            });
            flattenedPath.set(out);
            flattenToken += 1;
            ocrText.set('');
        } catch (e) {
            error.set(String(e));
            errorToast("Couldn't flatten the document", e, {
                hint: 'Make sure the four corners enclose the page, then try again.',
            });
        } finally {
            busy.set(false);
            phaseStore.set(null);
        }
    }

    function setEnhance(mode: EnhanceMode) {
        if (get(enhance) === mode) return;
        enhance.set(mode);
        // Re-run the warp with the new mode if we already have a preview.
        if (get(flattenedPath)) void flatten();
    }

    // ─── Export ───────────────────────────────────────────────────────
    async function exportFlattened() {
        const path = get(sourcePath);
        const pts = get(corners);
        if (!path || pts.length !== 4 || get(busy)) return;

        const base = basename(path).replace(/\.[^.]+$/, '') || 'document';
        const outPath = await save({
            defaultPath: `${base}_scanned.png`,
            filters: [
                { name: 'PNG', extensions: ['png'] },
                { name: 'JPEG', extensions: ['jpg', 'jpeg'] },
            ],
        });
        if (!outPath) return;

        busy.set(true);
        phaseStore.set('exporting');
        error.set(null);
        try {
            const written = await invoke<string>('camscan_warp', {
                path,
                corners: pts,
                enhance: get(enhance),
                outPath,
            });
            toast('Scanned document saved', 'success');
            void recordActivity({
                toolId: 'camscanner',
                summary: `Scanned document · ${get(enhance)}`,
                details: `Source: ${basename(path)} · Output: ${written}`,
                outcome: 'success',
            });
        } catch (e) {
            error.set(String(e));
            errorToast("Couldn't save the scanned document", e, {
                hint: 'Try a different folder — the destination may be read-only or full.',
                durationMs: 7000,
            });
            void recordActivity({
                toolId: 'camscanner',
                summary: 'CamScanner export failed',
                details: String(e),
                outcome: 'failed',
            });
        } finally {
            busy.set(false);
            phaseStore.set(null);
        }
    }

    // ─── Extract text (OCR) ───────────────────────────────────────────
    async function extractText() {
        const path = get(sourcePath);
        const pts = get(corners);
        if (!path || pts.length !== 4 || get(busy) || !ocrAvailable) return;
        busy.set(true);
        phaseStore.set('ocr');
        error.set(null);
        const opId = `camscan-ocr-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
        ocrOpId.set(opId);
        try {
            // Ensure a flattened image exists, then OCR it (not the raw photo).
            let flat = get(flattenedPath);
            if (!flat) {
                flat = await invoke<string>('camscan_warp', {
                    path,
                    corners: pts,
                    enhance: get(enhance),
                    outPath: null,
                });
                flattenedPath.set(flat);
                flattenToken += 1;
            }
            const text = await invoke<string>('ocr_image', {
                path: flat,
                langs: ocrLang,
                preprocess: false,
                psm: 6,
                operationId: opId,
            });
            ocrText.set(text);
            if (!text.trim()) {
                toast('No text was recognized — try a clearer scan.', 'info', 4500);
            }
        } catch (e) {
            const msg = String(e);
            if (msg.includes('Cancelled')) {
                toast('Text extraction cancelled', 'info', 2500);
            } else {
                error.set(msg);
                errorToast("Couldn't extract text", e, {
                    hint: 'Try a clearer scan or a different language.',
                });
            }
        } finally {
            if (get(ocrOpId) === opId) {
                busy.set(false);
                phaseStore.set(null);
                ocrOpId.set(null);
                ocrCancelRequested.set(false);
            }
        }
    }

    async function cancelOcr() {
        if (!get(ocrOpId) || get(ocrCancelRequested)) return;
        const operationId = get(ocrOpId)!;
        ocrCancelRequested.set(true);
        try {
            await invoke('cancel_ocr_operation', { operationId });
        } catch {
            // Best-effort cancel.
        }
    }

    async function copyText() {
        if (!get(ocrText)) return;
        try {
            await navigator.clipboard.writeText(get(ocrText));
            toast('Copied to clipboard.', 'success', 2000);
        } catch {
            toast('Could not copy to clipboard.', 'error', 3000);
        }
    }
</script>

<svelte:window onpaste={onPaste} />

<ToolPage
    icon={ScanLine}
    iconTint={ICON_TINT}
    title="Document Scanner"
    description="Turn a photo of a document into a clean, flattened scan — fully on-device. Drag the corners to fit the page, choose an enhancement, then export or extract the text."
    width="wide"
    fill={false}
>
    {#if !$sourcePath}
        <DropZone onFiles={onDropFiles} accept={IMAGE_EXTS}>
            {#snippet children()}
                <EmptyState
                    icon={ImageUp}
                    title="Drop a document photo to scan"
                    description="JPEG, PNG, or WebP. Drag and drop, paste from the clipboard, or browse. Everything runs locally — nothing is uploaded."
                    variant="dashed"
                >
                    {#snippet actions()}
                        <Button variant="secondary" icon={ImageUp} onclick={pickImage}>
                            Browse files
                        </Button>
                    {/snippet}
                </EmptyState>
            {/snippet}
        </DropZone>
    {:else}
        <!-- Toolbar -->
        <div class="cs-toolbar">
            <div class="cs-enhance">
                <span class="cs-label">Enhance</span>
                <div class="cs-seg">
                    {#each ENHANCE_OPTIONS as opt}
                        <button
                            type="button"
                            class="cs-seg-btn"
                            class:active={$enhance === opt.value}
                            disabled={$busy}
                            onclick={() => setEnhance(opt.value)}
                        >
                            {opt.label}
                        </button>
                    {/each}
                </div>
            </div>

            <div class="cs-spacer"></div>

            <Button variant="ghost" icon={ScanSearch} onclick={reDetect} disabled={$busy}>
                Re-detect
            </Button>
            <Button variant="ghost" icon={RotateCcw} onclick={resetCorners} disabled={$busy}>
                Reset corners
            </Button>
            <Button
                variant="secondary"
                icon={$busy && phase === 'flattening' ? Loader2 : Wand2}
                onclick={flatten}
                disabled={$busy}
            >
                {$busy && phase === 'flattening' ? 'Flattening…' : 'Flatten'}
            </Button>
            <Button
                variant="primary"
                icon={$busy && phase === 'exporting' ? Loader2 : Save}
                onclick={exportFlattened}
                disabled={$busy}
            >
                {$busy && phase === 'exporting' ? 'Saving…' : 'Export'}
            </Button>
            <Button variant="ghost" icon={ImageOff} onclick={unload} disabled={$busy}>
                Replace
            </Button>
        </div>

        {#if $error}
            <ErrorState
                title="Document scan failed"
                description={$error}
                retry={reDetect}
                retryLabel="Re-detect"
            />
        {/if}

        <!-- Editor + result -->
        <div class="cs-grid">
            <!-- Source with draggable corners -->
            <div class="cs-pane">
                <div class="cs-pane-head">
                    <span class="cs-pane-title">1 ? Adjust source</span>
                    <span class="cs-pane-hint">
                        {$detected ? 'Detected — drag to fine-tune' : 'Drag each corner to the page edge'}
                    </span>
                </div>
                <div class="cs-canvas">
                    <div class="cs-stage">
                        <img
                            bind:this={imgEl}
                            src={sourceSrc}
                            alt={basename($sourcePath)}
                            onload={onImageLoad}
                            class="cs-img"
                            draggable="false"
                        />
                        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                        <!-- Pointer-driven corner-drag surface; role="application"
                             signals the custom mouse-only interaction model. -->
                        <div
                            class="cs-overlay"
                            onmousemove={onCanvasMove}
                            onmouseup={endCornerDrag}
                            onmouseleave={endCornerDrag}
                            role="application"
                        >
                            {#if imgEl && imgNaturalW && imgNaturalH}
                                <svg class="cs-quad" preserveAspectRatio="none">
                                    <polygon points={quadPoints} />
                                </svg>
                                {#each $corners as c, i}
                                    <button
                                        type="button"
                                        class="cs-handle"
                                        class:dragging={draggingCorner === i}
                                        style={handleStyle(c)}
                                        onmousedown={(e) => startCornerDrag(i, e)}
                                        aria-label={`Corner ${i + 1}`}
                                    ></button>
                                {/each}
                            {/if}
                        </div>
                    </div>
                </div>
                <div class="cs-meta">
                    <span class="cs-meta-name" title={$sourcePath}>{basename($sourcePath)}</span>
                    {#if $sourceWidth && $sourceHeight}
                        <span>·</span>
                        <span>{$sourceWidth}×{$sourceHeight}px</span>
                    {/if}
                </div>
            </div>

            <!-- Flattened result -->
            <div class="cs-pane">
                <div class="cs-pane-head">
                    <span class="cs-pane-title">2 ? Preview scan</span>
                    {#if $flattenedPath}
                        <span class="cs-pane-hint">{ENHANCE_OPTIONS.find((o) => o.value === $enhance)?.label}</span>
                    {/if}
                </div>
                <div class="cs-canvas cs-result">
                    {#if $busy && (phase === 'flattening' || phase === 'exporting')}
                        <div class="cs-placeholder">
                            <Loader2 class="cs-spin" />
                            <span>Flattening…</span>
                        </div>
                    {:else if $flattenedPath}
                        {#key flattenToken}
                            <img src={flattenedSrc} alt="Flattened document" class="cs-img" draggable="false" />
                        {/key}
                    {:else}
                        <div class="cs-placeholder">
                            <Wand2 class="cs-placeholder-icon" />
                            <span>Press Flatten to preview the cleaned scan</span>
                        </div>
                    {/if}
                </div>
            </div>
        </div>

        <!-- Extract text -->
        <div class="cs-ocr">
            <div class="cs-ocr-head">
                <span class="cs-pane-title">3 ? Extract text</span>
                {#if ocrAvailable}
                    <div class="cs-ocr-lang">
                        <span class="cs-label">Language</span>
                        <Select bind:value={ocrLang} options={langOptions} size="sm" />
                    </div>
                {/if}
                <div class="cs-spacer"></div>
                {#if $busy && phase === 'ocr'}
                    <Button
                        variant="ghost"
                        onclick={cancelOcr}
                        disabled={$ocrCancelRequested}
                        loading={$ocrCancelRequested}
                    >
                        {$ocrCancelRequested ? 'Cancelling' : 'Cancel'}
                    </Button>
                {/if}
                <Button
                    variant="secondary"
                    icon={$busy && phase === 'ocr' ? Loader2 : FileText}
                    onclick={extractText}
                    disabled={$busy || !ocrAvailable}
                    title={ocrAvailable ? '' : 'Tesseract OCR is not available'}
                >
                    {$busy && phase === 'ocr' ? 'Reading…' : 'Extract text'}
                </Button>
                {#if $ocrText}
                    <Button variant="ghost" icon={Copy} onclick={copyText}>Copy</Button>
                {/if}
            </div>
            {#if !ocrAvailable}
                <p class="cs-ocr-note">
                    Text extraction needs Tesseract OCR installed and on your PATH. The scan,
                    flatten, and export steps work without it.
                </p>
            {:else}
                <textarea
                    class="cs-ocr-out"
                    readonly
                    placeholder="Extracted text appears here…"
                    value={$ocrText}
                ></textarea>
            {/if}
        </div>
    {/if}
</ToolPage>

<style>
    .cs-toolbar {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px 10px;
        padding: 0 0 12px;
        border: 0;
        border-bottom: 1px solid var(--color-divider, var(--color-border));
        border-radius: 0;
        background: transparent;
    }
    .cs-spacer {
        flex: 1;
    }
    .cs-label {
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }
    .cs-enhance {
        display: inline-flex;
        align-items: center;
        gap: 8px;
    }
    .cs-seg {
        display: inline-flex;
        padding: 2px;
        gap: 2px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: transparent;
    }
    .cs-seg-btn {
        position: relative;
        height: 28px;
        padding: 0 12px;
        border: none;
        border-radius: calc(var(--radius-control) - 2px);
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;
    }
    .cs-seg-btn:hover:not(:disabled):not(.active) {
        color: var(--color-text);
    }
    .cs-seg-btn.active {
        padding-left: 16px;
        background: var(--color-panel-2);
        color: var(--color-text);
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .cs-seg-btn.active::before {
        content: '';
        position: absolute;
        left: 5px;
        top: 7px;
        bottom: 7px;
        width: 2px;
        border-radius: 999px;
        background: var(--color-accent);
    }
    .cs-seg-btn:disabled {
        cursor: not-allowed;
        opacity: 0.5;
    }

    .cs-grid {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 16px;
        align-items: start;
    }
    @media (max-width: 900px) {
        .cs-grid {
            grid-template-columns: 1fr;
        }
    }
    .cs-pane {
        display: flex;
        flex-direction: column;
        gap: 8px;
        min-width: 0;
    }
    .cs-pane-head {
        display: flex;
        min-height: 26px;
        align-items: baseline;
        justify-content: space-between;
        gap: 8px;
        padding: 0 2px;
    }
    .cs-pane-title {
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }
    .cs-pane-hint {
        font-size: 11.5px;
        color: var(--color-text-secondary);
    }
    .cs-canvas {
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 14px;
        border: 1px solid color-mix(in srgb, var(--color-text) 8%, var(--color-border));
        border-radius: var(--radius-card);
        background: var(--color-panel);
        box-shadow: inset 0 1px 0 color-mix(in srgb, var(--color-text) 4%, transparent);
        overflow: auto;
        max-height: 64vh;
    }
    .cs-stage {
        position: relative;
        display: inline-block;
        line-height: 0;
        user-select: none;
    }
    .cs-img {
        display: block;
        max-width: 100%;
        max-height: 60vh;
        object-fit: contain;
        border-radius: var(--radius-control);
        -webkit-user-drag: none;
        user-select: none;
    }
    .cs-overlay {
        position: absolute;
        inset: 0;
    }
    .cs-quad {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 100%;
        pointer-events: none;
        overflow: visible;
    }
    .cs-quad polygon {
        fill: color-mix(in srgb, var(--color-accent) 12%, transparent);
        stroke: var(--color-accent);
        stroke-width: 2;
    }
    .cs-handle {
        position: absolute;
        width: 18px;
        height: 18px;
        margin: -9px 0 0 -9px;
        padding: 0;
        border: 2px solid var(--color-accent-contrast);
        border-radius: 50%;
        background: var(--color-accent);
        box-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
        cursor: grab;
        touch-action: none;
    }
    .cs-handle:hover,
    .cs-handle.dragging {
        transform: scale(1.18);
        cursor: grabbing;
    }

    .cs-result {
        min-height: 200px;
    }
    .cs-placeholder {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 10px;
        padding: 40px 20px;
        color: var(--color-muted);
        font-size: 13px;
        text-align: center;
    }
    .cs-placeholder :global(.cs-placeholder-icon) {
        width: 26px;
        height: 26px;
        opacity: 0.7;
    }
    :global(.cs-spin) {
        width: 22px;
        height: 22px;
        color: var(--color-accent);
        animation: cs-spin 800ms linear infinite;
    }
    @keyframes cs-spin {
        to {
            transform: rotate(360deg);
        }
    }

    .cs-meta {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
        font-size: 11.5px;
        color: var(--color-muted);
    }
    .cs-meta-name {
        color: var(--color-text-secondary);
        font-weight: 500;
        word-break: break-all;
    }

    .cs-ocr {
        display: flex;
        flex-direction: column;
        gap: 10px;
        padding: 14px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel);
        box-shadow: inset 0 1px 0 color-mix(in srgb, var(--color-text) 4%, transparent);
    }
    .cs-ocr-head {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 10px;
    }
    .cs-ocr-lang {
        display: inline-flex;
        align-items: center;
        gap: 6px;
    }
    .cs-ocr-note {
        margin: 0;
        font-size: 12.5px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }
    .cs-ocr-out {
        width: 100%;
        min-height: 160px;
        padding: 12px;
        background: var(--color-panel-2);
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        color: var(--color-text);
        font-size: 13px;
        line-height: 1.6;
        resize: vertical;
        outline: none;
    }
    .cs-ocr-out:focus {
        border-color: var(--color-accent);
    }
</style>
