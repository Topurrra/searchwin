// Screen Recorder control — the start/stop logic lifted out of the component so
// it can be driven from anywhere: the in-page buttons AND the global start/stop
// hotkey (handled in the root layout, main window only). Single source of truth,
// no duplication. Pure orchestration over the backend commands + the stores.
import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { save } from '@tauri-apps/plugin-dialog';
import { Window } from '@tauri-apps/api/window';
import { toast } from './toasts';
import { errorToast } from './errorToast';
import { recordActivity } from './activityLog';
import {
    recState,
    recSource,
    recRegion,
    recRedactions,
    recExcludeWindows,
    recPreset,
    recSystemAudio,
    recMic,
    recAudioSyncMs,
    recSaveFolder,
    recLastOutput,
    recGifExporting,
    recPaused,
    presetParams,
    resetRecorderEphemeral,
} from './screenRecorder';

function defaultName(): string {
    const d = new Date();
    const p = (n: number) => String(n).padStart(2, '0');
    return `recording_${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}_${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}.mp4`;
}

/** Remembered folder → straight to an auto-timestamped file (no dialog);
 *  otherwise fall back to the system save dialog (location picker). */
async function resolveOutPath(): Promise<string | null> {
    const folder = get(recSaveFolder).trim();
    if (folder) {
        try {
            const { join } = await import('@tauri-apps/api/path');
            return await join(folder, defaultName());
        } catch {
            /* couldn't build the path — fall through to the picker */
        }
    }
    return await save({
        defaultPath: defaultName(),
        filters: [{ name: 'MP4 video', extensions: ['mp4'] }],
    });
}

/** The recorder minimizes the MAIN window while capturing (out of the way + out
 *  of the recording). Target it explicitly so this works from any window's JS. */
async function minimizeMain(): Promise<void> {
    try {
        const main = await Window.getByLabel('main');
        await main?.minimize();
    } catch {
        /* non-fatal */
    }
}
async function restoreMain(): Promise<void> {
    try {
        const main = await Window.getByLabel('main');
        await main?.unminimize();
        await main?.setFocus();
    } catch {
        /* window may be closing — ignore */
    }
}

/** Start a recording with the current settings (source/region, preset, audio,
 *  sync). Returns true if it actually started. Safe to call when already
 *  recording (no-op). */
export async function startRecording(): Promise<boolean> {
    if (get(recState) !== 'idle') return false;
    const outPath = await resolveOutPath();
    if (!outPath) return false;
    const chosenRegion = get(recSource) === 'region' ? get(recRegion) : null;
    const { fps, bpp } = presetParams(get(recPreset));
    try {
        await invoke('screenrec_start', {
            outPath,
            fps,
            region: chosenRegion,
            redactions: get(recRedactions),
            excludeWindows: get(recExcludeWindows).map((w) => w.hwnd),
            systemAudio: get(recSystemAudio),
            mic: get(recMic),
            audioSyncMs: get(recAudioSyncMs),
            qualityBpp: bpp,
        });
        resetRecorderEphemeral();
        recLastOutput.set(null);
        recPaused.set(false);
        recState.set('recording');
        // Surface the content-protected floating controls; only minimize once
        // that toolbar (which carries Stop) is up, so Stop is always reachable.
        let toolbarOpen = false;
        try {
            await invoke('screenrec_open_toolbar');
            toolbarOpen = true;
        } catch {
            /* non-fatal — recording is running; in-page Stop remains */
        }
        if (toolbarOpen) await minimizeMain();
        return true;
    } catch (e) {
        await restoreMain();
        recState.set('idle');
        errorToast("Couldn't start recording", e, {
            hint: 'Make sure no other recording is in progress, then try again.',
        });
        return false;
    }
}

/** Stop the active recording, finalise, and restore the app. Returns true if it
 *  was recording. Safe to call when idle (no-op). */
export async function stopRecording(): Promise<boolean> {
    if (get(recState) !== 'recording') return false;
    recState.set('stopping');
    try {
        const path = await invoke<string>('screenrec_stop');
        recLastOutput.set(path);
        recPaused.set(false);
        recState.set('idle');
        // One close-owner: the toolbar self-closes on this event.
        void emit('screenrec:stopped', path);
        await restoreMain();
        toast('Recording saved', 'success');
        void recordActivity({
            toolId: 'screen-recorder',
            summary: 'Screen recording saved',
            details: path,
            outcome: 'success',
        });
        return true;
    } catch (e) {
        await restoreMain();
        recState.set('idle');
        errorToast("Couldn't finish the recording", e, {
            hint: 'The partial file may still be playable.',
            durationMs: 7000,
        });
        return false;
    }
}

/** Toggle: start when idle, stop when recording. Drives the global hotkey. */
export async function toggleRecording(): Promise<void> {
    const s = get(recState);
    if (s === 'idle') await startRecording();
    else if (s === 'recording') await stopRecording();
}

/** Pause the active recording (clock freezes, no frozen segment in the file).
 *  Safe to call when not recording / already paused (no-op). */
export async function pauseRecording(): Promise<void> {
    if (get(recState) !== 'recording' || get(recPaused)) return;
    try {
        await invoke('screenrec_pause');
        recPaused.set(true);
    } catch (e) {
        errorToast("Couldn't pause the recording", e);
    }
}

/** Resume a paused recording. Safe to call when not paused (no-op). */
export async function resumeRecording(): Promise<void> {
    if (get(recState) !== 'recording' || !get(recPaused)) return;
    try {
        await invoke('screenrec_resume');
        recPaused.set(false);
    } catch (e) {
        errorToast("Couldn't resume the recording", e);
    }
}

/** Toggle pause/resume on the active recording. */
export async function togglePause(): Promise<void> {
    if (get(recPaused)) await resumeRecording();
    else await pauseRecording();
}

/** Convert the last finished recording to a GIF next to it, then reveal it.
 *  Returns true on success. Can take a while on long recordings — the caller
 *  should show a spinner. */
export async function exportLastGif(): Promise<boolean> {
    const src = get(recLastOutput);
    if (!src || get(recGifExporting)) return false;
    recGifExporting.set(true);
    try {
        const gif = await invoke<string>('screenrec_export_gif', { srcPath: src });
        toast('GIF exported', 'success');
        void recordActivity({
            toolId: 'screen-recorder',
            summary: 'Recording exported to GIF',
            details: gif,
            outcome: 'success',
        });
        try {
            const { revealItemInDir } = await import('@tauri-apps/plugin-opener');
            await revealItemInDir(gif);
        } catch {
            /* reveal is best-effort */
        }
        return true;
    } catch (e) {
        errorToast("Couldn't export the GIF", e, {
            hint: 'Very long recordings make large GIFs and can take a while.',
        });
        return false;
    } finally {
        recGifExporting.set(false);
    }
}
