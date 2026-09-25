//! Shared resolver for FFmpeg, used by Screen Recorder and the media tools.
//!
//! Search doesn't carry FFmpeg: it's a pack (Settings › Packs), an LGPL
//! build the browser downloads, verifies and unpacks when asked, and names
//! in `packs.json` (core::packs). That one comes first. A person may still
//! choose their own `ffmpeg.exe` (kept locally) or have one on PATH.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, RwLock};

use serde::Serialize;
use tauri::AppHandle;

use super::secure_kv;

const FFMPEG_OVERRIDE_KEY: &str = "ffmpeg-directory";

const NOT_FOUND: &str = "FFmpeg isn't installed. Add the FFmpeg pack in Settings › Packs.";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static OVERRIDE_DIR: LazyLock<RwLock<Option<PathBuf>>> = LazyLock::new(|| RwLock::new(None));

/// Availability and capability information for the FFmpeg installation used by
/// KeepItLocal.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FfmpegStatus {
    pub available: bool,
    pub recorder_available: bool,
    pub ffprobe_available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// `pack` for Search's FFmpeg pack, `user` for an explicitly selected
    /// installation, `path` for PATH, or `none` when nothing resolves.
    pub source: String,
}

fn binary_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

fn selected_directory(picked: &Path) -> Result<PathBuf, String> {
    if picked.is_dir() {
        return Ok(picked.to_path_buf());
    }
    picked
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "bad_path".to_string())
}

fn pack_dir() -> Option<PathBuf> {
    crate::core::packs::bin_dir("ffmpeg")
}

fn override_dir() -> Option<PathBuf> {
    OVERRIDE_DIR.read().ok().and_then(|dir| dir.clone())
}

fn set_override_dir(dir: Option<PathBuf>) {
    if let Ok(mut current) = OVERRIDE_DIR.write() {
        *current = dir;
    }
}

fn new_command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn runs(program: &Path) -> bool {
    new_command(program)
        .args(["-hide_banner", "-version"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn resolve(stem: &str) -> Option<PathBuf> {
    for dir in pack_dir().into_iter().chain(override_dir()) {
        let selected = dir.join(binary_name(stem));
        if selected.is_file() && runs(&selected) {
            return Some(selected);
        }
    }

    let on_path = PathBuf::from(stem);
    runs(&on_path).then_some(on_path)
}

/// Resolve the pack's, the user-selected or the PATH-provided FFmpeg executable.
pub fn resolve_ffmpeg() -> Option<PathBuf> {
    resolve("ffmpeg")
}

/// Resolve the companion ffprobe executable when a media operation needs it.
pub fn resolve_ffprobe() -> Option<PathBuf> {
    resolve("ffprobe")
}

fn supports_encoder(ffmpeg: &Path, encoder: &str) -> bool {
    let Ok(output) = new_command(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return false;
    };

    output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .any(|entry| entry == encoder)
}

/// Build an FFmpeg command without showing a console window on Windows.
pub fn ffmpeg_command() -> Result<Command, String> {
    let path = resolve_ffmpeg().ok_or_else(|| {
        NOT_FOUND.to_string()
    })?;
    Ok(new_command(path))
}

/// Build an FFmpeg command suitable for Screen Recorder's H.264 output.
pub fn screen_recording_command() -> Result<Command, String> {
    let path = resolve_ffmpeg().ok_or_else(|| {
        NOT_FOUND.to_string()
    })?;
    if !supports_encoder(&path, "h264_mf") {
        return Err(
            "This FFmpeg installation cannot record with Windows H.264 (h264_mf). Choose a full Windows FFmpeg build."
                .to_string(),
        );
    }
    Ok(new_command(path))
}

/// Build an ffprobe command for media operations that need the input duration.
pub fn ffprobe_command() -> Result<Command, String> {
    let path = resolve_ffprobe().ok_or_else(|| {
        "ffprobe was not found beside your FFmpeg installation. Choose a full FFmpeg build with ffprobe."
            .to_string()
    })?;
    Ok(new_command(path))
}

/// Pick the best MP3 encoder offered by the user's FFmpeg build.
pub fn mp3_encoder() -> Result<&'static str, String> {
    let path = resolve_ffmpeg().ok_or_else(|| {
        NOT_FOUND.to_string()
    })?;
    if supports_encoder(&path, "libmp3lame") {
        Ok("libmp3lame")
    } else if supports_encoder(&path, "mp3") {
        Ok("mp3")
    } else {
        Err("This FFmpeg installation cannot encode MP3. Choose a full FFmpeg build.".to_string())
    }
}

fn version_line(program: &Path) -> Option<String> {
    let output = new_command(program)
        .args(["-hide_banner", "-version"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(|line| line.trim().to_string())
}

fn current_status() -> FfmpegStatus {
    let path = resolve_ffmpeg();
    let available = path.is_some();
    let recorder_available = path
        .as_deref()
        .is_some_and(|ffmpeg| supports_encoder(ffmpeg, "h264_mf"));
    let source = match path.as_deref() {
        Some(ffmpeg) if pack_dir().is_some_and(|dir| ffmpeg.starts_with(dir)) => "pack",
        Some(ffmpeg) if override_dir().is_some_and(|dir| ffmpeg.starts_with(dir)) => "user",
        Some(_) => "path",
        None => "none",
    }
    .to_string();

    FfmpegStatus {
        available,
        recorder_available,
        ffprobe_available: resolve_ffprobe().is_some(),
        version: path.as_deref().and_then(version_line),
        path: path.map(|value| value.to_string_lossy().to_string()),
        source,
    }
}

/// Restore a previously selected directory before any background command can
/// attempt to start a recording.
pub fn hydrate_override(app: &AppHandle) {
    let Ok(Some(path)) = secure_kv::secure_kv_get(app.clone(), FFMPEG_OVERRIDE_KEY.into()) else {
        return;
    };
    let path = path.trim();
    if !path.is_empty() {
        set_override_dir(Some(PathBuf::from(path)));
    }
}

#[tauri::command]
pub fn ffmpeg_status() -> FfmpegStatus {
    current_status()
}

/// Accept an `ffmpeg` binary or its parent directory, verify it runs, then
/// persist the directory as the explicit user choice.
#[tauri::command]
pub fn ffmpeg_set_path(app: AppHandle, path: String) -> Result<FfmpegStatus, String> {
    let directory = selected_directory(Path::new(path.trim()))?;
    let executable = directory.join(binary_name("ffmpeg"));
    if !executable.is_file() {
        return Err("no_ffmpeg".into());
    }
    if !runs(&executable) {
        return Err("not_runnable".into());
    }
    let value = directory
        .to_str()
        .ok_or_else(|| "bad_path".to_string())?
        .to_string();
    secure_kv::secure_kv_set(app, FFMPEG_OVERRIDE_KEY.into(), value)?;
    set_override_dir(Some(directory));
    Ok(current_status())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_ffmpeg_binary_uses_its_parent_directory() {
        let picked = Path::new(r"C:\Tools\ffmpeg\bin\ffmpeg.exe");
        assert_eq!(
            selected_directory(picked).expect("binary path has a parent"),
            PathBuf::from(r"C:\Tools\ffmpeg\bin")
        );
    }
}
