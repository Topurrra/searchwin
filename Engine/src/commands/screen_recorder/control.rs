//! Screen Recorder — recording control (the single active [`Recorder`]).
//!
//! These are plain functions; the `#[tauri::command]` wrappers live in the
//! always-compiled `commands::screenrec_cmds` so the commands exist (returning a
//! clear error) even in a build without the `screenrec` feature.
//!
//! Region coordinates are MONITOR-relative physical pixels (the frontend converts
//! from logical/CSS px via devicePixelRatio + monitor origin).
#![cfg(feature = "screenrec")]

use std::sync::{LazyLock, Mutex};

use super::audio::AudioOptions;
use super::encode::{start_recording, Recorder};
use super::CropRect;

struct ActiveRecording {
    recorder: Recorder,
}

static RECORDER: LazyLock<Mutex<Option<ActiveRecording>>> = LazyLock::new(|| Mutex::new(None));

/// Start recording the primary monitor to `out_path` (an `.mp4`). `region` is a
/// monitor-relative physical-pixel crop, or `None` for the whole monitor.
/// `excluded_windows` are HWNDs (as integers) whose live on-screen area is blacked
/// out of every frame — see `super::SharedExcludeWindows`. Errors if a recording
/// is already in progress.
pub fn start(
    out_path: String,
    fps: u32,
    region: Option<CropRect>,
    redactions: Vec<CropRect>,
    excluded_windows: Vec<i64>,
    audio: AudioOptions,
    audio_sync_ms: i32,
    quality_bpp: f64,
) -> Result<(), String> {
    let mut guard = RECORDER.lock().map_err(|_| "recorder lock poisoned".to_string())?;
    if guard.is_some() {
        return Err("A recording is already in progress".into());
    }
    let target = crate::core::safe_path::validate_user_write_target(&out_path)?;
    // Pre-flight: refuse to start with almost no free space (video is ~GBs/hour),
    // so the user isn't surprised by a take that dies a few minutes in.
    if let Some(free) = crate::core::throttle::free_disk_bytes(&target) {
        const MIN_FREE: u64 = 500 * 1024 * 1024; // 500 MB
        if free < MIN_FREE {
            return Err(format!(
                "Not enough free disk space to record ({} MB free). Free up some space and try again.",
                free / (1024 * 1024)
            ));
        }
    }
    // Fail before the capture session starts, with the same clear setup error
    // the Screen Recorder UI shows for a missing or incompatible FFmpeg.
    let _ = crate::commands::ffmpeg::screen_recording_command()?;
    let recorder = start_recording(
        &target,
        fps,
        region,
        redactions,
        excluded_windows,
        audio,
        audio_sync_ms,
        quality_bpp,
    )?;
    *guard = Some(ActiveRecording { recorder });
    Ok(())
}

/// Pause the active recording (idempotent). The video clock freezes and audio
/// drops samples, so the take gains no frozen segment. No-op if not recording.
pub fn pause() -> Result<(), String> {
    let guard = RECORDER.lock().map_err(|_| "recorder lock poisoned".to_string())?;
    match guard.as_ref() {
        Some(a) => {
            a.recorder.pause();
            Ok(())
        }
        None => Err("No recording in progress".into()),
    }
}

/// Resume a paused recording (idempotent). No-op if not recording.
pub fn resume() -> Result<(), String> {
    let guard = RECORDER.lock().map_err(|_| "recorder lock poisoned".to_string())?;
    match guard.as_ref() {
        Some(a) => {
            a.recorder.resume();
            Ok(())
        }
        None => Err("No recording in progress".into()),
    }
}

/// Strip a Windows verbatim / extended-length prefix (`\\?\`, `\\?\UNC\`). The
/// internal write path may be verbatim (fine for ffmpeg + std::fs), but the shell
/// "open"/"reveal" APIs reject `\\?\` — so the path we hand the UI must be plain.
fn shell_friendly_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// Stop the active recording cleanly and return the finished file path.
pub fn stop() -> Result<String, String> {
    let active = {
        let mut guard = RECORDER.lock().map_err(|_| "recorder lock poisoned".to_string())?;
        guard.take()
    };
    match active {
        Some(a) => Ok(shell_friendly_path(&a.recorder.stop()?.to_string_lossy())),
        None => Err("No recording in progress".into()),
    }
}

/// Poll status: `(recording, elapsed_ms, output_bytes, failed, paused)`. `elapsed_ms`
/// EXCLUDES paused time (it tracks the file's true length). `failed` is true if the
/// encoder died mid-take (the UI should stop + finalise the partial).
pub fn status() -> (bool, u64, u64, bool, bool) {
    let guard = RECORDER.lock().ok();
    match guard.as_ref().and_then(|g| g.as_ref()) {
        Some(a) => (
            true,
            a.recorder.recorded_ms(),
            a.recorder.output_bytes(),
            a.recorder.failed(),
            a.recorder.is_paused(),
        ),
        None => (false, 0, 0, false, false),
    }
}

/// Convert a finished recording to a GIF next to it. Bounded; see
/// [`super::encode::export_gif`]. ~12 fps, capped to 640px wide for a shareable
/// file. Returns the shell-friendly path to the produced `.gif`.
pub fn export_gif(src_path: String) -> Result<String, String> {
    let src = crate::core::safe_path::validate_user_write_target(&src_path)?;
    let gif = super::encode::export_gif(&src, 12, 640)?;
    Ok(shell_friendly_path(&gif.to_string_lossy()))
}
