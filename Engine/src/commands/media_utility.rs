//! Small, local-only media operations backed by the user's FFmpeg install.
//!
//! v1 deliberately contains only MP3 extraction and target-size MP4 compression.
//! It is not a general video converter or editor.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const MEBIBYTE: u64 = 1024 * 1024;
const AUDIO_BITRATE: u64 = 128_000;
const SIZE_SAFETY_MARGIN: f64 = 0.90;
const MIN_VIDEO_BITRATE: u64 = 400_000;
const MEDIA_PROGRESS_EVENT: &str = "media-progress";
const MEDIA_OPERATION_CANCELLED: &str = "Media operation cancelled.";
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Default)]
struct MediaOperation {
    cancelled: bool,
    child: Option<Arc<Mutex<Child>>>,
}

static MEDIA_OPERATIONS: LazyLock<Mutex<HashMap<String, MediaOperation>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct MediaOperationGuard {
    operation_id: String,
}

impl MediaOperationGuard {
    fn new(operation_id: String) -> Self {
        Self { operation_id }
    }
}

impl Drop for MediaOperationGuard {
    fn drop(&mut self) {
        clear_media_operation(&self.operation_id);
    }
}

#[derive(Clone, Serialize)]
struct MediaProgressEvent {
    operation_id: String,
    stage: String,
    progress: Option<u8>,
    processed_seconds: f64,
    duration_seconds: Option<f64>,
}

struct FfmpegRun {
    processed_seconds: f64,
}

struct FfmpegRunError {
    message: String,
    processed_seconds: f64,
}

impl FfmpegRunError {
    fn new(message: impl Into<String>, processed_seconds: f64) -> Self {
        Self {
            message: message.into(),
            processed_seconds,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaUtilityResult {
    pub output_path: String,
    pub original_bytes: u64,
    pub output_bytes: u64,
}

fn register_media_operation(operation_id: &str) -> Result<(), String> {
    if operation_id.trim().is_empty() {
        return Err("Missing media operation id.".into());
    }
    MEDIA_OPERATIONS
        .lock()
        .map_err(|_| "Media operation registry lock failed.".to_string())?
        .entry(operation_id.to_string())
        .or_default();
    Ok(())
}

fn clear_media_operation(operation_id: &str) {
    if let Ok(mut operations) = MEDIA_OPERATIONS.lock() {
        operations.remove(operation_id);
    }
}

fn is_media_operation_cancelled(operation_id: &str) -> bool {
    MEDIA_OPERATIONS
        .lock()
        .ok()
        .and_then(|operations| operations.get(operation_id).map(media_operation_is_cancelled))
        .unwrap_or(false)
}

fn media_operation_is_cancelled(operation: &MediaOperation) -> bool {
    operation.cancelled
}

fn request_media_cancel(operation: &mut MediaOperation) -> Option<Arc<Mutex<Child>>> {
    operation.cancelled = true;
    operation.child.clone()
}

fn register_media_child(operation_id: &str, child: Arc<Mutex<Child>>) -> Result<bool, String> {
    let mut operations = MEDIA_OPERATIONS
        .lock()
        .map_err(|_| "Media operation registry lock failed.".to_string())?;
    let operation = operations.entry(operation_id.to_string()).or_default();
    operation.child = Some(child);
    Ok(operation.cancelled)
}

fn clear_media_child(operation_id: &str) {
    if let Ok(mut operations) = MEDIA_OPERATIONS.lock() {
        if let Some(operation) = operations.get_mut(operation_id) {
            operation.child = None;
        }
    }
}

fn kill_media_child(child: &Arc<Mutex<Child>>) {
    if let Ok(mut child) = child.lock() {
        let _ = child.kill();
    }
}

fn try_wait_media_child(child: &Arc<Mutex<Child>>) -> Result<Option<ExitStatus>, String> {
    child
        .lock()
        .map_err(|_| "Media process lock failed.".to_string())?
        .try_wait()
        .map_err(|error| format!("Could not wait for FFmpeg: {error}"))
}

#[tauri::command]
pub fn cancel_media_operation(operation_id: String) -> Result<(), String> {
    if operation_id.trim().is_empty() {
        return Err("Missing media operation id.".into());
    }

    let child = {
        let mut operations = MEDIA_OPERATIONS
            .lock()
            .map_err(|_| "Media operation registry lock failed.".to_string())?;
        let operation = operations.entry(operation_id).or_default();
        request_media_cancel(operation)
    };
    if let Some(child) = child {
        kill_media_child(&child);
    }
    Ok(())
}

fn emit_media_progress(
    app: &AppHandle,
    operation_id: &str,
    stage: &str,
    progress: Option<u8>,
    processed_seconds: f64,
    duration_seconds: Option<f64>,
) {
    let _ = app.emit(
        MEDIA_PROGRESS_EVENT,
        MediaProgressEvent {
            operation_id: operation_id.to_string(),
            stage: stage.to_string(),
            progress,
            processed_seconds,
            duration_seconds,
        },
    );
}

fn parse_ffmpeg_progress_seconds(line: &str) -> Option<f64> {
    let (key, value) = line.trim().split_once('=')?;
    match key {
        "out_time_us" | "out_time_ms" => value
            .parse::<u64>()
            .ok()
            .map(|microseconds| microseconds as f64 / 1_000_000.0),
        "out_time" => {
            let mut parts = value.split(':');
            let hours = parts.next()?.parse::<f64>().ok()?;
            let minutes = parts.next()?.parse::<f64>().ok()?;
            let seconds = parts.next()?.parse::<f64>().ok()?;
            if parts.next().is_some()
                || !hours.is_finite()
                || !minutes.is_finite()
                || !seconds.is_finite()
                || hours < 0.0
                || minutes < 0.0
                || seconds < 0.0
            {
                return None;
            }
            Some(hours * 3_600.0 + minutes * 60.0 + seconds)
        }
        _ => None,
    }
}

fn interim_progress_percent(processed_seconds: f64, duration_seconds: Option<f64>) -> Option<u8> {
    let duration_seconds = duration_seconds?;
    if !processed_seconds.is_finite() || !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return None;
    }
    Some(
        ((processed_seconds / duration_seconds) * 100.0)
            .floor()
            .clamp(0.0, 99.0) as u8,
    )
}

fn ensure_extension(path: &Path, expected: &str) -> Result<(), String> {
    let actual = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!("Output must use the .{expected} extension."))
    }
}

fn unique_temp_path(output: &Path) -> Result<PathBuf, String> {
    let parent = output
        .parent()
        .ok_or_else(|| "Output path has no parent directory.".to_string())?;
    let stem = output
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("media");
    let extension = output
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("tmp");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "System clock is unavailable.".to_string())?
        .as_nanos();

    for suffix in 0..100u8 {
        let candidate = parent.join(format!(
            ".{stem}.keepitlocal-{}-{nonce}-{suffix}.{extension}",
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err("Could not reserve a temporary output path. Try again.".into())
}

fn prepare_paths(
    input_path: &str,
    output_path: &str,
    extension: &str,
) -> Result<(PathBuf, PathBuf, PathBuf, u64), String> {
    let input = crate::core::safe_path::validate_user_path(input_path)?;
    let output = crate::core::safe_path::validate_user_write_target(output_path)?;
    ensure_extension(&output, extension)?;
    if input == output {
        return Err("Choose a different file for the output.".into());
    }
    if output.exists() {
        return Err("That output file already exists. Choose a new name to keep it safe.".into());
    }
    let original_bytes = fs::metadata(&input)
        .map_err(|error| format!("Could not read the input file: {error}"))?
        .len();
    let temp = unique_temp_path(&output)?;
    Ok((input, output, temp, original_bytes))
}

fn output_error(operation: &str, output: &Output) -> String {
    ffmpeg_error(operation, &output.stderr)
}

fn ffmpeg_error(operation: &str, stderr: &[u8]) -> String {
    let details = String::from_utf8_lossy(stderr).trim().to_string();
    if details.is_empty() {
        format!("FFmpeg could not {operation}.")
    } else {
        format!("FFmpeg could not {operation}: {details}")
    }
}

fn run_ffmpeg(
    mut command: Command,
    app: &AppHandle,
    operation_id: &str,
    operation: &str,
    duration_seconds: Option<f64>,
) -> Result<FfmpegRun, FfmpegRunError> {
    if is_media_operation_cancelled(operation_id) {
        return Err(FfmpegRunError::new(MEDIA_OPERATION_CANCELLED, 0.0));
    }

    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| FfmpegRunError::new(format!("Could not start FFmpeg: {error}"), 0.0))?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(FfmpegRunError::new(
            "Could not read FFmpeg progress output.",
            0.0,
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(FfmpegRunError::new(
            "Could not read FFmpeg error output.",
            0.0,
        ));
    };

    let child = Arc::new(Mutex::new(child));
    let cancelled_before_tracking = match register_media_child(operation_id, child.clone()) {
        Ok(cancelled) => cancelled,
        Err(error) => {
            kill_media_child(&child);
            return Err(FfmpegRunError::new(error, 0.0));
        }
    };
    let mut kill_requested = cancelled_before_tracking;
    if kill_requested {
        kill_media_child(&child);
    }

    let (progress_sender, progress_receiver) = mpsc::channel();
    let stdout_handle = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if progress_sender.send(line).is_err() {
                break;
            }
        }
    });
    let stderr_handle = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = BufReader::new(stderr).read_to_end(&mut bytes);
        bytes
    });

    let mut processed_seconds: f64 = 0.0;
    let mut progress_read_error = None;
    let mut progress_closed = false;
    let status = loop {
        if progress_closed {
            thread::sleep(PROCESS_POLL_INTERVAL);
        } else {
            match progress_receiver.recv_timeout(PROCESS_POLL_INTERVAL) {
                Ok(Ok(line)) => {
                    if let Some(seconds) = parse_ffmpeg_progress_seconds(&line) {
                        processed_seconds = processed_seconds.max(seconds);
                        emit_media_progress(
                            app,
                            operation_id,
                            "processing",
                            interim_progress_percent(processed_seconds, duration_seconds),
                            processed_seconds,
                            duration_seconds,
                        );
                    }
                }
                Ok(Err(error)) => {
                    progress_read_error = Some(format!("Could not read FFmpeg progress: {error}"));
                    progress_closed = true;
                    if !kill_requested {
                        kill_media_child(&child);
                        kill_requested = true;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => progress_closed = true,
            }
        }

        match try_wait_media_child(&child) {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => {
                if !kill_requested {
                    kill_media_child(&child);
                }
                progress_read_error.get_or_insert(error);
                break None;
            }
        }

        if !kill_requested && is_media_operation_cancelled(operation_id) {
            kill_media_child(&child);
            kill_requested = true;
        }
    };

    while let Ok(Ok(line)) = progress_receiver.try_recv() {
        if let Some(seconds) = parse_ffmpeg_progress_seconds(&line) {
            processed_seconds = processed_seconds.max(seconds);
            emit_media_progress(
                app,
                operation_id,
                "processing",
                interim_progress_percent(processed_seconds, duration_seconds),
                processed_seconds,
                duration_seconds,
            );
        }
    }
    let _ = stdout_handle.join();
    let stderr = stderr_handle.join().unwrap_or_default();
    clear_media_child(operation_id);

    if is_media_operation_cancelled(operation_id) {
        Err(FfmpegRunError::new(
            MEDIA_OPERATION_CANCELLED,
            processed_seconds,
        ))
    } else if let Some(error) = progress_read_error {
        Err(FfmpegRunError::new(error, processed_seconds))
    } else if status.is_some_and(|status| status.success()) {
        Ok(FfmpegRun { processed_seconds })
    } else {
        Err(FfmpegRunError::new(
            ffmpeg_error(operation, &stderr),
            processed_seconds,
        ))
    }
}

fn finish_output(
    temp: &Path,
    output: &Path,
    original_bytes: u64,
) -> Result<MediaUtilityResult, String> {
    let output_bytes = fs::metadata(temp)
        .map_err(|error| format!("FFmpeg did not create an output file: {error}"))?
        .len();
    fs::rename(temp, output).map_err(|error| format!("Could not save the output file: {error}"))?;
    Ok(MediaUtilityResult {
        output_path: output.to_string_lossy().to_string(),
        original_bytes,
        output_bytes,
    })
}

fn settle_media_operation(
    app: &AppHandle,
    operation_id: &str,
    temp: &Path,
    duration_seconds: Option<f64>,
    result: Result<(MediaUtilityResult, FfmpegRun), FfmpegRunError>,
) -> Result<MediaUtilityResult, String> {
    match result {
        Ok((result, run)) => {
            emit_media_progress(
                app,
                operation_id,
                "completed",
                Some(100),
                run.processed_seconds,
                duration_seconds,
            );
            Ok(result)
        }
        Err(error) => {
            let _ = fs::remove_file(temp);
            if is_media_operation_cancelled(operation_id)
                || error.message == MEDIA_OPERATION_CANCELLED
            {
                emit_media_progress(
                    app,
                    operation_id,
                    "cancelled",
                    None,
                    error.processed_seconds,
                    duration_seconds,
                );
                Err(MEDIA_OPERATION_CANCELLED.into())
            } else {
                emit_media_progress(
                    app,
                    operation_id,
                    "error",
                    None,
                    error.processed_seconds,
                    duration_seconds,
                );
                Err(error.message)
            }
        }
    }
}

fn extract_audio_blocking(
    app: AppHandle,
    operation_id: String,
    input_path: String,
    output_path: String,
) -> Result<MediaUtilityResult, String> {
    if is_media_operation_cancelled(&operation_id) {
        emit_media_progress(&app, &operation_id, "cancelled", None, 0.0, None);
        return Err(MEDIA_OPERATION_CANCELLED.into());
    }

    let (input, output, temp, original_bytes) = match prepare_paths(&input_path, &output_path, "mp3") {
        Ok(paths) => paths,
        Err(error) => {
            if is_media_operation_cancelled(&operation_id) {
                emit_media_progress(&app, &operation_id, "cancelled", None, 0.0, None);
                return Err(MEDIA_OPERATION_CANCELLED.into());
            }
            emit_media_progress(&app, &operation_id, "error", None, 0.0, None);
            return Err(error);
        }
    };
    let result = (|| {
        let encoder = crate::commands::ffmpeg::mp3_encoder()
            .map_err(|error| FfmpegRunError::new(error, 0.0))?;
        let temp_arg = temp
            .to_str()
            .ok_or_else(|| FfmpegRunError::new("Output path is not valid UTF-8", 0.0))?;
        let input_arg = input
            .to_str()
            .ok_or_else(|| FfmpegRunError::new("Input path is not valid UTF-8", 0.0))?;
        let mut command = crate::commands::ffmpeg::ffmpeg_command()
            .map_err(|error| FfmpegRunError::new(error, 0.0))?;
        command.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-progress",
            "pipe:1",
            "-nostats",
            "-i",
            input_arg,
            "-vn",
            "-c:a",
            encoder,
            "-q:a",
            "2",
            "-n",
            temp_arg,
        ]);

        let run = run_ffmpeg(command, &app, &operation_id, "extract the audio", None)?;
        if is_media_operation_cancelled(&operation_id) {
            return Err(FfmpegRunError::new(
                MEDIA_OPERATION_CANCELLED,
                run.processed_seconds,
            ));
        }
        let result = finish_output(&temp, &output, original_bytes)
            .map_err(|error| FfmpegRunError::new(error, run.processed_seconds))?;
        Ok((result, run))
    })();
    settle_media_operation(&app, &operation_id, &temp, None, result)
}

fn input_duration_seconds(input: &Path) -> Result<f64, String> {
    let input_arg = input.to_str().ok_or("Input path is not valid UTF-8")?;
    let output = crate::commands::ffmpeg::ffprobe_command()?
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            input_arg,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Could not start ffprobe: {error}"))?;
    if !output.status.success() {
        return Err(output_error("read this video", &output));
    }
    let duration = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .map_err(|_| "ffprobe could not read this video's duration.".to_string())?;
    if !duration.is_finite() || duration <= 0.0 {
        return Err("This video has no usable duration.".into());
    }
    Ok(duration)
}

fn target_bytes(target_megabytes: u64) -> Result<u64, String> {
    if target_megabytes == 0 {
        return Err("Choose a target size greater than 0 MB.".into());
    }
    target_megabytes
        .checked_mul(MEBIBYTE)
        .ok_or_else(|| "That target size is too large.".into())
}

fn compute_video_bitrate(target_bytes: u64, duration_seconds: f64) -> Result<u64, String> {
    let total_bitrate = (target_bytes as f64 * 8.0 * SIZE_SAFETY_MARGIN / duration_seconds) as u64;
    let video_bitrate = total_bitrate.saturating_sub(AUDIO_BITRATE);
    if video_bitrate < MIN_VIDEO_BITRATE {
        return Err(
            "This video is too long for that target size without making it unwatchable. Choose a larger target."
                .into(),
        );
    }
    Ok(video_bitrate)
}

fn compress_video_blocking(
    app: AppHandle,
    operation_id: String,
    input_path: String,
    output_path: String,
    target_megabytes: u64,
) -> Result<MediaUtilityResult, String> {
    if is_media_operation_cancelled(&operation_id) {
        emit_media_progress(&app, &operation_id, "cancelled", None, 0.0, None);
        return Err(MEDIA_OPERATION_CANCELLED.into());
    }

    let (input, output, temp, original_bytes) = match prepare_paths(&input_path, &output_path, "mp4") {
        Ok(paths) => paths,
        Err(error) => {
            if is_media_operation_cancelled(&operation_id) {
                emit_media_progress(&app, &operation_id, "cancelled", None, 0.0, None);
                return Err(MEDIA_OPERATION_CANCELLED.into());
            }
            emit_media_progress(&app, &operation_id, "error", None, 0.0, None);
            return Err(error);
        }
    };
    let duration = match input_duration_seconds(&input) {
        Ok(duration) => duration,
        Err(error) => {
            let _ = fs::remove_file(&temp);
            if is_media_operation_cancelled(&operation_id) {
                emit_media_progress(&app, &operation_id, "cancelled", None, 0.0, None);
                return Err(MEDIA_OPERATION_CANCELLED.into());
            }
            emit_media_progress(&app, &operation_id, "error", None, 0.0, None);
            return Err(error);
        }
    };
    if is_media_operation_cancelled(&operation_id) {
        let _ = fs::remove_file(&temp);
        emit_media_progress(
            &app,
            &operation_id,
            "cancelled",
            None,
            0.0,
            Some(duration),
        );
        return Err(MEDIA_OPERATION_CANCELLED.into());
    }
    emit_media_progress(
        &app,
        &operation_id,
        "starting",
        None,
        0.0,
        Some(duration),
    );

    let result = (|| {
        let target_bytes = target_bytes(target_megabytes)
            .map_err(|error| FfmpegRunError::new(error, 0.0))?;
        let video_bitrate = compute_video_bitrate(target_bytes, duration)
            .map_err(|error| FfmpegRunError::new(error, 0.0))?;
        let max_buffer = video_bitrate.saturating_mul(2).to_string();
        let video_bitrate = video_bitrate.to_string();
        let input_arg = input
            .to_str()
            .ok_or_else(|| FfmpegRunError::new("Input path is not valid UTF-8", 0.0))?;
        let temp_arg = temp
            .to_str()
            .ok_or_else(|| FfmpegRunError::new("Output path is not valid UTF-8", 0.0))?;
        let mut command = crate::commands::ffmpeg::screen_recording_command()
            .map_err(|error| FfmpegRunError::new(error, 0.0))?;
        command.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-progress",
            "pipe:1",
            "-nostats",
            "-i",
            input_arg,
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
            "-c:v",
            "h264_mf",
            "-rate_control",
            "cbr",
            "-b:v",
            &video_bitrate,
            "-maxrate",
            &video_bitrate,
            "-bufsize",
            &max_buffer,
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-movflags",
            "+faststart",
            "-n",
            temp_arg,
        ]);

        let run = run_ffmpeg(
            command,
            &app,
            &operation_id,
            "compress this video",
            Some(duration),
        )?;
        if is_media_operation_cancelled(&operation_id) {
            return Err(FfmpegRunError::new(
                MEDIA_OPERATION_CANCELLED,
                run.processed_seconds,
            ));
        }
        let result = finish_output(&temp, &output, original_bytes)
            .map_err(|error| FfmpegRunError::new(error, run.processed_seconds))?;
        Ok((result, run))
    })();
    settle_media_operation(&app, &operation_id, &temp, Some(duration), result)
}

#[tauri::command]
pub async fn media_extract_audio(
    app: AppHandle,
    operation_id: String,
    input_path: String,
    output_path: String,
) -> Result<MediaUtilityResult, String> {
    register_media_operation(&operation_id)?;
    emit_media_progress(&app, &operation_id, "starting", None, 0.0, None);
    let worker_app = app.clone();
    let worker_operation_id = operation_id.clone();
    match tauri::async_runtime::spawn_blocking(move || {
        let _operation_guard = MediaOperationGuard::new(worker_operation_id.clone());
        extract_audio_blocking(worker_app, worker_operation_id, input_path, output_path)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => {
            clear_media_operation(&operation_id);
            let error = format!("Media worker failed: {error}");
            emit_media_progress(&app, &operation_id, "error", None, 0.0, None);
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn media_compress_video(
    app: AppHandle,
    operation_id: String,
    input_path: String,
    output_path: String,
    target_megabytes: u64,
) -> Result<MediaUtilityResult, String> {
    register_media_operation(&operation_id)?;
    emit_media_progress(&app, &operation_id, "starting", None, 0.0, None);
    let worker_app = app.clone();
    let worker_operation_id = operation_id.clone();
    match tauri::async_runtime::spawn_blocking(move || {
        let _operation_guard = MediaOperationGuard::new(worker_operation_id.clone());
        compress_video_blocking(
            worker_app,
            worker_operation_id,
            input_path,
            output_path,
            target_megabytes,
        )
    })
    .await
    {
        Ok(result) => result,
        Err(error) => {
            clear_media_operation(&operation_id);
            let error = format!("Media worker failed: {error}");
            emit_media_progress(&app, &operation_id, "error", None, 0.0, None);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computed_bitrate_reserves_audio_and_safety_margin() {
        let bitrate = compute_video_bitrate(25 * MEBIBYTE, 60.0).expect("one-minute video fits");
        assert!(bitrate > 3_000_000 && bitrate < 3_100_000);
    }

    #[test]
    fn computed_bitrate_rejects_an_unwatchable_target() {
        let error =
            compute_video_bitrate(10 * MEBIBYTE, 600.0).expect_err("ten minutes cannot fit");
        assert!(error.contains("too long"));
    }

    #[test]
    fn target_bytes_accepts_large_custom_targets() {
        assert_eq!(target_bytes(2_048).expect("2 GB target"), 2_048 * MEBIBYTE);
    }

    #[test]
    fn parses_ffmpeg_progress_timestamps() {
        assert_eq!(parse_ffmpeg_progress_seconds("out_time_us=1250000"), Some(1.25));
        assert_eq!(
            parse_ffmpeg_progress_seconds("out_time=01:02:03.5"),
            Some(3_723.5)
        );
        assert_eq!(parse_ffmpeg_progress_seconds("progress=continue"), None);
    }

    #[test]
    fn interim_compression_progress_is_duration_based_and_never_complete() {
        assert_eq!(interim_progress_percent(30.0, Some(120.0)), Some(25));
        assert_eq!(interim_progress_percent(120.0, Some(120.0)), Some(99));
        assert_eq!(interim_progress_percent(1.0, None), None);
    }

    #[test]
    fn cancellation_requested_before_spawn_stays_set() {
        let mut operation = MediaOperation::default();
        assert!(!media_operation_is_cancelled(&operation));
        assert!(request_media_cancel(&mut operation).is_none());
        assert!(media_operation_is_cancelled(&operation));
    }
}
