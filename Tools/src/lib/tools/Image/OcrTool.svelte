<script lang="ts">
    /*
      Image → Text (OCR) — fully on-device via Tesseract (commands/ocr.rs).
      Pick or drop an image, choose a language, run OCR, copy the text.
      Nothing touches the network.

      UI strings are plain English for now; the Phase 7 localization pass sweeps
      them into i18n keys (en + ka) like the rest of the app.
    */
    import { onMount } from 'svelte';
    import { invoke, convertFileSrc } from '@tauri-apps/api/core';
    import { open } from '@tauri-apps/plugin-dialog';
    import { ScanText, ImageUp, Copy, Loader2, FileWarning } from '@lucide/svelte';
    import DropZone from '$lib/DropZone.svelte';
    import { ToolPage, Button, Select } from '$lib/ui';
    import { toast } from '$lib/stores/toasts';
    import {
        ocrImagePath,
        ocrResultText,
        ocrLang,
        ocrPreprocess,
        ocrUniformBlock,
        ocrRunning,
        ocrCancelRequested,
        ocrCurrentOperationId,
        ocrError,
    } from '$lib/stores/ocrTool';
    import { subscribeToToolLaunchTarget } from '$lib/stores/toolLaunchTarget';

    // Images only — scanned PDFs are handled by the PDF → Word/Markdown converter.
    const IMAGE_EXTS = ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'tif', 'tiff', 'gif'];

    // Availability/language probe + run flags stay transient (re-probed on mount).
    let available = $state(true);
    let checked = $state(false);
    let languages = $state<string[]>([]);
    // Persisted across navigation via module stores (see stores/ocrTool): the
    // picked image, the recognized text, and the user's options survive nav.
    const lang = ocrLang;
    const imagePath = ocrImagePath;
    const resultText = ocrResultText;
    const preprocess = ocrPreprocess;
    // Page-segmentation: off = Tesseract auto (PSM 3); on = single uniform block
    // (PSM 6), which keeps numbered lists intact when auto splits them.
    const uniformBlock = ocrUniformBlock;
    const runningStore = ocrRunning;
    let running = $derived($runningStore);
    const cancelRequested = ocrCancelRequested;
    const currentOcrOpId = ocrCurrentOperationId;
    const error = ocrError;
    // Bumped on every new image so the asset URL is unique — defeats the
    // WebView2 image cache (a fresh pick kept showing the previous image).
    let previewToken = $state(0);

    onMount(() => {
        const stopTarget = subscribeToToolLaunchTarget('ocr-image-to-text', ({ targetFile }) => {
            setImage(targetFile);
        });
        void (async () => {
            try {
                available = await invoke<boolean>('ocr_available');
                if (available) {
                    languages = await invoke<string[]>('ocr_languages');
                    if (languages.length && !languages.includes($lang)) {
                        lang.set(languages.includes('eng') ? 'eng' : languages[0]);
                    }
                }
            } catch {
                available = false;
            } finally {
                checked = true;
            }
        })();
        return stopTarget;
    });

    let langOptions = $derived(
        (languages.length ? languages : ['eng']).map((value) => ({ value, label: value })),
    );

    // Cache-busted asset URL so each picked image actually re-loads.
    let previewSrc = $derived(
        $imagePath ? `${convertFileSrc($imagePath)}?v=${previewToken}` : '',
    );

    function setImage(path: string | null) {
        if (running) return;
        imagePath.set(path);
        previewToken += 1;
        resultText.set('');
        error.set(null);
    }

    async function pickImage() {
        const selected = await open({
            multiple: false,
            directory: false,
            filters: [{ name: 'Images', extensions: IMAGE_EXTS }],
        });
        if (typeof selected === 'string') setImage(selected);
    }

    function onDropFiles(paths: string[]) {
        if (paths[0]) setImage(paths[0]);
    }

    function basename(path: string): string {
        return path.split(/[\\/]/).pop() ?? path;
    }

    // Wave 2.5b — OCR cancel via subprocess-kill (the natural shape for
    // a single-process operation like tesseract). The operationId is
    // generated per run; cancel calls invoke('cancel_ocr_operation') with
    // that id, which kills the running tesseract Child on the backend.

    async function runOcr() {
        if (!$imagePath || running) return;
        runningStore.set(true);
        cancelRequested.set(false);
        error.set(null);
        resultText.set('');
        const opId = `ocr-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
        currentOcrOpId.set(opId);
        try {
            const psm = $uniformBlock ? 6 : undefined;
            resultText.set(await invoke<string>('ocr_image', {
                path: $imagePath,
                langs: $lang,
                preprocess: $preprocess,
                psm,
                operationId: opId,
            }));
            if (!$resultText.trim()) {
                toast('No text was recognized — try a clearer scan or a different language.', 'info', 4500);
            }
        } catch (e) {
            const errStr = String(e);
            // Distinguish user-cancel from genuine failure so the UI
            // doesn't yell at the user for cancelling.
            if (errStr.includes('Cancelled')) {
                error.set(null);
                toast('OCR cancelled', 'info', 2500);
            } else {
                error.set(errStr);
            }
        } finally {
            if ($currentOcrOpId === opId) {
                runningStore.set(false);
                cancelRequested.set(false);
                currentOcrOpId.set(null);
            }
        }
    }

    async function cancelOcr() {
        if (!$currentOcrOpId || $cancelRequested) return;
        const operationId = $currentOcrOpId;
        cancelRequested.set(true);
        try {
            await invoke('cancel_ocr_operation', { operationId });
        } catch {
            // Best-effort — if cancel call itself fails, the worst case
            // is the in-flight tesseract subprocess finishes normally.
        }
    }

    async function copyResult() {
        if (!$resultText) return;
        try {
            await navigator.clipboard.writeText($resultText);
            toast('Copied to clipboard.', 'success', 2000);
        } catch {
            toast('Could not copy to clipboard.', 'error', 3000);
        }
    }
</script>

<ToolPage
    icon={ScanText}
    iconTint="#06b6d4"
    title="Image to Text (OCR)"
    description="Extract text from images on-device with Tesseract. Pick a language, then run — nothing is uploaded."
    width="wide"
>
    {#if checked && !available}
        <div class="ocr-missing">
            <FileWarning size={18} />
            <div>
                <strong>Tesseract OCR isn't installed.</strong>
                Install Tesseract 5.x (with the language packs you need, e.g. Georgian)
                and make sure it's on your PATH, then reopen this tool.
            </div>
        </div>
    {:else}
        <div class="ocr-grid ocr-workspace">
            <!-- Left: input -->
            <div class="ocr-col ocr-source">
                <DropZone onFiles={onDropFiles} accept={IMAGE_EXTS}>
                    {#if $imagePath}
                        <div class="ocr-preview">
                            {#key previewToken}
                                <img src={previewSrc} alt={basename($imagePath)} />
                            {/key}
                            <div class="ocr-preview-name">{basename($imagePath)}</div>
                        </div>
                    {:else}
                        <div class="ocr-drop-hint">
                            <ImageUp size={26} />
                            <span>Drop an image here, or choose one below</span>
                        </div>
                    {/if}
                </DropZone>

                <div class="ocr-controls">
                    <Button variant="secondary" icon={ImageUp} onclick={pickImage} disabled={running}>
                        Choose image…
                    </Button>
                    <div class="ocr-lang">
                        <span class="ocr-lang-label">Language</span>
                        <Select bind:value={$lang} options={langOptions} size="sm" />
                    </div>
                    <label class="ocr-pre" title="Upscale + clean the image before OCR (helps thin glyphs like წ / ნ)">
                        <input type="checkbox" bind:checked={$preprocess} />
                        Enhance image
                    </label>
                    <label class="ocr-pre" title="Treat the whole image as one paragraph — keeps numbered lists together when auto-layout would otherwise split them">
                        <input type="checkbox" bind:checked={$uniformBlock} />
                        Single paragraph
                    </label>
                    {#if running}
                        <Button
                            variant="ghost"
                            onclick={cancelOcr}
                            disabled={$cancelRequested}
                            loading={$cancelRequested}
                        >
                            {$cancelRequested ? 'Cancelling' : 'Cancel'}
                        </Button>
                    {/if}
                    <Button
                        variant="primary"
                        icon={running ? Loader2 : ScanText}
                        onclick={runOcr}
                        disabled={!$imagePath || running}
                    >
                        {running ? 'Reading…' : 'Run OCR'}
                    </Button>
                </div>
            </div>

            <!-- Right: output -->
            <div class="ocr-col ocr-result">
                <div class="ocr-out-head">
                    <span class="ocr-out-title">Recognized text</span>
                    {#if $resultText}
                        <Button variant="ghost" icon={Copy} onclick={copyResult}>Copy</Button>
                    {/if}
                </div>
                {#if $error}
                    <div class="ocr-error">{$error}</div>
                {:else}
                    <textarea
                        class="ocr-out"
                        readonly
                        placeholder="The recognized text appears here…"
                        value={$resultText}
                    ></textarea>
                {/if}
            </div>
        </div>
    {/if}
</ToolPage>

<style>
    .ocr-grid {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 16px;
        align-items: start;
    }
    @media (max-width: 760px) {
        .ocr-grid {
            grid-template-columns: 1fr;
        }
    }
    .ocr-col {
        display: flex;
        flex-direction: column;
        gap: 12px;
        min-width: 0;
    }

    .ocr-drop-hint {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 8px;
        padding: 28px 16px;
        color: var(--color-muted);
        font-size: 13px;
        text-align: center;
    }
    .ocr-preview {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 8px;
        padding: 12px;
    }
    .ocr-preview img {
        max-width: 100%;
        max-height: 220px;
        object-fit: contain;
        border-radius: var(--radius-control);
        border: 1px solid var(--color-border);
    }
    .ocr-preview-name {
        font-size: 12px;
        color: var(--color-text-secondary);
        word-break: break-all;
        text-align: center;
    }

    .ocr-controls {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 10px;
    }
    .ocr-lang {
        display: flex;
        align-items: center;
        gap: 6px;
    }
    .ocr-lang-label {
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }
    .ocr-pre {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-size: 12px;
        color: var(--color-text-secondary);
        cursor: pointer;
        user-select: none;
    }
    .ocr-pre input {
        accent-color: var(--color-accent);
        cursor: pointer;
    }

    .ocr-out-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        min-height: 30px;
    }
    .ocr-out-title {
        font-size: 11px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }
    .ocr-out {
        width: 100%;
        min-height: 320px;
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
    .ocr-out:focus {
        border-color: var(--color-accent);
    }
    .ocr-error {
        padding: 12px;
        background: color-mix(in srgb, var(--color-error) 10%, transparent);
        border: 1px solid color-mix(in srgb, var(--color-error) 35%, var(--color-border));
        border-radius: var(--radius-control);
        color: var(--color-error);
        font-size: 13px;
        white-space: pre-wrap;
    }

    .ocr-missing {
        display: flex;
        align-items: flex-start;
        gap: 10px;
        padding: 14px 16px;
        background: color-mix(in srgb, var(--color-accent) 8%, var(--color-panel-2));
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        color: var(--color-text-secondary);
        font-size: 13px;
        line-height: 1.5;
    }

    .ocr-workspace {
        grid-template-columns: minmax(230px, 0.72fr) minmax(0, 1.28fr);
        gap: 12px;
        align-items: stretch;
    }

    .ocr-source,
    .ocr-result {
        padding: 14px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel);
        box-shadow: inset 0 1px 0 color-mix(in srgb, var(--color-text) 4%, transparent);
    }

    .ocr-source {
        align-self: start;
    }

    .ocr-preview,
    .ocr-drop-hint {
        min-height: 252px;
        border: 1px solid color-mix(in srgb, var(--color-text) 8%, var(--color-border));
        border-radius: var(--radius-control);
        background: color-mix(in srgb, var(--color-panel-2) 92%, transparent);
    }

    .ocr-preview img {
        max-height: 260px;
        border-color: color-mix(in srgb, var(--color-text) 8%, var(--color-border));
    }

    .ocr-controls {
        gap: 8px 10px;
        padding-top: 12px;
        border-top: 1px solid var(--color-divider, var(--color-border));
    }

    .ocr-result {
        min-height: 100%;
    }

    .ocr-out-head {
        padding: 0 2px;
    }

    .ocr-out {
        flex: 1;
        min-height: 390px;
        border-color: color-mix(in srgb, var(--color-text) 8%, var(--color-border));
        background: var(--color-panel-2);
    }

    @media (min-width: 761px) {
        .ocr-source {
            position: sticky;
            top: 0;
        }
    }

    @media (max-width: 760px) {
        .ocr-workspace {
            grid-template-columns: 1fr;
        }

        .ocr-source {
            position: static;
        }

        .ocr-out {
            min-height: 280px;
        }
    }
</style>
