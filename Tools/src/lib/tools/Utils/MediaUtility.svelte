<script lang="ts">
    import { onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { listen } from '@tauri-apps/api/event';
    import { open, save } from '@tauri-apps/plugin-dialog';
    import { Download, FileVideo, FolderOpen, FolderOutput, Gauge, Music2, RefreshCw, Video } from '@lucide/svelte';
    import DropZone from '$lib/DropZone.svelte';
    import ToolCancelButton from '$lib/components/ToolCancelButton.svelte';
    import {
        cancelMediaOperation,
        mediaCompletedPath,
        mediaCancelling,
        mediaError,
        mediaInputPath,
        mediaMode,
        mediaOutputPath,
        mediaProcessing,
        mediaProgress,
        mediaTargetMegabytes,
        initMediaUtility,
        runMediaOperation,
        setMediaInput,
        setMediaMode,
        setMediaOutput,
    } from '$lib/stores/mediaUtility';
    import { Button, ToolPage, ToolPanel } from '$lib/ui';

    type FfmpegStatus = {
        available: boolean;
        recorderAvailable: boolean;
        ffprobeAvailable: boolean;
        version: string | null;
    };

    const videoExtensions = ['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v'];
    const presets = [10, 25, 50];

    let status = $state<FfmpegStatus | null>(null);
    let checked = $state(false);
    let checking = $state(false);
    let locating = $state(false);
    let setupError = $state<string | null>(null);

    let compressionReady = $derived(Boolean(status?.available && status.ffprobeAvailable && status.recorderAvailable));
    let canRun = $derived(Boolean($mediaInputPath && $mediaOutputPath && !$mediaProcessing && status?.available && ($mediaMode === 'extract' || compressionReady)));

    async function refreshFfmpeg(): Promise<void> {
        checking = true;
        try {
            status = await invoke<FfmpegStatus>('ffmpeg_status');
            setupError = null;
        } catch (cause) {
            status = { available: false, recorderAvailable: false, ffprobeAvailable: false, version: null };
            setupError = String(cause);
        } finally {
            checked = true;
            checking = false;
        }
    }

    onMount(() => {
        void initMediaUtility();
        void refreshFfmpeg();
        // The pack installed (or removed) in Settings while this page is open.
        const stop = listen<{ id: string }>('packs-changed', ({ payload }) => {
            if (payload?.id === 'ffmpeg') void refreshFfmpeg();
        });
        return () => void stop.then((unlisten) => unlisten());
    });

    function getPack(): void {
        void invoke('host:packs.open').catch((cause) => (setupError = String(cause)));
    }

    async function locateFfmpeg(): Promise<void> {
        try {
            const picked = await open({
                title: 'Locate ffmpeg.exe',
                multiple: false,
                directory: false,
                filters: [{ name: 'FFmpeg', extensions: ['exe'] }],
            });
            if (typeof picked !== 'string') return;
            locating = true;
            setupError = null;
            status = await invoke<FfmpegStatus>('ffmpeg_set_path', { path: picked });
        } catch (cause) {
            const message = String(cause);
            setupError = message.includes('no_ffmpeg')
                ? 'That folder does not contain ffmpeg.exe.'
                : message.includes('not_runnable')
                  ? 'Search could not run that ffmpeg.exe.'
                  : message;
        } finally {
            locating = false;
            checked = true;
        }
    }

    function basename(path: string): string {
        return path.split(/[\\/]/).pop() ?? path;
    }

    function outputName(path: string): string {
        const name = basename(path).replace(/\.[^.]+$/, '') || 'media';
        return $mediaMode === 'extract' ? `${name}.mp3` : `${name}-compressed.mp4`;
    }

    async function pickVideo(): Promise<void> {
        const picked = await open({
            title: 'Choose a video',
            multiple: false,
            filters: [{ name: 'Videos', extensions: videoExtensions }],
        });
        if (typeof picked === 'string') setMediaInput(picked);
    }

    async function chooseOutput(): Promise<void> {
        if (!$mediaInputPath) return;
        const path = await save({
            title: $mediaMode === 'extract' ? 'Save MP3 as' : 'Save compressed video as',
            defaultPath: outputName($mediaInputPath),
            filters: [$mediaMode === 'extract'
                ? { name: 'MP3 Audio', extensions: ['mp3'] }
                : { name: 'MP4 Video', extensions: ['mp4'] }],
        });
        if (path) setMediaOutput(path);
    }

    function formatDuration(seconds: number): string {
        const rounded = Math.max(0, Math.floor(seconds));
        const minutes = Math.floor(rounded / 60);
        const remaining = rounded % 60;
        return `${minutes}:${remaining.toString().padStart(2, '0')}`;
    }
</script>

<ToolPage
    icon={Video}
    iconTint="#f97316"
    title="Media Utility"
    description="Extract an MP3 or make a video small enough to share, entirely on your device."
    width="medium"
    fill={false}
>
    {#snippet actions()}
        <ToolCancelButton running={$mediaProcessing} cancelling={$mediaCancelling} onCancel={cancelMediaOperation} label="Cancel" />
    {/snippet}

    {#if checked && !status?.available}
        <ToolPanel as="section" padding="md" tone="panel-2">
            <div class="notice" role="status">
                <FileVideo aria-hidden="true" />
                <div>
                    <strong>Media Utility runs on the FFmpeg pack.</strong>
                    <p>Add it in Settings › Packs: a one-time download from its publisher. Your media stays on this device.</p>
                    {#if setupError}<p class="setup-error">{setupError}</p>{/if}
                    <div class="notice-actions">
                        <Button variant="primary" size="sm" icon={Download} onclick={getPack}>Get the FFmpeg pack</Button>
                        <Button variant="secondary" size="sm" icon={FolderOpen} loading={locating} onclick={() => void locateFfmpeg()}>
                            Use my own ffmpeg.exe
                        </Button>
                        <Button variant="secondary" size="sm" icon={RefreshCw} loading={checking} onclick={() => void refreshFfmpeg()}>
                            Re-check
                        </Button>
                    </div>
                </div>
            </div>
        </ToolPanel>
    {:else if checked && $mediaMode === 'compress' && !compressionReady}
        <ToolPanel as="section" padding="md" tone="panel-2">
            <div class="notice" role="status">
                <Gauge aria-hidden="true" />
                <div>
                    <strong>Compression needs ffprobe and Windows H.264 (h264_mf).</strong>
                    <p>Choose a full FFmpeg build with ffprobe, then reopen this tool. MP3 extraction remains available.</p>
                </div>
            </div>
        </ToolPanel>
    {/if}

    <div class="media-layout" class:is-disabled={checked && !status?.available}>
        <ToolPanel as="section" padding="md">
            <span class="eyebrow">1. Source video</span>
            <DropZone onFiles={(paths) => setMediaInput(paths[0] ?? null)} accept={videoExtensions}>
                {#snippet children()}
                    <button class="source-drop" type="button" onclick={() => void pickVideo()} disabled={$mediaProcessing}>
                        <FileVideo size={26} aria-hidden="true" />
                        {#if $mediaInputPath}
                            <span class="source-name">{basename($mediaInputPath)}</span>
                            <span class="source-path" title={$mediaInputPath}>{$mediaInputPath}</span>
                            <span class="source-action">Choose another video</span>
                        {:else}
                            <span class="source-name">Drop a video here, or choose one</span>
                            <span class="source-path">MP4, MOV, MKV, AVI, WebM, or M4V</span>
                        {/if}
                    </button>
                {/snippet}
            </DropZone>
        </ToolPanel>

        <ToolPanel as="section" padding="md">
            <span class="eyebrow">2. What do you need?</span>
            <div class="mode-grid" role="group" aria-label="Media operation">
                <button type="button" class:active={$mediaMode === 'extract'} onclick={() => setMediaMode('extract')} disabled={$mediaProcessing}>
                    <Music2 size={18} aria-hidden="true" />
                    <span><strong>Extract MP3</strong><small>Keep just the audio</small></span>
                </button>
                <button type="button" class:active={$mediaMode === 'compress'} onclick={() => setMediaMode('compress')} disabled={$mediaProcessing}>
                    <Gauge size={18} aria-hidden="true" />
                    <span><strong>Compress video</strong><small>Choose the output size you need</small></span>
                </button>
            </div>

            {#if $mediaMode === 'compress'}
                <div class="preset-row">
                    <span class="preset-label">Target size</span>
                    <div class="presets" role="group" aria-label="Target video size">
                        {#each presets as preset}
                            <button type="button" class:active={$mediaTargetMegabytes === preset} onclick={() => mediaTargetMegabytes.set(preset)} disabled={$mediaProcessing} aria-pressed={$mediaTargetMegabytes === preset}>
                                Discord {preset} MB
                            </button>
                        {/each}
                    </div>
                    <label class="custom-target">
                        <span>Custom</span>
                        <input
                            type="number"
                            min="1"
                            step="1"
                            bind:value={$mediaTargetMegabytes}
                            disabled={$mediaProcessing}
                            aria-label="Custom target video size in megabytes"
                        />
                        <span>MB</span>
                    </label>
                </div>
            {/if}
        </ToolPanel>

        <ToolPanel as="section" padding="md">
            <div class="output-head">
                <span class="eyebrow">3. Save destination</span>
                <Button variant="secondary" size="sm" icon={FolderOutput} onclick={() => void chooseOutput()} disabled={!$mediaInputPath || $mediaProcessing}>
                    Choose location
                </Button>
            </div>
            <p class:placeholder={!$mediaOutputPath} class="destination" title={$mediaOutputPath || undefined}>
                {$mediaOutputPath || 'Choose where to save the finished file.'}
            </p>
        </ToolPanel>

        {#if $mediaError}
            <div class="error" role="alert">{$mediaError}</div>
        {/if}
        {#if $mediaCompletedPath}
            <div class="success" role="status">Finished and saved to {$mediaCompletedPath}</div>
        {/if}

        <div class="run-row">
            {#if checked && status?.version}
                <span class="ffmpeg-version" title={status.version}>Ready: {status.version}</span>
            {/if}
            <Button variant="primary" icon={$mediaMode === 'extract' ? Music2 : Gauge} loading={$mediaProcessing} onclick={() => void runMediaOperation()} disabled={!canRun}>
                {$mediaProcessing ? 'Processing…' : $mediaMode === 'extract' ? 'Extract MP3' : `Compress to ${$mediaTargetMegabytes} MB`}
            </Button>
        </div>
        {#if $mediaProcessing}
            <div class="run-status" role="status">
                {#if $mediaCancelling}
                    <span>Cancelling media operation…</span>
                {:else if $mediaMode === 'compress' && $mediaProgress?.progress !== null && $mediaProgress?.progress !== undefined}
                    <div class="progress-copy"><span>Compressing video · {$mediaProgress.progress}%</span>{#if $mediaProgress.duration_seconds !== null}<span>Source timeline: {formatDuration($mediaProgress.processed_seconds)} / {formatDuration($mediaProgress.duration_seconds)}</span>{/if}</div>
                    <div class="progress-track" aria-label={`Compression progress: ${$mediaProgress.progress}%`}><i style={`width: ${$mediaProgress.progress}%`}></i></div>
                {:else}
                    <span>{$mediaMode === 'extract' ? 'Extracting audio…' : 'Preparing video compression…'} You can switch tools and return here.</span>
                {/if}
            </div>
        {/if}
    </div>
</ToolPage>

<style>
    .media-layout { display: grid; gap: 12px; }
    .media-layout.is-disabled { opacity: .5; pointer-events: none; }
    .eyebrow { display: block; margin-bottom: 10px; color: var(--color-text-secondary); font-size: 11px; font-weight: 600; letter-spacing: .06em; text-transform: uppercase; }
    .notice { display: flex; gap: 10px; color: var(--color-text-secondary); font-size: 13px; line-height: 1.5; }
    .notice :global(svg) { flex: 0 0 auto; color: var(--color-accent); }
    .notice strong { color: var(--color-text); }
    .notice p { margin: 2px 0 0; }
    .notice-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
    .setup-error { color: var(--color-error); }
    .source-drop { display: grid; width: 100%; min-height: 132px; place-content: center; gap: 5px; padding: 20px; border: 1px dashed var(--color-border-strong); border-radius: var(--radius-control); background: var(--color-panel-2); color: var(--color-text-secondary); text-align: center; cursor: pointer; }
    .source-drop:not(:disabled):hover { border-color: var(--color-accent); color: var(--color-text); }
    .source-drop:focus-visible, .mode-grid button:focus-visible, .presets button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
    .source-drop :global(svg) { justify-self: center; color: var(--color-accent); }
    .source-name { max-width: 100%; overflow: hidden; color: var(--color-text); font-size: 14px; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
    .source-path, .source-action { max-width: 100%; overflow: hidden; font-size: 12px; text-overflow: ellipsis; white-space: nowrap; }
    .source-action { margin-top: 4px; color: var(--color-accent); }
    .mode-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
    .mode-grid button { display: flex; align-items: center; gap: 10px; padding: 12px; border: 1px solid var(--color-border); border-radius: var(--radius-control); background: var(--color-panel-2); color: var(--color-text-secondary); text-align: left; }
    .mode-grid button:not(:disabled):hover { border-color: var(--color-border-strong); color: var(--color-text); }
    .mode-grid button.active { border-color: var(--color-accent); background: color-mix(in srgb, var(--color-accent) 10%, var(--color-panel-2)); color: var(--color-text); }
    .mode-grid :global(svg) { flex: 0 0 auto; color: var(--color-accent); }
    .mode-grid span { display: grid; gap: 2px; }
    .mode-grid strong { font-size: 13px; }
    .mode-grid small { color: var(--color-text-secondary); font-size: 11px; }
    .preset-row { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin-top: 14px; }
    .preset-label { color: var(--color-text-secondary); font-size: 12px; }
    .presets { display: flex; flex-wrap: wrap; gap: 6px; }
    .presets button { padding: 5px 8px; border: 1px solid var(--color-border); border-radius: var(--radius-control); background: transparent; color: var(--color-text-secondary); font-size: 12px; }
    .presets button.active { border-color: var(--color-accent); background: color-mix(in srgb, var(--color-accent) 10%, transparent); color: var(--color-text); }
    .custom-target { display: inline-flex; align-items: center; gap: 5px; color: var(--color-text-secondary); font-size: 12px; }
    .custom-target input { width: 78px; min-height: 28px; padding: 0 7px; border: 1px solid var(--color-border); border-radius: var(--radius-control); background: var(--color-panel-2); color: var(--color-text); font: inherit; }
    .custom-target input:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
    .output-head, .run-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
    .output-head .eyebrow { margin: 0; }
    .destination { margin: 10px 0 0; overflow: hidden; color: var(--color-text); font-family: var(--font-mono, ui-monospace, monospace); font-size: 12px; text-overflow: ellipsis; white-space: nowrap; }
    .destination.placeholder { color: var(--color-text-secondary); font-family: inherit; }
    .error, .success { padding: 10px 12px; border-left: 2px solid; font-size: 13px; line-height: 1.45; white-space: pre-wrap; }
    .error { border-color: var(--color-error); color: var(--color-error); }
    .success { border-color: var(--color-success, var(--color-accent)); color: var(--color-text-secondary); }
    .ffmpeg-version { min-width: 0; overflow: hidden; color: var(--color-text-secondary); font-size: 11px; text-overflow: ellipsis; white-space: nowrap; }
    .run-status { display: grid; gap: 7px; margin: -4px 0 0; color: var(--color-text-secondary); font-size: 12px; }
    .progress-copy { display: flex; justify-content: space-between; gap: 10px; }
    .progress-track { height: 3px; overflow: hidden; border-radius: 999px; background: var(--color-border); }
    .progress-track i { display: block; height: 100%; border-radius: inherit; background: var(--color-accent); transition: width var(--dur-micro) var(--ease-out); }
    @media (max-width: 560px) { .mode-grid { grid-template-columns: 1fr; } .run-row { align-items: flex-end; } .ffmpeg-version { display: none; } }
    @media (prefers-reduced-motion: reduce) { .source-drop, .mode-grid button { transition: none; } }
</style>
