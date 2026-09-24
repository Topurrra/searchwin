// Screen Recorder state — lives in a module store so an in-progress recording
// survives the workspace {#key} nav-unmount (the recording itself runs in the
// Rust backend; this just mirrors its status for the UI).
import { writable } from 'svelte/store';

export type RecState = 'idle' | 'recording' | 'stopping';

/** A capture region in MONITOR-RELATIVE PHYSICAL pixels (origin 0,0 =
 *  primary monitor top-left). `null` everywhere = full-screen capture. */
export type RecRegion = { x: number; y: number; w: number; h: number };

export const recState = writable<RecState>('idle');
export const recLastOutput = writable<string | null>(null);
export const recGifExporting = writable(false);
export const recElapsedMs = writable(0);
export const recBytes = writable(0);
/** True while the active recording is paused (clock frozen, audio dropped). Mirrors
 *  the backend status; drives the Pause/Resume control + a paused badge. */
export const recPaused = writable(false);

/** Audio capture toggles. System = WASAPI loopback of whatever is playing;
 *  mic = the default input device. Either/both/neither. */
export const recSystemAudio = writable(true);
export const recMic = writable(false);

/** A writable that persists to localStorage so a once-per-machine setting (sync
 *  trim, quality preset, save folder) survives app restarts. */
function persisted<T extends string | number>(key: string, initial: T) {
    let start = initial;
    if (typeof localStorage !== 'undefined') {
        const raw = localStorage.getItem(key);
        if (raw !== null) {
            start = (typeof initial === 'number' ? (Number(raw) as T) : (raw as T));
            if (typeof initial === 'number' && Number.isNaN(start as number)) start = initial;
        }
    }
    const store = writable<T>(start);
    if (typeof localStorage !== 'undefined') {
        store.subscribe((v) => {
            try {
                localStorage.setItem(key, String(v));
            } catch {
                /* storage full / unavailable — non-fatal */
            }
        });
    }
    return store;
}

/** A/V sync trim in ms, applied at mux. Positive delays the audio (use when the
 *  audio plays AHEAD of the video — common with screen capture); negative delays
 *  the video. Calibrated once per machine; persisted across restarts. */
export const recAudioSyncMs = persisted<number>('kil.rec.audioSyncMs', 0);

/** Quality preset — couples frame rate + bitrate budget into one understandable
 *  choice (instead of a raw bitrate slider). Persisted across restarts. */
export type RecPreset = 'small' | 'balanced' | 'high';
export const recPreset = persisted<RecPreset>('kil.rec.preset', 'balanced');

/** fps + bits-per-pixel budget for each preset. `bpp` feeds the backend's bitrate
 *  target (resolution × fps × bpp). Pure — used by both the recorder and Settings. */
export function presetParams(p: RecPreset): { fps: number; bpp: number; label: string; desc: string } {
    switch (p) {
        case 'small':
            return { fps: 24, bpp: 0.08, label: 'Small', desc: '24 fps · smallest files, easy to share' };
        case 'high':
            return { fps: 60, bpp: 0.22, label: 'High', desc: '60 fps · smoothest, largest files' };
        case 'balanced':
        default:
            return { fps: 30, bpp: 0.15, label: 'Balanced', desc: '30 fps · recommended' };
    }
}

/** Default folder recordings are saved into (auto-timestamped name). Empty = ask
 *  each time via the system save dialog. Persisted across restarts. */
export const recSaveFolder = persisted<string>('kil.rec.saveFolder', '');

/** Capture source: the whole primary monitor, or a chosen sub-rectangle. */
export type RecSource = 'full' | 'region';
export const recSource = writable<RecSource>('full');
/** The chosen region (physical px) when recSource === 'region'. Persisted in
 *  the module store so it survives the workspace {#key} nav-unmount. */
export const recRegion = writable<RecRegion | null>(null);

/** Privacy redaction rectangles (monitor-relative physical px) blacked out of
 *  every recorded frame — the sensitive pixels never enter the file. Empty =
 *  none. Survives nav like the region; reset on app restart. */
export const recRedactions = writable<RecRegion[]>([]);

/** A top-level window the user chose to HIDE from the recording. */
export type RecWindow = { hwnd: number; title: string };
/** Windows excluded from capture (solid black via SetWindowDisplayAffinity) for
 *  the recording's duration. Transient — HWNDs are only valid this session, so
 *  this never persists and resets on app restart. */
export const recExcludeWindows = writable<RecWindow[]>([]);

export function resetRecorderEphemeral(): void {
    recElapsedMs.set(0);
    recBytes.set(0);
}
