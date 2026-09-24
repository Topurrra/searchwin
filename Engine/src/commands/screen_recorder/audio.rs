//! Screen Recorder — Stage 4: audio capture (system loopback + microphone).
//!
//! Each enabled source is captured as raw PCM and piped to its OWN ffmpeg, which
//! encodes a small temp AAC file — the exact same single-stdin pipe pattern the
//! video path uses (so the same bounded-stop discipline applies: close stdin for
//! a clean EOF, kill as a last resort, never hang). At stop the caller muxes the
//! video with the audio temp(s) in one `-c:v copy` pass and lets ffmpeg do the
//! resampling + mixing (`amix`) — no hand-rolled DSP or A/V-sync math in Rust.
//!
//! Why two capture backends:
//!   - SYSTEM audio = WASAPI loopback of the default render endpoint. cpal cannot
//!     do loopback on Windows, so we use the `wasapi` crate directly.
//!   - MICROPHONE = cpal default input stream (cross-platform, simple).
//! Each runs on its own dedicated MTA thread (the COM-per-thread rule proven by
//! the Stage 0 harness); a missing device degrades that one source to "off"
//! rather than failing the recording.
#![cfg(feature = "screenrec")]

use std::collections::VecDeque;
use std::io::{BufWriter, Read, Write};
#[cfg(test)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
#[cfg(test)]
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(test)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;
/// Capture-time mic gain. Kept at unity — loudness is handled cleanly at mux via
/// ffmpeg `dynaudnorm` (adaptive), which lifts a quiet voice far better than a
/// fixed pre-gain and never clips. Left as a knob for future per-device tuning.
const MIC_GAIN: f32 = 1.0;

/// Which audio sources to capture. Both off ⇒ no audio (video stays as-is).
#[derive(Clone, Copy, Debug)]
pub struct AudioOptions {
    pub system: bool,
    pub mic: bool,
}
impl AudioOptions {
    pub fn any(&self) -> bool {
        self.system || self.mic
    }
}

/// The discovered PCM format of a source, handed from the capture thread back to
/// the encoder setup so ffmpeg is told the exact sample rate / channels / format.
struct PcmFormat {
    sample_rate: u32,
    channels: u16,
    /// ffmpeg input format token — "f32le" or "s16le".
    ffmpeg_fmt: &'static str,
}

/// One running audio source: a capture thread feeding a dedicated ffmpeg encoder.
struct AudioSource {
    label: &'static str,
    stop: Arc<AtomicBool>,
    capture: Option<JoinHandle<()>>,
    child: Option<Child>,
    stderr_drain: Option<JoinHandle<()>>,
    temp: PathBuf,
}

/// A running audio capture (0–2 sources). Call [`AudioCapture::stop`] to finish.
pub struct AudioCapture {
    sources: Vec<AudioSource>,
}

/// Initialise COM as MTA on the current thread (one dedicated thread per source).
/// `RPC_E_CHANGED_MODE` is tolerated; we never `CoUninitialize` (the repo rule).
fn init_mta() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let _ = hr.0 == RPC_E_CHANGED_MODE; // fine either way
}

/// Drain a child's stderr on its own thread (deadlock guard — same as the video
/// encoder). Returns `None` if there is no stderr handle.
fn spawn_stderr_drain(stderr: Option<std::process::ChildStderr>) -> Option<JoinHandle<()>> {
    stderr.map(|mut err| {
        thread::Builder::new()
            .name("kil-aud-ffmpeg-stderr".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = err.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                }
            })
            .expect("spawn audio stderr drain")
    })
}

/// Spawn an ffmpeg that reads raw PCM on stdin and encodes AAC to `temp`.
fn spawn_aac_encoder(
    temp: &Path,
    fmt: &PcmFormat,
) -> Result<(Child, ChildStdin, Option<JoinHandle<()>>), String> {
    let sr = fmt.sample_rate.to_string();
    let ch = fmt.channels.to_string();
    let mut child = crate::commands::ffmpeg::screen_recording_command()?
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            fmt.ffmpeg_fmt,
            "-ar",
            &sr,
            "-ac",
            &ch,
            "-i",
            "pipe:0",
            // FFmpeg's built-in AAC encoder, rather than optional fdk-aac.
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-movflags",
            "frag_keyframe+empty_moov+default_base_moof",
            "-y",
            temp.to_str().ok_or("audio temp path is not valid UTF-8")?,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to launch audio ffmpeg: {e}"))?;
    let stdin = child.stdin.take().ok_or("audio ffmpeg stdin unavailable")?;
    let drain = spawn_stderr_drain(child.stderr.take());
    Ok((child, stdin, drain))
}

/// Start capturing the requested sources, each to a temp `.m4a` next to
/// `out_dir/<stem>.<label>.m4a`. Sources that have no device (e.g. no mic) are
/// silently skipped. Returns an [`AudioCapture`] with the live sources.
pub fn start_audio(
    out_dir: &Path,
    stem: &str,
    opts: AudioOptions,
    pause: super::SharedPause,
) -> Result<AudioCapture, String> {
    // Phase 1: spawn ALL capture threads at once so the devices initialise in
    // parallel. Starting them sequentially offset the sources by ~100ms (an
    // ffmpeg spawn + device init each), which you'd hear as an echo/delay between
    // the system audio and the mic.
    let mut pending = Vec::new();
    if opts.system {
        match spawn_source("system", out_dir, stem, capture_loopback, pause.clone()) {
            Ok(p) => pending.push(p),
            Err(e) => eprintln!("[screenrec] system audio unavailable: {e}"),
        }
    }
    if opts.mic {
        match spawn_source("mic", out_dir, stem, capture_mic, pause.clone()) {
            Ok(p) => pending.push(p),
            Err(e) => eprintln!("[screenrec] microphone unavailable: {e}"),
        }
    }

    // Phase 2: collect each device's format and spawn its encoder — but hold the
    // stdin (the capture threads block until handed it). The slow ffmpeg spawns
    // happen here, BEFORE any stream starts, so they don't offset the sources.
    let mut staged = Vec::new();
    for p in pending {
        match p.fmt_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(fmt) => match spawn_aac_encoder(&p.temp, &fmt) {
                Ok((child, stdin, drain)) => staged.push((p, child, drain, stdin)),
                Err(e) => {
                    eprintln!("[screenrec] {} encoder failed: {e}", p.label);
                    p.stop.store(true, Ordering::SeqCst);
                }
            },
            Err(_) => {
                eprintln!("[screenrec] {} produced no format within 3s", p.label);
                p.stop.store(true, Ordering::SeqCst);
            }
        }
    }

    // Phase 3: hand every capture its stdin back-to-back so they start streaming
    // within milliseconds of each other (tight inter-source + A/V sync).
    let mut sources = Vec::new();
    for (p, child, stderr_drain, stdin) in staged {
        if p.stdin_tx.send(stdin).is_ok() {
            sources.push(AudioSource {
                label: p.label,
                stop: p.stop,
                capture: Some(p.capture),
                child: Some(child),
                stderr_drain,
                temp: p.temp,
            });
        }
    }
    Ok(AudioCapture { sources })
}

/// A capture thread spawned and blocked waiting for its encoder's stdin (phase 1).
struct PendingSource {
    label: &'static str,
    temp: PathBuf,
    stop: Arc<AtomicBool>,
    capture: JoinHandle<()>,
    fmt_rx: mpsc::Receiver<PcmFormat>,
    stdin_tx: mpsc::Sender<ChildStdin>,
}

/// Spin a capture thread and return immediately (it discovers its format and then
/// blocks until [`start_audio`] hands it the encoder stdin in phase 3).
fn spawn_source(
    label: &'static str,
    out_dir: &Path,
    stem: &str,
    run: fn(Arc<AtomicBool>, super::SharedPause, mpsc::Sender<PcmFormat>, mpsc::Receiver<ChildStdin>),
    pause: super::SharedPause,
) -> Result<PendingSource, String> {
    let temp = out_dir.join(format!("{stem}.{label}.m4a"));
    let _ = std::fs::remove_file(&temp);

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let (fmt_tx, fmt_rx) = mpsc::channel::<PcmFormat>();
    let (stdin_tx, stdin_rx) = mpsc::channel::<ChildStdin>();

    let capture = thread::Builder::new()
        .name(format!("kil-aud-{label}"))
        .spawn(move || run(stop_thread, pause, fmt_tx, stdin_rx))
        .map_err(|e| format!("Failed to spawn {label} capture thread: {e}"))?;

    Ok(PendingSource { label, temp, stop, capture, fmt_rx, stdin_tx })
}

impl AudioCapture {
    /// Number of sources that actually came up (devices present + encoder spawned).
    pub fn active(&self) -> usize {
        self.sources.len()
    }

    /// Total bytes the audio encoders have written so far (for the live size readout).
    pub fn bytes_on_disk(&self) -> u64 {
        self.sources
            .iter()
            .map(|s| std::fs::metadata(&s.temp).map(|m| m.len()).unwrap_or(0))
            .sum()
    }

    /// Stop every source cleanly (bounded — mirrors the video encoder): signal
    /// stop, close stdin for EOF, and kill ffmpeg if it does not finish promptly.
    /// Returns the temp `.m4a` paths that hold real audio (in start order).
    pub fn stop(self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for mut src in self.sources {
            src.stop.store(true, Ordering::SeqCst);

            // The capture thread stops streaming and drops the encoder stdin (EOF).
            if let Some(cap) = src.capture.take() {
                let (tx, rx) = mpsc::channel::<()>();
                if let Ok(watchdog) = thread::Builder::new()
                    .name("kil-aud-join".into())
                    .spawn(move || {
                        let _ = cap.join();
                        let _ = tx.send(());
                    })
                {
                    if rx.recv_timeout(Duration::from_secs(2)).is_err() {
                        // Capture wedged — killing ffmpeg unblocks any stuck write.
                        if let Some(child) = src.child.as_mut() {
                            let _ = child.kill();
                        }
                        let _ = rx.recv_timeout(Duration::from_secs(2));
                    }
                    let _ = watchdog.join();
                }
            }

            // ffmpeg finalises after EOF; bound the wait and kill as a last resort.
            if let Some(mut child) = src.child.take() {
                let _ = wait_bounded(&mut child, Duration::from_secs(5));
            }
            if let Some(d) = src.stderr_drain.take() {
                let _ = d.join();
            }

            // Only report a temp that actually has bytes.
            if std::fs::metadata(&src.temp).map(|m| m.len() > 256).unwrap_or(false) {
                out.push(src.temp);
            } else {
                let _ = std::fs::remove_file(&src.temp);
                let _ = src.label;
            }
        }
        out
    }
}

/// Bounded child wait: poll until exit or `timeout`, then kill. No std timeout
/// exists, so this stays dependency-free (mirrors `encode::wait_bounded`).
fn wait_bounded(child: &mut Child, timeout: Duration) {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return,
        }
    }
}

// ─── System audio: WASAPI loopback of the default render endpoint ────────────

fn capture_loopback(
    stop: Arc<AtomicBool>,
    pause: super::SharedPause,
    fmt_tx: mpsc::Sender<PcmFormat>,
    stdin_rx: mpsc::Receiver<ChildStdin>,
) {
    init_mta();
    if let Err(e) = run_loopback(&stop, &pause, &fmt_tx, &stdin_rx) {
        eprintln!("[screenrec] loopback capture ended: {e}");
    }
}

fn run_loopback(
    stop: &Arc<AtomicBool>,
    pause: &super::SharedPause,
    fmt_tx: &mpsc::Sender<PcmFormat>,
    stdin_rx: &mpsc::Receiver<ChildStdin>,
) -> Result<(), Box<dyn std::error::Error>> {
    use wasapi::{Direction, ShareMode};

    let device = wasapi::get_default_device(&Direction::Render)?;
    let mut audio_client = device.get_iaudioclient()?;
    let format = audio_client.get_mixformat()?;
    let sample_rate = format.get_samplespersec();
    let channels = format.get_nchannels();
    let bits = format.get_bitspersample();
    let blockalign = format.get_blockalign() as usize;
    let ffmpeg_fmt = if bits == 16 { "s16le" } else { "f32le" };

    fmt_tx
        .send(PcmFormat { sample_rate, channels, ffmpeg_fmt })
        .map_err(|_| "loopback: encoder setup dropped")?;

    // Initialise the client up-front (overlaps with the encoder spawn) so that the
    // moment the stdin arrives we can start the stream — keeps the system and mic
    // start times tightly aligned, avoiding an audible offset between them.
    let (def_period, _min_period) = audio_client.get_periods()?;
    audio_client.initialize_client(
        &format,
        def_period,
        &Direction::Capture, // loopback captures the RENDER endpoint in capture mode
        &ShareMode::Shared,
        true, // loopback
    )?;
    let h_event = audio_client.set_get_eventhandle()?;
    let capture_client = audio_client.get_audiocaptureclient()?;

    // Wait for the encoder's stdin, then begin streaming immediately.
    let stdin = stdin_rx.recv_timeout(Duration::from_secs(3))?;
    let mut writer = BufWriter::new(stdin);
    audio_client.start_stream()?;

    let mut queue: VecDeque<u8> = VecDeque::new();
    while !stop.load(Ordering::SeqCst) {
        // Wake on the next buffer, but time out so we re-check `stop` promptly.
        if h_event.wait_for_event(200).is_err() {
            continue;
        }
        capture_client.read_from_device_to_deque(&mut queue)?;
        // Drain whole frames out of the device buffer either way (so it never backs
        // up), but DROP them while paused — skipping the paused span keeps this track
        // aligned with the video, which also skips it.
        let n = queue.len() - (queue.len() % blockalign.max(1));
        if n > 0 {
            let chunk: Vec<u8> = queue.drain(..n).collect();
            if !pause.is_paused() && writer.write_all(&chunk).is_err() {
                break; // ffmpeg closed its input
            }
        }
    }

    let _ = audio_client.stop_stream();
    let _ = writer.flush();
    drop(writer); // EOF → ffmpeg finalises
    Ok(())
}

// ─── Microphone: cpal default input stream ──────────────────────────────────

fn capture_mic(
    stop: Arc<AtomicBool>,
    pause: super::SharedPause,
    fmt_tx: mpsc::Sender<PcmFormat>,
    stdin_rx: mpsc::Receiver<ChildStdin>,
) {
    init_mta();
    if let Err(e) = run_mic(&stop, &pause, &fmt_tx, &stdin_rx) {
        eprintln!("[screenrec] mic capture ended: {e}");
    }
}

fn run_mic(
    stop: &Arc<AtomicBool>,
    pause: &super::SharedPause,
    fmt_tx: &mpsc::Sender<PcmFormat>,
    stdin_rx: &mpsc::Receiver<ChildStdin>,
) -> Result<(), Box<dyn std::error::Error>> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::SampleFormat;

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("no default input device (microphone)")?;
    let supported = device.default_input_config()?;
    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels();
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    // We always emit f32le to ffmpeg regardless of the device's native format.
    fmt_tx
        .send(PcmFormat { sample_rate, channels, ffmpeg_fmt: "f32le" })
        .map_err(|_| "mic: encoder setup dropped")?;
    let stdin = stdin_rx.recv_timeout(Duration::from_secs(3))?;
    let writer = Arc::new(Mutex::new(BufWriter::new(stdin)));

    let w = writer.clone();
    let p = pause.clone();
    let err_fn = |e| eprintln!("[screenrec] mic stream error: {e}");

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| write_f32(&w, data, MIC_GAIN, &p),
            err_fn,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                let f: Vec<f32> = data.iter().map(|&s| s as f32 / 32768.0).collect();
                write_f32(&w, &f, MIC_GAIN, &p);
            },
            err_fn,
            None,
        )?,
        SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                let f: Vec<f32> =
                    data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0).collect();
                write_f32(&w, &f, MIC_GAIN, &p);
            },
            err_fn,
            None,
        )?,
        other => return Err(format!("unsupported mic sample format: {other:?}").into()),
    };

    stream.play()?;
    while !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));
    }
    drop(stream); // stop the callback
    if let Ok(mut g) = writer.lock() {
        let _ = g.flush(); // EOF on drop → ffmpeg finalises
    }
    Ok(())
}

/// Write interleaved f32 samples to the shared encoder stdin as little-endian,
/// applying `gain` (clamped to [-1, 1] so a boost can't clip into garbage). Drops
/// the samples entirely while paused, so the mic track skips the paused span and
/// stays aligned with the (also-paused) video.
fn write_f32(
    writer: &Arc<Mutex<BufWriter<ChildStdin>>>,
    data: &[f32],
    gain: f32,
    pause: &super::SharedPause,
) {
    if pause.is_paused() {
        return;
    }
    let mut bytes = Vec::with_capacity(data.len() * 4);
    for &s in data {
        let v = if gain == 1.0 { s } else { (s * gain).clamp(-1.0, 1.0) };
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    if let Ok(mut g) = writer.lock() {
        let _ = g.write_all(&bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffprobe_path() -> PathBuf {
        crate::commands::ffmpeg::resolve_ffprobe().unwrap_or_else(|| PathBuf::from("ffprobe"))
    }

    // Real-hardware: capture ~2s of SYSTEM audio (+ mic if present) to temp AAC
    // and verify each is a valid AAC stream of ~2s. Ignored by default.
    //   cargo test --no-default-features --features screenrec --lib \
    //     captures_audio_to_m4a -- --ignored --nocapture
    #[test]
    #[ignore = "captures real audio devices; run with --ignored"]
    fn captures_audio_to_m4a() {
        let dir = std::env::temp_dir();
        let pause = Arc::new(super::super::PauseState::new());
        let cap = start_audio(&dir, "kil-audtest", AudioOptions { system: true, mic: true }, pause)
            .expect("start audio");
        thread::sleep(Duration::from_secs(2));
        let temps = cap.stop();

        assert!(!temps.is_empty(), "expected at least the system-audio track");
        for t in &temps {
            let probe = Command::new(ffprobe_path())
                .args([
                    "-v", "error", "-select_streams", "a:0",
                    "-show_entries", "stream=codec_name",
                    "-show_entries", "format=duration",
                    "-of", "default=noprint_wrappers=1",
                    t.to_str().unwrap(),
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .expect("run ffprobe");
            let info = String::from_utf8_lossy(&probe.stdout);
            eprintln!("ffprobe({}):\n{info}", t.display());
            assert!(probe.status.success(), "ffprobe failed for {}", t.display());
            assert!(info.contains("codec_name=aac"), "expected AAC in {}", t.display());
        }
    }
}
