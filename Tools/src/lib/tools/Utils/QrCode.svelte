<script lang="ts">
    import { invoke } from '@tauri-apps/api/core';
    import { save } from '@tauri-apps/plugin-dialog';
    import { get } from 'svelte/store';
    import { toast } from '$lib/stores/toasts';
    import {
        qrText,
        qrErrorCorrection,
        qrScale,
        qrMargin,
        qrPngBase64,
        qrSvg,
        qrSize,
        qrError,
        qrGenerating,
        qrGenerationId,
        qrRequestedSignature,
        qrAppliedGenerationId,
        type QrErrorCorrection,
    } from '$lib/stores/qrCode';
    import LoadingState from '$lib/components/LoadingState.svelte';
    import { QrCode, Download, Image as ImageIcon } from '@lucide/svelte';
    import { _ } from 'svelte-i18n';
    import { ToolPage } from '$lib/ui';

    const size = $derived($qrSize);

    type QrGenerationRequest = {
        signature: string;
        value: string;
        errorCorrection: QrErrorCorrection;
        scale: number;
        margin: number;
    };

    function signatureFor(request: Omit<QrGenerationRequest, 'signature'>) {
        return JSON.stringify([request.value, request.errorCorrection, request.scale, request.margin]);
    }

    async function generate(generationId: number, request: QrGenerationRequest) {
        if (get(qrGenerationId) !== generationId || get(qrRequestedSignature) !== request.signature) return;
        qrAppliedGenerationId.set(generationId);
        const { value, errorCorrection, scale, margin } = request;
        if (!value.trim()) {
            qrPngBase64.set('');
            qrSvg.set('');
            qrSize.set(0);
            qrError.set(null);
            qrGenerating.set(false);
            return;
        }
        qrGenerating.set(true);
        qrError.set(null);
        try {
            const result = await invoke<{ png_base64: string; svg: string; size: number }>('generate_qr', {
                text: value,
                errorCorrection,
                scale,
                margin,
            });
            if (get(qrGenerationId) !== generationId || get(qrRequestedSignature) !== request.signature) return;
            qrPngBase64.set(result.png_base64);
            qrSvg.set(result.svg);
            qrSize.set(result.size);
        } catch (e) {
            if (get(qrGenerationId) !== generationId || get(qrRequestedSignature) !== request.signature) return;
            qrError.set(String(e));
            qrPngBase64.set('');
            qrSvg.set('');
            qrSize.set(0);
        } finally {
            if (get(qrGenerationId) === generationId && get(qrRequestedSignature) === request.signature) {
                qrGenerating.set(false);
            }
        }
    }

    // Debounced live regeneration. The request signature is session state, so a remount
    // with unchanged inputs keeps its existing Rust job instead of invalidating it.
    $effect(() => {
        const request = {
            value: $qrText,
            errorCorrection: $qrErrorCorrection,
            scale: $qrScale,
            margin: $qrMargin,
        };
        const signature = signatureFor(request);
        let generationId = get(qrGenerationId);
        if (get(qrRequestedSignature) !== signature) {
            qrGenerationId.update((current) => {
                generationId = current + 1;
                return generationId;
            });
            qrRequestedSignature.set(signature);
        }
        if (get(qrAppliedGenerationId) === generationId) return;

        const timeout = setTimeout(() => void generate(generationId, { ...request, signature }), 200);
        return () => clearTimeout(timeout);
    });

    async function savePng() {
        if (!$qrPngBase64) return;
        const path = await save({
            defaultPath: 'qrcode.png',
            filters: [{ name: 'PNG Image', extensions: ['png'] }],
        });
        if (!path) return;
        try {
            await invoke('save_qr_png', { base64Data: $qrPngBase64, path });
            toast($_('tool.qrCode.pngSaved'), 'success');
        } catch (e) {
            qrError.set(String(e));
            toast($_('tool.qrCode.saveFailed'), 'error');
        }
    }

    async function saveSvg() {
        if (!$qrSvg) return;
        const path = await save({
            defaultPath: 'qrcode.svg',
            filters: [{ name: 'SVG Image', extensions: ['svg'] }],
        });
        if (!path) return;
        try {
            await invoke('save_qr_svg', { svg: $qrSvg, path });
            toast($_('tool.qrCode.svgSaved'), 'success');
        } catch (e) {
            qrError.set(String(e));
            toast($_('tool.qrCode.saveFailed'), 'error');
        }
    }

    /** i18n keys for each error-correction level description. */
    const ecDescriptions = {
        L: 'tool.qrCode.ecLow',
        M: 'tool.qrCode.ecMedium',
        Q: 'tool.qrCode.ecQuartile',
        H: 'tool.qrCode.ecHigh',
    };
</script>

<ToolPage
    icon={QrCode}
    iconTint="#f59e0b"
    title={$_('tool.qrCode.heroTitle')}
    description={$_('tool.qrCode.heroDescription')}
    width="wide"
    fill={false}
>

    <!-- Toolbar -->
    <div class="qr-toolbar flex flex-wrap items-center gap-3 rounded-2xl border border-border bg-panel p-3 md:p-4">
        <div class="flex-1"></div>
        <button
            onclick={saveSvg}
            disabled={!$qrSvg}
            class="qr-action inline-flex h-10 items-center gap-1.5 rounded-xl border border-border bg-panel-2 px-3 text-sm hover:border-accent/70 disabled:opacity-40"
        >
            <Download class="h-4 w-4" /> {$_('tool.qrCode.saveSvg')}
        </button>
        <button
            onclick={savePng}
            disabled={!$qrPngBase64}
            class="qr-primary inline-flex h-10 items-center gap-1.5 rounded-xl bg-accent px-4 text-sm font-medium text-accent-contrast hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed"
        >
            <Download class="h-4 w-4" /> {$_('tool.qrCode.savePng')}
        </button>
    </div>

    <!-- Work area: controls + preview -->
    <div class="qr-workspace rounded-2xl border border-border bg-panel p-4 md:p-5">
        <div class="qr-grid grid gap-6 lg:grid-cols-2">
            <!-- Left: controls -->
            <div class="qr-config space-y-4">
                <div>
                    <label for="qr-content" class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">
                        {$_('tool.qrCode.content')}
                    </label>
                    <textarea
                            id="qr-content"
                            bind:value={$qrText}
                            placeholder={$_('tool.qrCode.contentPlaceholder')}
                            class="qr-input w-full h-32 bg-panel-2 border border-border rounded-lg p-3 font-mono text-sm
                 text-text placeholder:text-muted resize-none
                 focus:outline-none focus:border-accent transition-colors"
                    ></textarea>
                    <div class="text-xs text-muted mt-1">{$_('tool.qrCode.chars', { values: { count: $qrText.length } })}</div>
                </div>

                <div>
                    <span id="qr-error-correction-label" class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">
                        {$_('tool.qrCode.errorCorrection')}
                    </span>
                    <div class="qr-ec-options flex gap-2" role="group" aria-labelledby="qr-error-correction-label">
                        {#each ['L', 'M', 'Q', 'H'] as level}
                            <button type="button" class:is-active={$qrErrorCorrection === level} aria-pressed={$qrErrorCorrection === level}
                                    class="qr-ec-button flex-1 px-3 py-2 rounded-lg text-sm font-medium transition-colors"
                                    onclick={() => ($qrErrorCorrection = level as QrErrorCorrection)}
                            >
                                {level}
                            </button>
                        {/each}
                    </div>
                    <div class="text-xs text-muted mt-2">{$_(ecDescriptions[$qrErrorCorrection])}</div>
                </div>

                <div>
                    <label for="qr-scale" class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">
                        {$_('tool.qrCode.scale', { values: { count: $qrScale } })}
                    </label>
                    <input
                            id="qr-scale"
                            type="range"
                            min="2"
                            max="20"
                            bind:value={$qrScale}
                            class="qr-range w-full"
                    />
                </div>

                <div>
                    <span class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">
                        {$_('tool.qrCode.quietZone', { values: { state: $qrMargin > 0 ? $_('tool.qrCode.on') : $_('tool.qrCode.off') } })}
                    </span>
                    <button type="button" class:is-active={$qrMargin > 0} aria-pressed={$qrMargin > 0}
                            class="qr-toggle px-3 py-1.5 rounded-lg text-sm transition-colors"
                            onclick={() => ($qrMargin = $qrMargin > 0 ? 0 : 2)}
                    >
                        {$qrMargin > 0 ? $_('tool.qrCode.enabled') : $_('tool.qrCode.disabled')}
                    </button>
                </div>
            </div>

            <!-- Right: preview -->
            <div class="qr-preview">
                <span class="block text-xs uppercase tracking-wider text-muted font-semibold mb-2">
                    {$_('tool.qrCode.preview')}
                </span>
                <div class="qr-preview-canvas bg-panel-2 border border-border rounded-xl p-6 flex items-center justify-center min-h-[400px]">
                    {#if $qrGenerating}
                        <LoadingState variant="block" label={$_('tool.qrCode.generating')} />
                    {:else if $qrPngBase64}
                        <div class="space-y-3 text-center">
                            <div class="qr-image-shell bg-white p-4 rounded inline-block">
                                <img
                                        src="data:image/png;base64,{$qrPngBase64}"
                                        alt="QR Code"
                                        class="qr-image block"
                                        style="image-rendering: pixelated; max-width: 100%;"
                                />
                            </div>
                            <div class="text-xs text-muted">{size}×{size}px</div>
                        </div>
                    {:else if $qrError}
                        <div class="text-error text-sm text-center">{$qrError}</div>
                    {:else}
                        <div class="text-muted text-sm flex flex-col items-center gap-2">
                            <ImageIcon class="h-8 w-8 opacity-60" />
                            {$_('tool.qrCode.enterContent')}
                        </div>
                    {/if}
                </div>
            </div>
        </div>
    </div>
</ToolPage>
<style>
    .qr-toolbar {
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
    .qr-toolbar > :global(.flex-1) { flex: 1; }
    .qr-action, .qr-primary {
        min-height: 34px;
        padding: 0 11px;
        border-radius: var(--radius-control, 8px);
        font-size: 12px;
    }
    .qr-action { background: transparent; color: var(--color-text-secondary); }
    .qr-action:hover:not(:disabled) { border-color: color-mix(in srgb, var(--color-text) 24%, var(--color-border)); color: var(--color-text); }
    .qr-primary { border-color: var(--color-accent); background: var(--color-accent); color: var(--color-accent-contrast); }
    .qr-primary:hover:not(:disabled) { background: var(--color-accent-hover, var(--color-accent)); }
    .qr-action:focus-visible, .qr-primary:focus-visible, .qr-input:focus-visible, .qr-ec-button:focus-visible,
    .qr-toggle:focus-visible, .qr-range:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }

    .qr-workspace { padding: 4px 0 0; border: 0; border-radius: 0; background: transparent; }
    .qr-grid { gap: 12px; }
    .qr-config, .qr-preview {
        min-width: 0;
        padding: 14px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card, 12px);
        background: var(--color-panel);
    }
    .qr-config :global(.mb-2), .qr-preview :global(.mb-2) { margin-bottom: 10px; }
    .qr-input {
        box-sizing: border-box;
        min-height: 9rem;
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, Consolas, monospace);
        font-size: 12px;
        line-height: 1.55;
    }
    .qr-ec-options { padding: 3px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); background: var(--color-panel); }
    .qr-ec-button {
        position: relative;
        min-height: 32px;
        padding: 0 10px 0 15px;
        border: 0;
        border-radius: calc(var(--radius-control, 8px) - 2px);
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12px;
        cursor: pointer;
    }
    .qr-ec-button:hover { color: var(--color-text); }
    .qr-ec-button.is-active { background: var(--color-panel-2); color: var(--color-text); }
    .qr-ec-button.is-active::before {
        position: absolute;
        top: 7px;
        bottom: 7px;
        left: 5px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }
    .qr-range { accent-color: var(--color-accent); }
    .qr-toggle {
        position: relative;
        min-height: 32px;
        padding: 0 11px 0 16px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control, 8px);
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12px;
        cursor: pointer;
    }
    .qr-toggle:hover { color: var(--color-text); }
    .qr-toggle.is-active { background: var(--color-panel-2); color: var(--color-text); }
    .qr-toggle.is-active::before {
        position: absolute;
        top: 6px;
        bottom: 6px;
        left: 5px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }
    .qr-preview { display: flex; flex-direction: column; }
    .qr-preview-canvas {
        display: flex;
        flex: 1;
        align-items: center;
        justify-content: center;
        min-height: 22rem;
        padding: 20px;
        border-radius: var(--radius-control, 8px);
        background: var(--color-panel-2);
    }
    .qr-image-shell { padding: 14px; border: 1px solid var(--color-border); border-radius: var(--radius-control, 8px); box-shadow: 0 12px 30px color-mix(in srgb, var(--color-text) 8%, transparent); }
    .qr-image { image-rendering: pixelated; max-width: 100%; }
    @media (max-width: 700px) {
        .qr-toolbar > :global(.flex-1) { display: none; }
        .qr-toolbar { justify-content: flex-end; }
    }
</style>
