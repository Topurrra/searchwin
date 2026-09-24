use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{Emitter, State, Window};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::AesMode;
use zip::CompressionMethod as ZipCompression;

use crate::CancelFlag;

const ARCHIVE_BUFFER_SIZE: usize = 256 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 250_000;
const MAX_EXTRACTED_BYTES: u64 = 50 * 1024 * 1024 * 1024;
const MAX_COMPRESSION_RATIO: u64 = 500;
const ARCHIVE_WORKER_ARG: &str = "--keepitlocal-archive-worker";
const ARCHIVE_WORKER_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Deserialize, Serialize)]
pub struct CreateArchiveOptions {
    pub paths: Vec<String>,
    pub output_path: String,
    pub format: String,
    pub password: Option<String>,
    pub compression_level: u32,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct CreateArchiveResult {
    pub output_path: String,
    pub total_files: usize,
    pub original_size: u64,
    pub archive_size: u64,
    pub elapsed_ms: u64,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ArchiveProgressEvent {
    pub current_file: String,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Deserialize)]
pub struct ExtractOptions {
    pub archive_path: String,
    pub output_dir: String,
    pub password: Option<String>,
    pub overwrite: bool,
}

#[derive(Deserialize)]
pub struct InspectArchiveOptions {
    pub path: String,
    pub password: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct ExtractResult {
    pub output_dir: String,
    pub format: String,
    pub total_files: usize,
    pub total_bytes: u64,
    pub elapsed_ms: u64,
    pub skipped: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct ArchiveListEntry {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub encrypted: bool,
}

#[derive(Serialize, Clone)]
pub struct ArchiveInfo {
    pub format: String,
    pub entries: Vec<ArchiveListEntry>,
    pub total_size: u64,
    pub has_encryption: bool,
}

struct ArchiveSourceEntry {
    absolute: PathBuf,
    archive_name: String,
    size: u64,
}

#[derive(Clone)]
enum ArchiveProgressTarget {
    Window(Window),
    Worker,
}

#[derive(Deserialize, Serialize)]
struct ArchiveWorkerRequest {
    options: CreateArchiveOptions,
    staging_output_path: String,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ArchiveWorkerMessage {
    Progress { event: ArchiveProgressEvent },
    Result { result: CreateArchiveResult },
    Error { error: String },
}

struct ArchiveCreateProcess {
    child: Arc<Mutex<Child>>,
}

static ARCHIVE_CREATE_PROCESS: LazyLock<Mutex<Option<ArchiveCreateProcess>>> =
    LazyLock::new(|| Mutex::new(None));
static ARCHIVE_CREATE_CANCELLED: AtomicBool = AtomicBool::new(false);

struct ArchiveCreateProcessGuard {
    child: Arc<Mutex<Child>>,
}

impl Drop for ArchiveCreateProcessGuard {
    fn drop(&mut self) {
        if let Ok(mut process) = ARCHIVE_CREATE_PROCESS.lock() {
            if process
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(&active.child, &self.child))
            {
                *process = None;
            }
        }
    }
}

fn reset_cancel(flag: &CancelFlag) {
    flag.0.store(false, Ordering::Relaxed);
}

fn is_cancelled(flag: &CancelFlag) -> bool {
    flag.0.load(Ordering::Relaxed)
}

struct CancelReader<'a> {
    file: File,
    flag: &'a CancelFlag,
}

impl Read for CancelReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if is_cancelled(self.flag) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Cancelled by user",
            ));
        }
        self.file.read(buffer)
    }
}

#[tauri::command]
pub fn cancel_archive_operation(flag: State<'_, CancelFlag>) {
    flag.0.store(true, Ordering::Relaxed);
    if let Some(child) = active_archive_create_child() {
        ARCHIVE_CREATE_CANCELLED.store(true, Ordering::Relaxed);
        kill_archive_create_child(&child);
    }
}

pub fn stop_all_archive_workers() {
    let child = ARCHIVE_CREATE_PROCESS
        .lock()
        .ok()
        .and_then(|mut process| process.take().map(|process| process.child));
    if let Some(child) = child {
        ARCHIVE_CREATE_CANCELLED.store(true, Ordering::Relaxed);
        kill_archive_create_child(&child);
    }
}

fn active_archive_create_child() -> Option<Arc<Mutex<Child>>> {
    ARCHIVE_CREATE_PROCESS
        .lock()
        .ok()
        .and_then(|process| process.as_ref().map(|process| process.child.clone()))
}

fn register_archive_create_child(child: Arc<Mutex<Child>>) -> Result<(), String> {
    let mut process = ARCHIVE_CREATE_PROCESS
        .lock()
        .map_err(|_| "Archive worker registry lock failed.".to_string())?;
    if let Some(active) = process.as_ref() {
        let running = active
            .child
            .lock()
            .ok()
            .and_then(|mut child| child.try_wait().ok())
            .is_some_and(|status| status.is_none());
        if running {
            return Err("An archive operation is already running.".into());
        }
    }
    *process = Some(ArchiveCreateProcess { child });
    Ok(())
}

fn kill_archive_create_child(child: &Arc<Mutex<Child>>) {
    if let Ok(mut child) = child.lock() {
        let _ = child.kill();
    }
}

fn try_wait_archive_create_child(child: &Arc<Mutex<Child>>) -> Result<Option<ExitStatus>, String> {
    child
        .lock()
        .map_err(|_| "Archive worker process lock failed.".to_string())?
        .try_wait()
        .map_err(|error| format!("Could not wait for archive worker: {error}"))
}

async fn run_archive_work<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("Archive worker failed: {error}"))?
}

fn canonicalish(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn normalized_archive_output_path(path: &Path, format: &str) -> Result<PathBuf, String> {
    let suffix = match format {
        "zip" => ".zip",
        "7z" => ".7z",
        "tar.gz" => ".tar.gz",
        "tar.xz" => ".tar.xz",
        "tar.zst" => ".tar.zst",
        _ => return Err(format!("Unsupported archive format: {format}")),
    };
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Output path needs a filename".to_string())?;

    if filename.to_ascii_lowercase().ends_with(suffix) {
        return Ok(path.to_path_buf());
    }
    if path.extension().is_none() {
        return Ok(path.with_file_name(format!("{filename}{suffix}")));
    }

    Err(format!(
        "Output filename must end in {suffix} for {} archives",
        format.to_ascii_uppercase()
    ))
}

fn archive_output_parts(path: &Path) -> (PathBuf, String, String, String) {
    let parent = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("archive")
        .to_string();
    let lower_filename = filename.to_ascii_lowercase();
    let compound_suffix = [".tar.zst", ".tar.gz", ".tar.xz"]
        .iter()
        .find(|suffix| lower_filename.ends_with(**suffix))
        .copied();
    let stem = compound_suffix
        .map(|suffix| filename[..filename.len() - suffix.len()].to_string())
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("archive")
                .to_string()
        });
    let suffix = compound_suffix
        .map(str::to_string)
        .or_else(|| {
            path.extension()
                .and_then(|value| value.to_str())
                .map(|extension| format!(".{extension}"))
        })
        .unwrap_or_default();

    (parent, filename, stem, suffix)
}

fn archive_output_candidate(path: &Path, index: usize) -> PathBuf {
    let (parent, filename, stem, suffix) = archive_output_parts(path);
    let candidate_name = if index == 0 {
        filename
    } else if suffix.is_empty() {
        format!("{stem}_{index}")
    } else {
        format!("{stem}_{index}{suffix}")
    };
    parent.join(candidate_name)
}

fn archive_staging_path(path: &Path) -> Result<PathBuf, String> {
    let (parent, _, stem, suffix) = archive_output_parts(path);
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "System clock is unavailable.".to_string())?
        .as_nanos();

    for attempt in 0..100u8 {
        let candidate = parent.join(format!(
            ".{stem}.keepitlocal-archive-{}-{nonce}-{attempt}{suffix}",
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err("Could not reserve a temporary archive path. Try again.".into())
}

#[cfg(windows)]
fn move_staged_archive_without_overwrite(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| format!("Could not finalize archive: {error}"))
}

#[cfg(not(windows))]
fn move_staged_archive_without_overwrite(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Err("Output archive already exists.".into());
    }
    std::fs::rename(source, destination)
        .map_err(|error| format!("Could not finalize archive: {error}"))
}

fn promote_staged_archive(staging: &Path, requested_output: &Path) -> Result<PathBuf, String> {
    for index in 0..10_000 {
        let candidate = archive_output_candidate(requested_output, index);
        match move_staged_archive_without_overwrite(staging, &candidate) {
            Ok(()) => return Ok(candidate),
            Err(_) if candidate.exists() => continue,
            Err(error) => return Err(error),
        }
    }

    Err("Could not find a free output filename".into())
}

fn ensure_output_not_inside_inputs(output: &Path, inputs: &[PathBuf]) -> Result<(), String> {
    let output_abs = if output.exists() {
        canonicalish(output)
    } else if let Some(parent) = output.parent() {
        canonicalish(parent).join(output.file_name().unwrap_or_default())
    } else {
        output.to_path_buf()
    };

    for input in inputs {
        let input_abs = canonicalish(input);
        if input_abs.is_file() && input_abs == output_abs {
            return Err("Output archive cannot overwrite one of the selected input files".into());
        }
        if input_abs.is_dir() && output_abs.starts_with(&input_abs) {
            return Err(format!(
                "Output archive is inside selected input folder: {}. Choose an output location outside the folder being archived.",
                input_abs.display()
            ));
        }
    }
    Ok(())
}

fn check_entry_count(count: usize) -> Result<(), String> {
    if count > MAX_ARCHIVE_ENTRIES {
        Err(format!(
            "Archive has too many entries ({count}). Limit is {MAX_ARCHIVE_ENTRIES}."
        ))
    } else {
        Ok(())
    }
}

fn check_extract_limits(
    next_total_bytes: u64,
    entries: usize,
    archive_size: Option<u64>,
) -> Result<(), String> {
    check_entry_count(entries)?;
    if next_total_bytes > MAX_EXTRACTED_BYTES {
        return Err(format!(
            "Extraction would create more than {} GiB of data. This is blocked to avoid archive-bomb style accidents.",
            MAX_EXTRACTED_BYTES / 1024 / 1024 / 1024
        ));
    }
    if let Some(archive_size) = archive_size {
        if archive_size > 0 && next_total_bytes / archive_size > MAX_COMPRESSION_RATIO {
            return Err(
                "Archive expansion ratio is suspiciously high. Extraction blocked for safety."
                    .into(),
            );
        }
    }
    Ok(())
}

fn is_tar_regular_file(entry_type: tar::EntryType) -> bool {
    entry_type.is_file()
}

fn collect_entries(
    inputs: &[PathBuf],
    flag: &CancelFlag,
) -> Result<(Vec<ArchiveSourceEntry>, u64), String> {
    let mut entries = Vec::new();
    let mut total_bytes = 0u64;

    for input in inputs {
        if is_cancelled(flag) {
            return Err("Cancelled by user".into());
        }

        let metadata = std::fs::metadata(input)
            .map_err(|error| format!("Cannot stat {}: {error}", input.display()))?;
        if metadata.is_file() {
            let archive_name = input
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| format!("Invalid filename: {}", input.display()))?
                .to_string();
            total_bytes = total_bytes
                .checked_add(metadata.len())
                .ok_or_else(|| "Selected files are too large".to_string())?;
            entries.push(ArchiveSourceEntry {
                absolute: input.clone(),
                archive_name,
                size: metadata.len(),
            });
            continue;
        }

        if !metadata.is_dir() {
            continue;
        }

        let folder_name = input
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("Invalid folder name: {}", input.display()))?;

        for walk_entry in WalkDir::new(input).follow_links(false) {
            if is_cancelled(flag) {
                return Err("Cancelled by user".into());
            }

            let walk_entry =
                walk_entry.map_err(|error| format!("Cannot read {}: {error}", input.display()))?;
            if !walk_entry.file_type().is_file() {
                continue;
            }
            check_entry_count(entries.len() + 1)?;

            let relative = walk_entry
                .path()
                .strip_prefix(input)
                .map_err(|error| format!("Path strip failed: {error}"))?;
            let size = walk_entry
                .metadata()
                .map_err(|error| format!("Cannot stat {}: {error}", walk_entry.path().display()))?
                .len();
            total_bytes = total_bytes
                .checked_add(size)
                .ok_or_else(|| "Selected files are too large".to_string())?;
            entries.push(ArchiveSourceEntry {
                absolute: walk_entry.path().to_path_buf(),
                archive_name: format!(
                    "{folder_name}/{}",
                    relative.to_string_lossy().replace('\\', "/")
                ),
                size,
            });
        }
    }

    check_entry_count(entries.len())?;
    if entries.is_empty() {
        return Err("No files to archive".into());
    }
    Ok((entries, total_bytes))
}

fn append_tar_entries<W: Write>(
    tar_writer: &mut tar::Builder<W>,
    entries: &[ArchiveSourceEntry],
    window: &ArchiveProgressTarget,
    flag: &CancelFlag,
    total_bytes: u64,
    format: &str,
) -> Result<u64, String> {
    tar_writer.mode(tar::HeaderMode::Deterministic);
    let mut bytes_done = 0u64;

    for (index, entry) in entries.iter().enumerate() {
        if is_cancelled(flag) {
            return Err("Cancelled by user".into());
        }
        emit_progress(
            window,
            &entry.archive_name,
            index,
            entries.len(),
            bytes_done,
            total_bytes,
        );

        let input = File::open(&entry.absolute)
            .map_err(|error| format!("Cannot read {}: {error}", entry.absolute.display()))?;
        let metadata = input
            .metadata()
            .map_err(|error| format!("Cannot read {}: {error}", entry.absolute.display()))?;
        let mut header = tar::Header::new_gnu();
        header.set_metadata_in_mode(&metadata, tar::HeaderMode::Deterministic);
        tar_writer
            .append_data(
                &mut header,
                &entry.archive_name,
                CancelReader { file: input, flag },
            )
            .map_err(|error| {
                if is_cancelled(flag) {
                    reset_cancel(flag);
                    "Cancelled by user".into()
                } else {
                    format!("{format} add failed: {error}")
                }
            })?;
        bytes_done += entry.size;

        emit_progress(
            window,
            &entry.archive_name,
            index + 1,
            entries.len(),
            bytes_done,
            total_bytes,
        );
    }

    Ok(bytes_done)
}

#[tauri::command]
pub async fn create_archive(
    window: Window,
    flag: State<'_, CancelFlag>,
    options: CreateArchiveOptions,
) -> Result<CreateArchiveResult, String> {
    let worker_flag = CancelFlag(flag.0.clone());
    reset_cancel(&worker_flag);
    ARCHIVE_CREATE_CANCELLED.store(false, Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || {
        create_archive_in_worker_process(window, worker_flag, options)
    })
    .await
    .map_err(|error| format!("Archive worker monitor failed: {error}"))?
}

fn create_archive_to_staging(
    window: ArchiveProgressTarget,
    flag: CancelFlag,
    options: CreateArchiveOptions,
    staging_output_path: PathBuf,
) -> Result<CreateArchiveResult, String> {
    let format = options.format.trim().to_ascii_lowercase();
    let inputs = options
        .paths
        .iter()
        .map(crate::core::safe_path::validate_user_path)
        .collect::<Result<Vec<_>, _>>()?;
    let output = crate::core::safe_path::validate_user_write_target(&staging_output_path)?;
    ensure_output_not_inside_inputs(&output, &inputs)?;

    let start = std::time::Instant::now();
    let (entries, total_bytes) = match collect_entries(&inputs, &flag) {
        Ok(entries) => entries,
        Err(error) => return Err(error),
    };
    let output_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .map_err(|error| format!("Cannot create temporary archive: {error}"))?;
    let level = options.compression_level.clamp(0, 9);
    let password = options
        .password
        .as_deref()
        .filter(|value| !value.is_empty());

    let output_file = match format.as_str() {
        "zip" => {
            let mut writer = zip::ZipWriter::new(BufWriter::new(output_file));
            let mut bytes_done = 0u64;
            let mut last_emit_bytes = 0u64;

            for (index, entry) in entries.iter().enumerate() {
                if is_cancelled(&flag) {
                    let _ = writer.finish();
                    reset_cancel(&flag);
                    return Err("Cancelled by user".into());
                }

                emit_progress(
                    &window,
                    &entry.archive_name,
                    index,
                    entries.len(),
                    bytes_done,
                    total_bytes,
                );
                let options = SimpleFileOptions::default()
                    .compression_method(if level == 0 {
                        ZipCompression::Stored
                    } else {
                        ZipCompression::Deflated
                    })
                    .compression_level(Some(level as i64));
                let options = match password {
                    Some(password) => options.with_aes_encryption(AesMode::Aes256, password),
                    None => options,
                };
                writer
                    .start_file(&entry.archive_name, options)
                    .map_err(|error| format!("ZIP entry start failed: {error}"))?;

                let input = File::open(&entry.absolute).map_err(|error| {
                    format!("Cannot read {}: {error}", entry.absolute.display())
                })?;
                let mut reader = BufReader::with_capacity(ARCHIVE_BUFFER_SIZE, input);
                let result = copy_with_cancel(
                    &mut reader,
                    &mut writer,
                    || is_cancelled(&flag),
                    u64::MAX,
                    |written| {
                        bytes_done += written;
                        if bytes_done - last_emit_bytes >= ARCHIVE_BUFFER_SIZE as u64 {
                            emit_progress(
                                &window,
                                &entry.archive_name,
                                index,
                                entries.len(),
                                bytes_done,
                                total_bytes,
                            );
                            last_emit_bytes = bytes_done;
                        }
                    },
                );
                if let Err(error) = result {
                    let _ = writer.finish();
                    reset_cancel(&flag);
                    return Err(error);
                }
            }

            emit_progress(
                &window,
                "",
                entries.len(),
                entries.len(),
                bytes_done,
                total_bytes,
            );
            let buffer = writer
                .finish()
                .map_err(|error| format!("Finalize failed: {error}"))?;
            buffer
                .into_inner()
                .map_err(|error| format!("ZIP finalization flush failed: {error}"))?
        }
        "7z" => {
            let mut writer = sevenz_rust2::SevenZWriter::new(output_file)
                .map_err(|error| format!("Cannot create 7z archive: {error}"))?;
            if let Some(password) = password {
                writer.set_content_methods(vec![
                    sevenz_rust2::AesEncoderOptions::new(sevenz_rust2::Password::from(password))
                        .into(),
                    sevenz_rust2::lzma::LZMA2Options::with_preset(level).into(),
                ]);
                writer.set_encrypt_header(true);
            } else {
                writer.set_content_methods(vec![sevenz_rust2::lzma::LZMA2Options::with_preset(
                    level,
                )
                .into()]);
            }

            let mut bytes_done = 0u64;
            for (index, entry) in entries.iter().enumerate() {
                if is_cancelled(&flag) {
                    reset_cancel(&flag);
                    return Err("Cancelled by user".into());
                }
                emit_progress(
                    &window,
                    &entry.archive_name,
                    index,
                    entries.len(),
                    bytes_done,
                    total_bytes,
                );

                let input = File::open(&entry.absolute).map_err(|error| {
                    format!("Cannot read {}: {error}", entry.absolute.display())
                })?;
                let archive_entry = sevenz_rust2::SevenZArchiveEntry::from_path(
                    &entry.absolute,
                    entry.archive_name.clone(),
                );
                if let Err(error) = writer.push_archive_entry(
                    archive_entry,
                    Some(CancelReader {
                        file: input,
                        flag: &flag,
                    }),
                ) {
                    if is_cancelled(&flag) {
                        reset_cancel(&flag);
                        return Err("Cancelled by user".into());
                    }
                    return Err(format!("7z add failed: {error}"));
                }
                bytes_done += entry.size;
                emit_progress(
                    &window,
                    &entry.archive_name,
                    index + 1,
                    entries.len(),
                    bytes_done,
                    total_bytes,
                );
            }
            writer
                .finish()
                .map_err(|error| format!("7z finalize failed: {error}"))?
        }
        "tar.gz" | "tar.xz" | "tar.zst" => {
            if password.is_some() {
                return Err(
                    "tar formats do not support passwords. Use ZIP or 7Z for encryption.".into(),
                );
            }
            let buffer = BufWriter::new(output_file);

            match format.as_str() {
                "tar.gz" => {
                    let encoder =
                        flate2::write::GzEncoder::new(buffer, flate2::Compression::new(level));
                    let mut tar_writer = tar::Builder::new(encoder);
                    let bytes_done = match append_tar_entries(
                        &mut tar_writer,
                        &entries,
                        &window,
                        &flag,
                        total_bytes,
                        "tar.gz",
                    ) {
                        Ok(bytes_done) => bytes_done,
                        Err(error) => {
                            drop(tar_writer);
                            reset_cancel(&flag);
                            return Err(error);
                        }
                    };
                    let encoder = tar_writer
                        .into_inner()
                        .map_err(|error| format!("tar finalize: {error}"))?;
                    let buffer = encoder
                        .finish()
                        .map_err(|error| format!("gzip finalize: {error}"))?;
                    emit_progress(
                        &window,
                        "",
                        entries.len(),
                        entries.len(),
                        bytes_done,
                        total_bytes,
                    );
                    buffer
                        .into_inner()
                        .map_err(|error| format!("gzip finalization flush failed: {error}"))?
                }
                "tar.xz" => {
                    let encoder = xz2::write::XzEncoder::new(buffer, level);
                    let mut tar_writer = tar::Builder::new(encoder);
                    let bytes_done = match append_tar_entries(
                        &mut tar_writer,
                        &entries,
                        &window,
                        &flag,
                        total_bytes,
                        "tar.xz",
                    ) {
                        Ok(bytes_done) => bytes_done,
                        Err(error) => {
                            drop(tar_writer);
                            reset_cancel(&flag);
                            return Err(error);
                        }
                    };
                    let encoder = tar_writer
                        .into_inner()
                        .map_err(|error| format!("tar finalize: {error}"))?;
                    let buffer = encoder
                        .finish()
                        .map_err(|error| format!("xz finalize: {error}"))?;
                    emit_progress(
                        &window,
                        "",
                        entries.len(),
                        entries.len(),
                        bytes_done,
                        total_bytes,
                    );
                    buffer
                        .into_inner()
                        .map_err(|error| format!("xz finalization flush failed: {error}"))?
                }
                "tar.zst" => {
                    let encoder = zstd::stream::write::Encoder::new(buffer, level as i32)
                        .map_err(|error| format!("zstd initialize failed: {error}"))?;
                    let mut tar_writer = tar::Builder::new(encoder);
                    let bytes_done = match append_tar_entries(
                        &mut tar_writer,
                        &entries,
                        &window,
                        &flag,
                        total_bytes,
                        "tar.zst",
                    ) {
                        Ok(bytes_done) => bytes_done,
                        Err(error) => {
                            drop(tar_writer);
                            reset_cancel(&flag);
                            return Err(error);
                        }
                    };
                    let encoder = tar_writer
                        .into_inner()
                        .map_err(|error| format!("tar finalize: {error}"))?;
                    let buffer = encoder
                        .finish()
                        .map_err(|error| format!("zstd finalize: {error}"))?;
                    emit_progress(
                        &window,
                        "",
                        entries.len(),
                        entries.len(),
                        bytes_done,
                        total_bytes,
                    );
                    buffer
                        .into_inner()
                        .map_err(|error| format!("zstd finalization flush failed: {error}"))?
                }
                _ => unreachable!(),
            }
        }
        _ => unreachable!("archive format was validated before reserving output"),
    };

    output_file
        .sync_all()
        .map_err(|error| format!("Could not finalize temporary archive: {error}"))?;
    let archive_size = std::fs::metadata(&output)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    Ok(CreateArchiveResult {
        output_path: output.to_string_lossy().to_string(),
        total_files: entries.len(),
        original_size: total_bytes,
        archive_size,
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

fn emit_progress(
    window: &ArchiveProgressTarget,
    file: &str,
    files_done: usize,
    files_total: usize,
    bytes_done: u64,
    bytes_total: u64,
) {
    let event = ArchiveProgressEvent {
        current_file: file.to_string(),
        files_done,
        files_total,
        bytes_done,
        bytes_total,
    };
    match window {
        ArchiveProgressTarget::Window(window) => {
            let _ = window.emit("archive-progress", event);
        }
        ArchiveProgressTarget::Worker => {
            let _ = write_archive_worker_message(&ArchiveWorkerMessage::Progress { event });
        }
    }
}

fn write_archive_worker_message(message: &ArchiveWorkerMessage) -> Result<(), String> {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer(&mut stdout, message)
        .map_err(|error| format!("Could not write archive worker message: {error}"))?;
    stdout
        .write_all(b"\n")
        .map_err(|error| format!("Could not write archive worker message: {error}"))?;
    stdout
        .flush()
        .map_err(|error| format!("Could not flush archive worker message: {error}"))
}

fn validate_archive_create_target(options: &CreateArchiveOptions) -> Result<PathBuf, String> {
    let format = options.format.trim().to_ascii_lowercase();
    let requested_output_path = normalized_archive_output_path(Path::new(&options.output_path), &format)?;
    let inputs = options
        .paths
        .iter()
        .map(crate::core::safe_path::validate_user_path)
        .collect::<Result<Vec<_>, _>>()?;
    let requested_output = crate::core::safe_path::validate_user_write_target(&requested_output_path)?;
    ensure_output_not_inside_inputs(&requested_output, &inputs)?;
    Ok(requested_output)
}

fn spawn_archive_create_worker(
    request: &ArchiveWorkerRequest,
) -> Result<(Arc<Mutex<Child>>, std::process::ChildStdout), String> {
    let mut command = Command::new(
        std::env::current_exe()
            .map_err(|error| format!("Cannot resolve KeepItLocal executable: {error}"))?,
    );
    command
        .arg(ARCHIVE_WORKER_ARG)
        .arg("create")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("Cannot start archive worker: {error}"))?;
    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Could not open archive worker input.".into());
    };
    if let Err(error) = serde_json::to_writer(&mut stdin, request) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("Could not send archive worker request: {error}"));
    }
    drop(stdin);
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Could not read archive worker output.".into());
    };
    Ok((Arc::new(Mutex::new(child)), stdout))
}

fn remove_staging_archive(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn create_archive_in_worker_process(
    window: Window,
    flag: CancelFlag,
    options: CreateArchiveOptions,
) -> Result<CreateArchiveResult, String> {
    let requested_output = validate_archive_create_target(&options)?;
    if is_cancelled(&flag) {
        return Err("Cancelled by user".into());
    }
    let staging_output = archive_staging_path(&requested_output)?;
    let request = ArchiveWorkerRequest {
        options,
        staging_output_path: staging_output.to_string_lossy().to_string(),
    };
    let (child, stdout) = match spawn_archive_create_worker(&request) {
        Ok(worker) => worker,
        Err(error) => {
            remove_staging_archive(&staging_output);
            return Err(error);
        }
    };

    if let Err(error) = register_archive_create_child(child.clone()) {
        kill_archive_create_child(&child);
        let _ = child.lock().map(|mut child| child.wait());
        remove_staging_archive(&staging_output);
        return Err(error);
    }
    let _process_guard = ArchiveCreateProcessGuard {
        child: child.clone(),
    };
    if is_cancelled(&flag) {
        ARCHIVE_CREATE_CANCELLED.store(true, Ordering::Relaxed);
        kill_archive_create_child(&child);
    }

    let (sender, receiver) = mpsc::channel::<Result<ArchiveWorkerMessage, String>>();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let message = line
                .map_err(|error| format!("Could not read archive worker output: {error}"))
                .and_then(|line| {
                    serde_json::from_str::<ArchiveWorkerMessage>(&line)
                        .map_err(|error| format!("Invalid archive worker output: {error}"))
                });
            if sender.send(message).is_err() {
                break;
            }
        }
    });

    let mut worker_result = None;
    let mut worker_error = None;
    let mut cancellation_killed = ARCHIVE_CREATE_CANCELLED.load(Ordering::Relaxed);
    let status = loop {
        match receiver.recv_timeout(ARCHIVE_WORKER_POLL_INTERVAL) {
            Ok(Ok(ArchiveWorkerMessage::Progress { event })) => {
                let _ = window.emit("archive-progress", event);
            }
            Ok(Ok(ArchiveWorkerMessage::Result { result })) => worker_result = Some(result),
            Ok(Ok(ArchiveWorkerMessage::Error { error })) => worker_error = Some(error),
            Ok(Err(error)) => worker_error = Some(error),
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => {}
        }

        match try_wait_archive_create_child(&child) {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                kill_archive_create_child(&child);
                let _ = child.lock().map(|mut child| child.wait());
                remove_staging_archive(&staging_output);
                return Err(error);
            }
        }

        if !cancellation_killed && is_cancelled(&flag) {
            ARCHIVE_CREATE_CANCELLED.store(true, Ordering::Relaxed);
            kill_archive_create_child(&child);
            cancellation_killed = true;
        }
    };

    let _ = reader.join();
    while let Ok(message) = receiver.try_recv() {
        match message {
            Ok(ArchiveWorkerMessage::Progress { event }) => {
                let _ = window.emit("archive-progress", event);
            }
            Ok(ArchiveWorkerMessage::Result { result }) => worker_result = Some(result),
            Ok(ArchiveWorkerMessage::Error { error }) | Err(error) => worker_error = Some(error),
        }
    }

    if ARCHIVE_CREATE_CANCELLED.load(Ordering::Relaxed) || is_cancelled(&flag) {
        remove_staging_archive(&staging_output);
        return Err("Cancelled by user".into());
    }
    if let Some(error) = worker_error {
        remove_staging_archive(&staging_output);
        return Err(error);
    }
    if !status.success() {
        remove_staging_archive(&staging_output);
        return Err("Archive worker stopped before completing the archive.".into());
    }
    let Some(mut result) = worker_result else {
        remove_staging_archive(&staging_output);
        return Err("Archive worker finished without a result.".into());
    };
    let output = promote_staged_archive(&staging_output, &requested_output).map_err(|error| {
        format!(
            "{error} The completed archive remains at {}.",
            staging_output.display()
        )
    })?;
    result.output_path = output.to_string_lossy().to_string();
    Ok(result)
}

pub fn maybe_run_archive_worker_from_args() -> bool {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let Some(position) = args
        .iter()
        .position(|argument| argument.to_string_lossy() == ARCHIVE_WORKER_ARG)
    else {
        return false;
    };
    let mode = args
        .get(position + 1)
        .and_then(|argument| argument.to_str())
        .unwrap_or_default();
    let result = match mode {
        "create" => run_archive_create_worker(),
        _ => Err("Archive worker was started with invalid arguments.".into()),
    };
    if let Err(error) = result {
        let _ = write_archive_worker_message(&ArchiveWorkerMessage::Error { error });
    }
    true
}

fn run_archive_create_worker() -> Result<(), String> {
    let request: ArchiveWorkerRequest = serde_json::from_reader(std::io::stdin().lock())
        .map_err(|error| format!("Could not read archive worker request: {error}"))?;
    let flag = CancelFlag(Arc::new(AtomicBool::new(false)));
    let result = create_archive_to_staging(
        ArchiveProgressTarget::Worker,
        flag,
        request.options,
        PathBuf::from(request.staging_output_path),
    )?;
    write_archive_worker_message(&ArchiveWorkerMessage::Result { result })
}

fn format_from_filename(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some("tar.gz")
    } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        Some("tar.xz")
    } else if name.ends_with(".tar.zst") || name.ends_with(".tzst") {
        Some("tar.zst")
    } else if name.ends_with(".zip") {
        Some("zip")
    } else if name.ends_with(".7z") {
        Some("7z")
    } else {
        None
    }
}

fn detect_format(path: &Path) -> Result<String, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Invalid filename".to_string())?;
    if let Some(format) = format_from_filename(name) {
        return Ok(format.to_string());
    }

    let mut file = File::open(path).map_err(|error| format!("Cannot open: {error}"))?;
    let mut header = [0u8; 8];
    let read = file
        .read(&mut header)
        .map_err(|error| format!("Cannot read header: {error}"))?;
    if read >= 4
        && (header.starts_with(b"PK\x03\x04")
            || header.starts_with(b"PK\x05\x06")
            || header.starts_with(b"PK\x07\x08"))
    {
        return Ok("zip".into());
    }
    if read >= 6 && header.starts_with(b"7z\xBC\xAF\x27\x1C") {
        return Ok("7z".into());
    }
    if read >= 2 && header.starts_with(&[0x1F, 0x8B]) {
        return Ok("tar.gz".into());
    }
    if read >= 6 && header.starts_with(b"\xFD7zXZ\x00") {
        return Ok("tar.xz".into());
    }
    if read >= 4 && header.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        return Ok("tar.zst".into());
    }

    Err("Unsupported or unknown archive format".into())
}

fn open_tar_reader(path: &Path, format: &str) -> Result<Box<dyn Read>, String> {
    let file = File::open(path).map_err(|error| format!("Cannot open: {error}"))?;
    let reader = BufReader::new(file);
    match format {
        "tar.gz" => Ok(Box::new(flate2::read::GzDecoder::new(reader))),
        "tar.xz" => Ok(Box::new(xz2::read::XzDecoder::new(reader))),
        "tar.zst" => Ok(Box::new(
            zstd::stream::read::Decoder::new(reader)
                .map_err(|error| format!("zstd initialize failed: {error}"))?,
        )),
        _ => Err(format!("Unsupported TAR format: {format}")),
    }
}

#[tauri::command]
pub fn inspect_archive(options: InspectArchiveOptions) -> Result<ArchiveInfo, String> {
    let path = crate::core::safe_path::validate_user_path(options.path)?;
    let format = detect_format(&path)?;
    let archive_size = std::fs::metadata(&path)
        .map(|metadata| metadata.len())
        .ok();
    let mut entries = Vec::new();
    let mut total_size = 0u64;
    let mut has_encryption = false;

    match format.as_str() {
        "zip" => {
            let file = File::open(&path).map_err(|error| format!("Cannot open: {error}"))?;
            let mut archive =
                zip::ZipArchive::new(file).map_err(|error| format!("Not a valid ZIP: {error}"))?;
            check_entry_count(archive.len())?;
            for index in 0..archive.len() {
                let entry = archive
                    .by_index_raw(index)
                    .map_err(|error| format!("Read entry failed: {error}"))?;
                let encrypted = entry.encrypted();
                has_encryption |= encrypted;
                let size = entry.size();
                total_size = total_size.saturating_add(size);
                entries.push(ArchiveListEntry {
                    name: entry.name().to_string(),
                    size,
                    is_dir: entry.is_dir(),
                    encrypted,
                });
            }
            check_extract_limits(total_size, entries.len(), archive_size)?;
        }
        "7z" => {
            let password = options
                .password
                .as_deref()
                .filter(|value| !value.is_empty())
                .map(sevenz_rust2::Password::from)
                .unwrap_or_else(sevenz_rust2::Password::empty);
            let archive = sevenz_rust2::SevenZReader::open(&path, password).map_err(|error| {
                let message = error.to_string();
                if message.to_ascii_lowercase().contains("password") {
                    "Password required or incorrect to inspect this 7Z archive".to_string()
                } else {
                    format!("7z open failed: {message}")
                }
            })?;
            check_entry_count(archive.archive().files.len())?;
            for entry in &archive.archive().files {
                if entry.has_stream() && !entry.is_anti_item {
                    total_size = total_size.saturating_add(entry.size());
                }
                entries.push(ArchiveListEntry {
                    name: entry.name().to_string(),
                    size: entry.size(),
                    is_dir: entry.is_directory(),
                    encrypted: false,
                });
            }
            check_extract_limits(total_size, entries.len(), archive_size)?;
        }
        "tar.gz" | "tar.xz" | "tar.zst" => {
            let reader = open_tar_reader(&path, &format)?;
            let mut archive = tar::Archive::new(reader);
            for entry in archive
                .entries()
                .map_err(|error| format!("TAR list failed: {error}"))?
            {
                let entry = entry.map_err(|error| format!("TAR entry failed: {error}"))?;
                check_entry_count(entries.len() + 1)?;
                let header = entry.header();
                let path = entry.path().map_err(|error| format!("Bad path: {error}"))?;
                let size = header.size().unwrap_or(0);
                check_extract_limits(
                    total_size.saturating_add(size),
                    entries.len() + 1,
                    archive_size,
                )?;
                total_size = total_size.saturating_add(size);
                entries.push(ArchiveListEntry {
                    name: path.to_string_lossy().to_string(),
                    size,
                    is_dir: header.entry_type().is_dir(),
                    encrypted: false,
                });
            }
        }
        _ => return Err(format!("Unsupported archive format: {format}")),
    }

    Ok(ArchiveInfo {
        format,
        entries,
        total_size,
        has_encryption,
    })
}

fn copy_with_cancel<R, W, C, P>(
    reader: &mut R,
    writer: &mut W,
    cancelled: C,
    max_bytes: u64,
    mut progress: P,
) -> Result<u64, String>
where
    R: Read,
    W: Write,
    C: Fn() -> bool,
    P: FnMut(u64),
{
    let mut buffer = vec![0u8; ARCHIVE_BUFFER_SIZE];
    let mut copied = 0u64;

    loop {
        if cancelled() {
            return Err("Cancelled by user".into());
        }
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("Read failed: {error}"))?;
        if read == 0 {
            return Ok(copied);
        }

        copied = copied
            .checked_add(read as u64)
            .ok_or_else(|| "Archive entry is too large".to_string())?;
        if copied > max_bytes {
            return Err("Extraction exceeds the configured safety limit".into());
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|error| format!("Write failed: {error}"))?;
        progress(read as u64);
    }
}

#[tauri::command]
pub async fn extract_archive(
    window: Window,
    flag: State<'_, CancelFlag>,
    options: ExtractOptions,
) -> Result<ExtractResult, String> {
    let worker_flag = CancelFlag(flag.0.clone());
    run_archive_work(move || {
        extract_archive_inner(ArchiveProgressTarget::Window(window), worker_flag, options)
    })
    .await
}

fn extract_archive_inner(
    window: ArchiveProgressTarget,
    flag: CancelFlag,
    options: ExtractOptions,
) -> Result<ExtractResult, String> {
    let archive = crate::core::safe_path::validate_user_path(&options.archive_path)?;
    let output_dir = crate::core::safe_path::forbid_system_path(&options.output_dir)?;

    reset_cancel(&flag);
    let start = std::time::Instant::now();
    std::fs::create_dir_all(&output_dir)
        .map_err(|error| format!("Cannot create output dir: {error}"))?;
    let output_dir = crate::core::safe_path::forbid_system_path(
        output_dir
            .canonicalize()
            .map_err(|error| format!("Cannot access output dir: {error}"))?,
    )?;

    let format = detect_format(&archive)?;
    let archive_size = std::fs::metadata(&archive)
        .map(|metadata| metadata.len())
        .ok();
    let mut total_files = 0usize;
    let mut total_bytes = 0u64;
    let mut skipped = Vec::new();
    let password = options
        .password
        .as_deref()
        .filter(|value| !value.is_empty());

    match format.as_str() {
        "zip" => {
            let file = File::open(&archive).map_err(|error| format!("Cannot open: {error}"))?;
            let mut zip =
                zip::ZipArchive::new(file).map_err(|error| format!("Not a valid ZIP: {error}"))?;
            let count = zip.len();
            check_entry_count(count)?;

            let mut declared_total = 0u64;
            for index in 0..count {
                let entry = zip
                    .by_index_raw(index)
                    .map_err(|error| format!("ZIP entry read failed: {error}"))?;
                if !entry.is_dir() {
                    declared_total = declared_total.saturating_add(entry.size());
                }
            }
            check_extract_limits(declared_total, count, archive_size)?;

            for index in 0..count {
                if is_cancelled(&flag) {
                    reset_cancel(&flag);
                    return Err("Cancelled by user".into());
                }
                let mut entry = match password {
                    Some(password) => zip
                        .by_index_decrypt(index, password.as_bytes())
                        .map_err(|error| format!("ZIP read failed (wrong password?): {error}"))?,
                    None => zip.by_index(index).map_err(|error| {
                        let message = error.to_string().to_ascii_lowercase();
                        if message.contains("password") || message.contains("encrypted") {
                            "This archive is password-protected".to_string()
                        } else {
                            format!("ZIP read failed: {error}")
                        }
                    })?,
                };

                let name = entry.name().to_string();
                emit_progress(&window, &name, index, count, total_bytes, declared_total);
                let safe = sanitize_path(&output_dir, &name)?;
                if entry.is_dir() {
                    std::fs::create_dir_all(&safe)
                        .map_err(|error| format!("Cannot create directory: {error}"))?;
                    continue;
                }
                if let Some(parent) = safe.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|error| format!("Cannot create directory: {error}"))?;
                }
                if safe.exists() && !options.overwrite {
                    skipped.push(name);
                    continue;
                }

                let mut output =
                    File::create(&safe).map_err(|error| format!("Create failed: {error}"))?;
                let max_bytes = MAX_EXTRACTED_BYTES.saturating_sub(total_bytes);
                let result = copy_with_cancel(
                    &mut entry,
                    &mut output,
                    || is_cancelled(&flag),
                    max_bytes,
                    |written| {
                        total_bytes += written;
                        emit_progress(&window, &name, index, count, total_bytes, declared_total);
                    },
                );
                if let Err(error) = result {
                    drop(output);
                    let _ = std::fs::remove_file(&safe);
                    reset_cancel(&flag);
                    return Err(error);
                }
                total_files += 1;
                emit_progress(
                    &window,
                    &name,
                    index + 1,
                    count,
                    total_bytes,
                    declared_total,
                );
            }
        }
        "7z" => {
            let password = password
                .map(sevenz_rust2::Password::from)
                .unwrap_or_else(sevenz_rust2::Password::empty);
            let mut seven_z =
                sevenz_rust2::SevenZReader::open(&archive, password).map_err(|error| {
                    let message = error.to_string();
                    if message.to_ascii_lowercase().contains("password") {
                        "Password required or incorrect".to_string()
                    } else {
                        format!("7z open failed: {message}")
                    }
                })?;
            let entries: Vec<_> = seven_z.archive().files.iter().cloned().collect();
            let count = entries.len();
            let declared_total = entries
                .iter()
                .filter(|entry| !entry.is_directory())
                .fold(0u64, |total, entry| total.saturating_add(entry.size()));
            check_extract_limits(declared_total, count, archive_size)?;

            let output_dir = output_dir.clone();
            let overwrite = options.overwrite;
            let flag_inner = flag.0.clone();
            let window_inner = window.clone();
            let skipped_entries = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let counters = std::sync::Arc::new(std::sync::Mutex::new((0usize, 0u64, 0usize)));
            let skipped_callback = skipped_entries.clone();
            let counters_callback = counters.clone();

            let result = seven_z.for_each_entries(|entry, reader| {
                if flag_inner.load(Ordering::Relaxed) {
                    return Err(sevenz_rust2::Error::other("Cancelled"));
                }

                let (index, bytes_done) = {
                    let mut counters = counters_callback
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    counters.2 += 1;
                    (counters.2 - 1, counters.1)
                };
                emit_progress(
                    &window_inner,
                    entry.name(),
                    index,
                    count,
                    bytes_done,
                    declared_total,
                );

                let safe = sanitize_path(&output_dir, entry.name())
                    .map_err(|error| sevenz_rust2::Error::other(format!("Bad path: {error}")))?;
                if entry.is_directory() {
                    std::fs::create_dir_all(&safe)
                        .map_err(|error| sevenz_rust2::Error::other(format!("mkdir: {error}")))?;
                    return Ok(true);
                }
                if let Some(parent) = safe.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|error| sevenz_rust2::Error::other(format!("mkdir: {error}")))?;
                }
                if safe.exists() && !overwrite {
                    skipped_callback
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(entry.name().to_string());
                    return Ok(true);
                }

                let mut output = File::create(&safe)
                    .map_err(|error| sevenz_rust2::Error::other(error.to_string()))?;
                let extraction = (|| -> Result<(), sevenz_rust2::Error> {
                    let mut buffer = vec![0u8; ARCHIVE_BUFFER_SIZE];
                    loop {
                        if flag_inner.load(Ordering::Relaxed) {
                            return Err(sevenz_rust2::Error::other("Cancelled"));
                        }
                        let read = reader.read(&mut buffer).map_err(|error| {
                            sevenz_rust2::Error::other(format!("read: {error}"))
                        })?;
                        if read == 0 {
                            return Ok(());
                        }
                        let bytes_done = {
                            let mut counters = counters_callback
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            let next = counters.1.checked_add(read as u64).ok_or_else(|| {
                                sevenz_rust2::Error::other("Archive entry is too large")
                            })?;
                            if next > MAX_EXTRACTED_BYTES {
                                return Err(sevenz_rust2::Error::other(
                                    "Extraction exceeds the configured safety limit",
                                ));
                            }
                            counters.1 = next;
                            next
                        };
                        output.write_all(&buffer[..read]).map_err(|error| {
                            sevenz_rust2::Error::other(format!("write: {error}"))
                        })?;
                        emit_progress(
                            &window_inner,
                            entry.name(),
                            index,
                            count,
                            bytes_done,
                            declared_total,
                        );
                    }
                })();
                if let Err(error) = extraction {
                    drop(output);
                    let _ = std::fs::remove_file(&safe);
                    return Err(error);
                }
                counters_callback
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .0 += 1;
                Ok(true)
            });

            if is_cancelled(&flag) {
                reset_cancel(&flag);
                return Err("Cancelled by user".into());
            }
            result.map_err(|error| format!("7z extract failed: {error}"))?;

            let counters = counters
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            total_files = counters.0;
            total_bytes = counters.1;
            skipped = std::sync::Arc::try_unwrap(skipped_entries)
                .map(|entries| {
                    entries
                        .into_inner()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                })
                .unwrap_or_default();
        }
        "tar.gz" | "tar.xz" | "tar.zst" => {
            let reader = open_tar_reader(&archive, &format)?;
            let mut tar_archive = tar::Archive::new(reader);
            let mut index = 0usize;

            for entry in tar_archive
                .entries()
                .map_err(|error| format!("TAR list failed: {error}"))?
            {
                if is_cancelled(&flag) {
                    reset_cancel(&flag);
                    return Err("Cancelled by user".into());
                }
                check_entry_count(index + 1)?;

                let mut entry = entry.map_err(|error| format!("TAR entry failed: {error}"))?;
                let path = entry
                    .path()
                    .map_err(|error| format!("TAR path failed: {error}"))?
                    .to_path_buf();
                let name = path.to_string_lossy().to_string();
                emit_progress(&window, &name, index, 0, total_bytes, 0);
                let safe = sanitize_path(&output_dir, &name)?;
                let entry_type = entry.header().entry_type();

                if entry_type.is_dir() {
                    std::fs::create_dir_all(&safe)
                        .map_err(|error| format!("Cannot create directory: {error}"))?;
                    index += 1;
                    continue;
                }
                if !is_tar_regular_file(entry_type) {
                    skipped.push(format!("{name} (unsupported TAR entry type)"));
                    index += 1;
                    continue;
                }

                let size = entry.header().size().unwrap_or(0);
                check_extract_limits(total_bytes.saturating_add(size), index + 1, archive_size)?;
                if let Some(parent) = safe.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|error| format!("Cannot create directory: {error}"))?;
                }
                if safe.exists() && !options.overwrite {
                    skipped.push(name);
                    index += 1;
                    continue;
                }

                let mut output =
                    File::create(&safe).map_err(|error| format!("Create failed: {error}"))?;
                let max_bytes = MAX_EXTRACTED_BYTES.saturating_sub(total_bytes);
                let result = copy_with_cancel(
                    &mut entry,
                    &mut output,
                    || is_cancelled(&flag),
                    max_bytes,
                    |written| {
                        total_bytes += written;
                        emit_progress(&window, &name, index, 0, total_bytes, 0);
                    },
                );
                if let Err(error) = result {
                    drop(output);
                    let _ = std::fs::remove_file(&safe);
                    reset_cancel(&flag);
                    return Err(error);
                }
                total_files += 1;
                index += 1;
            }
        }
        _ => return Err(format!("Unsupported archive format: {format}")),
    }

    reset_cancel(&flag);
    Ok(ExtractResult {
        output_dir: output_dir.to_string_lossy().to_string(),
        format,
        total_files,
        total_bytes,
        elapsed_ms: start.elapsed().as_millis() as u64,
        skipped,
    })
}

fn sanitize_path(base: &Path, entry_name: &str) -> Result<PathBuf, String> {
    let raw = entry_name.replace('\\', "/");
    if raw.is_empty() || raw.contains('\0') {
        return Err("Refusing empty or invalid archive entry path".into());
    }
    if raw.starts_with('/') {
        return Err(format!("Refusing absolute archive path: {entry_name}"));
    }

    let trimmed = raw.trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Refusing empty or invalid archive entry path".into());
    }

    let mut clean = PathBuf::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            Component::ParentDir => return Err(format!("Refusing unsafe path: {entry_name}")),
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Refusing absolute archive path: {entry_name}"));
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(format!("Refusing invalid archive path: {entry_name}"));
    }

    let canonical_base = base
        .canonicalize()
        .map_err(|error| format!("Cannot access output directory: {error}"))?;
    let candidate = canonical_base.join(clean);
    if !candidate.starts_with(&canonical_base) {
        return Err(format!("Path escapes output directory: {entry_name}"));
    }

    let mut existing_ancestor = candidate.parent();
    while let Some(ancestor) = existing_ancestor {
        if ancestor.exists() {
            let canonical_ancestor = ancestor
                .canonicalize()
                .map_err(|error| format!("Cannot access output directory: {error}"))?;
            if !canonical_ancestor.starts_with(&canonical_base) {
                return Err(format!("Path escapes output directory: {entry_name}"));
            }
            break;
        }
        existing_ancestor = ancestor.parent();
    }
    if let Ok(metadata) = candidate.symlink_metadata() {
        if metadata.file_type().is_symlink() {
            let target = candidate
                .canonicalize()
                .map_err(|_| format!("Refusing unresolved archive link: {entry_name}"))?;
            if !target.starts_with(&canonical_base) {
                return Err(format!("Path escapes output directory: {entry_name}"));
            }
        }
    }

    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "keepitlocal-archive-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create temp directory");
        path
    }

    #[test]
    fn archive_work_runs_on_tauris_blocking_executor() {
        let command_thread = std::thread::current().id();
        let worker_thread =
            tauri::async_runtime::block_on(run_archive_work(|| Ok(std::thread::current().id())))
                .expect("run archive worker");

        assert_ne!(worker_thread, command_thread);
    }

    #[test]
    fn cancel_reader_interrupts_reads_after_cancellation() {
        let directory = test_dir("cancel-reader");
        let source = directory.join("source.txt");
        std::fs::write(&source, b"data").expect("write source");
        let flag = CancelFlag(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )));
        let mut reader = CancelReader {
            file: File::open(&source).expect("open source"),
            flag: &flag,
        };

        assert_eq!(reader.read(&mut [0; 1]).expect("read source"), 1);
        flag.0.store(true, Ordering::Relaxed);
        let error = reader
            .read(&mut [0; 1])
            .expect_err("cancelled read must interrupt");
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
        std::fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn recognizes_supported_archive_extensions() {
        assert_eq!(format_from_filename("backup.ZIP"), Some("zip"));
        assert_eq!(format_from_filename("backup.7z"), Some("7z"));
        assert_eq!(format_from_filename("backup.tgz"), Some("tar.gz"));
        assert_eq!(format_from_filename("backup.txz"), Some("tar.xz"));
        assert_eq!(format_from_filename("backup.tar.zst"), Some("tar.zst"));
        assert_eq!(format_from_filename("backup.rar"), None);
    }

    #[test]
    fn detects_tar_zst_magic_without_an_extension() {
        let directory = test_dir("zstd-magic");
        let archive = directory.join("payload.bin");
        std::fs::write(&archive, [0x28, 0xB5, 0x2F, 0xFD, 0, 0, 0, 0]).expect("write header");
        assert_eq!(detect_format(&archive).as_deref(), Ok("tar.zst"));
        std::fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn rejects_archive_paths_that_escape_the_output_directory() {
        let directory = test_dir("path");
        assert!(sanitize_path(&directory, "../escape.txt").is_err());
        assert!(sanitize_path(&directory, "/escape.txt").is_err());
        assert!(sanitize_path(&directory, r"C:\escape.txt").is_err());
        assert_eq!(
            sanitize_path(&directory, "nested/file.txt").expect("safe entry"),
            directory
                .canonicalize()
                .expect("canonical temp directory")
                .join("nested")
                .join("file.txt")
        );
        std::fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn rejects_archive_output_inside_an_input_directory() {
        let directory = test_dir("output");
        let input = directory.join("source");
        std::fs::create_dir_all(&input).expect("create input directory");
        let output = input.join("archive.zip");
        assert!(ensure_output_not_inside_inputs(&output, &[input]).is_err());
        std::fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn normalizes_and_validates_the_archive_output_extension() {
        let directory = test_dir("extension");
        let no_extension = directory.join("backup");
        assert_eq!(
            normalized_archive_output_path(&no_extension, "tar.zst").expect("append suffix"),
            directory.join("backup.tar.zst")
        );
        assert!(normalized_archive_output_path(&directory.join("backup.zip"), "7z").is_err());
        std::fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn promotes_a_staged_archive_without_overwriting_an_existing_archive() {
        let directory = test_dir("promotion");
        let requested = directory.join("backup.tar.zst");
        let staging = directory.join(".backup.keepitlocal-archive-test.tar.zst");
        std::fs::write(&requested, b"existing archive").expect("write existing archive");
        std::fs::write(&staging, b"new archive").expect("write staged archive");

        let output = promote_staged_archive(&staging, &requested).expect("promote archive output");
        assert_eq!(
            output.file_name().and_then(|name| name.to_str()),
            Some("backup_1.tar.zst")
        );
        assert_eq!(std::fs::read(&requested).expect("read original"), b"existing archive");
        assert_eq!(std::fs::read(&output).expect("read promoted archive"), b"new archive");
        assert!(!staging.exists());
        std::fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn tar_preview_rejects_a_declared_archive_bomb_before_reading_contents() {
        let directory = test_dir("tar-preview-limit");
        let archive_path = directory.join("large.tar.gz");
        let mut header = tar::Header::new_gnu();
        header.set_path("large.bin").expect("set tar path");
        header.set_size(MAX_EXTRACTED_BYTES + 1);
        header.set_mode(0o644);
        header.set_cksum();

        let file = File::create(&archive_path).expect("create archive");
        let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        encoder.write_all(header.as_bytes()).expect("write tar header");
        encoder.finish().expect("finish gzip archive");

        let error = match inspect_archive(InspectArchiveOptions {
            path: archive_path.to_string_lossy().to_string(),
            password: None,
        }) {
            Err(error) => error,
            Ok(_) => panic!("preview must reject oversized tar"),
        };
        assert!(error.contains("Extraction would create more"));
        std::fs::remove_dir_all(directory).expect("remove temp directory");
    }

    #[test]
    fn inspects_a_password_protected_7z_when_given_the_password() {
        let directory = test_dir("encrypted-7z");
        let source = directory.join("source.txt");
        let archive_path = directory.join("protected.7z");
        std::fs::write(&source, b"private text").expect("write source");

        let mut writer = sevenz_rust2::SevenZWriter::create(&archive_path).expect("create 7z");
        writer.set_content_methods(vec![
            sevenz_rust2::AesEncoderOptions::new(sevenz_rust2::Password::from("secret"))
                .into(),
            sevenz_rust2::lzma::LZMA2Options::with_preset(6).into(),
        ]);
        writer.set_encrypt_header(true);
        let entry = sevenz_rust2::SevenZArchiveEntry::from_path(&source, "source.txt".into());
        writer
            .push_archive_entry(entry, Some(File::open(&source).expect("open source")))
            .expect("add source");
        writer.finish().expect("finish 7z");

        let info = inspect_archive(InspectArchiveOptions {
            path: archive_path.to_string_lossy().to_string(),
            password: Some("secret".into()),
        })
        .expect("inspect encrypted 7z");
        assert_eq!(info.entries.len(), 1);
        std::fs::remove_dir_all(directory).expect("remove temp directory");
    }
}
