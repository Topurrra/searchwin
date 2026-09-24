//! Screen Recorder — Stage 2: encode captured BGRA frames to MP4 via ffmpeg.
//!
//! The capture engine ([`super::start_capture`]) delivers BGRA frames on a
//! channel at the monitor's refresh cadence. This stage drives a **constant-
//! framerate (CFR) pump**: a single monotonic clock (`t0`) decides how many
//! frames should exist by now (`round((now-t0)*fps)`), and we write the latest
//! captured frame that many times — duplicating when capture is idle, coalescing
//! when it is ahead. rawvideo carries no timestamps, so the pump *is* the clock;
//! this keeps the output real-time and drift-free.
//!
//! Hard correctness rules (each a known footgun):
//!   - **Drain stderr** on its own thread for the whole session, or ffmpeg blocks
//!     writing stderr while we block writing stdin → deadlock (the repo already
//!     has the trap pattern in cron_tasks.rs).
//!   - **Stop = close stdin (EOF)**, never kill — killing truncates the file.
//!   - **Fragmented MP4** (`frag_keyframe+empty_moov+default_base_moof`) so even a
//!     hard-killed/crashed recording stays playable (no missing moov atom).
//!   - **Windows H.264** via `h264_mf` (Media Foundation), never libx264.
#![cfg(feature = "screenrec")]
// Live once wired to Tauri commands in Stage 3 (see mod.rs note).
#![allow(dead_code)]

use std::io::{Read, Write};
#[cfg(test)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ExitStatus, Stdio};
#[cfg(test)]
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::audio::{self, AudioCapture, AudioOptions};
use super::{start_capture, CaptureSession, CaptureTarget, Frame};

/// Same flag the rest of the repo uses to keep spawned consoles hidden.
#[cfg(test)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A running MP4 recording. Call [`Recorder::stop`] for a clean finish.
pub struct Recorder {
    stop: Arc<AtomicBool>,
    /// Set by the pump if ffmpeg dies mid-recording (e.g. disk full / encoder
    /// fault). `status()` surfaces it so the UI can stop + finalise the partial
    /// instead of silently "saving" a truncated file.
    failed: Arc<AtomicBool>,
    pump: Option<JoinHandle<Result<(), String>>>,
    capture: Option<CaptureSession>,
    child: Option<Child>,
    stderr_drain: Option<JoinHandle<()>>,
    /// Where the VIDEO is written: a temp file when audio is on (so we can mux at
    /// stop), otherwise the user's final path.
    out_path: PathBuf,
    /// The user's final output path (== `out_path` when there is no audio).
    final_path: PathBuf,
    /// Live audio capture (system loopback + mic), muxed into `final_path` at stop.
    audio: Option<AudioCapture>,
    /// A/V sync trim applied at mux: positive delays audio (when audio leads the
    /// video), negative delays video. Calibrated by the user per machine.
    audio_sync_ms: i32,
    /// The video reference clock (pump start). Paired with `pause` to report the
    /// recorded (pause-excluded) duration.
    t0: Instant,
    /// Pause/resume state shared with the pump + audio lanes (see [`super::PauseState`]).
    pause: super::SharedPause,
}

/// Start recording the primary monitor to `out_path` (an `.mp4`) at `fps`.
///
/// Video-only for Stage 2. Blocks briefly to capture the first frame (which fixes
/// the output dimensions for the take).
pub fn start_recording(
    out_path: &Path,
    fps: u32,
    region: Option<super::CropRect>,
    redactions: Vec<super::CropRect>,
    exclude_windows: Vec<i64>,
    audio_opts: AudioOptions,
    audio_sync_ms: i32,
    quality_bpp: f64,
) -> Result<Recorder, String> {
    let fps = fps.clamp(1, 240);
    let crop: super::SharedCrop = Arc::new(Mutex::new(region));
    let redact: super::SharedRedactions = Arc::new(Mutex::new(redactions));
    let exclude: super::SharedExcludeWindows = Arc::new(Mutex::new(exclude_windows));
    let (capture, rx) =
        start_capture(CaptureTarget::PrimaryMonitor, crop.clone(), redact, exclude, true)?;

    // First frame → output geometry (fixed for the take; ffmpeg -s is immutable).
    let first = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "No frame captured within 5s".to_string())?;
    // Output dimensions are fixed for the take and MUST be even (h264). A drawn
    // region can be any size, so round the first frame DOWN to even and letterbox
    // every frame — including this first one — into it. (The pump already
    // letterboxes later/resized frames via fit_bgra_to.)
    let w = (first.width - first.width % 2).max(2);
    let h = (first.height - first.height % 2).max(2);
    let first = if first.width == w && first.height == h {
        first
    } else {
        Frame { width: w, height: h, bgra: fit_bgra_to(&first, w, h) }
    };

    // One pause handle shared by the video pump and every audio lane, so a pause
    // freezes both in lock-step (see super::PauseState).
    let pause: super::SharedPause = Arc::new(super::PauseState::new());

    // Start audio capture as close to the video timeline as possible (right after
    // the first frame fixes geometry, just before the video encoder spawns) so the
    // two streams line up. Each source encodes to its own temp AAC; we mux them in
    // at stop. An audio glitch never fails the recording — it degrades to silent.
    let audio = if audio_opts.any() {
        let dir = out_path.parent().unwrap_or_else(|| Path::new("."));
        let stem = format!(
            "{}.kiltmp",
            out_path.file_stem().and_then(|s| s.to_str()).unwrap_or("recording")
        );
        match audio::start_audio(dir, &stem, audio_opts, pause.clone()) {
            Ok(cap) if cap.active() > 0 => Some(cap),
            Ok(_) => None, // no source came up (no devices) → silent video
            Err(e) => {
                eprintln!("[screenrec] audio disabled: {e}");
                None
            }
        }
    } else {
        None
    };
    // With audio, the video goes to a temp file and is muxed with the audio into
    // the final path at stop; without audio it writes straight to the final path.
    let final_path = out_path.to_path_buf();
    let video_path = if audio.is_some() {
        out_path.with_extension("kiltmp.video.mp4")
    } else {
        final_path.clone()
    };

    // Bitrate TARGET scales with resolution × fps × the preset's bits-per-pixel
    // budget. Screen content (sharp text + bursty motion like fast-scrolling a
    // dense document) is the worst case for H.264, so we pair the target with
    // unconstrained VBR + the "archive" scenario (below): the encoder spikes well
    // above target during a burst — clean fast scrolls — while averaging near it
    // for sane file size. `quality_bpp` comes from the chosen preset (Small ≈ 0.08,
    // Balanced ≈ 0.15, High ≈ 0.22). Clamped so tiny/huge captures stay sane.
    let bpp = quality_bpp.clamp(0.02, 0.5);
    let bitrate =
        ((w as f64 * h as f64 * fps as f64 * bpp) as u64).clamp(2_000_000, 80_000_000);
    let bitrate_arg = bitrate.to_string();

    let mut child = crate::commands::ffmpeg::screen_recording_command()?
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            // Raw BGRA video on stdin at the chosen CFR.
            "-f",
            "rawvideo",
            "-pix_fmt",
            "bgra",
            "-s",
            &format!("{w}x{h}"),
            "-r",
            &fps.to_string(),
            "-i",
            "pipe:0",
            "-an", // video only (Stage 4 adds audio)
            // License-clean H.264 via Media Foundation; convert BGRA → yuv420p.
            "-c:v",
            "h264_mf",
            // Unconstrained VBR + screen-content scenario: lets the encoder spend
            // extra bits on bursty motion (fast text scrolls) rather than blocking
            // up at a hard bitrate ceiling.
            "-rate_control",
            "u_vbr",
            "-scenario",
            "archive",
            "-pix_fmt",
            "yuv420p",
            "-b:v",
            &bitrate_arg,
            // Crash-safe fragmented MP4: a killed/crashed file is still playable.
            "-movflags",
            "frag_keyframe+empty_moov+default_base_moof",
            "-y",
            video_path.to_str().ok_or("Output path is not valid UTF-8")?,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to launch ffmpeg: {e}"))?;

    let stdin = child.stdin.take().ok_or("ffmpeg stdin unavailable")?;

    // Drain ffmpeg stderr for the whole session (deadlock guard).
    let stderr_drain = child.stderr.take().map(|mut err| {
        thread::Builder::new()
            .name("kil-ffmpeg-stderr".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = err.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                }
            })
            .expect("spawn stderr drain")
    });

    let stop = Arc::new(AtomicBool::new(false));
    let stop_pump = stop.clone();
    let failed = Arc::new(AtomicBool::new(false));
    let failed_pump = failed.clone();
    let t0 = Instant::now(); // the video reference clock
    let pump_pause = pause.clone();
    let pump = thread::Builder::new()
        .name("kil-cfr-pump".into())
        .spawn(move || {
            pump_frames(rx, stdin, first, w, h, fps, stop_pump, failed_pump, t0, pump_pause)
        })
        .map_err(|e| format!("Failed to spawn CFR pump: {e}"))?;

    Ok(Recorder {
        stop,
        failed,
        pump: Some(pump),
        capture: Some(capture),
        child: Some(child),
        stderr_drain,
        out_path: video_path,
        final_path,
        audio,
        audio_sync_ms,
        t0,
        pause,
    })
}

/// Letterbox a BGRA frame into `out_w`×`out_h`, preserving aspect (black bars).
/// Channel order is irrelevant to scaling, so we treat the BGRA bytes as RGBA.
fn fit_bgra_to(src: &Frame, out_w: u32, out_h: u32) -> Vec<u8> {
    use image::{imageops, Rgba, RgbaImage};
    let mut out = RgbaImage::from_pixel(out_w, out_h, Rgba([0, 0, 0, 255]));
    let Some(srcimg) = RgbaImage::from_raw(src.width, src.height, src.bgra.clone()) else {
        return out.into_raw();
    };
    let scale = (out_w as f32 / src.width.max(1) as f32)
        .min(out_h as f32 / src.height.max(1) as f32);
    let sw = ((src.width as f32 * scale).round() as u32).clamp(1, out_w);
    let sh = ((src.height as f32 * scale).round() as u32).clamp(1, out_h);
    let scaled = imageops::resize(&srcimg, sw, sh, imageops::FilterType::Triangle);
    let ox = ((out_w - sw) / 2) as i64;
    let oy = ((out_h - sh) / 2) as i64;
    imageops::overlay(&mut out, &scaled, ox, oy);
    out.into_raw()
}

impl Recorder {
    /// Live on-disk size of the recording so far (video + any audio tracks). With
    /// audio the video goes to a temp file, so the user's final path is empty until
    /// the mux at stop — status must read the files actually being written.
    pub fn output_bytes(&self) -> u64 {
        let video = std::fs::metadata(&self.out_path).map(|m| m.len()).unwrap_or(0);
        let audio = self.audio.as_ref().map(|a| a.bytes_on_disk()).unwrap_or(0);
        video + audio
    }

    /// True if the encoder died mid-recording (the take is truncated; finalising
    /// still yields a playable file thanks to fragmented MP4).
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::SeqCst)
    }

    /// Pause the recording: the video clock freezes and the audio lanes drop samples
    /// (both skip the paused wall-time), so the file gains no frozen segment.
    pub fn pause(&self) {
        self.pause.pause();
    }

    /// Resume a paused recording (no-op if already running).
    pub fn resume(&self) {
        self.pause.resume();
    }

    pub fn is_paused(&self) -> bool {
        self.pause.is_paused()
    }

    /// Recorded duration so far (ms), EXCLUDING paused time — i.e. the length of the
    /// file being produced, which is what the elapsed readout should show.
    pub fn recorded_ms(&self) -> u64 {
        self.pause.effective_elapsed(self.t0).as_millis() as u64
    }

    /// Stop and finalise — ALWAYS bounded, never blocks forever.
    ///
    /// The clean path is: signal stop → the pump breaks, drops stdin (EOF) → ffmpeg
    /// writes its trailer and exits. But the pump's one blocking call is
    /// `stdin.write_all` of a multi-MB frame into ffmpeg's small pipe; if the
    /// encoder wedges, that write never returns and the pump never observes stop.
    /// The ONLY thing that can unblock a stuck write is closing ffmpeg's read end —
    /// i.e. killing ffmpeg — so on timeout we do exactly what `Drop` does: kill it.
    /// The fragmented MP4 stays playable either way. This guarantees `stop()`
    /// returns within a few seconds on every path.
    pub fn stop(mut self) -> Result<PathBuf, String> {
        self.stop.store(true, Ordering::SeqCst);

        // Join the pump without blocking forever. Hand the join to a watchdog
        // thread and wait on a channel; if it doesn't finish promptly the pump is
        // stuck in write_all → kill ffmpeg, which makes that write error out and
        // the pump unwind.
        let mut killed = false;
        if let Some(p) = self.pump.take() {
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            let watchdog = thread::Builder::new()
                .name("kil-pump-join".into())
                .spawn(move || {
                    let r = p.join();
                    let _ = tx.send(());
                    r
                })
                .map_err(|e| format!("Failed to spawn pump-join watchdog: {e}"))?;

            if rx.recv_timeout(Duration::from_secs(2)).is_err() {
                // Pump wedged in write_all — kill ffmpeg to release it.
                if let Some(child) = self.child.as_mut() {
                    let _ = child.kill();
                    killed = true;
                }
                // The write now errors and the pump returns; bounded.
                let _ = rx.recv_timeout(Duration::from_secs(2));
            }
            // Reap the watchdog (bounded: the pump has returned by now).
            let _ = watchdog.join();
        }

        // Stop WGC capture (independent of ffmpeg).
        if let Some(c) = self.capture.take() {
            let _ = c.stop();
        }

        // ffmpeg writes its trailer after EOF and exits. Bound the wait so a stuck
        // finalise can't hang Stop; kill as a last resort (still playable).
        if let Some(mut child) = self.child.take() {
            match wait_bounded(&mut child, Duration::from_secs(5)) {
                // Clean exit: surface a genuine encoder failure, but only if we
                // didn't kill it ourselves (a killed ffmpeg reports non-success).
                Some(status) if !killed && !status.success() => {
                    return Err(format!("ffmpeg exited with {status}"));
                }
                _ => {}
            }
        }
        if let Some(d) = self.stderr_drain.take() {
            let _ = d.join();
        }

        // No audio → the video already IS the final file.
        let Some(audio) = self.audio.take() else {
            return Ok(self.out_path.clone());
        };

        // Stop audio (bounded) and mux it into the final file. If no audio track
        // survived (all devices failed mid-take), just promote the video temp to
        // the final path so the user still gets their recording.
        let tracks = audio.stop();
        if tracks.is_empty() {
            let _ = std::fs::rename(&self.out_path, &self.final_path);
            return Ok(self.final_path.clone());
        }

        let mux = mux_av(&self.out_path, &tracks, &self.final_path, self.audio_sync_ms);
        // Clean up temps regardless (the audio temps are useless on their own).
        for t in &tracks {
            let _ = std::fs::remove_file(t);
        }
        match mux {
            Ok(()) => {
                let _ = std::fs::remove_file(&self.out_path);
                Ok(self.final_path.clone())
            }
            // Mux failed — fall back to the playable video-only temp so nothing is
            // lost; promote it to the final path.
            Err(e) => {
                eprintln!("[screenrec] audio mux failed ({e}); saving video only");
                let _ = std::fs::rename(&self.out_path, &self.final_path);
                Ok(self.final_path.clone())
            }
        }
    }
}

/// A captured track is the microphone if its temp filename carries the "mic"
/// label (see `audio::start_audio`'s `<stem>.<label>.m4a` naming). Used to decide
/// which track gets loudness-normalised at mux.
fn is_mic_track(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).map(|n| n.contains(".mic.")).unwrap_or(false)
}

/// Mux the recorded video with one or more audio tracks into `final_out`.
/// Video is stream-copied (no re-encode); a single audio track is copied (or
/// loudness-normalised if it is the mic), and multiple tracks are mixed with
/// ffmpeg's `amix` (resampling handled by ffmpeg). Finite files → always
/// terminates, so a plain wait is safe.
fn mux_av(
    video: &Path,
    tracks: &[PathBuf],
    final_out: &Path,
    audio_sync_ms: i32,
) -> Result<(), String> {
    let v = video.to_str().ok_or("video path is not valid UTF-8")?;
    let out = final_out.to_str().ok_or("output path is not valid UTF-8")?;

    // A/V sync trim via -itsoffset (applies to the NEXT input). Positive = audio
    // leads → delay the audio inputs; negative = audio lags → delay the video.
    let off = format!("{:.3}", (audio_sync_ms.unsigned_abs() as f64) / 1000.0);

    let mut args: Vec<String> =
        ["-hide_banner", "-loglevel", "error"].iter().map(|s| s.to_string()).collect();
    if audio_sync_ms < 0 {
        args.push("-itsoffset".into());
        args.push(off.clone());
    }
    args.push("-i".into());
    args.push(v.into());
    for t in tracks {
        if audio_sync_ms > 0 {
            args.push("-itsoffset".into());
            args.push(off.clone());
        }
        args.push("-i".into());
        args.push(t.to_str().ok_or("audio track path is not valid UTF-8")?.into());
    }
    if tracks.len() == 1 {
        args.extend(["-map", "0:v:0", "-map", "1:a:0"].iter().map(|s| s.to_string()));
        if is_mic_track(&tracks[0]) {
            // A lone mic track is usually quiet — normalise it loud (re-encode).
            args.extend(
                ["-af", "dynaudnorm", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k"]
                    .iter()
                    .map(|s| s.to_string()),
            );
        } else {
            // System audio is already at level → copy through untouched (instant).
            args.extend(["-c", "copy"].iter().map(|s| s.to_string()));
        }
    } else {
        // Mix all tracks into one stereo AAC track; copy the video. The mic input
        // gets dynaudnorm first so the (quiet) voice sits clearly over the system
        // audio rather than being buried.
        let mut filter = String::new();
        let mut mix = String::new();
        for (i, t) in tracks.iter().enumerate() {
            let inp = i + 1;
            if is_mic_track(t) {
                filter.push_str(&format!("[{inp}:a]dynaudnorm[a{inp}];"));
                mix.push_str(&format!("[a{inp}]"));
            } else {
                mix.push_str(&format!("[{inp}:a]"));
            }
        }
        filter.push_str(&format!("{mix}amix=inputs={}:normalize=0[a]", tracks.len()));
        args.extend(
            ["-filter_complex", &filter, "-map", "0:v:0", "-map", "[a]", "-c:v", "copy", "-c:a",
                "aac", "-b:a", "192k"]
                .iter()
                .map(|s| s.to_string()),
        );
    }
    // Crash-safe fragmented MP4, same as the video path.
    args.extend(
        ["-movflags", "frag_keyframe+empty_moov+default_base_moof", "-y", out]
            .iter()
            .map(|s| s.to_string()),
    );

    let mut child = crate::commands::ffmpeg::screen_recording_command()?
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to launch mux ffmpeg: {e}"))?;
    let drain = spawn_aac_stderr(child.stderr.take());
    let status = child.wait().map_err(|e| format!("mux wait failed: {e}"))?;
    if let Some(d) = drain {
        let _ = d.join();
    }
    if !status.success() {
        return Err(format!("mux ffmpeg exited with {status}"));
    }
    Ok(())
}

/// Drain a child's stderr on its own thread (deadlock guard for the mux pass).
fn spawn_aac_stderr(stderr: Option<std::process::ChildStderr>) -> Option<JoinHandle<()>> {
    stderr.map(|mut err| {
        thread::Builder::new()
            .name("kil-mux-stderr".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = err.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                }
            })
            .expect("spawn mux stderr drain")
    })
}

/// Wait for a child up to `timeout`, polling so we never block unboundedly.
/// Returns `Some(status)` if it exited on its own; on timeout, kills it and
/// returns `None`. (No `Child::wait_timeout` in std — this stays dependency-free.)
fn wait_bounded(child: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // Best-effort teardown if the caller never called stop(). The fragmented
        // MP4 means even a killed ffmpeg leaves a playable file.
        self.stop.store(true, Ordering::SeqCst);
        if let Some(p) = self.pump.take() {
            let _ = p.join();
        }
        if let Some(c) = self.capture.take() {
            let _ = c.stop();
        }
        if let Some(mut child) = self.child.take() {
            // stdin already dropped by the pump → ffmpeg should be finishing; give
            // it a moment, otherwise kill (still playable thanks to fragmented MP4).
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(d) = self.stderr_drain.take() {
            let _ = d.join();
        }
        // Best-effort audio teardown (no mux on the abnormal Drop path — the video
        // temp is left as a playable fragmented file).
        if let Some(audio) = self.audio.take() {
            let _ = audio.stop();
        }
    }
}

/// Constant-framerate pump: write `latest` once per `1/fps` of wall-clock elapsed.
fn pump_frames(
    rx: Receiver<Frame>,
    mut stdin: ChildStdin,
    first: Frame,
    w: u32,
    h: u32,
    fps: u32,
    stop: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    t0: Instant,
    pause: super::SharedPause,
) -> Result<(), String> {
    let mut latest = first;
    let mut written: u64 = 0;

    loop {
        // Stop promptly: break BEFORE any end-of-stream catch-up. Flushing a large
        // backlog here (e.g. after a heavy live resize slowed the pipeline) would
        // block ffmpeg finalize for a long time — leaving the capture frame up and
        // the save arriving "late". A couple of dropped trailing frames is fine.
        if stop.load(Ordering::SeqCst) {
            break;
        }

        // Coalesce all queued frames into the freshest one. A differently-sized
        // frame means the live region was resized — letterbox it into the fixed
        // output geometry (ffmpeg -s is immutable for the take).
        while let Ok(f) = rx.try_recv() {
            latest = if f.width == w && f.height == h {
                f
            } else {
                Frame { width: w, height: h, bgra: fit_bgra_to(&f, w, h) }
            };
        }

        // The pause-aware clock: while paused, effective_elapsed STOPS advancing, so
        // `target` freezes → no frames are written for the paused span → resume
        // continues seamlessly (no frozen segment baked into the file). `latest`
        // keeps being refreshed below, so resume captures the live screen.
        let target = (pause.effective_elapsed(t0).as_secs_f64() * fps as f64) as u64;
        while written < target {
            // Stay responsive to Stop even while catching up a backlog.
            if stop.load(Ordering::SeqCst) {
                break;
            }
            if stdin.write_all(&latest.bgra).is_err() {
                // ffmpeg closed its input (died: disk full / encoder fault). Flag
                // it so status() can report the failure instead of pretending the
                // recording is still healthy.
                if !stop.load(Ordering::SeqCst) {
                    failed.store(true, Ordering::SeqCst);
                }
                return Err("ffmpeg input pipe closed unexpectedly".to_string());
            }
            written += 1;
        }

        thread::sleep(Duration::from_millis(2));
    }

    // Drop stdin → EOF → ffmpeg finalises the file.
    drop(stdin);
    Ok(())
}

/// Convert a finished recording (any ffmpeg-readable video) into an optimised GIF
/// next to it (`foo.mp4` → `foo.gif`). Two-pass palette (palettegen → paletteuse)
/// for clean colour, downscaled + frame-rate-reduced so the result is shareable,
/// not gigantic. Bounded so a pathological input can't hang the command.
pub fn export_gif(src: &Path, fps: u32, max_width: u32) -> Result<PathBuf, String> {
    if !src.exists() {
        return Err("That recording no longer exists on disk.".into());
    }
    let gif = src.with_extension("gif");
    let palette = src.with_extension("kiltmp.palette.png");
    let _ = std::fs::remove_file(&palette);
    let fps = fps.clamp(5, 30);
    let max_width = max_width.clamp(120, 1920);
    let src_s = src.to_str().ok_or("source path is not valid UTF-8")?;
    let gif_s = gif.to_str().ok_or("output path is not valid UTF-8")?;
    let pal_s = palette.to_str().ok_or("palette path is not valid UTF-8")?;
    // Reduce fps + cap width (never upscale; keep aspect; even height). The comma
    // inside min() is escaped so ffmpeg's filtergraph parser doesn't split on it.
    let vf = format!("fps={fps},scale=min({max_width}\\,iw):-2:flags=lanczos");

    // Pass 1: derive a 256-colour palette from the whole clip.
    run_ffmpeg_to_completion(&[
        "-hide_banner", "-loglevel", "error", "-y",
        "-i", src_s,
        "-vf", &format!("{vf},palettegen=stats_mode=diff"),
        pal_s,
    ])?;
    // Pass 2: render the GIF against that palette.
    let result = run_ffmpeg_to_completion(&[
        "-hide_banner", "-loglevel", "error", "-y",
        "-i", src_s,
        "-i", pal_s,
        "-lavfi", &format!("{vf}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5"),
        gif_s,
    ]);
    let _ = std::fs::remove_file(&palette);
    result?;
    Ok(gif)
}

/// Run a one-shot ffmpeg command to completion, bounded. Drains stderr (deadlock
/// guard) and surfaces it on failure. Kills + errors if it outruns `300s`.
fn run_ffmpeg_to_completion(args: &[&str]) -> Result<(), String> {
    let mut child = crate::commands::ffmpeg::ffmpeg_command()?
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to launch ffmpeg: {e}"))?;
    let drain = child.stderr.take().map(|mut err| {
        thread::Builder::new()
            .name("kil-gif-stderr".into())
            .spawn(move || {
                let mut s = String::new();
                let _ = err.read_to_string(&mut s);
                s
            })
            .expect("spawn gif stderr drain")
    });
    let status = wait_bounded(&mut child, Duration::from_secs(300));
    let stderr_text = drain.and_then(|d| d.join().ok()).unwrap_or_default();
    match status {
        Some(s) if s.success() => Ok(()),
        Some(s) => Err(format!(
            "ffmpeg exited with {s}{}",
            if stderr_text.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", stderr_text.trim())
            }
        )),
        None => Err("GIF export timed out (the recording may be very long).".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffprobe_path() -> PathBuf {
        crate::commands::ffmpeg::resolve_ffprobe().unwrap_or_else(|| PathBuf::from("ffprobe"))
    }

    // wait_bounded must NEVER block past its timeout: a process that outlives the
    // bound is killed and the call returns None promptly. This is the guarantee
    // that makes Recorder::stop bounded even if ffmpeg wedges on a full pipe.
    #[test]
    fn wait_bounded_kills_on_timeout() {
        // `ping -n 30` runs ~29s — far past our 300ms bound.
        let mut child = Command::new("ping")
            .args(["127.0.0.1", "-n", "30"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .expect("spawn ping");
        let t0 = Instant::now();
        let status = wait_bounded(&mut child, Duration::from_millis(300));
        let elapsed = t0.elapsed();
        assert!(status.is_none(), "expected a timeout-kill (None), got {status:?}");
        assert!(elapsed < Duration::from_secs(3), "wait_bounded overran its bound: {elapsed:?}");
    }

    // A process that exits before the timeout is reported as Some(status).
    #[test]
    fn wait_bounded_reports_clean_exit() {
        let mut child = Command::new("cmd")
            .args(["/C", "exit", "0"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .expect("spawn cmd");
        let status = wait_bounded(&mut child, Duration::from_secs(5));
        assert!(status.is_some(), "expected a clean exit status");
        assert!(status.unwrap().success(), "cmd /C exit 0 should succeed");
    }

    // Real-hardware: record ~2s of the primary monitor to MP4 and verify it is a
    // playable H.264 file of the right geometry/duration. Ignored by default.
    //   cargo test --no-default-features --features screenrec --lib \
    //     records_primary_monitor_to_mp4 -- --ignored --nocapture
    #[test]
    #[ignore = "records the real primary monitor; run with --ignored"]
    fn records_primary_monitor_to_mp4() {
        let out = std::env::temp_dir().join("kil-screenrec-stage2.mp4");
        let _ = std::fs::remove_file(&out);

        let rec = start_recording(
            &out,
            30,
            None,
            vec![],
            vec![],
            AudioOptions { system: false, mic: false },
            0,
            0.15,
        )
        .expect("start recording");
        thread::sleep(Duration::from_secs(2));
        let path = rec.stop().expect("clean stop");

        let meta = std::fs::metadata(&path).expect("output exists");
        assert!(meta.len() > 10_000, "MP4 is non-trivial ({} bytes)", meta.len());

        // ffprobe: must report an H.264 video stream + a real duration.
        let probe = Command::new(ffprobe_path())
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name,width,height",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1",
                path.to_str().unwrap(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .expect("run ffprobe");
        let info = String::from_utf8_lossy(&probe.stdout);
        eprintln!("ffprobe:\n{info}");
        assert!(probe.status.success(), "ffprobe failed: {}", String::from_utf8_lossy(&probe.stderr));
        assert!(info.contains("codec_name=h264"), "expected an H.264 stream");

        let duration: f64 = info
            .lines()
            .find_map(|l| l.strip_prefix("duration="))
            .and_then(|d| d.trim().parse().ok())
            .unwrap_or(0.0);
        assert!(duration > 1.0, "duration {duration}s should be ~2s");
        eprintln!("recorded {}s -> {}", duration, path.display());
    }

    // Real-hardware: record a REGION (sub-rect) of the primary monitor. Isolates
    // the crop path (full-screen works; region was reported broken). Ignored.
    //   cargo test --no-default-features --features screenrec --lib \
    //     records_region_to_mp4 -- --ignored --nocapture
    #[test]
    #[ignore = "records a region of the real primary monitor; run with --ignored"]
    fn records_region_to_mp4() {
        use crate::commands::screen_recorder::CropRect;
        let out = std::env::temp_dir().join("kil-screenrec-region.mp4");
        let _ = std::fs::remove_file(&out);

        // Deliberately ODD dimensions — a hand-drawn region is almost never even.
        // resolve_crop must round the crop down to even (641->640, 481->480) so the
        // capture matches the encoder output and skips the per-frame letterbox resize.
        let region = Some(CropRect { x: 101, y: 101, w: 641, h: 481 });
        let rec = start_recording(
            &out,
            30,
            region,
            vec![],
            vec![],
            AudioOptions { system: false, mic: false },
            0,
            0.15,
        )
        .expect("start region recording");
        thread::sleep(Duration::from_secs(2));
        let path = rec.stop().expect("clean stop");

        let meta = std::fs::metadata(&path).expect("output exists");
        assert!(meta.len() > 10_000, "MP4 is non-trivial ({} bytes)", meta.len());

        let probe = Command::new(ffprobe_path())
            .args([
                "-v", "error", "-select_streams", "v:0",
                "-show_entries", "stream=codec_name,width,height",
                "-show_entries", "format=duration",
                "-of", "default=noprint_wrappers=1",
                path.to_str().unwrap(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .expect("run ffprobe");
        let info = String::from_utf8_lossy(&probe.stdout);
        eprintln!("ffprobe(region):\n{info}");
        assert!(probe.status.success(), "ffprobe failed: {}", String::from_utf8_lossy(&probe.stderr));
        assert!(info.contains("codec_name=h264"), "expected an H.264 stream");
        assert!(info.contains("width=640"), "expected 640px wide, got:\n{info}");
        assert!(info.contains("height=480"), "expected 480px tall, got:\n{info}");
        eprintln!("recorded region -> {}", path.display());
    }

    // Real-hardware: record video + SYSTEM audio and verify the muxed MP4 has BOTH
    // an H.264 video stream and an AAC audio stream, and the temps are cleaned up.
    //   cargo test --no-default-features --features screenrec --lib \
    //     records_av_to_mp4 -- --ignored --nocapture
    #[test]
    #[ignore = "records real screen + system audio; run with --ignored"]
    fn records_av_to_mp4() {
        let out = std::env::temp_dir().join("kil-screenrec-av.mp4");
        let _ = std::fs::remove_file(&out);

        // Both sources → exercises the dynaudnorm(mic) + amix mux path. Use a small
        // sync trim so the -itsoffset mux path is exercised too.
        let rec = start_recording(
            &out,
            30,
            None,
            vec![],
            vec![],
            AudioOptions { system: true, mic: true },
            120,
            0.15,
        )
        .expect("start a/v recording");
        thread::sleep(Duration::from_secs(2));
        let path = rec.stop().expect("clean stop");

        assert_eq!(path, out, "final path should be the requested output");
        let meta = std::fs::metadata(&path).expect("output exists");
        assert!(meta.len() > 10_000, "MP4 is non-trivial ({} bytes)", meta.len());

        let probe = Command::new(ffprobe_path())
            .args([
                "-v", "error",
                "-show_entries", "stream=codec_type,codec_name",
                "-of", "default=noprint_wrappers=1",
                path.to_str().unwrap(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .expect("run ffprobe");
        let info = String::from_utf8_lossy(&probe.stdout);
        eprintln!("ffprobe(av):\n{info}");
        assert!(probe.status.success(), "ffprobe failed: {}", String::from_utf8_lossy(&probe.stderr));
        assert!(info.contains("codec_name=h264"), "expected an H.264 video stream:\n{info}");
        assert!(info.contains("codec_name=aac"), "expected an AAC audio stream:\n{info}");

        let vtmp = out.with_extension("kiltmp.video.mp4");
        assert!(!vtmp.exists(), "video temp should be cleaned up: {}", vtmp.display());
        eprintln!("recorded a/v -> {}", path.display());
    }
}
