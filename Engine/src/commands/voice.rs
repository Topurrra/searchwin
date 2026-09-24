//! Voice-to-text — the offline Vosk engine behind the frontend commands.
//!
//! ## Engine
//!
//! **Vosk** — Alphacephei's offline Vosk engine. Real-time streaming,
//! broad language support (incl. Georgian), private by construction —
//! the model runs in-process, so no audio ever leaves the machine.
//! Compiled in by default (the `vosk` Cargo feature); needs
//! `libvosk.dll` alongside the .exe (shipped via `tauri.conf.json`
//! `bundle.resources`) and a downloaded language-model folder on disk.
//!
//! The Windows WinRT `SpeechRecognition` engine was removed (see
//! `CoreFeaturesPlan.md` `[Voice]` #0): it is Windows-only, a closed OS
//! component KeepItLocal cannot audit or prove is offline, and could not
//! serve the grammar-constrained command-mode ambition. Voice now runs
//! only on Vosk, an engine KeepItLocal ships and fully controls.
//!
//! ## Caveats surfaced via `voice_check_availability`
//!
//!   - Requires `libvosk.dll` + a model folder.
//!   - First use prompts for microphone access (Windows Privacy →
//!     Microphone → KeepItLocal). The user has to allow this once.
//!
//! ## Recognition model
//!
//! Single-shot recognition exposes a single-utterance API: record →
//! end-of-speech → return text. Continuous dictation keeps one
//! persistent audio stream + recognizer open and emits per-utterance
//! events as Vosk reports phrase boundaries.

#![cfg(windows)]

use serde::Serialize;

#[cfg(feature = "vosk")]
use std::collections::HashMap;
#[cfg(feature = "vosk")]
use std::sync::atomic::AtomicBool;
#[cfg(feature = "vosk")]
use std::sync::LazyLock;

/// Active Vosk recognition's cancellation flag. `voice_recognize_once`
/// installs an `Arc<AtomicBool>` here while recording; the frontend
/// can call `voice_cancel_recognize` to flip it true, causing the
/// audio poll loop to exit at its next iteration boundary (within
/// ~50ms) instead of waiting for end-of-utterance silence.
///
/// `None` when no recognition is in flight. The recognize function
/// clears this on its way out.
#[cfg(feature = "vosk")]
static VOSK_ACTIVE_CANCEL: LazyLock<std::sync::Mutex<Option<std::sync::Arc<AtomicBool>>>> =
    LazyLock::new(|| std::sync::Mutex::new(None));

/// Catalog of speech-to-text models the installer can fetch on-demand
/// after install. We don't bundle these in the .exe because:
///   - Even the small models add tens of MB; the large ones are well
///     over a gigabyte. Bundling them would bloat the installer.
///   - Users often want to try multiple sizes / languages. A built-in
///     downloader sidesteps the "find the model, copy URL, pick
///     directory" dance the user complained about.
///
/// Curated list — not exhaustive — covers a small/fast and a
/// large/accurate Vosk variant in English and Georgian. Entries are ZIP
/// archives extracted on download.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VoiceModelSpec {
    /// Stable id used by the frontend to request a download.
    pub id: String,
    /// Human-readable label for the picker UI.
    pub label: String,
    /// What this model is good at — one short sentence for the picker
    /// help text.
    pub description: String,
    /// Approximate download size. Used by the UI to warn before kicking
    /// off a multi-hundred-megabyte transfer.
    pub size_label: String,
    /// Which engine consumes this model. Frontend uses this to filter
    /// the picker to whatever engine is currently selected.
    pub engine: String,
    /// Direct download URL. We prefer HuggingFace mirrors because their
    /// hosting is free, fast, and not rate-limited for anonymous reads.
    pub url: String,
    /// Filename written to disk. Frontend joins this onto the user's
    /// chosen download folder to form the full target path.
    pub filename: String,
    /// `true` for the model KeepItLocal recommends as the default
    /// starting point — the lightest viable choice, safe on the 4â€“8 GB
    /// low-end hardware the app targets.
    pub recommended: bool,
    /// `true` when the model's resident memory footprint is large enough
    /// (≥ ~1 GB) to warrant an upfront warning before download.
    pub heavy: bool,
    /// Approximate RESIDENT RAM once loaded — distinct from the on-disk
    /// download size (`size_label`). The upfront memory disclosure shown
    /// when the user is choosing a model.
    pub ram_label: String,
}

/// Returns the curated catalog of downloadable models. Pure data —
/// no I/O — so it's cheap to call from a settings page on every render.
/// Frontends can also hardcode this list; surfacing it from Rust keeps
/// the canonical URLs in one place and lets us add models without a
/// frontend redeploy if we ever ship over-the-air updates.
#[tauri::command]
pub fn voice_list_downloadable_models() -> Vec<VoiceModelSpec> {
    vec![
        // â”€â”€â”€ Vosk — ZIP archives, extracted on download â”€â”€â”€
        VoiceModelSpec {
            id: "vosk-en-small".into(),
            label: "Vosk · English (small)".into(),
            description: "Fast, lightweight English model. Good on modest hardware.".into(),
            size_label: "~40 MB".into(),
            engine: "vosk".into(),
            url: "https://alphacephei.com/vosk/models/vosk-model-small-en-us-0.15.zip".into(),
            filename: "vosk-model-small-en-us-0.15.zip".into(),
            recommended: true,
            heavy: false,
            ram_label: "~0.3 GB RAM".into(),
        },
        VoiceModelSpec {
            id: "vosk-en-large".into(),
            label: "Vosk · English (large)".into(),
            description: "High-accuracy English model. Heavier — best on a capable machine.".into(),
            size_label: "~1.8 GB".into(),
            engine: "vosk".into(),
            url: "https://alphacephei.com/vosk/models/vosk-model-en-us-0.22.zip".into(),
            filename: "vosk-model-en-us-0.22.zip".into(),
            recommended: false,
            heavy: true,
            ram_label: "~2 GB RAM".into(),
        },
        VoiceModelSpec {
            id: "vosk-ka-small".into(),
            label: "Vosk · Georgian (small)".into(),
            description: "Fast, lightweight Georgian (ქართული) model.".into(),
            size_label: "~45 MB".into(),
            engine: "vosk".into(),
            url: "https://alphacephei.com/vosk/models/vosk-model-small-ka-0.42.zip".into(),
            filename: "vosk-model-small-ka-0.42.zip".into(),
            recommended: false,
            heavy: false,
            ram_label: "~0.3 GB RAM".into(),
        },
        VoiceModelSpec {
            id: "vosk-ka-large".into(),
            label: "Vosk · Georgian (large)".into(),
            description: "Higher-accuracy Georgian (ქართული) model.".into(),
            size_label: "~700 MB".into(),
            engine: "vosk".into(),
            url: "https://alphacephei.com/vosk/models/vosk-model-ka-0.42.zip".into(),
            filename: "vosk-model-ka-0.42.zip".into(),
            recommended: false,
            heavy: true,
            ram_label: "~1 GB RAM".into(),
        },
    ]
}

/// Downloads a model file to `target_dir/filename`. Returns the absolute
/// path of the downloaded file on success.
///
/// Implementation uses Windows' built-in `curl.exe` (shipped with every
/// Windows 10 1803+ and Windows 11) rather than a Rust HTTP client, so
/// we don't pull `reqwest` (and TLS, and the related deps) into the
/// build just for occasional model fetches.
///
/// Why `curl.exe` and NOT PowerShell `Invoke-WebRequest`:
///   The previous implementation shelled out to `Invoke-WebRequest`.
///   On Windows PowerShell 5.1 (the version that ships in-box) IWR
///   buffers the ENTIRE response body in memory before writing it to
///   `-OutFile`. For a 75-466 MB model that means hundreds of MB of
///   RAM and a download that appears frozen for minutes — exactly the
///   "PowerShell window open, doing nothing, app unresponsive" bug.
///   `curl.exe` streams straight to disk: constant memory, fast, and
///   it follows HuggingFace's CDN redirects natively.
///
/// Two other fixes folded in here:
///   - CREATE_NO_WINDOW: the old code spawned a visible console
///     window that stole focus. We now pass the creation flag so the
///     download runs completely invisibly.
///   - --connect-timeout: a dead network now fails fast (30s) instead
///     of hanging the spinner indefinitely.
///
/// Threading: declared `#[tauri::command(async)]`. This matters — a
/// plain `#[tauri::command]` on a *synchronous* fn runs on Tauri's
/// MAIN thread, so the blocking `curl.output()` below would freeze the
/// whole UI for the entire download (no scrolling, no clicks — exactly
/// the "app goes very slow while downloading" bug). The `(async)` form
/// tells Tauri to run this sync fn on a dedicated worker thread
/// instead, so the download proceeds without touching the UI thread.
/// Frontend still just `await`s the result and shows a spinner.
#[tauri::command(async)]
pub fn voice_download_model(
    app: tauri::AppHandle,
    model_id: String,
    target_dir: String,
) -> Result<String, String> {
    use std::io::Read;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::time::Duration;
    use tauri::Emitter;

    let model = voice_list_downloadable_models()
        .into_iter()
        .find(|m| m.id == model_id)
        .ok_or_else(|| format!("Unknown model id: {model_id}"))?;

    let dir = PathBuf::from(&target_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create target directory '{target_dir}': {e}"))?;

    let final_target = dir.join(&model.filename);
    // #25 -- resumable downloads: curl streams into a `.partial` file;
    // an interrupted attempt leaves it on disk and the next attempt
    // resumes it (`-C -`) instead of restarting the GB-scale fetch. The
    // file is promoted to its real name only once the transfer
    // completes, so a `.zip` on disk is always a whole file.
    let partial = dir.join(format!("{}.partial", &model.filename));
    let partial_str = partial
        .to_str()
        .ok_or_else(|| "Target path has invalid UTF-8".to_string())?
        .to_string();

    // Resolve the total size up front (best effort) so the UI can show a real
    // percentage instead of a blind spinner. 0 means the server wouldn't report
    // it — the UI then shows bytes-downloaded with an indeterminate bar.
    let total = fetch_content_length(&model.url).unwrap_or(0);

    // curl.exe flags:
    //   -L                  follow redirects — HuggingFace 302s to a CDN.
    //   -f                  fail (non-zero exit) on HTTP >= 400 instead
    //                        of writing the error page to the file.
    //   -sS                 silent but still print errors to stderr.
    //   --connect-timeout    bail after 30s if the host is unreachable
    //                        (no more infinite "doing nothing" spinner).
    //   --retry 2            ride out transient network blips.
    //   -o <path>            stream straight to the target file.
    let mut command = Command::new("curl.exe");
    let mut args: Vec<&str> = vec![
        "-L", "-f", "-sS", "--connect-timeout", "30", "--retry", "2",
    ];
    // #25 -- resume a leftover partial download from where it stopped.
    if partial.exists() {
        args.push("-C");
        args.push("-");
    }
    args.push("-o");
    args.push(&partial_str);
    args.push(&model.url);
    command.args(&args);

    // CREATE_NO_WINDOW (0x08000000) — run the downloader with no
    // console window. Without this Windows flashes a curl/conhost
    // window that steals focus from KeepItLocal and reads as "the app
    // froze and a terminal popped up".
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    // Capture curl's stderr (errors only, thanks to -sS) for a clear message
    // on failure, then stream in the background while we poll the growing
    // .partial file and emit progress events to the UI.
    command.stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| {
        format!("Failed to launch curl.exe for download: {e}. curl ships with Windows 10 1803 and later.")
    })?;

    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("Download process error: {e}"))?
        {
            break status;
        }
        let downloaded = std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
        let _ = app.emit(
            "voice-model-download-progress",
            ModelDownloadProgress {
                model_id: model_id.clone(),
                downloaded,
                total,
            },
        );
        std::thread::sleep(Duration::from_millis(400));
    };

    if !status.success() {
        let mut stderr = String::new();
        if let Some(mut handle) = child.stderr.take() {
            let _ = handle.read_to_string(&mut stderr);
        }
        // #25 -- KEEP the partial file on failure so the next download
        // attempt resumes it with `-C -` rather than restarting.
        return Err(format!(
            "Model download failed (exit {}): {}. The partial download \
             was kept -- press Download again to resume it.",
            status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    // Land the progress bar at 100% before the (brief) extraction begins.
    let _ = app.emit(
        "voice-model-download-progress",
        ModelDownloadProgress {
            model_id: model_id.clone(),
            downloaded: std::fs::metadata(&partial).map(|m| m.len()).unwrap_or(total),
            total,
        },
    );

    // Sanity check — curl's `-f` already fails on HTTP 4xx/5xx, but
    // keep a size floor as belt-and-suspenders: a Vosk model archive is
    // at minimum a few MB, so anything tinier means we somehow got an
    // error page or truncated transfer despite a zero exit code.
    // #25 -- the transfer finished; promote the completed `.partial` to
    // its real name so a `.zip` on disk is always a whole file.
    let _ = std::fs::remove_file(&final_target);
    std::fs::rename(&partial, &final_target)
        .map_err(|e| format!("Could not finalize the downloaded file: {e}"))?;

    let metadata = std::fs::metadata(&final_target)
        .map_err(|e| format!("Downloaded file unreadable: {e}"))?;
    if metadata.len() < 1_000_000 {
        let _ = std::fs::remove_file(&final_target);
        return Err(format!(
            "Downloaded file is suspiciously small ({} bytes) — the URL likely returned an error page. Check your internet connection and try again.",
            metadata.len()
        ));
    }

    // Vosk models are ZIP archives containing one top-level model folder.
    // Extract the archive, drop the .zip, and return the model FOLDER
    // path — Vosk's recognizer wants a folder, not a file.
    extract_vosk_archive(&final_target, &dir)
}

/// Extract a downloaded Vosk model ZIP into `dir` and return the absolute
/// path of the extracted model folder. Vosk archives always contain one
/// top-level folder (e.g. `vosk-model-small-en-us-0.15/`); we read that
/// name from the archive, extract, drop the now-redundant `.zip`, and hand
/// back the folder path — which is what `voskModelPath` must point at.
fn extract_vosk_archive(
    zip_path: &std::path::Path,
    dir: &std::path::Path,
) -> Result<String, String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("Cannot open downloaded archive: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        // #25 -- a truncated download loses the ZIP's central directory
        // (it sits at the END of the file), so a "not a valid ZIP" here
        // is the usual symptom of an incomplete transfer.
        format!(
            "The downloaded file is not a valid ZIP archive ({e}). The \
             download was likely corrupted or incomplete -- try again."
        )
    })?;
    if archive.is_empty() {
        return Err("Downloaded model archive is empty.".to_string());
    }
    // Top-level folder = the first path segment of the first entry. Zip
    // entry paths are always '/'-separated regardless of platform.
    let top_dir = {
        let first = archive
            .by_index(0)
            .map_err(|e| format!("Cannot read model archive: {e}"))?;
        first
            .name()
            .split('/')
            .next()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .ok_or_else(|| "Model archive has no top-level folder.".to_string())?
    };
    archive
        .extract(dir)
        .map_err(|e| format!("Could not extract model archive: {e}"))?;
    // The .zip is large and redundant once extracted.
    let _ = std::fs::remove_file(zip_path);
    let model_dir = dir.join(&top_dir);
    // #25 -- integrity check: a genuine Vosk model folder always has
    // `am/` and `conf/` subdirectories. Their absence means the archive
    // was not a Vosk model, or arrived incomplete -- fail loudly here
    // rather than leaving the recognizer to fail cryptically later. The
    // zip's own per-entry CRC32 checksums were already verified by the
    // `extract` call above.
    if !model_dir.join("am").is_dir() || !model_dir.join("conf").is_dir() {
        let _ = std::fs::remove_dir_all(&model_dir);
        return Err("The downloaded archive does not look like a Vosk model \
             (its am/ or conf/ folder is missing). The download may be \
             incomplete -- try again."
            .to_string());
    }
    model_dir
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Extracted model path has invalid UTF-8.".to_string())
}

/// Progress payload emitted as `voice-model-download-progress` while a model
/// downloads, so the UI shows a real bar (bytes + %) instead of a blind
/// spinner. `total` is 0 when the server wouldn't report Content-Length.
#[derive(Clone, Serialize)]
struct ModelDownloadProgress {
    model_id: String,
    downloaded: u64,
    total: u64,
}

/// Best-effort total download size via an HTTP HEAD (`curl -sIL` follows the
/// redirect to the CDN). Returns None when no Content-Length comes back — the
/// UI then degrades to an indeterminate bar rather than failing.
fn fetch_content_length(url: &str) -> Option<u64> {
    use std::process::Command;
    let mut command = Command::new("curl.exe");
    command.args(["-sIL", "--connect-timeout", "30", url]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command.output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            line.trim()
                .to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|rest| rest.trim().parse::<u64>().ok())
        })
        .last()
}

/// An installed (downloaded + extracted) Vosk model folder, surfaced so the UI
/// can offer "pick an installed model" instead of a raw folder file-picker.
#[derive(Serialize)]
pub struct InstalledVoiceModel {
    /// Folder name, e.g. "vosk-model-en-us-0.22".
    name: String,
    /// Absolute path to the model folder (what `voskModelPath` points at).
    path: String,
    /// Total size on disk, in bytes.
    size_bytes: u64,
}

/// Scan the app's `vosk-models` folder for already-installed models. A valid
/// Vosk model folder always has `am/` and `conf/` subdirectories. Returns an
/// empty list (not an error) when the folder doesn't exist yet.
#[tauri::command]
pub fn voice_list_installed_models(
    app: tauri::AppHandle,
) -> Result<Vec<InstalledVoiceModel>, String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not resolve the app data directory: {e}"))?
        .join("vosk-models");
    let mut models = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(models);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("am").is_dir() && path.join("conf").is_dir() {
            let size_bytes = dir_size_bytes(&path);
            models.push(InstalledVoiceModel {
                name: entry.file_name().to_string_lossy().to_string(),
                path: path.to_string_lossy().to_string(),
                size_bytes,
            });
        }
    }
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

/// Sum the byte sizes of every file under `dir` (best effort).
fn dir_size_bytes(dir: &std::path::Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.metadata().ok())
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
        .sum()
}

/// Frontend-facing cancellation. Best-effort: if nothing's recording
/// this is a no-op. Used by the voice button to make "click off"
/// stop the mic immediately rather than after the natural pause.
#[tauri::command]
pub fn voice_cancel_recognize() {
    #[cfg(feature = "vosk")]
    {
        use std::sync::atomic::Ordering;
        if let Some(flag) = VOSK_ACTIVE_CANCEL.lock().unwrap().as_ref() {
            flag.store(true, Ordering::Release);
        }
    }
}

/// Releases every cached speech model from memory.
///
/// Why this exists: `VOSK_MODEL_CACHE` deliberately keeps loaded models
/// resident so a second recognition doesn't pay the cold-load cost
/// again (several seconds for a large language model). The downside is
/// RAM — a loaded Vosk model is anywhere from ~0.3 GB (small) to well
/// over 1 GB resident, and without eviction that memory is pinned for
/// the entire process lifetime even after the user is completely done
/// dictating. That's the "RAM stays high after I stop" report.
///
/// Dropping the cached `Arc`s lets Vosk free its buffers. We then call
/// `EmptyWorkingSet` so Windows actually trims the freed pages out of
/// the process working set — without that, the committed-but-unused
/// pages linger and Task Manager keeps showing the old high number even
/// though the memory is logically free.
///
/// Safe to call mid-recognition: an in-flight recognition holds its own
/// cloned `Arc`, so clearing the cache map just means the *next* call
/// reloads — the running one finishes untouched, then frees on drop.
fn release_voice_models_internal() {
    #[cfg(feature = "vosk")]
    {
        if let Ok(mut cache) = VOSK_MODEL_CACHE.lock() {
            cache.clear();
        }
    }
    #[cfg(windows)]
    trim_working_set();
}

/// Asks Windows to trim this process's working set. Called right after
/// dropping cached models so the freed pages visibly leave the RAM
/// figure instead of lingering as committed-but-idle working set.
///
/// `EmptyWorkingSet` pages everything out; genuinely-needed pages fault
/// back in within a few hundred ms of next use. That brief re-fault is
/// an acceptable trade for the user actually seeing RAM drop after they
/// stop voice — and the timing is right (they just finished a task).
#[cfg(windows)]
fn trim_working_set() {
    use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
    use windows::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no
    // cleanup; EmptyWorkingSet only asks the OS to trim our own pages.
    unsafe {
        let _ = EmptyWorkingSet(GetCurrentProcess());
    }
}

/// Frontend-facing model release. Call when the user leaves a voice
/// surface — closes the Voice tool page, dismisses the voice overlay,
/// or minimizes KeepItLocal to tray. The next recognition transparently
/// reloads the model. Cheap no-op when nothing is cached.
#[tauri::command]
pub fn voice_release_models() {
    release_voice_models_internal();
}

// â”€â”€â”€ Idle model-release backstop â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Event-driven release (overlay close, page unmount, disarm) and the
// frontend's own idle timer already free models on the common paths.
// This is the BACKEND guarantee: resource release must not *depend* on a
// frontend timer firing or a cleanup call being reached. After
// `IDLE_RELEASE_MS` with no recognition activity — and no continuous
// session running — cached models are dropped no matter what the
// frontend did. The watcher is self-terminating: it exists only while
// voice was recently used and exits once it has released, so a
// truly-idle app runs no voice thread at all (no perpetual polling).

/// Idle window before the backend force-releases cached voice models.
/// Slightly longer than the frontend's 60 s timer so this acts as a
/// backstop for the paths that timer does not cover, rather than racing
/// it.
const IDLE_RELEASE_MS: i64 = 90_000;

/// Unix-ms of the last recognition start. 0 = voice never used this run.
static LAST_VOICE_ACTIVITY_MS: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);
/// True while the idle-watcher thread is alive — keeps it a singleton.
static IDLE_WATCHER_RUNNING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn voice_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// True while a continuous-dictation session is live. That is active
/// voice use, so the idle backstop must not release a model under it.
fn continuous_session_active() -> bool {
    #[cfg(feature = "vosk")]
    if CONTINUOUS_SESSION
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
    {
        return true;
    }
    false
}

/// Record voice activity and make sure the idle-release watcher is
/// running. Call at the start of every recognition. The watcher is
/// spawned at most once and self-terminates after it releases.
fn touch_voice_activity() {
    use std::sync::atomic::Ordering;
    LAST_VOICE_ACTIVITY_MS.store(voice_now_ms(), Ordering::Release);
    if IDLE_WATCHER_RUNNING.swap(true, Ordering::AcqRel) {
        return; // a watcher is already on duty
    }
    let _ = std::thread::Builder::new()
        .name("keepitlocal-voice-idle-watcher".into())
        .spawn(|| {
            use std::sync::atomic::Ordering;
            loop {
                let idle = voice_now_ms() - LAST_VOICE_ACTIVITY_MS.load(Ordering::Acquire);
                if idle < IDLE_RELEASE_MS {
                    std::thread::sleep(std::time::Duration::from_millis(
                        (IDLE_RELEASE_MS - idle).max(1) as u64,
                    ));
                    continue;
                }
                if continuous_session_active() {
                    // Active use — re-check after another full window.
                    std::thread::sleep(std::time::Duration::from_millis(IDLE_RELEASE_MS as u64));
                    continue;
                }
                release_voice_models_internal();
                IDLE_WATCHER_RUNNING.store(false, Ordering::Release);
                // Race guard: if a recognition touched activity between the
                // idle check above and clearing the flag, re-claim the slot
                // so that recognition's model is still released later.
                if voice_now_ms() - LAST_VOICE_ACTIVITY_MS.load(Ordering::Acquire)
                    < IDLE_RELEASE_MS
                    && !IDLE_WATCHER_RUNNING.swap(true, Ordering::AcqRel)
                {
                    continue;
                }
                return;
            }
        });
}

/// Process-wide cache of loaded Vosk models, keyed by the absolute
/// folder path the user picked in Settings. Loading a Vosk model from
/// disk is the slow step in any recognition (~200ms for the small
/// English model, several seconds for larger language models); we
/// only want to pay it once per app lifetime per model.
///
/// `Arc<Model>` lets multiple concurrent recognitions share the same
/// instance — `vosk::Recognizer::new(&Model, ...)` only borrows the
/// model, so the recognizer's lifetime is independent and we can drop
/// the recognizer between utterances without invalidating the cache.
///
/// We never evict from this cache during a session. If a user swaps
/// between models the prior model stays in memory; that's an acceptable
/// tradeoff for the latency win, and worst-case memory is bounded by
/// the number of distinct models the user has pointed at (typically 1).
#[cfg(feature = "vosk")]
static VOSK_MODEL_CACHE: LazyLock<std::sync::Mutex<HashMap<String, std::sync::Arc<vosk::Model>>>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

/// Active continuous-dictation session (Vosk only).
///
/// Distinct from the single-shot path: in continuous mode the audio
/// stream stays open across multiple utterances. Vosk's recognizer
/// emits `voice-final` events whenever it detects end-of-utterance
/// silence, then resets and starts decoding the next phrase — no
/// gap, no audio lost between utterances. This is what users mean
/// when they say "real" continuous dictation.
///
/// Single-shot recognize_with_vosk and continuous can't run at the
/// same time on the same input device (cpal would race). The session
/// holds a stop flag so `voice_stop_continuous` can break the loop.
#[cfg(feature = "vosk")]
struct ContinuousSession {
    stop: std::sync::Arc<AtomicBool>,
}

#[cfg(feature = "vosk")]
static CONTINUOUS_SESSION: LazyLock<std::sync::Mutex<Option<ContinuousSession>>> =
    LazyLock::new(|| std::sync::Mutex::new(None));

// --- Microphone arbitration (#17) -----------------------------------
//
// The microphone is a single OS resource, but several frontend
// surfaces want it: command mode, push-to-talk, the overlays, the
// Voice tool. Every recognition path (single-shot and continuous)
// passes through `mic_acquire` before it opens an audio stream and
// `mic_release` when it ends, so two recognizers can never collide on
// the device. Acquisition is serialized through `MIC_OWNER`, the one
// process-global that records who currently holds the mic -- and
// because every window's `invoke` lands in this one Rust process, that
// makes the arbiter authoritative across windows with no coordination.
//
// Priority (`mic_priority`): a higher-priority request preempts a
// lower one -- stops its session and emits `voice-preempted` so that
// surface can update its UI; an equal-or-higher incumbent refuses the
// request (`MicGrant::Busy`). Push-to-talk is the momentary, explicit
// gesture and tops the ladder; command mode is the ambient background
// and yields to every focused surface, resuming when the mic next
// falls idle (`voice-mic-idle`).

/// Who currently holds the microphone.
#[cfg(feature = "vosk")]
struct MicOwner {
    /// Requesting surface -- the `source` label the command carries
    /// ("push-to-talk", "command-mode", "voice-overlay", ...).
    client: String,
    /// Resolved priority; higher preempts lower.
    priority: i32,
    /// "continuous" or "single-shot".
    mode: &'static str,
    /// Monotonic id, unique per acquisition.
    session_id: u64,
}

#[cfg(feature = "vosk")]
static MIC_OWNER: LazyLock<std::sync::Mutex<Option<MicOwner>>> =
    LazyLock::new(|| std::sync::Mutex::new(None));

/// Monotonic source of `MicOwner::session_id`.
#[cfg(feature = "vosk")]
static MIC_SESSION_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The microphone priority ladder. Higher wins. Push-to-talk is the
/// momentary explicit gesture and tops the ladder; the overlays and
/// the Voice tool are focused surfaces in the middle; command mode is
/// the ambient background and sits at the floor so anything preempts
/// it. An unknown surface lands mid-tier (conservative).
#[cfg(feature = "vosk")]
fn mic_priority(client: &str) -> i32 {
    match client {
        "push-to-talk" => 40,
        "mouse-grid" | "ui-elements" => 35,
        "voice-overlay" | "search-overlay" | "voice-to-text-page" => 30,
        "command-mode" => 10,
        _ => 20,
    }
}

/// Outcome of a `mic_acquire` attempt.
#[cfg(feature = "vosk")]
enum MicGrant {
    /// Granted. `preempted` is true when a lower-priority holder was
    /// stopped to make room (the caller waits for the device to free).
    Granted { session_id: u64, preempted: bool },
    /// Refused -- an equal-or-higher-priority surface holds the mic.
    Busy,
}

/// Stop whatever recognition currently holds the mic so a preemptor
/// can take the device: a continuous session via its stop flag, a
/// single-shot recognize via the cancel flag.
#[cfg(feature = "vosk")]
fn preempt_running_session(mode: &str) {
    use std::sync::atomic::Ordering;
    if mode == "continuous" {
        if let Some(prev) = CONTINUOUS_SESSION.lock().unwrap().take() {
            prev.stop.store(true, Ordering::Release);
        }
    } else if let Some(flag) = VOSK_ACTIVE_CANCEL.lock().unwrap().as_ref() {
        flag.store(true, Ordering::Release);
    }
}

/// Try to acquire the microphone for `client` in `mode`. The single
/// arbitration chokepoint -- every recognition path calls this before
/// it opens an audio stream. See the module note above for the policy.
#[cfg(feature = "vosk")]
fn mic_acquire(app: &tauri::AppHandle, client: &str, mode: &'static str) -> MicGrant {
    use std::sync::atomic::Ordering;
    let priority = mic_priority(client);
    let (session_id, preempted) = {
        let mut owner = MIC_OWNER.lock().unwrap();
        let preempted = match owner.as_ref() {
            // Same surface re-acquiring its own slot -- always allowed.
            Some(cur) if cur.client == client => None,
            // Held by an equal-or-higher-priority surface -- refused.
            Some(cur) if priority <= cur.priority => return MicGrant::Busy,
            // Held by a lower-priority surface -- preempt it.
            Some(cur) => Some((cur.client.clone(), cur.session_id, cur.mode)),
            None => None,
        };
        let session_id = MIC_SESSION_SEQ.fetch_add(1, Ordering::Relaxed);
        *owner = Some(MicOwner {
            client: client.to_string(),
            priority,
            mode,
            session_id,
        });
        (session_id, preempted)
    };
    let grant = match preempted {
        Some((old_client, old_session_id, old_mode)) => {
            // Stop the preempted session, then tell its surface so its
            // UI state does not go stale behind a blind backend stop.
            preempt_running_session(old_mode);
            use tauri::Emitter;
            let _ = app.emit(
                "voice-preempted",
                serde_json::json!({
                    "client": old_client,
                    "sessionId": old_session_id,
                    "by": client,
                }),
            );
            MicGrant::Granted {
                session_id,
                preempted: true,
            }
        }
        None => MicGrant::Granted {
            session_id,
            preempted: false,
        },
    };
    // #24 -- the mic is now held. Light the tray "listening" indicator
    // here, at the one arbitration chokepoint, so EVERY capture path
    // (single-shot, continuous, command mode, the overlays) shows it,
    // and a preemption hand-off stays correct: the new owner's acquire
    // sets it true, and the preempted session's release is a no-op.
    crate::set_tray_listening(app, true);
    grant
}

/// Release the mic if `session_id` still owns it. A no-op when a
/// higher-priority surface has already preempted us (its acquire
/// replaced the owner), so a late release can never clobber the new
/// owner. Emits `voice-mic-idle` when the mic falls genuinely idle --
/// the cue command mode uses to resume after being preempted.
#[cfg(feature = "vosk")]
fn mic_release(app: &tauri::AppHandle, session_id: u64) {
    let became_idle = {
        let mut owner = MIC_OWNER.lock().unwrap();
        if owner.as_ref().map(|o| o.session_id) == Some(session_id) {
            *owner = None;
            true
        } else {
            false
        }
    };
    if became_idle {
        use tauri::Emitter;
        let _ = app.emit("voice-mic-idle", serde_json::json!({}));
        // #24 -- the mic is genuinely idle now; clear the tray
        // indicator. Skipped when this release was a no-op (a
        // higher-priority surface preempted us and still holds the
        // mic), so the indicator never wrongly goes dark mid-handoff.
        crate::set_tray_listening(app, false);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceAvailability {
    /// True when the voice engine is compiled into this build. Vosk is a
    /// default feature, so this is normally true; it is false only in a
    /// `--no-default-features` build without `vosk`.
    pub available: bool,
    /// Best-effort hint when `available` is false — what the user can
    /// do to fix it. Localized in English; UI surfaces verbatim.
    pub hint: Option<String>,
}

/// Quick health check — reports whether the voice engine is compiled
/// into this build. Used by the frontend on tool open to decide whether
/// to render the mic UI or a "voice not available" card. Model
/// configuration (the Vosk model folder) is a separate check the
/// frontend does from the user's settings.
#[tauri::command]
pub fn voice_check_availability() -> VoiceAvailability {
    let compiled = cfg!(feature = "vosk");
    VoiceAvailability {
        available: compiled,
        hint: if compiled {
            None
        } else {
            Some(
                "Voice support was not compiled into this build. \
                 Rebuild with the default features (the `vosk` engine)."
                    .to_string(),
            )
        },
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceRecognitionResult {
    /// The transcribed text. Empty string if the recognizer heard
    /// silence or noise it couldn't decode — `status` carries the
    /// detail.
    pub text: String,
    /// Confidence bucket — "high", "medium", "low", or "rejected".
    /// Useful for UIs that want to surface uncertainty visually.
    pub confidence: String,
    /// Coarse outcome: "success" if we got any usable text,
    /// "no_speech" if nothing was heard, "cancelled" if the user
    /// stopped, "audio_error" / "language_unsupported" /
    /// "permission_denied" / "vosk_setup_required" /
    /// "vosk_not_compiled" / "other" for failures.
    pub status: String,
}

/// Single-shot recognition — record one utterance, return the text.
///
/// `model_path` carries the Vosk model folder.
///
/// `source` is an opaque label echoed back in `voice-partial` events
/// so the frontend can route partial transcripts to the right surface.
///
/// `app` is injected by Tauri and used to (a) emit partial-result
/// events while recognition is in flight and (b) swap the tray icon
/// to the listening state.
#[tauri::command]
pub async fn voice_recognize_once(
    app: tauri::AppHandle,
    model_path: Option<String>,
    source: Option<String>,
) -> Result<VoiceRecognitionResult, String> {
    touch_voice_activity();

    // Arbitrate (#17): claim the mic for this single-shot recognition.
    #[cfg(feature = "vosk")]
    let mic_session = match mic_acquire(&app, source.as_deref().unwrap_or(""), "single-shot") {
        MicGrant::Granted {
            session_id,
            preempted,
        } => {
            if preempted {
                // We stopped a session -- let its cpal stream release
                // the device before we open ours.
                tauri::async_runtime::spawn_blocking(|| {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                })
                .await
                .ok();
            }
            session_id
        }
        MicGrant::Busy => {
            return Ok(VoiceRecognitionResult {
                text: String::new(),
                confidence: "rejected".into(),
                status: "mic_busy".into(),
            });
        }
    };
    // The tray "listening" indicator is driven by the mic arbiter
    // (`mic_acquire` / `mic_release`) since #24 -- one chokepoint, every
    // capture path, preemption-correct -- so it is not set here.
    let result = recognize_with_vosk(app.clone(), model_path.unwrap_or_default(), source).await;

    // Release the mic slot. No-op if a higher-priority surface already
    // preempted us mid-recognition (its acquire replaced the owner).
    #[cfg(feature = "vosk")]
    mic_release(&app, mic_session);

    result
}

// â”€â”€â”€ Continuous dictation â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// One persistent audio stream, one persistent recognizer. The cpal
// callback feeds samples directly into the recognizer; whenever
// Vosk reports `Finalized`, we emit a `voice-final` event with the
// utterance text and reset the recognizer so the next phrase starts
// fresh. The user gets gap-free dictation for as long as they want
// to talk — the loop only exits when `voice_stop_continuous` flips
// the stop flag.
//
// The single-shot `voice_recognize_once` path stays exactly as it
// was: same session-bookkeeping, same cancel slot, same return
// shape. The Voice tool page picks between the two paths based on
// whether the user clicks "Speak once" (single-shot) or
// "Continuous dictation" (this).

/// Start a long-running dictation session — a true streaming session:
/// one persistent audio stream + recognizer, gap-free phrase boundary
/// firing.
///
/// Idempotent — if a session is already running this stops it first
/// and waits briefly for the audio thread to clean up before starting
/// a fresh one.
///
/// Emits two event streams while running:
///   - `voice-final`   { text, source } — fires per finalized utterance
///   - `voice-partial` { text, source } — fires while decoding (~5Hz)
///
/// Pair with `voice_stop_continuous` to end the session. Closing the
/// app via the X button (which hides to tray) also stops it via the
/// frontend's window-hide watcher.
#[tauri::command]
pub async fn voice_start_continuous(
    app: tauri::AppHandle,
    model_path: Option<String>,
    source: Option<String>,
    grammar: Option<Vec<String>>,
) -> Result<(), String> {
    touch_voice_activity();
    start_vosk_continuous(app, model_path, source, grammar).await
}

async fn start_vosk_continuous(
    app: tauri::AppHandle,
    model_path: Option<String>,
    source: Option<String>,
    grammar: Option<Vec<String>>,
) -> Result<(), String> {
    #[cfg(feature = "vosk")]
    {
        use std::sync::atomic::Ordering;

        let model_path = model_path.unwrap_or_default();
        if model_path.trim().is_empty() {
            return Err("vosk_setup_required".to_string());
        }
        let _ = crate::core::safe_path::forbid_system_path(&model_path)
            .map_err(|e| format!("Vosk model path rejected: {e}"))?;

        // Arbitrate (#17): claim the mic. A lower-priority holder is
        // preempted; an equal-or-higher one refuses us with mic_busy.
        let session_id = match mic_acquire(&app, source.as_deref().unwrap_or(""), "continuous") {
            MicGrant::Granted { session_id, .. } => session_id,
            MicGrant::Busy => return Err("mic_busy".to_string()),
        };

        // Stop any existing session first. Done outside the lock
        // hold so the worker thread (which doesn't acquire the
        // session lock) can finish cleaning up its audio stream.
        if let Some(prev) = CONTINUOUS_SESSION.lock().unwrap().take() {
            prev.stop.store(true, Ordering::Release);
        }
        // Register THIS session's stop flag BEFORE the settle delay,
        // so a higher-priority surface that preempts us mid-startup
        // (#17) can flag this session to stop. The worker below
        // early-exits if the flag is already set when it starts.
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        *CONTINUOUS_SESSION.lock().unwrap() = Some(ContinuousSession {
            stop: std::sync::Arc::clone(&stop),
        });

        // Brief pause so the previous cpal stream releases the audio
        // device before the worker opens a new one. cpal supports
        // multiple streams in principle, but back-to-back open/close
        // on the same device occasionally races on Windows WASAPI.
        tauri::async_runtime::spawn_blocking(|| {
            std::thread::sleep(std::time::Duration::from_millis(250));
        })
        .await
        .ok();

        // Spin up the worker thread that owns the audio stream + the
        // recognizer for this session. Returns immediately — events
        // arrive on the frontend asynchronously.
        let app_clone = app.clone();
        std::thread::Builder::new()
            .name("keepitlocal-voice-continuous".to_string())
            .spawn(move || {
                // Release the mic slot when the worker exits -- normal
                // stop, preemption, or a startup error all land here.
                let app_for_release = app_clone.clone();
                if let Err(error) =
                    run_vosk_continuous(app_clone, model_path, source, grammar, stop)
                {
                    eprintln!("vosk continuous session error: {error}");
                }
                mic_release(&app_for_release, session_id);
            })
            .map_err(|e| format!("Cannot spawn continuous session thread: {e}"))?;
        return Ok(());
    }
    #[cfg(not(feature = "vosk"))]
    {
        let _ = (app, model_path, source, grammar);
        Err("vosk_not_compiled".to_string())
    }
}

/// Stop the running continuous-dictation session, if any. Safe to
/// call from anywhere — frontend Stop button, window-hide watcher,
/// app-quit hook, etc. Returns immediately; the worker thread
/// drops its audio stream on its next iteration boundary (~50ms).
#[tauri::command]
pub fn voice_stop_continuous() {
    #[cfg(feature = "vosk")]
    {
        use std::sync::atomic::Ordering;
        if let Some(session) = CONTINUOUS_SESSION.lock().unwrap().take() {
            session.stop.store(true, Ordering::Release);
        }
    }
    // Free cached models on stop. This call clears the cache map and
    // trims the working set immediately; the continuous worker also
    // calls release on its way out (after its in-flight Arc drops),
    // so the model memory is reclaimed both promptly and completely.
    // Stopping continuous dictation is an unambiguous "done with
    // voice" signal — the user shouldn't pay 1-2 GB of idle RAM for a
    // model they've finished using.
    release_voice_models_internal();
}

/// Whether the #23 webrtc-vad pre-filter gates continuous recognition.
/// OFF by default -- a voice-activity detector can misclassify quiet
/// speech, and silently dropping a user's words is far worse than the
/// CPU the gate would save. Opt-in via Settings -> Voice;
/// `voice_set_vad_enabled` keeps it in sync with that setting.
static VAD_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Enable or disable the #23 VAD pre-filter. Called by the frontend
/// whenever the `voiceVadEnabled` setting changes. Takes effect on the
/// next continuous session (an in-flight session keeps its current gate).
#[tauri::command]
pub fn voice_set_vad_enabled(enabled: bool) {
    VAD_ENABLED.store(enabled, std::sync::atomic::Ordering::Release);
}

// --- Voice-activity gate (#23) ---------------------------------------
//
// webrtc-vad is a small deterministic DSP voice-activity detector (the
// WebRTC project's VAD) -- NOT an ML model, in keeping with the
// deterministic-over-ML rule. It pre-filters the continuous mic stream:
// Vosk is fed speech plus a generous trailing-silence hangover, but NOT
// fed during sustained idle silence. That saves CPU while command mode
// listens in the background and stops ambient noise from decoding into
// spurious partials.

/// 20 ms of 16 kHz mono PCM -- 320 samples. webrtc-vad accepts only
/// 10 / 20 / 30 ms frames.
#[cfg(feature = "vosk")]
const VAD_FRAME: usize = 320;

/// Trailing silence still fed to Vosk after speech stops -- comfortably
/// longer than Vosk's own endpoint window, so Vosk always finalizes the
/// utterance before the gate closes. 1.5 s at 16 kHz.
#[cfg(feature = "vosk")]
const VAD_HANGOVER_SAMPLES: usize = 24_000;

/// A voice-activity gate -- pre-filters the mic stream ahead of Vosk.
#[cfg(feature = "vosk")]
struct VadGate {
    vad: webrtc_vad::Vad,
    /// Samples of continuous non-speech since the last voiced frame.
    silent_samples: usize,
}

// VadGate is built on the run_vosk_continuous thread and then used ONLY
// on the single cpal callback thread (cpal serializes its callbacks).
// webrtc-vad's Vad wraps a C instance and is not Send; the one-time
// move into that thread, and exclusive use there, make this sound.
#[cfg(feature = "vosk")]
unsafe impl Send for VadGate {}

#[cfg(feature = "vosk")]
impl VadGate {
    fn new() -> Self {
        let mut vad =
            webrtc_vad::Vad::new_with_rate(webrtc_vad::SampleRate::Rate16kHz);
        // `Quality` is the LEAST-aggressive webrtc-vad mode -- it errs
        // toward classifying borderline audio as speech. For a gate
        // whose failure mode is "dropped the user's words", erring
        // toward feeding Vosk is the right bias.
        vad.set_mode(webrtc_vad::VadMode::Quality);
        // Start past the hangover so Vosk is not fed until the user
        // actually speaks.
        Self {
            vad,
            silent_samples: VAD_HANGOVER_SAMPLES + 1,
        }
    }

    /// Whether this chunk of 16 kHz mono PCM should reach Vosk: true for
    /// speech and the hangover tail after it, false during sustained
    /// silence. Fails OPEN -- a too-short / malformed frame counts as
    /// speech, so the gate can never swallow a real utterance.
    fn should_feed(&mut self, pcm: &[i16]) -> bool {
        let voiced = if pcm.len() < VAD_FRAME {
            true
        } else {
            pcm.chunks_exact(VAD_FRAME)
                .any(|frame| self.vad.is_voice_segment(frame).unwrap_or(true))
        };
        if voiced {
            self.silent_samples = 0;
            true
        } else {
            self.silent_samples = self.silent_samples.saturating_add(pcm.len());
            self.silent_samples <= VAD_HANGOVER_SAMPLES
        }
    }
}

#[cfg(feature = "vosk")]
fn run_vosk_continuous(
    app: tauri::AppHandle,
    model_path: String,
    source: Option<String>,
    grammar: Option<Vec<String>>,
    stop: std::sync::Arc<AtomicBool>,
) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, StreamConfig};
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tauri::Emitter;
    use vosk::{DecodingState, Recognizer};

    // Preempted before we even started -- a higher-priority surface
    // grabbed the mic during the caller's settle delay (#17). Bail
    // before opening any audio stream.
    if stop.load(std::sync::atomic::Ordering::Acquire) {
        return Ok(());
    }

    const TARGET_SAMPLE_RATE: f32 = 16_000.0;

    // Resolve / load the model (uses the same process-wide cache as
    // single-shot, so repeated continuous starts are instant).
    let model = {
        let mut cache = VOSK_MODEL_CACHE.lock().unwrap();
        if let Some(existing) = cache.get(&model_path) {
            Arc::clone(existing)
        } else {
            let loaded = vosk::Model::new(&model_path).ok_or_else(|| {
                format!(
                    "Vosk model failed to load from '{model_path}'. \
                     Make sure the path points at a folder containing am/, conf/, \
                     graph/, and ivector/ subdirectories (typical Vosk model layout)."
                )
            })?;
            let arc = Arc::new(loaded);
            cache.insert(model_path.clone(), Arc::clone(&arc));
            arc
        }
    };
    // Grammar-constrained recognition (command mode): when a grammar is
    // supplied the recognizer only decodes utterances built from those
    // phrases' words — far faster and more accurate than open dictation,
    // which is what makes continuous command listening cheap enough to
    // honour the golden rules. The appended "[unk]" token lets speech
    // outside the grammar decode as "unknown" (then ignored upstream)
    // rather than being force-fit to the nearest command phrase. No
    // grammar → ordinary open-vocabulary recognition (normal dictation).
    let recognizer_handle = match &grammar {
        Some(phrases) if !phrases.is_empty() => {
            let mut g: Vec<&str> = phrases.iter().map(String::as_str).collect();
            g.push("[unk]");
            Recognizer::new_with_grammar(&model, TARGET_SAMPLE_RATE, &g)
        }
        _ => Recognizer::new(&model, TARGET_SAMPLE_RATE),
    }
    .ok_or_else(|| "Could not initialize Vosk recognizer.".to_string())?;
    let recognizer = Arc::new(Mutex::new(recognizer_handle));

    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "No default audio input device.".to_string())?;
    let supported = device
        .default_input_config()
        .map_err(|e| format!("Could not query input device config: {e}"))?;
    let device_sample_rate = supported.sample_rate().0 as f32;
    let device_channels = supported.channels();
    let sample_format = supported.sample_format();
    let needs_resample = (device_sample_rate - TARGET_SAMPLE_RATE).abs() > 1.0;
    let stream_config: StreamConfig = supported.into();

    // Audio callback — runs on a cpal-owned thread. KEY DIFFERENCE
    // vs single-shot: when Finalized fires, we DON'T set stop. We
    // emit the text and reset the recognizer so the next utterance
    // gets a clean slate. Audio keeps flowing, no gap.
    let stop_for_cb = Arc::clone(&stop);
    let recognizer_for_cb = Arc::clone(&recognizer);
    let app_for_cb = app.clone();
    let source_for_cb = source.clone();
    // #23 -- the VAD pre-filter, OPT-IN (off by default). Built only
    // when the user has enabled it; `None` means every chunk is fed to
    // Vosk, exactly as before #23. Moved into whichever sample-format
    // callback is used, then touched only on that one cpal thread.
    let mut vad_gate: Option<VadGate> = if VAD_ENABLED.load(Ordering::Acquire) {
        Some(VadGate::new())
    } else {
        None
    };

    let err_cb = move |err| eprintln!("cpal continuous stream error: {err}");

    let stream = match sample_format {
        SampleFormat::I16 => device
            .build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    if stop_for_cb.load(Ordering::Acquire) {
                        return;
                    }
                    let mono = downmix_i16(data, device_channels as usize);
                    let pcm = if needs_resample {
                        resample_linear_i16(&mono, device_sample_rate, TARGET_SAMPLE_RATE)
                    } else {
                        mono
                    };
                    // #23 -- when the VAD pre-filter is enabled, skip
                    // feeding Vosk during sustained silence.
                    if let Some(gate) = vad_gate.as_mut() {
                        if !gate.should_feed(&pcm) {
                            return;
                        }
                    }
                    let mut rec = recognizer_for_cb.lock().unwrap();
                    if let Ok(DecodingState::Finalized) = rec.accept_waveform(&pcm) {
                        let text = rec
                            .result()
                            .single()
                            .map(|r| r.text.to_string())
                            .unwrap_or_default();
                        if !text.trim().is_empty() {
                            let _ = app_for_cb.emit(
                                "voice-final",
                                serde_json::json!({
                                    "text": text,
                                    "source": source_for_cb,
                                }),
                            );
                        }
                        // Reset internal state so the next phrase
                        // doesn't accumulate decoder context from
                        // the one we just emitted.
                        rec.reset();
                    }
                },
                err_cb,
                None,
            )
            .map_err(|e| format!("Could not open audio input (i16): {e}"))?,
        SampleFormat::F32 => device
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    if stop_for_cb.load(Ordering::Acquire) {
                        return;
                    }
                    let mono = downmix_f32(data, device_channels as usize);
                    let pcm_f32 = if needs_resample {
                        resample_linear_f32(&mono, device_sample_rate, TARGET_SAMPLE_RATE)
                    } else {
                        mono
                    };
                    let pcm: Vec<i16> = pcm_f32
                        .into_iter()
                        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
                        .collect();
                    // #23 -- when the VAD pre-filter is enabled, skip
                    // feeding Vosk during sustained silence.
                    if let Some(gate) = vad_gate.as_mut() {
                        if !gate.should_feed(&pcm) {
                            return;
                        }
                    }
                    let mut rec = recognizer_for_cb.lock().unwrap();
                    if let Ok(DecodingState::Finalized) = rec.accept_waveform(&pcm) {
                        let text = rec
                            .result()
                            .single()
                            .map(|r| r.text.to_string())
                            .unwrap_or_default();
                        if !text.trim().is_empty() {
                            let _ = app_for_cb.emit(
                                "voice-final",
                                serde_json::json!({
                                    "text": text,
                                    "source": source_for_cb,
                                }),
                            );
                        }
                        rec.reset();
                    }
                },
                err_cb,
                None,
            )
            .map_err(|e| format!("Could not open audio input (f32): {e}"))?,
        other => {
            return Err(format!(
                "Audio input sample format {other:?} not supported by this build."
            ));
        }
    };

    stream
        .play()
        .map_err(|e| format!("Could not start audio stream: {e}"))?;

    // Main thread: emit partials + watch for stop. Nothing here
    // touches the recognizer except for partial reads, which can
    // run concurrently with the callback's accept_waveform writes
    // because both go through the same Mutex.
    let mut last_partial = String::new();
    while !stop.load(Ordering::Acquire) {
        let partial = {
            let mut rec = recognizer.lock().unwrap();
            rec.partial_result().partial.to_string()
        };
        let trimmed = partial.trim();
        if !trimmed.is_empty() && trimmed != last_partial.as_str() {
            last_partial = trimmed.to_string();
            let _ = app.emit(
                "voice-partial",
                serde_json::json!({
                    "text": trimmed,
                    "source": source,
                }),
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    drop(stream);
    // Best-effort: emit one final result if there's residual decoded
    // text in the recognizer at stop time.
    let tail = {
        let mut rec = recognizer.lock().unwrap();
        rec.final_result()
            .single()
            .map(|r| r.text.to_string())
            .unwrap_or_default()
    };
    if !tail.trim().is_empty() {
        let _ = app.emit(
            "voice-final",
            serde_json::json!({
                "text": tail,
                "source": source,
            }),
        );
    }
    let _ = Instant::now;
    // Session over — release the cached Vosk model so its memory is
    // reclaimed now rather than pinned until process exit. See
    // release_voice_models_internal.
    release_voice_models_internal();
    Ok(())
}

// â”€â”€â”€ Vosk backend â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Two implementations live in this file:
//   - The real one, gated behind `#[cfg(feature = "vosk")]`. Requires
//     libvosk.dll + an extracted language model on disk.
//   - The stub, gated behind `#[cfg(not(feature = "vosk"))]`. Returns
//     `vosk_not_compiled` so the frontend can surface a helpful card.
//
// Building both forms behind cfg means downstream users who don't have
// libvosk on their build machine can still compile the project.

#[cfg(feature = "vosk")]
async fn recognize_with_vosk(
    app: tauri::AppHandle,
    model_path: String,
    source: Option<String>,
) -> Result<VoiceRecognitionResult, String> {
    if model_path.trim().is_empty() {
        return Ok(VoiceRecognitionResult {
            text: String::new(),
            confidence: "rejected".into(),
            status: "vosk_setup_required".into(),
        });
    }
    // Path validation — refuses model paths under Windows / Program Files
    // / etc. Even though the model is read-only, we don't want users
    // accidentally pointing at random system folders.
    let _ = crate::core::safe_path::forbid_system_path(&model_path)
        .map_err(|e| format!("Vosk model path rejected: {e}"))?;

    let model_path_clone = model_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        vosk_capture_and_recognize(app, model_path_clone, source)
    })
    .await
    .map_err(|e| format!("Vosk worker join failed: {e}"))?
}

#[cfg(not(feature = "vosk"))]
async fn recognize_with_vosk(
    _app: tauri::AppHandle,
    model_path: String,
    _source: Option<String>,
) -> Result<VoiceRecognitionResult, String> {
    if model_path.trim().is_empty() {
        return Ok(VoiceRecognitionResult {
            text: String::new(),
            confidence: "rejected".into(),
            status: "vosk_setup_required".into(),
        });
    }
    // Even when Vosk isn't compiled in, run path validation so misuse
    // surfaces consistently across both build modes.
    let _ = crate::core::safe_path::forbid_system_path(&model_path)
        .map_err(|e| format!("Vosk model path rejected: {e}"))?;
    Ok(VoiceRecognitionResult {
        text: String::new(),
        confidence: "rejected".into(),
        status: "vosk_not_compiled".into(),
    })
}

#[cfg(feature = "vosk")]
fn vosk_capture_and_recognize(
    app: tauri::AppHandle,
    model_path: String,
    source: Option<String>,
) -> Result<VoiceRecognitionResult, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, StreamConfig};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tauri::Emitter;
    use vosk::{DecodingState, Recognizer};

    // Vosk wants 16 kHz mono 16-bit PCM. We let cpal pick a config that
    // matches (or resample on the fly if it can't).
    const TARGET_SAMPLE_RATE: f32 = 16_000.0;

    // Resolve the Vosk model — hot path on second+ recognition because
    // we cache the loaded model at process scope. The first load for a
    // given path can still take seconds (small English ~200ms, large
    // multi-language several seconds); subsequent calls are essentially
    // free.
    let model = {
        let mut cache = VOSK_MODEL_CACHE.lock().unwrap();
        if let Some(existing) = cache.get(&model_path) {
            Arc::clone(existing)
        } else {
            let loaded = vosk::Model::new(&model_path).ok_or_else(|| {
                format!(
                    "Vosk model failed to load from '{model_path}'. \
                     Make sure the path points at a folder containing am/, conf/, \
                     graph/, and ivector/ subdirectories (typical Vosk model layout)."
                )
            })?;
            let arc = Arc::new(loaded);
            cache.insert(model_path.clone(), Arc::clone(&arc));
            arc
        }
    };
    let recognizer_handle =
        Recognizer::new(&model, TARGET_SAMPLE_RATE).ok_or_else(|| {
            "Could not initialize Vosk recognizer for this model + sample rate.".to_string()
        })?;
    // Wrap the recognizer in a mutex so the cpal callback (which is
    // FnMut + Send) can mutate it. cpal's data callback fires from an
    // OS audio thread, hence the Send+Sync requirement.
    let recognizer = Arc::new(Mutex::new(recognizer_handle));

    // Pick the default input device. If none, surface a clear error.
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "No default audio input device. Is a microphone connected?".to_string())?;

    let supported = device
        .default_input_config()
        .map_err(|e| format!("Could not query input device config: {e}"))?;
    let device_sample_rate = supported.sample_rate().0 as f32;
    let device_channels = supported.channels();
    let sample_format = supported.sample_format();

    // We always feed the recognizer at TARGET_SAMPLE_RATE. If the device
    // sample rate differs we do a poor-man's linear-interpolation
    // resample inside the callback — fine for speech (which is bandlimited
    // well below 8 kHz Nyquist) and avoids pulling in a full DSP crate.
    let needs_resample = (device_sample_rate - TARGET_SAMPLE_RATE).abs() > 1.0;

    // Atomic stop flag — flipped from the main thread when end-of-speech
    // is detected, the safety cap fires, or the frontend issues an
    // explicit `voice_cancel_recognize` (typically when the user
    // toggles the mic off). The audio callback + the poll loop both
    // check this on every batch so shutdown is responsive.
    //
    // We also publish this flag to a process-global slot so the
    // cancel command can reach it without threading state through
    // every layer.
    let stop = Arc::new(AtomicBool::new(false));
    *VOSK_ACTIVE_CANCEL.lock().unwrap() = Some(Arc::clone(&stop));
    // Channel-of-one for the final transcript text. The callback writes
    // here when the recognizer reports a finalized result.
    let result_text = Arc::new(Mutex::new(None::<String>));

    let stop_in_cb = Arc::clone(&stop);
    let result_in_cb = Arc::clone(&result_text);
    let recognizer_in_cb = Arc::clone(&recognizer);

    let stream_config: StreamConfig = supported.into();

    // Build the input stream. cpal's `build_input_stream` is generic
    // over sample format, so we branch once and reuse the same closure
    // logic with type-appropriate conversion to i16 PCM.
    let err_cb = move |err| eprintln!("cpal stream error: {err}");

    // Map cpal's BuildStreamError text into a frontend-friendly status
    // when possible. Windows reports microphone permission denial as
    // 0x80070005 ("Access is denied") wrapped in a BackendSpecific
    // error — we surface that as `permission_denied` so the frontend
    // uses the same helpful toast as the Windows engine path. Other
    // errors bubble up as raw strings.
    let classify = |label: &str, err: cpal::BuildStreamError| -> Result<cpal::Stream, _> {
        let s = err.to_string();
        let lower = s.to_ascii_lowercase();
        if lower.contains("access is denied") || lower.contains("0x80070005") {
            // Early-return the structured result up the call chain by
            // packing it into the outer Result<VoiceRecognitionResult,_>.
            Err(Ok::<VoiceRecognitionResult, String>(VoiceRecognitionResult {
                text: String::new(),
                confidence: "rejected".into(),
                status: "permission_denied".into(),
            }))
        } else {
            Err(Err::<VoiceRecognitionResult, String>(format!(
                "Could not open audio input ({label}): {s}"
            )))
        }
    };

    // Helper to unwrap classify's nested Result: returns the
    // VoiceRecognitionResult early (permission_denied), or surfaces the
    // String error to the caller.
    macro_rules! build_or_return {
        ($expr:expr, $label:expr) => {
            match $expr {
                Ok(stream) => stream,
                Err(err) => match classify($label, err) {
                    Err(Ok(result)) => return Ok(result),
                    Err(Err(msg)) => return Err(msg),
                    _ => unreachable!(),
                },
            }
        };
    }

    let stream = match sample_format {
        SampleFormat::I16 => build_or_return!(
            device.build_input_stream(
                &stream_config,
                move |data: &[i16], _| {
                    if stop_in_cb.load(Ordering::Acquire) {
                        return;
                    }
                    let mono = downmix_i16(data, device_channels as usize);
                    let pcm = if needs_resample {
                        resample_linear_i16(&mono, device_sample_rate, TARGET_SAMPLE_RATE)
                    } else {
                        mono
                    };
                    let mut rec = recognizer_in_cb.lock().unwrap();
                    // accept_waveform returns Result<DecodingState, _>;
                    // Finalized means the recognizer detected end of
                    // utterance and has a finalized transcript. Other
                    // states (Running, Failed) are not actionable for
                    // single-shot capture — we just keep feeding audio.
                    if let Ok(DecodingState::Finalized) = rec.accept_waveform(&pcm) {
                        let text = rec.result().single().map(|r| r.text.to_string());
                        if let Some(t) = text {
                            if !t.trim().is_empty() {
                                *result_in_cb.lock().unwrap() = Some(t);
                                stop_in_cb.store(true, Ordering::Release);
                            }
                        }
                    }
                },
                err_cb,
                None,
            ),
            "i16"
        ),
        SampleFormat::F32 => build_or_return!(
            device.build_input_stream(
                &stream_config,
                move |data: &[f32], _| {
                    if stop_in_cb.load(Ordering::Acquire) {
                        return;
                    }
                    let mono = downmix_f32(data, device_channels as usize);
                    let pcm_f32 = if needs_resample {
                        resample_linear_f32(&mono, device_sample_rate, TARGET_SAMPLE_RATE)
                    } else {
                        mono
                    };
                    let pcm: Vec<i16> = pcm_f32
                        .into_iter()
                        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
                        .collect();
                    let mut rec = recognizer_in_cb.lock().unwrap();
                    if let Ok(DecodingState::Finalized) = rec.accept_waveform(&pcm) {
                        let text = rec.result().single().map(|r| r.text.to_string());
                        if let Some(t) = text {
                            if !t.trim().is_empty() {
                                *result_in_cb.lock().unwrap() = Some(t);
                                stop_in_cb.store(true, Ordering::Release);
                            }
                        }
                    }
                },
                err_cb,
                None,
            ),
            "f32"
        ),
        SampleFormat::U16 => {
            return Err(
                "Audio input format U16 is unsupported by this build. \
                 Please switch your default microphone to a 16-bit or 32-bit format."
                    .into(),
            );
        }
        other => {
            return Err(format!(
                "Audio input sample format {other:?} not supported by this build."
            ));
        }
    };

    stream
        .play()
        .map_err(|e| format!("Could not start audio stream: {e}"))?;

    // Poll the stop flag with a hard cap so the user can't get stuck if
    // they stay silent forever. Vosk's end-of-utterance detection fires
    // on the natural pause; the cap is a safety net.
    //
    // Within the poll we also peek at the recognizer's *partial* result
    // (the in-progress decoding) and emit it as a `voice-partial` event
    // every ~200ms when the text changes. The frontend streams these
    // into a live "Hearing: â€¦" indicator, so users see words appearing
    // as they speak instead of waiting for end-of-utterance silence.
    let started = Instant::now();
    let max_listen = Duration::from_secs(30);
    let silence_cap = Duration::from_secs(15);
    let mut last_partial = String::new();
    let mut last_partial_emit = Instant::now()
        .checked_sub(Duration::from_millis(250))
        .unwrap_or_else(Instant::now);
    while !stop.load(Ordering::Acquire) {
        if started.elapsed() > max_listen {
            break;
        }
        if started.elapsed() > silence_cap && result_text.lock().unwrap().is_none() {
            // No text after ~15s of audio — likely silence or
            // unintelligible noise. Stop so the user can retry.
            break;
        }

        // Emit a partial-result event when the recognizer's current
        // best guess changes. We throttle to 5 Hz so the frontend
        // isn't flooded mid-utterance.
        if last_partial_emit.elapsed() >= Duration::from_millis(200) {
            let partial_text = {
                let mut rec = recognizer.lock().unwrap();
                rec.partial_result().partial.to_string()
            };
            let trimmed = partial_text.trim();
            if !trimmed.is_empty() && trimmed != last_partial.as_str() {
                last_partial = trimmed.to_string();
                let _ = app.emit(
                    "voice-partial",
                    serde_json::json!({
                        "text": trimmed,
                        "source": source,
                    }),
                );
            }
            last_partial_emit = Instant::now();
        }

        std::thread::sleep(Duration::from_millis(50));
    }

    drop(stream); // graceful close
    // Clear the cancellation slot now that we own the wind-down. If
    // we leave it dangling, a stale cancel request from the frontend
    // could land just as the next recognize starts and abort it
    // before it begins.
    *VOSK_ACTIVE_CANCEL.lock().unwrap() = None;

    // Detect *why* we exited so the caller can distinguish "user
    // cancelled mid-utterance" from "no speech detected" — the first
    // shouldn't surface as an error, the second is fine to surface
    // (it's how the recognizer signals silence).
    let cancelled = stop.load(Ordering::Acquire) && result_text.lock().unwrap().is_none();

    // Final flush — ask the recognizer for its current best guess in
    // case the stream stopped before Finalized fired.
    let final_text = {
        let mut rec = recognizer.lock().unwrap();
        let buffered = result_text.lock().unwrap().clone();
        buffered.unwrap_or_else(|| {
            rec.final_result()
                .single()
                .map(|r| r.text.to_string())
                .unwrap_or_default()
        })
    };

    if final_text.trim().is_empty() {
        Ok(VoiceRecognitionResult {
            text: String::new(),
            confidence: "rejected".into(),
            status: if cancelled { "cancelled".into() } else { "no_speech".into() },
        })
    } else {
        Ok(VoiceRecognitionResult {
            text: final_text,
            // Vosk doesn't expose a single confidence bucket per result
            // by default — its per-word confidences live behind
            // `set_words(true)`. For v1 we report "high" on success;
            // future iteration can wire per-word confidences.
            confidence: "high".into(),
            status: if cancelled { "cancelled".into() } else { "success".into() },
        })
    }
}

// â”€â”€â”€ Audio helpers (only compiled when Vosk is on) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Downmix interleaved multi-channel i16 audio to mono by averaging.
/// Vosk only takes mono; most laptop mics are stereo.
#[cfg(feature = "vosk")]
fn downmix_i16(data: &[i16], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return data.to_vec();
    }
    data.chunks_exact(channels)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / channels as i32) as i16
        })
        .collect()
}

#[cfg(feature = "vosk")]
fn downmix_f32(data: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }
    data.chunks_exact(channels)
        .map(|frame| {
            let sum: f32 = frame.iter().sum();
            sum / channels as f32
        })
        .collect()
}

/// Linear-interpolation resampler. Crude but fine for speech bandlimited
/// well below the Nyquist frequency of typical capture rates (44.1/48k).
/// Avoids pulling in a heavier DSP crate just for one feature.
#[cfg(feature = "vosk")]
fn resample_linear_i16(data: &[i16], from_hz: f32, to_hz: f32) -> Vec<i16> {
    if (from_hz - to_hz).abs() < 1.0 {
        return data.to_vec();
    }
    let ratio = from_hz / to_hz;
    let out_len = (data.len() as f32 / ratio).ceil() as usize;
    (0..out_len)
        .map(|i| {
            let src_idx = i as f32 * ratio;
            let i0 = src_idx.floor() as usize;
            let i1 = (i0 + 1).min(data.len().saturating_sub(1));
            let frac = src_idx - i0 as f32;
            let s0 = data.get(i0).copied().unwrap_or(0) as f32;
            let s1 = data.get(i1).copied().unwrap_or(0) as f32;
            (s0 + (s1 - s0) * frac) as i16
        })
        .collect()
}

#[cfg(feature = "vosk")]
fn resample_linear_f32(data: &[f32], from_hz: f32, to_hz: f32) -> Vec<f32> {
    if (from_hz - to_hz).abs() < 1.0 {
        return data.to_vec();
    }
    let ratio = from_hz / to_hz;
    let out_len = (data.len() as f32 / ratio).ceil() as usize;
    (0..out_len)
        .map(|i| {
            let src_idx = i as f32 * ratio;
            let i0 = src_idx.floor() as usize;
            let i1 = (i0 + 1).min(data.len().saturating_sub(1));
            let frac = src_idx - i0 as f32;
            let s0 = data.get(i0).copied().unwrap_or(0.0);
            let s1 = data.get(i1).copied().unwrap_or(0.0);
            s0 + (s1 - s0) * frac
        })
        .collect()
}
