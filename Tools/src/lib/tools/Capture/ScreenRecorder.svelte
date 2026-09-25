<script lang="ts">
    /*
      Screen Recorder — record the primary monitor to a clean MP4, fully
      on-device. The recording runs in the Rust backend (WGC capture →
      ffmpeg h264_mf → fragmented MP4); this surface starts/stops it and
      mirrors live status. Recording state lives in stores/screenRecorder.ts
      so it survives the workspace {#key} nav-unmount — navigate away and the
      capture keeps running; return and the in-progress session reattaches.

      This is the full-screen path. Region select + the floating
      content-protected capture toolbar + live move/resize land next.
    */
    import { onMount, onDestroy } from 'svelte';
    import { get, writable } from 'svelte/store';
    import { invoke } from '@tauri-apps/api/core';
    import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event';
    import { getCurrentWindow } from '@tauri-apps/api/window';
    import { open } from '@tauri-apps/plugin-dialog';
    import {
        startRecording,
        stopRecording,
        togglePause,
        exportLastGif,
    } from '$lib/stores/screenRecorderControl';
    import {
        Video,
        Circle,
        Square,
        Loader2,
        MonitorPlay,
        Crop,
        X,
        Volume2,
        VolumeX,
        Mic,
        MicOff,
        Play,
        Pause,
        FolderOpen,
        EyeOff,
        Film,
        AlertTriangle,
        Download,
        RefreshCw,
    } from '@lucide/svelte';
    import { ToolPage, Button } from '$lib/ui';
    import { toast } from '$lib/stores/toasts';
    import { errorToast } from '$lib/stores/errorToast';
    import { recordActivity } from '$lib/stores/activityLog';
    import {
        recState,
        recLastOutput,
        recGifExporting,
        recElapsedMs,
        recBytes,
        recPaused,
        recSource,
        recRegion,
        recRedactions,
        recExcludeWindows,
        recSystemAudio,
        recMic,
        recAudioSyncMs,
        recPreset,
        presetParams,
        type RecRegion,
        type RecWindow,
        type RecPreset,
    } from '$lib/stores/screenRecorder';

    const ICON_TINT = '#ef4444'; // record red

    const state = recState;
    const preset = recPreset;
    const lastOutput = recLastOutput;
    const elapsedMs = recElapsedMs;
    const bytes = recBytes;
    const paused = recPaused;
    const source = recSource;
    const region = recRegion;
    const redactions = recRedactions;
    const excludeWins = recExcludeWindows;
    const systemAudio = recSystemAudio;
    const mic = recMic;
    const audioSyncMs = recAudioSyncMs;

    const mainWin = getCurrentWindow();
    // The app is minimized during region selection + recording (so you select on /
    // record your real desktop, not the app). Restore it afterward.
    async function restoreMain() {
        try {
            await mainWin.unminimize();
            await mainWin.setFocus();
        } catch {
            /* window may be closing — ignore */
        }
    }

    const PRESET_CHOICES: RecPreset[] = ['small', 'balanced', 'high'];

    type FfmpegStatus = {
        available: boolean;
        recorderAvailable: boolean;
        path: string | null;
        version: string | null;
        source: 'pack' | 'user' | 'path' | 'none';
    };
    const ffmpegStatus = writable<FfmpegStatus | null>(null);
    const ffmpegChecking = writable(true);
    const ffmpegLocating = writable(false);
    const ffmpegSetupError = writable<string | null>(null);

    async function refreshFfmpeg() {
        ffmpegChecking.set(true);
        try {
            ffmpegStatus.set(await invoke<FfmpegStatus>('ffmpeg_status'));
        } catch (error) {
            ffmpegStatus.set(null);
            ffmpegSetupError.set(String(error));
        } finally {
            ffmpegChecking.set(false);
        }
    }

    async function locateFfmpeg() {
        try {
            const picked = await open({
                title: 'Locate ffmpeg.exe',
                multiple: false,
                directory: false,
                filters: [{ name: 'FFmpeg', extensions: ['exe'] }],
            });
            if (typeof picked !== 'string') return;
            ffmpegLocating.set(true);
            ffmpegSetupError.set(null);
            ffmpegStatus.set(await invoke<FfmpegStatus>('ffmpeg_set_path', { path: picked }));
        } catch (error) {
            const message = String(error);
            ffmpegSetupError.set(
                message.includes('no_ffmpeg')
                    ? 'That folder does not contain ffmpeg.exe.'
                    : message.includes('not_runnable')
                      ? 'Search could not run that ffmpeg.exe.'
                      : message,
            );
        } finally {
            ffmpegLocating.set(false);
        }
    }

    /** Settings › Packs, where the FFmpeg pack is one click. */
    async function getFfmpeg() {
        try {
            await invoke('host:packs.open');
        } catch (error) {
            errorToast("Couldn't open Settings › Packs", error);
        }
    }

    let pollTimer: ReturnType<typeof setInterval> | null = null;
    // Active "region-selected"/"region-cancelled" listeners while the overlay is
    // open — kept so we can drop them if this view unmounts mid-selection.
    let regionUnlistens: UnlistenFn[] = [];
    // Toolbar "stopped" listener — lives for the whole view lifetime so a Stop
    // from the floating toolbar syncs this page's state.
    let stoppedUnlisten: UnlistenFn | null = null;
    let packsUnlisten: UnlistenFn | null = null;

    function fmtTime(ms: number): string {
        const total = Math.floor(ms / 1000);
        const m = Math.floor(total / 60);
        const s = total % 60;
        return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
    }
    function fmtBytes(b: number): string {
        if (b < 1024) return `${b} B`;
        if (b < 1024 * 1024) return `${(b / 1024).toFixed(0)} KB`;
        return `${(b / 1024 / 1024).toFixed(1)} MB`;
    }
    function basename(p: string): string {
        return p.split(/[\\/]/).pop() ?? p;
    }

    async function poll() {
        try {
            const s = await invoke<{
                recording: boolean;
                elapsedMs: number;
                outputBytes: number;
                failed: boolean;
                paused: boolean;
            }>('screenrec_status');
            if (s.failed) {
                // The encoder died mid-take (disk full / fault). Finalise the
                // partial (still playable) and tell the user honestly — never a
                // silent "saved" over a truncated file.
                stopPoll();
                state.set('stopping');
                try {
                    const path = await invoke<string>('screenrec_stop');
                    lastOutput.set(path);
                } catch {
                    /* already torn down */
                }
                state.set('idle');
                void emit('screenrec:stopped', '');
                void restoreMain();
                errorToast(
                    'Recording stopped early',
                    'The encoder stopped — your disk may be full. We saved what was recorded so far.',
                    { durationMs: 9000 },
                );
                return;
            }
            if (s.recording) {
                if (get(state) === 'idle') state.set('recording');
                elapsedMs.set(s.elapsedMs);
                bytes.set(s.outputBytes);
                paused.set(s.paused);
            } else if (get(state) === 'recording') {
                // Backend stopped on its own; the toolbar closes off its own poll.
                state.set('idle');
                stopPoll();
                void restoreMain();
            }
        } catch {
            /* transient — keep polling */
        }
    }
    function startPoll() {
        stopPoll();
        pollTimer = setInterval(poll, 1000);
    }
    function stopPoll() {
        if (pollTimer) {
            clearInterval(pollTimer);
            pollTimer = null;
        }
    }

    // Drive the poll by recording state, so the timer/size update on this page
    // however the recording began — the in-page button OR the global hotkey
    // (which starts it from the layout, even when this page wasn't the trigger).
    $effect(() => {
        const live = $state === 'recording' || $state === 'stopping';
        if (live && !pollTimer) startPoll();
        else if (!live && pollTimer) stopPoll();
    });

    onMount(() => {
        // Reattach to a recording that was running before this view mounted.
        void poll();
        void refreshFfmpeg();
        // The pack installed (or removed) in Settings while this page is open.
        void listen<{ id: string }>('packs-changed', ({ payload }) => {
            if (payload?.id === 'ffmpeg') void refreshFfmpeg();
        }).then((un) => (packsUnlisten = un));
        // Sync when the user stops from the floating capture toolbar.
        void listen<string>('screenrec:stopped', (e) => {
            if (get(state) === 'recording' || get(state) === 'stopping') {
                state.set('idle');
                stopPoll();
                void restoreMain();
                if (e.payload) {
                    lastOutput.set(e.payload);
                    void recordActivity({
                        toolId: 'screen-recorder',
                        summary: 'Screen recording saved',
                        details: e.payload,
                        outcome: 'success',
                    });
                }
                toast('Recording saved', 'success');
            }
        }).then((un) => (stoppedUnlisten = un));
        // The global start/stop hotkey is handled in the root layout (main window)
        // so it works from any page, plus the floating toolbar for stop-while-
        // minimized — this component no longer needs its own hotkey listener.
    });
    onDestroy(() => {
        stopPoll();
        stoppedUnlisten?.();
        packsUnlisten?.();
        for (const un of regionUnlistens) un();
        regionUnlistens = [];
        cleanupRedactListeners();
    });

    // ── Region selection ─────────────────────────────────────────────────
    function fmtRegion(r: RecRegion): string {
        return `${r.w} × ${r.h}px at (${r.x}, ${r.y})`;
    }

    async function selectRegion() {
        // Drop any previous listeners before opening a fresh overlay.
        for (const un of regionUnlistens) un();
        regionUnlistens = [];
        try {
            const selected = await listen<RecRegion>('screenrec:region-selected', (e) => {
                region.set(e.payload);
                source.set('region');
                cleanupRegionListeners();
                void restoreMain();
            });
            const cancelled = await listen('screenrec:region-cancelled', () => {
                cleanupRegionListeners();
                void restoreMain();
            });
            regionUnlistens = [selected, cancelled];
            // Get the app out of the way so you select on your real desktop.
            try {
                await mainWin.minimize();
            } catch {
                /* non-fatal */
            }
            await invoke('screenrec_open_region_selector');
        } catch (e) {
            cleanupRegionListeners();
            void restoreMain();
            errorToast("Couldn't open region selector", e);
        }
    }

    function cleanupRegionListeners() {
        for (const un of regionUnlistens) un();
        regionUnlistens = [];
    }

    function clearRegion() {
        region.set(null);
        source.set('full');
    }

    // ── Privacy redaction ────────────────────────────────────────────────
    let redactUnlistens: UnlistenFn[] = [];
    function cleanupRedactListeners() {
        for (const un of redactUnlistens) un();
        redactUnlistens = [];
    }
    async function markRedactions() {
        cleanupRedactListeners();
        try {
            const selected = await listen<RecRegion[]>('screenrec:redactions-selected', (e) => {
                redactions.set(e.payload ?? []);
                cleanupRedactListeners();
                void restoreMain();
            });
            const cancelled = await listen('screenrec:redactions-cancelled', () => {
                cleanupRedactListeners();
                void restoreMain();
            });
            redactUnlistens = [selected, cancelled];
            // Get the app out of the way so you mark on your real desktop.
            try {
                await mainWin.minimize();
            } catch {
                /* non-fatal */
            }
            await invoke('screenrec_open_redact_selector');
        } catch (e) {
            cleanupRedactListeners();
            void restoreMain();
            errorToast("Couldn't open the privacy overlay", e);
        }
    }
    function clearRedactions() {
        redactions.set([]);
    }

    // ── Hide windows (WDA_EXCLUDEFROMCAPTURE) ────────────────────────────
    // Local writable stores (not $state runes): this component aliases the
    // recState store to `state`, which shadows the `$state` rune — so picker
    // reactivity rides the component's existing store-based pattern instead.
    const showWindowPicker = writable(false);
    const windowList = writable<RecWindow[]>([]);
    const loadingWindows = writable(false);
    async function toggleWindowPicker() {
        if (get(showWindowPicker)) {
            showWindowPicker.set(false);
            return;
        }
        showWindowPicker.set(true);
        loadingWindows.set(true);
        try {
            windowList.set(await invoke<RecWindow[]>('screenrec_list_windows'));
        } catch (e) {
            windowList.set([]);
            errorToast("Couldn't list open windows", e);
        } finally {
            loadingWindows.set(false);
        }
    }
    function toggleWindow(win: RecWindow) {
        excludeWins.update((list) =>
            list.some((w) => w.hwnd === win.hwnd)
                ? list.filter((w) => w.hwnd !== win.hwnd)
                : [...list, win],
        );
    }
    function clearExcludeWindows() {
        excludeWins.set([]);
    }

    async function playOutput() {
        const p = get(lastOutput);
        if (!p) return;
        try {
            // Open via the backend launcher the rest of the app uses (path-validated
            // + ShellExecute). The JS opener plugin's open_path is scope-restricted
            // and rejects arbitrary user paths ("Not allowed to open path …"); the
            // backend command isn't, and works for any folder the recording was saved to.
            await invoke('open_search_result_path', { path: p });
        } catch (e) {
            errorToast("Couldn't open the video", e);
        }
    }
    async function revealOutput() {
        const p = get(lastOutput);
        if (!p) return;
        try {
            const { revealItemInDir } = await import('@tauri-apps/plugin-opener');
            await revealItemInDir(p);
        } catch (e) {
            errorToast("Couldn't open the folder", e);
        }
    }
    // GIF export can take a while on a long recording — local spinner store (the
    // `$state` rune is shadowed here by the recState alias, so use a store).
    const exportingGif = recGifExporting;
    async function doExportGif() {
        await exportLastGif();
    }

    // Start/stop live in the shared control module so the global hotkey (handled
    // in the root layout) and these buttons run the exact same flow. The poll is
    // driven reactively by state below, so it attaches however a recording began.
    async function start() {
        await startRecording();
    }
    async function stop() {
        await stopRecording();
    }
</script>

<ToolPage
    icon={Video}
    iconTint={ICON_TINT}
    title="Screen Recorder"
    description="Record your primary monitor to a clean MP4 — fully on-device, nothing uploaded. Keeps recording while you work in other tools."
    width="wide"
    fill={false}
>
    {#if $state === 'recording' || $state === 'stopping'}
        <!-- Recording status -->
        <div class="sr-rec-card" class:stopping={$state === 'stopping'} class:paused={$paused}>
            <div class="sr-rec-left">
                <span class="sr-dot" class:paused={$paused} aria-hidden="true"></span>
                <div class="sr-rec-meta">
                    <span class="sr-rec-label">
                        {$state === 'stopping' ? 'Finishing…' : $paused ? 'Paused' : 'Recording'}
                    </span>
                    <span class="sr-rec-sub">
                        {$source === 'region' && $region
                            ? `Region ${$region.w}×${$region.h}`
                            : 'Primary monitor'} · {presetParams($preset).fps} fps · {$systemAudio ||
                        $mic
                            ? [$systemAudio ? 'System' : '', $mic ? 'Mic' : '']
                                  .filter(Boolean)
                                  .join(' + ')
                            : 'No audio'} · {fmtBytes($bytes)}
                    </span>
                </div>
            </div>
            <div class="sr-timer" role="timer" aria-label="Elapsed recording time">
                {fmtTime($elapsedMs)}
            </div>
            {#if $state === 'recording'}
                <button
                    type="button"
                    class="sr-pause-btn"
                    onclick={togglePause}
                    aria-label={$paused ? 'Resume recording' : 'Pause recording'}
                >
                    {#if $paused}
                        <Play size={15} /> Resume
                    {:else}
                        <Pause size={15} /> Pause
                    {/if}
                </button>
            {/if}
            <Button
                variant="primary"
                icon={$state === 'stopping' ? Loader2 : Square}
                onclick={stop}
                disabled={$state === 'stopping'}
            >
                {$state === 'stopping' ? 'Saving…' : 'Stop & save'}
            </Button>
        </div>
        <p class="sr-note">
            The recording continues even if you switch tools — come back here or stop it anytime.
        </p>
    {:else if $ffmpegChecking}
        <div class="sr-ffmpeg-card" aria-live="polite">
            <span class="sr-spin"><Loader2 size={18} /></span>
            Checking for FFmpeg…
        </div>
    {:else if !$ffmpegStatus?.recorderAvailable}
        <div class="sr-ffmpeg-card sr-ffmpeg-setup">
            <div class="sr-ffmpeg-icon"><AlertTriangle size={20} /></div>
            <div class="sr-ffmpeg-body">
                <h3>{$ffmpegStatus?.available ? 'This FFmpeg cannot record your screen' : 'Screen Recorder runs on the FFmpeg pack'}</h3>
                <p>
                    {#if $ffmpegStatus?.available}
                        Screen Recorder needs the Windows H.264 encoder (<code>h264_mf</code>), which the FFmpeg pack has.
                    {:else}
                        Add it in Settings › Packs: a one-time download from its publisher. Your recordings stay on this device.
                    {/if}
                </p>
                {#if $ffmpegSetupError}<p class="sr-ffmpeg-error">{$ffmpegSetupError}</p>{/if}
                <div class="sr-ffmpeg-actions">
                    <Button variant="primary" icon={Download} onclick={() => void getFfmpeg()}>
                        Get the FFmpeg pack
                    </Button>
                    <Button variant="secondary" icon={FolderOpen} loading={$ffmpegLocating} onclick={() => void locateFfmpeg()}>
                        Use my own ffmpeg.exe
                    </Button>
                    <Button variant="ghost" icon={RefreshCw} onclick={() => void refreshFfmpeg()}>
                        Re-check
                    </Button>
                </div>
            </div>
        </div>
    {:else}
        <!-- Idle: configure + start -->
        <div class="sr-start-card">
            <div class="sr-start-icon">
                {#if $source === 'region'}<Crop />{:else}<MonitorPlay />{/if}
            </div>
            <div class="sr-start-body">
                <h3 class="sr-start-title">
                    {$source === 'region' ? 'Record a region' : 'Record the full screen'}
                </h3>
                <p class="sr-start-desc">
                    {$source === 'region'
                        ? 'Capture just part of your primary monitor to an MP4 (H.264). Everything runs locally — the video never leaves your machine.'
                        : 'Captures your primary monitor to an MP4 (H.264). Everything runs locally — the video never leaves your machine.'}
                </p>

                <div class="sr-fps">
                    <span class="sr-fps-label">Capture</span>
                    <div class="sr-seg">
                        <button
                            type="button"
                            class="sr-seg-btn"
                            class:active={$source === 'full'}
                            onclick={clearRegion}
                            aria-pressed={$source === 'full'}
                        >
                            Full screen
                        </button>
                        <button
                            type="button"
                            class="sr-seg-btn"
                            class:active={$source === 'region'}
                            onclick={() => source.set('region')}
                            aria-pressed={$source === 'region'}
                        >
                            Region
                        </button>
                    </div>
                </div>

                {#if $source === 'region'}
                    <div class="sr-region">
                        {#if $region}
                            <span class="sr-region-info">{fmtRegion($region)}</span>
                            <button type="button" class="sr-region-edit" onclick={selectRegion}>
                                Reselect
                            </button>
                            <button type="button" class="sr-region-clear" onclick={clearRegion}>
                                <X size={13} />
                                Clear
                            </button>
                        {:else}
                            <Button variant="secondary" icon={Crop} onclick={selectRegion}>
                                Select region
                            </Button>
                            <span class="sr-region-hint">Drag a rectangle on your screen.</span>
                        {/if}
                    </div>
                {/if}

                <div class="sr-audio">
                    <span class="sr-fps-label">Audio</span>
                    <button
                        type="button"
                        class="sr-aud-btn"
                        class:on={$systemAudio}
                        onclick={() => systemAudio.set(!$systemAudio)}
                        aria-pressed={$systemAudio}
                    >
                        {#if $systemAudio}<Volume2 size={14} />{:else}<VolumeX size={14} />{/if}
                        System sound
                    </button>
                    <button
                        type="button"
                        class="sr-aud-btn"
                        class:on={$mic}
                        onclick={() => mic.set(!$mic)}
                        aria-pressed={$mic}
                    >
                        {#if $mic}<Mic size={14} />{:else}<MicOff size={14} />{/if}
                        Microphone
                    </button>
                </div>

                {#if $systemAudio || $mic}
                    <div class="sr-audio">
                        <span class="sr-fps-label">Audio sync</span>
                        <input
                            class="sr-sync-input"
                            type="number"
                            step="20"
                            min="-1000"
                            max="1000"
                            bind:value={$audioSyncMs}
                            aria-label="Audio sync offset in milliseconds"
                        />
                        <span class="sr-sync-unit">ms</span>
                        <span class="sr-region-hint">
                            If the audio plays ahead of the video, increase this.
                        </span>
                    </div>
                {/if}

                <div class="sr-audio">
                    <span class="sr-fps-label">Privacy</span>
                    <button
                        type="button"
                        class="sr-aud-btn"
                        class:on={$redactions.length > 0}
                        onclick={markRedactions}
                    >
                        <EyeOff size={14} />
                        {$redactions.length > 0
                            ? `${$redactions.length} hidden area${$redactions.length > 1 ? 's' : ''}`
                            : 'Mark private areas'}
                    </button>
                    {#if $redactions.length > 0}
                        <button type="button" class="sr-region-clear" onclick={clearRedactions}>
                            <X size={13} />
                            Clear
                        </button>
                    {/if}
                    <span class="sr-region-hint">
                        Black out anything sensitive — those pixels never enter the recording.
                    </span>
                </div>

                <div class="sr-audio">
                    <span class="sr-fps-label">Hide windows</span>
                    <button
                        type="button"
                        class="sr-aud-btn"
                        class:on={$excludeWins.length > 0}
                        onclick={toggleWindowPicker}
                    >
                        <EyeOff size={14} />
                        {$excludeWins.length > 0
                            ? `${$excludeWins.length} window${$excludeWins.length > 1 ? 's' : ''} hidden`
                            : 'Choose windows to hide'}
                    </button>
                    {#if $excludeWins.length > 0}
                        <button type="button" class="sr-region-clear" onclick={clearExcludeWindows}>
                            <X size={13} />
                            Clear
                        </button>
                    {/if}
                    <span class="sr-region-hint">
                        A black box tracks each window as it moves — its contents never reach the recording.
                    </span>
                </div>

                {#if $showWindowPicker}
                    <div class="sr-winpick">
                        {#if $loadingWindows}
                            <div class="sr-winpick-msg">
                                <span class="sr-spin"><Loader2 size={14} /></span>
                                Finding open windows…
                            </div>
                        {:else if $windowList.length === 0}
                            <div class="sr-winpick-msg">No open windows found.</div>
                        {:else}
                            <div class="sr-winpick-list">
                                {#each $windowList as win (win.hwnd)}
                                    <label class="sr-winrow">
                                        <input
                                            type="checkbox"
                                            checked={$excludeWins.some((w) => w.hwnd === win.hwnd)}
                                            onchange={() => toggleWindow(win)}
                                        />
                                        <span class="sr-winrow-title" title={win.title}>{win.title}</span>
                                    </label>
                                {/each}
                            </div>
                        {/if}
                        <div class="sr-winpick-foot">
                            <button
                                type="button"
                                class="sr-region-clear"
                                onclick={() => showWindowPicker.set(false)}
                            >
                                Done
                            </button>
                        </div>
                    </div>
                {/if}

                <div class="sr-fps">
                    <span class="sr-fps-label">Quality</span>
                    <div class="sr-seg">
                        {#each PRESET_CHOICES as choice}
                            <button
                                type="button"
                                class="sr-seg-btn"
                                class:active={$preset === choice}
                                onclick={() => preset.set(choice)}
                                aria-pressed={$preset === choice}
                                title={presetParams(choice).desc}
                            >
                                {presetParams(choice).label}
                            </button>
                        {/each}
                    </div>
                    <span class="sr-region-hint">{presetParams($preset).desc}</span>
                </div>
                <div class="sr-start-action">
                    <Button
                        variant="primary"
                        icon={Circle}
                        onclick={start}
                        disabled={$source === 'region' && !$region}
                    >
                        Start recording
                    </Button>
                </div>
            </div>
        </div>

        {#if $lastOutput}
            <div class="sr-last">
                <Video size={15} />
                <span class="sr-last-text">Saved <strong>{basename($lastOutput)}</strong></span>
                <span class="sr-last-path" title={$lastOutput}>{$lastOutput}</span>
                <div class="sr-last-actions">
                    <button type="button" class="sr-last-btn" onclick={playOutput}>
                        <Play size={13} /> Play
                    </button>
                    <button type="button" class="sr-last-btn" onclick={revealOutput}>
                        <FolderOpen size={13} /> Reveal
                    </button>
                    <button
                        type="button"
                        class="sr-last-btn"
                        onclick={doExportGif}
                        disabled={$exportingGif}
                        title="Convert this recording to a shareable GIF"
                    >
                        {#if $exportingGif}
                            <span class="sr-spin"><Loader2 size={13} /></span> Making GIF…
                        {:else}
                            <Film size={13} /> Export GIF
                        {/if}
                    </button>
                </div>
            </div>
        {/if}
    {/if}
</ToolPage>

<style>
    .sr-ffmpeg-card {
        display: flex;
        align-items: center;
        gap: 12px;
        padding: 18px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel);
        color: var(--color-text-secondary);
        font-size: 13px;
    }
    .sr-ffmpeg-setup {
        align-items: flex-start;
    }
    .sr-ffmpeg-icon {
        display: grid;
        flex: 0 0 auto;
        place-items: center;
        width: 36px;
        height: 36px;
        color: var(--color-warning, #d79922);
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .sr-ffmpeg-body {
        min-width: 0;
    }
    .sr-ffmpeg-body h3 {
        margin: 0;
        color: var(--color-text);
        font-size: 14px;
        font-weight: 600;
    }
    .sr-ffmpeg-body p {
        margin: 4px 0 0;
        max-width: 62ch;
        line-height: 1.5;
    }
    .sr-ffmpeg-body code {
        color: var(--color-text);
        font-family: var(--font-mono, ui-monospace, monospace);
        font-size: 0.92em;
    }
    .sr-ffmpeg-error {
        color: var(--color-error);
    }
    .sr-ffmpeg-actions {
        display: flex;
        flex-wrap: wrap;
        gap: 8px;
        margin-top: 14px;
    }
    .sr-start-card {
        display: grid;
        grid-template-columns: 40px minmax(0, 1fr);
        gap: 16px;
        padding-top: 4px;
    }
    .sr-start-icon {
        display: grid;
        place-items: center;
        width: 40px;
        height: 40px;
        border-radius: 10px;
        color: #ef4444;
        box-shadow:
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 10%, transparent),
            inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .sr-start-icon :global(svg) {
        width: 20px;
        height: 20px;
    }
    .sr-start-body {
        display: flex;
        flex-direction: column;
        min-width: 0;
    }
    .sr-start-title {
        margin: 0;
        font-size: 16px;
        font-weight: 600;
        letter-spacing: -0.01em;
        color: var(--color-text);
    }
    .sr-start-desc {
        margin: 4px 0 18px;
        max-width: 58ch;
        font-size: 13px;
        line-height: 1.5;
        color: var(--color-text-secondary);
    }
    .sr-fps {
        display: flex;
        align-items: center;
        gap: 10px;
        flex-wrap: wrap;
        margin: 0;
        padding: 14px 0;
        border-top: 1px solid var(--color-border);
    }
    .sr-fps-label {
        flex: 0 0 104px;
        font-size: 11px;
        text-transform: uppercase;
        letter-spacing: 0.06em;
        color: var(--color-muted);
    }
    .sr-seg {
        display: inline-flex;
        align-items: center;
        padding: 2px;
        gap: 2px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel);
    }
    .sr-seg-btn {
        position: relative;
        height: 28px;
        padding: 0 14px 0 17px;
        border: none;
        border-radius: calc(var(--radius-control) - 2px);
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;
        transition: color var(--dur-micro) var(--ease-out), background var(--dur-micro) var(--ease-out);
    }
    .sr-seg-btn:hover:not(.active) {
        color: var(--color-text);
    }
    .sr-seg-btn.active {
        background: var(--color-panel-2);
        color: var(--color-text);
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .sr-seg-btn.active::before {
        position: absolute;
        top: 7px;
        bottom: 7px;
        left: 5px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';

    }
    .sr-audio {
        display: flex;
        align-items: center;
        gap: 8px;
        flex-wrap: wrap;
        margin: 0;
        padding: 14px 0;
        border-top: 1px solid var(--color-border);
    }
    .sr-aud-btn {
        position: relative;
        display: inline-flex;
        align-items: center;
        gap: 6px;
        height: 30px;
        padding: 0 12px 0 17px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel);
        color: var(--color-text-secondary);
        font-size: 12.5px;
        font-weight: 500;
        cursor: pointer;
        transition: color var(--dur-micro) var(--ease-out), background var(--dur-micro) var(--ease-out);
    }
    .sr-aud-btn:hover {
        color: var(--color-text);
    }
    .sr-aud-btn.on {
        border-color: var(--color-border);
        background: var(--color-panel-2);
        color: var(--color-text);
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    }
    .sr-aud-btn.on::before {
        position: absolute;
        top: 7px;
        bottom: 7px;
        left: 5px;
        width: 3px;
        border-radius: 999px;
        background: var(--color-accent);
        content: '';
    }
    .sr-aud-btn.on :global(svg) {
        color: var(--color-accent);
    }
    .sr-sync-input {
        width: 72px;
        height: 30px;
        padding: 0 8px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-size: 12.5px;
        font-variant-numeric: tabular-nums;
    }
    .sr-sync-unit {
        font-size: 12px;
        color: var(--color-muted);
    }

    .sr-region {
        display: flex;
        align-items: center;
        gap: 10px;
        flex-wrap: wrap;
        margin: -2px 0 0 114px;
        padding: 0 0 14px;
    }
    .sr-region-info {
        font-size: 12.5px;
        font-weight: 600;
        font-variant-numeric: tabular-nums;
        color: var(--color-text);
        padding: 4px 10px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel-2);
    }
    .sr-region-edit,
    .sr-region-clear {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        height: 26px;
        padding: 0 10px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: transparent;
        color: var(--color-text-secondary);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;
    }
    .sr-region-edit:hover,
    .sr-region-clear:hover {
        color: var(--color-text);
        background: var(--color-panel-2);
    }
    .sr-region-hint {
        font-size: 12px;
        color: var(--color-muted);
    }

    .sr-fps > .sr-region-hint,
    .sr-audio > .sr-region-hint {
        flex: 0 0 100%;
        box-sizing: border-box;
        padding-left: 114px;
    }
    /* ── Hide-windows picker ── */
    .sr-start-action {
        display: flex;
        margin-top: 2px;
        padding-top: 18px;
        border-top: 1px solid var(--color-border);
    }
    .sr-winpick {
        width: calc(100% - 114px);
        max-width: 520px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel);
        overflow: hidden;
        margin: -2px 0 14px 114px;
    }
    .sr-winpick-msg {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 12px 14px;
        font-size: 12.5px;
        color: var(--color-muted);
    }
    .sr-winpick-list {
        max-height: 240px;
        overflow-y: auto;
        padding: 4px;
    }
    .sr-winrow {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 7px 10px;
        border-radius: var(--radius-control);
        cursor: pointer;
        font-size: 12.5px;
        color: var(--color-text-secondary);
    }
    .sr-winrow:hover {
        background: color-mix(in srgb, var(--color-text) 7%, transparent);
        color: var(--color-text);
    }
    .sr-winrow input {
        width: 15px;
        height: 15px;
        flex-shrink: 0;
        accent-color: var(--color-accent);
        cursor: pointer;
    }
    .sr-winrow-title {
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
    }
    .sr-winpick-foot {
        display: flex;
        justify-content: flex-end;
        padding: 8px 10px;
        border-top: 1px solid var(--color-border);
    }
    .sr-spin {
        display: inline-flex;
        animation: sr-spin 0.8s linear infinite;
    }
    @keyframes sr-spin {
        to {
            transform: rotate(360deg);
        }
    }
    @media (prefers-reduced-motion: reduce) {
        .sr-spin {
            animation: none;
        }
    }

    .sr-rec-card {
        display: flex;
        align-items: center;
        gap: 16px;
        padding: 18px 20px 18px 24px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-card);
        background: var(--color-panel);
        box-shadow:
            inset 3px 0 0 #ef4444,
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 7%, transparent);
    }
    .sr-rec-left {
        display: flex;
        align-items: center;
        gap: 12px;
        flex: 1;
        min-width: 0;
    }
    .sr-dot {
        width: 14px;
        height: 14px;
        border-radius: 50%;
        background: #ef4444;
        box-shadow: 0 0 0 0 color-mix(in srgb, #ef4444 60%, transparent);
        animation: sr-pulse 1.6s ease-out infinite;
    }
    .sr-rec-card.stopping .sr-dot {
        animation: none;
        opacity: 0.5;
    }
    .sr-dot.paused {
        animation: none;
        background: #f59e0b; /* amber: recording held */
        box-shadow: none;
    }
    .sr-rec-card.paused {
        box-shadow:
            inset 3px 0 0 #f59e0b,
            inset 0 1px 0 color-mix(in srgb, var(--color-text) 7%, transparent);
    }
    .sr-pause-btn {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        height: 36px;
        padding: 0 14px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel-2);
        color: var(--color-text);
        font-size: 13px;
        font-weight: 600;
        cursor: pointer;
    }
    .sr-pause-btn:hover {
        background: var(--color-panel-3, var(--color-panel-2));
        border-color: var(--color-accent);
    }
    @keyframes sr-pulse {
        0% {
            box-shadow: 0 0 0 0 color-mix(in srgb, #ef4444 55%, transparent);
        }
        100% {
            box-shadow: 0 0 0 10px transparent;
        }
    }
    @media (prefers-reduced-motion: reduce) {
        .sr-dot {
            animation: none;
        }
    }
    .sr-rec-meta {
        display: flex;
        flex-direction: column;
        gap: 2px;
        min-width: 0;
    }
    .sr-rec-label {
        font-size: 14px;
        font-weight: 600;
        color: var(--color-text);
    }
    .sr-rec-sub {
        font-size: 12px;
        color: var(--color-text-secondary);
    }
    .sr-timer {
        padding-left: 16px;
        border-left: 1px solid var(--color-border);
        font-variant-numeric: tabular-nums;
        font-size: 22px;
        font-weight: 600;
        color: var(--color-text);
        letter-spacing: 0.02em;
    }
    .sr-note {
        margin: 10px 2px 0;
        font-size: 12px;
        color: var(--color-muted);
    }

    .sr-last {
        display: flex;
        align-items: center;
        gap: 8px;
        margin-top: 14px;
        padding: 12px 14px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel);
        box-shadow: inset 0 1px 0 color-mix(in srgb, var(--color-text) 6%, transparent);
        font-size: 12.5px;
        color: var(--color-text-secondary);
        flex-wrap: wrap;
    }
    .sr-last-text strong {
        color: var(--color-text);
        font-weight: 600;
    }
    .sr-last-path {
        color: var(--color-muted);
        word-break: break-all;
    }
    .sr-last-actions {
        display: inline-flex;
        gap: 6px;
        margin-left: auto;
    }
    .sr-last-btn {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        height: 28px;
        padding: 0 12px;
        border: 1px solid var(--color-border);
        border-radius: var(--radius-control);
        background: var(--color-panel);
        color: var(--color-text);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;
    }
    .sr-last-btn:hover {
        border-color: var(--color-accent);
        color: var(--color-accent);
    }
    @media (max-width: 720px) {
        .sr-start-card {
            grid-template-columns: 1fr;
            gap: 12px;
        }
        .sr-fps-label {
            flex-basis: 100%;
        }
        .sr-region,
        .sr-winpick {
            width: 100%;
            margin-left: 0;
        }
        .sr-fps > .sr-region-hint,
        .sr-audio > .sr-region-hint {
            padding-left: 0;
        }
        .sr-rec-card {
            flex-wrap: wrap;
        }
        .sr-timer {
            margin-left: auto;
        }
        .sr-last-actions {
            width: 100%;
            margin-left: 0;
        }
    }
</style>
