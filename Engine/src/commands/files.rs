use super::local_db;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use walkdir::WalkDir;

const HASH_BUFFER_SIZE: usize = 1024 * 1024;
/// Quality Pass Wave 1 user feedback (2026-05-29): live progress event
/// name. Frontend listens via Tauri's `listen` API and updates the
/// scan-progress card with real counters instead of just a spinner.
/// Emitted from the walker (every ~500 entries) and the hash phase
/// (every ~50 hashes) so the user can tell the scan is alive even on
/// a multi-minute walk over a huge library.
pub const DUPLICATE_SCAN_PROGRESS_EVENT: &str = "duplicate-scan-progress";
const PARTIAL_HASH_CHUNK_SIZE: usize = 64 * 1024;
const MAX_COLLECTED_PATHS: usize = 250_000;
const DEFAULT_DUPLICATE_HASH_THREADS: usize = 4;
const FILE_OPERATIONS_DIR: &str = "file-operations";
const FILE_RECOVERY_FILE: &str = "recovery.json";
const FILE_RECOVERY_VERSION: u32 = 1;
const MAX_FILE_TRANSACTIONS: usize = 20;
static DUPLICATE_SCAN_CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize, Clone)]
struct FileRecoveryEntry {
    original_path: String,
    current_path: String,
    is_dir: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct FileRecoveryTransaction {
    id: String,
    kind: String,
    created_at: u128,
    entry_count: usize,
    description: String,
    entries: Vec<FileRecoveryEntry>,
}

#[derive(Serialize, Deserialize, Clone)]
struct FileRecoveryLog {
    version: u32,
    transactions: Vec<FileRecoveryTransaction>,
}

#[derive(Serialize, Clone)]
pub struct RecoveryTransactionSummary {
    pub id: String,
    pub kind: String,
    pub created_at: u128,
    pub entry_count: usize,
    pub description: String,
}

#[derive(Serialize, Clone)]
pub struct FileRecoveryState {
    pub bulk_rename: Option<RecoveryTransactionSummary>,
    pub duplicate_move: Option<RecoveryTransactionSummary>,
}

#[derive(Serialize, Clone)]
pub struct MoveDuplicatesResponse {
    pub results: Vec<MoveFileResult>,
    pub recovery: Option<RecoveryTransactionSummary>,
}

#[derive(Serialize, Clone)]
pub struct RenameApplyResponse {
    pub results: Vec<RenameApplyResult>,
    pub recovery: Option<RecoveryTransactionSummary>,
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn duplicate_scan_cancelled() -> bool {
    DUPLICATE_SCAN_CANCELLED.load(Ordering::Relaxed)
}

fn duplicate_cancel_error() -> String {
    "Duplicate scan cancelled".to_string()
}

fn duplicate_thread_count(max_threads: Option<usize>) -> usize {
    let available = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(DEFAULT_DUPLICATE_HASH_THREADS)
        .max(1);

    match max_threads {
        // Fast mode: use most cores, but leave one logical core free when possible.
        Some(0) => available.saturating_sub(1).max(1),
        Some(value) => value.clamp(1, available),
        None => DEFAULT_DUPLICATE_HASH_THREADS.min(available).max(1),
    }
}

fn duplicate_cancelled_result(scanned_files: usize, hashed_files: usize) -> DuplicateScanResult {
    DuplicateScanResult {
        success: false,
        canceled: true,
        error: Some(duplicate_cancel_error()),
        scanned_files,
        hashed_files,
        duplicate_groups: vec![],
        duplicate_files: 0,
        reclaimable_bytes: 0,
    }
}

// ============================================================
// Shared helpers
// ============================================================

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_string()
}

fn extension_lower(path: &Path) -> String {
    path.extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_lowercase()
}

fn is_hidden_path(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(value) => value.to_string_lossy().starts_with('.'),
        _ => false,
    })
}

fn format_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn path_dedupe_key(path: &Path) -> String {
    let formatted = fs::canonicalize(path)
        .map(|canonical| format_path(&canonical))
        .unwrap_or_else(|_| format_path(path));

    if cfg!(windows) {
        formatted.to_lowercase()
    } else {
        formatted
    }
}

pub(crate) fn describe_io_error(action: &str, path: &Path, error: &std::io::Error) -> String {
    match error.raw_os_error() {
        Some(5) | Some(32) => format!(
            "{action} blocked for {}. The file may be open in another app or you may not have permission.",
            format_path(path)
        ),
        Some(206) => format!("Path is too long for some Windows operations: {}", format_path(path)),
        Some(3) => format!("Path was not found: {}", format_path(path)),
        _ => format!("{action} failed for {}: {error}", format_path(path)),
    }
}

fn path_too_long(path: &Path) -> bool {
    format_path(path).chars().count() > 240
}

fn normalize_for_guard(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_lowercase()
        .trim_end_matches('/')
        .to_string()
}

fn is_windows_drive_root(normalized: &str) -> bool {
    let bytes = normalized.as_bytes();
    bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

fn is_safe_user_location(normalized: &str) -> bool {
    // Explicitly allow normal user locations. These are common places users expect
    // to scan: Desktop, Downloads, Documents, Pictures, project folders, etc.
    // We only use this to avoid over-blocking; root/system locations are still blocked.
    normalized.starts_with("c:/users/")
        || normalized.starts_with("d:/users/")
        || normalized.starts_with("e:/users/")
        || normalized.starts_with("/users/")
        || normalized.starts_with("/home/")
}

pub(crate) fn is_dangerous_scan_root(path: &Path) -> bool {
    let normalized = normalize_for_guard(path);

    if normalized.is_empty() || normalized == "/" || is_windows_drive_root(&normalized) {
        return true;
    }

    if is_safe_user_location(&normalized) {
        return false;
    }

    let blocked_system_locations = [
        "c:/windows",
        "c:/program files",
        "c:/program files (x86)",
        "c:/programdata",
        "c:/system volume information",
        "/system",
        "/library",
        "/applications",
        "/bin",
        "/sbin",
        "/usr",
        "/var",
        "/etc",
        "/dev",
        "/proc",
        "/sys",
        "/run",
        "/boot",
    ];

    blocked_system_locations
        .iter()
        .any(|blocked| normalized == *blocked || normalized.starts_with(&format!("{blocked}/")))
}

fn validate_scan_roots(roots: &[String]) -> Result<(), String> {
    if roots.is_empty() {
        return Err("Choose at least one folder or file".into());
    }

    for root in roots {
        let path = PathBuf::from(root);
        if is_dangerous_scan_root(&path) {
            return Err(format!(
                "Refusing to scan whole-drive/system location: {root}. Choose a normal user folder instead, such as Desktop, Downloads, Documents, Pictures, or a project folder."
            ));
        }
    }

    Ok(())
}

fn unique_destination_path(dest_dir: &Path, source: &Path) -> PathBuf {
    let name = source
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("file");
    let candidate = dest_dir.join(name);
    if !candidate.exists() {
        return candidate;
    }

    let stem = source
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("file");
    let ext = source.extension().and_then(|v| v.to_str()).unwrap_or("");

    for index in 1..10_000 {
        let name = if ext.is_empty() {
            format!("{stem}_{index}")
        } else {
            format!("{stem}_{index}.{ext}")
        };
        let candidate = dest_dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_millis())
        .unwrap_or(0);

    if ext.is_empty() {
        dest_dir.join(format!("{stem}_{millis}"))
    } else {
        dest_dir.join(format!("{stem}_{millis}.{ext}"))
    }
}

/// Quality Pass Wave 1 / Option C (2026-05-28): cache-aware wrapper
/// around `hash_stable_file`. Looks up `(path_lowercase, mtime, algo)`
/// in the persistent hash cache and returns the cached hex hash on
/// hit; on miss, computes the hash and stores it for the next scan.
/// `cache_ctx = None` falls through to a plain hash_stable_file call
/// (matches the pre-Option-C behavior).
fn compute_or_lookup_hash(
    path: &Path,
    size: u64,
    algorithm: &str,
    cache_ctx: Option<&super::duplicate_cache::DuplicateCacheCtx>,
) -> Option<String> {
    let mtime_ms = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let path_lower = path.to_string_lossy().to_lowercase().into_bytes();

    if let Some(ctx) = cache_ctx {
        if let Some(hash_bytes) =
            super::duplicate_cache::lookup(ctx, &path_lower, mtime_ms, algorithm)
        {
            if let Ok(hash_str) = String::from_utf8(hash_bytes) {
                return Some(hash_str);
            }
        }
    }

    let hash_str = hash_stable_file(path, size, algorithm).ok()?;
    if let Some(ctx) = cache_ctx {
        super::duplicate_cache::store(
            ctx,
            &path_lower,
            mtime_ms,
            algorithm,
            hash_str.as_bytes(),
        );
    }
    Some(hash_str)
}

fn hash_file(path: &Path, algorithm: &str) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
    let mut buffer = vec![0_u8; HASH_BUFFER_SIZE];

    match algorithm {
        "sha256" => {
            let mut hasher = Sha256::new();
            loop {
                let read = file
                    .read(&mut buffer)
                    .map_err(|e| format!("Cannot read file: {e}"))?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok(bytes_to_hex(&hasher.finalize()))
        }
        _ => {
            let mut hasher = blake3::Hasher::new();
            loop {
                let read = file
                    .read(&mut buffer)
                    .map_err(|e| format!("Cannot read file: {e}"))?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok(hasher.finalize().to_hex().to_string())
        }
    }
}

fn hash_stable_file(path: &Path, expected_size: u64, algorithm: &str) -> Result<String, String> {
    let before =
        fs::metadata(path).map_err(|e| format!("Cannot inspect file before hashing: {e}"))?;
    if before.len() != expected_size {
        return Err("File size changed before hashing".into());
    }
    let before_modified = before.modified().ok();

    let hash = hash_file(path, algorithm)?;

    let after =
        fs::metadata(path).map_err(|e| format!("Cannot inspect file after hashing: {e}"))?;
    if after.len() != expected_size || after.modified().ok() != before_modified {
        return Err("File changed while KeepItLocal was hashing it".into());
    }

    Ok(hash)
}

fn read_chunk_at(path: &Path, offset: u64, max_len: usize) -> Result<Vec<u8>, String> {
    use std::io::{Seek, SeekFrom};

    let mut file = fs::File::open(path).map_err(|e| format!("Cannot open file: {e}"))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("Cannot seek file: {e}"))?;

    let mut buffer = vec![0_u8; max_len];
    let read = file
        .read(&mut buffer)
        .map_err(|e| format!("Cannot read file: {e}"))?;
    buffer.truncate(read);
    Ok(buffer)
}

fn partial_fingerprint(path: &Path, size: u64) -> Result<String, String> {
    let chunk = PARTIAL_HASH_CHUNK_SIZE.min(size as usize);
    let mut hasher = blake3::Hasher::new();

    hasher.update(&size.to_le_bytes());

    let first = read_chunk_at(path, 0, chunk)?;
    hasher.update(&(first.len() as u64).to_le_bytes());
    hasher.update(&first);

    if size > PARTIAL_HASH_CHUNK_SIZE as u64 {
        let middle_offset = size.saturating_sub(chunk as u64) / 2;
        let middle = read_chunk_at(path, middle_offset, chunk)?;
        hasher.update(&(middle_offset).to_le_bytes());
        hasher.update(&(middle.len() as u64).to_le_bytes());
        hasher.update(&middle);
    }

    if size > (PARTIAL_HASH_CHUNK_SIZE * 2) as u64 {
        let last_offset = size.saturating_sub(chunk as u64);
        let last = read_chunk_at(path, last_offset, chunk)?;
        hasher.update(&(last_offset).to_le_bytes());
        hasher.update(&(last.len() as u64).to_le_bytes());
        hasher.update(&last);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// Quality Pass Wave 1 user feedback (2026-05-28): the previous hard
/// 250 000-path cap inside `collect_paths` made DuplicateFinder broken
/// for its primary use case (find duplicates across a whole Pictures
/// library, which often holds 500 k+ photos). The cap is now an
/// optional parameter — DuplicateFinder passes `None` so it can walk
/// arbitrarily large trees; BulkRename still passes `Some(MAX_COLLECTED_PATHS)`
/// because typical rename workloads don't justify 1 GB of path strings.
fn collect_paths(
    roots: &[String],
    recursive: bool,
    include_files: bool,
    include_dirs: bool,
    include_hidden: bool,
    follow_symlinks: bool,
    cancel_flag: Option<&AtomicBool>,
    max_paths: Option<usize>,
) -> Result<Vec<PathBuf>, String> {
    validate_scan_roots(roots)?;

    let is_cancelled =
        |flag: Option<&AtomicBool>| flag.map(|f| f.load(Ordering::Relaxed)).unwrap_or(false);

    if is_cancelled(cancel_flag) {
        return Err(duplicate_cancel_error());
    }

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for root in roots {
        if is_cancelled(cancel_flag) {
            return Err(duplicate_cancel_error());
        }

        let root_path = PathBuf::from(root);
        if !root_path.exists() {
            continue;
        }

        if root_path.is_file() {
            if include_files && (include_hidden || !is_hidden_path(&root_path)) {
                let key = path_dedupe_key(&root_path);
                if seen.insert(key) {
                    out.push(root_path);
                }
            }
            continue;
        }

        if root_path.is_dir() {
            if !recursive {
                if include_dirs && (include_hidden || !is_hidden_path(&root_path)) {
                    let key = path_dedupe_key(&root_path);
                    if seen.insert(key) {
                        out.push(root_path.clone());
                    }
                }

                if let Ok(read_dir) = fs::read_dir(&root_path) {
                    for entry in read_dir.flatten() {
                        if is_cancelled(cancel_flag) {
                            return Err(duplicate_cancel_error());
                        }

                        let path = entry.path();
                        if !include_hidden && is_hidden_path(&path) {
                            continue;
                        }
                        if path.is_file() && include_files {
                            let key = path_dedupe_key(&path);
                            if seen.insert(key) {
                                out.push(path);
                            }
                        } else if path.is_dir() && include_dirs {
                            let key = path_dedupe_key(&path);
                            if seen.insert(key) {
                                out.push(path);
                            }
                        }
                    }
                }
                continue;
            }

            let mut visited = 0_usize;
            for entry in WalkDir::new(&root_path)
                .follow_links(follow_symlinks)
                .into_iter()
                .filter_entry(|e| include_hidden || !is_hidden_path(e.path()))
                .flatten()
            {
                visited += 1;
                if visited.is_multiple_of(512) && is_cancelled(cancel_flag) {
                    return Err(duplicate_cancel_error());
                }

                let path = entry.path().to_path_buf();
                if path == root_path && !include_dirs {
                    continue;
                }
                if entry.file_type().is_file() && include_files {
                    let key = path_dedupe_key(&path);
                    if seen.insert(key) {
                        out.push(path);
                    }
                } else if entry.file_type().is_dir() && include_dirs {
                    let key = path_dedupe_key(&path);
                    if seen.insert(key) {
                        out.push(path);
                    }
                }
            }
        }
    }

    if let Some(cap) = max_paths {
        if out.len() > cap {
            return Err(format!(
                "Scan collected more than {cap} items. Choose a smaller folder or disable recursive mode."
            ));
        }
    }

    out.sort_by(|a, b| format_path(a).cmp(&format_path(b)));
    Ok(out)
}

// ============================================================
// Duplicate File Finder
// ============================================================

#[derive(Deserialize)]
pub struct DuplicateScanOptions {
    pub roots: Vec<String>,
    pub recursive: bool,
    pub include_hidden: bool,
    pub follow_symlinks: bool,
    pub min_size: u64,
    pub hash_algorithm: String,     // "blake3" | "sha256"
    pub max_threads: Option<usize>, // None = balanced default, 0 = fast/all cores minus one
    /// Quality Pass Wave 1 / DF-3 (2026-05-28): comma- or whitespace-
    /// separated list of file extensions (without the dot) to restrict
    /// the scan to. Empty = scan every file kind (the original
    /// behavior). Matching is case-insensitive; the dot is stripped.
    /// Example: "jpg,png,pdf" or "jpg png pdf".
    #[serde(default)]
    pub extension_filter: String,
    /// Quality Pass Wave 1 / DF-1 (2026-05-29): scan mode selector.
    /// `"exact"` (default) keeps the byte-identical pipeline that's
    /// been here since v0 — BLAKE3/SHA-256 of the full file bytes.
    /// `"perceptual"` switches to dHash-on-images: catches resized,
    /// re-encoded, lightly cropped, slightly recolored visual
    /// duplicates that byte-hashing would miss. Anything else maps
    /// to `"exact"` so older frontends + bad JSON payloads can't
    /// accidentally lose data.
    #[serde(default = "default_hash_strategy")]
    pub hash_strategy: String,
    /// Quality Pass Wave 1 / DF-1 (2026-05-29): only consulted when
    /// `hash_strategy == "perceptual"`. Hamming-distance threshold —
    /// two perceptual fingerprints whose bits differ by ≤ this count
    /// are grouped as "similar". Lower = stricter (closer to
    /// byte-identical), higher = looser (more candidate matches +
    /// more false positives). Sensible range: 0–18 out of 64 bits.
    /// Default 10 covers typical "JPEG-re-encoded / lightly resized"
    /// without flooding the UI.
    #[serde(default = "default_similarity_threshold")]
    pub similarity_threshold: u32,
}

fn default_hash_strategy() -> String {
    "exact".to_string()
}

fn default_similarity_threshold() -> u32 {
    super::perceptual_hash::DEFAULT_SIMILARITY_THRESHOLD
}

#[derive(Serialize, Clone)]
pub struct DuplicateFileEntry {
    pub path: String,
    pub file_name: String,
    pub size: u64,
    pub modified_ms: Option<u128>,
    pub extension: String,
}

#[derive(Serialize, Clone)]
pub struct DuplicateGroup {
    pub hash: String,
    pub size: u64,
    pub count: usize,
    pub wasted_bytes: u64,
    pub files: Vec<DuplicateFileEntry>,
}

#[derive(Serialize, Clone)]
pub struct DuplicateScanResult {
    pub success: bool,
    pub canceled: bool,
    pub error: Option<String>,
    pub scanned_files: usize,
    pub hashed_files: usize,
    pub duplicate_groups: Vec<DuplicateGroup>,
    pub duplicate_files: usize,
    pub reclaimable_bytes: u64,
}

#[derive(Deserialize)]
pub struct MoveDuplicatesOptions {
    pub paths: Vec<String>,
    pub destination_dir: String,
}

#[derive(Serialize, Clone)]
pub struct MoveFileResult {
    pub source_path: String,
    pub output_path: String,
    pub success: bool,
    pub error: Option<String>,
}

fn move_file_preserving_single_copy(source: &Path, target: &Path) -> Result<(), String> {
    match fs::rename(source, target) {
        Ok(_) => Ok(()),
        Err(rename_err) => match fs::copy(source, target) {
            Ok(_) => match fs::remove_file(source) {
                Ok(_) => Ok(()),
                Err(remove_err) => {
                    let _ = fs::remove_file(target);
                    Err(format!(
                        "{} Copy fallback succeeded but removing the original failed, so the copied file was removed to avoid leaving duplicates behind.",
                        describe_io_error("Move", source, &remove_err)
                    ))
                }
            },
            Err(copy_err) => Err(format!(
                "{} Copy fallback also failed for {}: {copy_err}",
                describe_io_error("Move", source, &rename_err),
                format_path(target)
            )),
        },
    }
}

#[tauri::command]
pub fn cancel_duplicate_scan() {
    DUPLICATE_SCAN_CANCELLED.store(true, Ordering::Relaxed);
}

#[tauri::command]
pub async fn find_duplicate_files(
    app: AppHandle,
    options: DuplicateScanOptions,
) -> DuplicateScanResult {
    // Security gate: every scan root must resolve to a real, non-system
    // path before we walk it. Rejects `..` traversal and Windows /
    // Program Files / Startup folders. Returns the standard error shape
    // on rejection so the frontend can render it like any other failure.
    for root in &options.roots {
        if let Err(e) = crate::core::safe_path::validate_user_path(root) {
            return DuplicateScanResult {
                success: false,
                canceled: false,
                error: Some(e),
                scanned_files: 0,
                hashed_files: 0,
                duplicate_groups: vec![],
                duplicate_files: 0,
                reclaimable_bytes: 0,
            };
        }
    }

    DUPLICATE_SCAN_CANCELLED.store(false, Ordering::Relaxed);

    // Quality Pass Wave 1 / Option B (2026-05-28): if the user has a
    // filename index built, derive its state dir so the blocking worker
    // can preload the (path, size, mtime) table and skip the metadata
    // stat for indexed files. Failing here is non-fatal — the worker
    // falls back to walker-only (Option A).
    let indexed_state_dir = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("file-search-index"));

    // Quality Pass Wave 1 user feedback (2026-05-29): clone the app
    // handle into the worker so it can emit live progress events. The
    // walk + hash phases each call `app.emit(DUPLICATE_SCAN_PROGRESS_EVENT, ..)`
    // periodically so the frontend shows ticking counters instead of a
    // dead green spinner on multi-minute scans.
    let app_for_progress = app.clone();

    tauri::async_runtime::spawn_blocking(move || {
        find_duplicate_files_blocking(options, indexed_state_dir, Some(app_for_progress))
    })
        .await
        .unwrap_or_else(|e| DuplicateScanResult {
            success: false,
            canceled: false,
            error: Some(format!("Duplicate scan worker failed: {e}")),
            scanned_files: 0,
            hashed_files: 0,
            duplicate_groups: vec![],
            duplicate_files: 0,
            reclaimable_bytes: 0,
        })
}

/// Quality Pass Wave 1 / DF-1 (2026-05-29): perceptual image scan
/// pipeline. Branches off `find_duplicate_files_blocking` once the
/// parallel walker has finished collecting candidate paths.
///
/// Steps:
///   1. Flatten the size-bucketed walker output, filter to image-eligible
///      extensions (png/jpg/webp/tif/bmp/gif).
///   2. Look up each path in the persistent perceptual cache (path +
///      mtime). On miss, decode + dHash the image and store the fresh
///      fingerprint.
///   3. Group by Hamming distance ≤ `threshold` via Union-Find — pairs
///      and triples and chains all coalesce into proper transitive
///      groups.
///   4. Convert each group into a DuplicateGroup shape the existing
///      frontend already renders. `hash` becomes the 16-hex
///      representative fingerprint; `wasted_bytes` = sum of all
///      members minus the largest (the "keep the highest-resolution
///      copy" heuristic).
///
/// All errors are silently skipped per-file — a corrupt JPEG or an
/// unreadable WEBP doesn't fail the scan; we just drop that file.
fn run_perceptual_image_scan(
    by_size: HashMap<u64, Vec<PathBuf>>,
    scanned_files: usize,
    similarity_threshold: u32,
    max_threads: Option<usize>,
    indexed_state_dir: Option<&Path>,
    progress_app: Option<&AppHandle>,
) -> DuplicateScanResult {
    use super::perceptual_cache::{
        ALGO_DHASH_8X8_DCT, DEFAULT_MAX_BYTES, DEFAULT_TTL_DAYS, PerceptualCacheCtx,
    };
    use super::perceptual_hash::{SimilarImageGroup, group_similar_images, is_image_eligible};

    // Reuse the same "emit a small JSON payload on the same event" helper
    // shape that the byte-identical pipeline uses, so the frontend's
    // progress listener doesn't have to learn a second event name.
    fn emit_progress(
        app: Option<&AppHandle>,
        scanned: usize,
        hashed: usize,
        phase: &str,
    ) {
        if let Some(app) = app {
            let _ = app.emit(
                DUPLICATE_SCAN_PROGRESS_EVENT,
                serde_json::json!({
                    "scanned": scanned,
                    "hashed": hashed,
                    "phase": phase,
                }),
            );
        }
    }

    if duplicate_scan_cancelled() {
        return duplicate_cancelled_result(scanned_files, 0);
    }

    // Flatten the size buckets into (path, size) and filter to images.
    // Unlike the byte-identical path we DO NOT require size > 1 grouping
    // here — perceptual hashing finds matches across totally different
    // file sizes (a 4 K original vs its 800-px Instagram crop).
    let image_jobs: Vec<(PathBuf, u64)> = by_size
        .into_iter()
        .flat_map(|(size, paths)| paths.into_iter().map(move |p| (p, size)))
        .filter(|(path, _)| is_image_eligible(path))
        .collect();

    if image_jobs.is_empty() {
        return DuplicateScanResult {
            success: true,
            canceled: false,
            error: None,
            scanned_files,
            hashed_files: 0,
            duplicate_groups: vec![],
            duplicate_files: 0,
            reclaimable_bytes: 0,
        };
    }

    // Persistent (path, mtime) → phash cache. Mirrors the exact-byte
    // pipeline's `cache_ctx` plumbing: `None` when no state dir is
    // available (build-time, brand-new install), in which case the
    // scan still works — it just hashes every image from scratch.
    let cache_ctx = indexed_state_dir.map(PerceptualCacheCtx::new);

    let thread_count = duplicate_thread_count(max_threads);
    let hashed_counter = Arc::new(AtomicUsize::new(0));

    // Hash every image (in parallel) producing (path_string, size, phash).
    let hash_records: Vec<(String, u64, super::perceptual_hash::PerceptualHash)> =
        match ThreadPoolBuilder::new().num_threads(thread_count).build() {
            Ok(pool) => pool.install(|| hash_image_jobs_parallel(
                &image_jobs,
                cache_ctx.as_ref(),
                progress_app,
                &hashed_counter,
                scanned_files,
                emit_progress,
            )),
            Err(_) => hash_image_jobs_parallel(
                &image_jobs,
                cache_ctx.as_ref(),
                progress_app,
                &hashed_counter,
                scanned_files,
                emit_progress,
            ),
        };

    if duplicate_scan_cancelled() {
        return duplicate_cancelled_result(scanned_files, hash_records.len());
    }

    let hashed_files = hash_records.len();

    // Union-Find grouping. O(N²) Hamming comparisons in v1 — fine up
    // to ~50 K images per scan; beyond that we'll need a BK-tree (see
    // the perceptual_hash.rs module note).
    let groups: Vec<SimilarImageGroup> = group_similar_images(&hash_records, similarity_threshold);

    // Convert into the DuplicateGroup-shape the frontend already
    // renders. wasted_bytes = total bytes - max bytes in group (the
    // "keep the largest copy" reclaim estimate; the frontend will
    // re-sort + re-key for "kept" overrides).
    let mut duplicate_groups: Vec<DuplicateGroup> = groups
        .into_iter()
        .filter_map(|group| {
            let files: Vec<DuplicateFileEntry> = group
                .paths
                .iter()
                .zip(group.sizes.iter())
                .filter_map(|(path_str, size)| {
                    let path_buf = PathBuf::from(path_str);
                    let modified_ms = fs::metadata(&path_buf)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_millis());
                    Some(DuplicateFileEntry {
                        path: format_path(&path_buf),
                        file_name: file_name(&path_buf),
                        size: *size,
                        modified_ms,
                        extension: extension_lower(&path_buf),
                    })
                })
                .collect();
            if files.len() < 2 {
                return None;
            }
            let max_size = files.iter().map(|f| f.size).max().unwrap_or(0);
            let total_size: u64 = files.iter().map(|f| f.size).sum();
            let wasted = total_size.saturating_sub(max_size);
            Some(DuplicateGroup {
                hash: group.representative_hash,
                size: max_size,
                count: files.len(),
                wasted_bytes: wasted,
                files,
            })
        })
        .collect();

    duplicate_groups.sort_by(|a, b| {
        b.wasted_bytes
            .cmp(&a.wasted_bytes)
            .then(b.count.cmp(&a.count))
    });

    let duplicate_files = duplicate_groups
        .iter()
        .map(|g| g.count.saturating_sub(1))
        .sum();
    let reclaimable_bytes = duplicate_groups.iter().map(|g| g.wasted_bytes).sum();

    // Persistent cache GC — same TTL+LRU pattern as the byte-identical
    // pipeline. Non-fatal — a GC failure here just means the cache may
    // grow over the cap until the next successful scan.
    if let Some(ctx) = cache_ctx.as_ref() {
        match super::perceptual_cache::gc(
            &ctx.db_path,
            DEFAULT_TTL_DAYS,
            DEFAULT_MAX_BYTES,
        ) {
            Ok(stats) if stats.entries_before > 0 => {
                eprintln!(
                    "perceptual_cache: GC kept {}/{} entries ({:.1} MB / {:.1} MB), dropped {} TTL + {} LRU",
                    stats.entries_after,
                    stats.entries_before,
                    stats.bytes_after as f64 / 1_048_576.0,
                    stats.bytes_before as f64 / 1_048_576.0,
                    stats.ttl_dropped,
                    stats.lru_dropped,
                );
            }
            Ok(_) => {}
            Err(e) => eprintln!("perceptual_cache: GC failed (non-fatal): {e}"),
        }
        // Algo id is captured here so a future algorithm bump leaves
        // stale entries in the cache to age out via TTL rather than
        // poisoning fresh scans.
        let _ = ALGO_DHASH_8X8_DCT;
    }

    DuplicateScanResult {
        success: true,
        canceled: false,
        error: None,
        scanned_files,
        hashed_files,
        duplicate_groups,
        duplicate_files,
        reclaimable_bytes,
    }
}

/// Shared inner loop for the perceptual hashing rayon pass — pulled
/// out so it can run inside the bespoke ThreadPool when one builds,
/// and fall back to the global rayon pool when it doesn't (matches
/// the byte-identical pipeline's twin-arm pattern).
fn hash_image_jobs_parallel(
    image_jobs: &[(PathBuf, u64)],
    cache_ctx: Option<&super::perceptual_cache::PerceptualCacheCtx>,
    progress_app: Option<&AppHandle>,
    hashed_counter: &Arc<AtomicUsize>,
    scanned_files: usize,
    emit_progress: fn(Option<&AppHandle>, usize, usize, &str),
) -> Vec<(String, u64, super::perceptual_hash::PerceptualHash)> {
    use super::perceptual_cache::ALGO_DHASH_8X8_DCT;
    use super::perceptual_hash::compute_perceptual_hash;

    image_jobs
        .par_iter()
        .filter_map(|(path, size)| {
            if duplicate_scan_cancelled() {
                return None;
            }

            // Path key for the perceptual cache. Lowercase the path so
            // a re-scan of "C:/Pics/cat.jpg" hits the entry written for
            // "c:\pics\cat.jpg" earlier.
            let path_lower = path.to_string_lossy().to_lowercase().into_bytes();
            let mtime_ms = fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            let phash = if let Some(ctx) = cache_ctx {
                if let Some(cached) = super::perceptual_cache::lookup(
                    ctx,
                    &path_lower,
                    mtime_ms,
                    ALGO_DHASH_8X8_DCT,
                ) {
                    cached
                } else {
                    let fresh = compute_perceptual_hash(path)?;
                    super::perceptual_cache::store(
                        ctx,
                        &path_lower,
                        mtime_ms,
                        ALGO_DHASH_8X8_DCT,
                        fresh,
                    );
                    fresh
                }
            } else {
                compute_perceptual_hash(path)?
            };

            let n = hashed_counter.fetch_add(1, Ordering::Relaxed) + 1;
            // Same 10-step tick as the byte-identical pipeline so the
            // UI feels equally responsive across both modes.
            if n.is_multiple_of(10) {
                emit_progress(progress_app, scanned_files, n, "perceptual");
            }

            Some((format_path(path), *size, phash))
        })
        .collect()
}

fn find_duplicate_files_blocking(
    options: DuplicateScanOptions,
    indexed_state_dir: Option<PathBuf>,
    progress_app: Option<AppHandle>,
) -> DuplicateScanResult {
    // Quality Pass Wave 1 user feedback (2026-05-29): live progress
    // helper. Emits a small JSON payload on the duplicate-scan-progress
    // Tauri event. Errors swallowed — progress is best-effort.
    fn emit_progress(
        app: Option<&AppHandle>,
        scanned: usize,
        hashed: usize,
        phase: &str,
    ) {
        if let Some(app) = app {
            let _ = app.emit(
                DUPLICATE_SCAN_PROGRESS_EVENT,
                serde_json::json!({
                    "scanned": scanned,
                    "hashed": hashed,
                    "phase": phase,
                }),
            );
        }
    }
    if options.roots.is_empty() {
        return DuplicateScanResult {
            success: false,
            canceled: false,
            error: Some("Choose at least one folder or file".into()),
            scanned_files: 0,
            hashed_files: 0,
            duplicate_groups: vec![],
            duplicate_files: 0,
            reclaimable_bytes: 0,
        };
    }

    // Quality Pass Wave 1 user feedback / Option A (2026-05-28):
    // parallel walker. Replaces the previous serial collect_paths()
    // + by_size loop with a fused parallel walk using ignore crate's
    // WalkBuilder::build_parallel(). Per-file work (stat + filter +
    // size-group insert) happens on N worker threads; shared state
    // is a single Mutex<HashMap> whose contention is brief (just a
    // HashMap insert per file). 4-8× faster on multi-core machines
    // for the same workload; no intermediate Vec<PathBuf> so memory
    // tracks the size-group set rather than every path.
    //
    // The previous validate_scan_roots() check via collect_paths
    // (which rejected `..` traversal and system paths) is repeated
    // inline below to keep the security gate.
    if let Err(error) = validate_scan_roots(&options.roots) {
        return DuplicateScanResult {
            success: false,
            canceled: error == duplicate_cancel_error(),
            error: Some(error),
            scanned_files: 0,
            hashed_files: 0,
            duplicate_groups: vec![],
            duplicate_files: 0,
            reclaimable_bytes: 0,
        };
    }

    // Quality Pass Wave 1 / DF-3: parse the extension filter once.
    // Empty -> None -> filter is a no-op below. Lower-cased so user
    // typing matches "JPG" the same as ".jpg".
    let extension_filter: Option<std::collections::HashSet<String>> = {
        let trimmed = options.extension_filter.trim();
        if trimmed.is_empty() {
            None
        } else {
            let set: std::collections::HashSet<String> = trimmed
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_start_matches('.').to_ascii_lowercase())
                .collect();
            if set.is_empty() { None } else { Some(set) }
        }
    };

    let by_size_shared: Arc<Mutex<HashMap<u64, Vec<PathBuf>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let scanned_files_atomic = Arc::new(AtomicUsize::new(0));
    let walk_thread_count = duplicate_thread_count(options.max_threads);
    let include_hidden = options.include_hidden;
    let follow_symlinks = options.follow_symlinks;
    let recursive = options.recursive;
    let min_size = options.min_size;

    // Quality Pass Wave 1 user feedback (2026-05-29): WizTree-style MFT
    // fast-path for DuplicateFinder. The `mft_walker` module exists but
    // is DISABLED pending more runtime debugging — initial Windows test
    // showed it returning ~24 garbage entries instead of the full tree
    // (raw-volume read alignment + ntfs-crate tree traversal both need
    // more work, hard to verify without an iterative Windows test loop).
    //
    // The working DuplicateFinder pipeline is currently:
    //   * Parallel walker (Option A, ignore crate) for the full tree
    //   * Filename-index reuse (Option B) for indexed files
    //   * Persistent hash cache (Option C) for re-scans
    //
    // This trio handled the user's 902 k-file library correctly already.
    // MFT is the "first-scan speedup on unindexed libraries" win that
    // remains — tracked in v1Goals.md → Phase 9 pre-launch as a
    // "return-before-launch" item. The mft_walker module stays in tree
    // so the partially-fixed code is preserved for the next iteration.
    let mft_used = false;

    // Quality Pass Wave 1 / Option B (2026-05-28): index reuse. If the
    // filename index exists AND covers some of the scan roots, pre-load
    // (path → size) for indexed files and skip them in the walker. Net
    // effect: zero metadata stats for already-indexed files (~10-100x
    // speedup on indexed roots). Stale entries handled naturally — the
    // hash phase opens each file, and missing files are silently skipped.
    //
    // `indexed_paths_skip` is the lowercase set the walker checks; it
    // also receives lowercase paths from the walker so deduped entries
    // never get processed twice (a file could match more than one root).
    let indexed_paths_skip: Arc<Mutex<HashSet<String>>> =
        Arc::new(Mutex::new(HashSet::new()));
    let mut indexed_scanned_count: usize = 0;
    if let Some(state_dir) = indexed_state_dir.as_ref() {
        if let Some(indexed) = super::search::enumerate_indexed_file_sizes(state_dir) {
            // Normalize scan roots once for prefix-matching. Lowercase +
            // forward-slash like the Wave 8.4.1 fix in text_extract.rs.
            let roots_normalized: Vec<String> = options
                .roots
                .iter()
                .map(|r| r.to_lowercase().replace('\\', "/").trim_end_matches('/').to_string())
                .collect();

            let mut by_size_lock = by_size_shared.lock().unwrap_or_else(|e| e.into_inner());
            let mut skip_lock = indexed_paths_skip
                .lock()
                .unwrap_or_else(|e| e.into_inner());

            for (path_lower, (real_path, size, _mtime)) in indexed.iter() {
                let path_norm = path_lower.replace('\\', "/");
                let in_scope = roots_normalized
                    .iter()
                    .any(|r| path_norm == *r || path_norm.starts_with(&format!("{}/", r)));
                if !in_scope {
                    continue;
                }
                if *size < min_size {
                    continue;
                }
                if let Some(filter_set) = &extension_filter {
                    let ext = Path::new(real_path)
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|s| s.to_ascii_lowercase())
                        .unwrap_or_default();
                    if !filter_set.contains(&ext) {
                        continue;
                    }
                }
                by_size_lock
                    .entry(*size)
                    .or_default()
                    .push(PathBuf::from(real_path));
                skip_lock.insert(path_lower.clone());
                indexed_scanned_count += 1;
            }
            drop(by_size_lock);
            drop(skip_lock);
        }
    }
    if indexed_scanned_count > 0 {
        scanned_files_atomic.fetch_add(indexed_scanned_count, Ordering::Relaxed);
    }

    // Quality Pass Wave 1 (2026-05-29): MFT fast-path already populated
    // by_size for the whole tree — skip the walker entirely.
    let roots_to_walk: Vec<&String> = if mft_used {
        Vec::new()
    } else {
        options.roots.iter().collect()
    };
    for root in roots_to_walk {
        if duplicate_scan_cancelled() {
            return duplicate_cancelled_result(
                scanned_files_atomic.load(Ordering::Relaxed),
                0,
            );
        }

        let mut builder = ignore::WalkBuilder::new(root);
        builder.hidden(!include_hidden);
        // DuplicateFinder does NOT honor .gitignore / .ignore — users
        // legitimately want to dedup tracked + ignored files alike
        // (think node_modules duplicates across projects). The walker
        // is purely an enumerate; filtering happens via the user's
        // extension_filter / min_size knobs.
        builder.standard_filters(false);
        builder.git_ignore(false);
        builder.git_exclude(false);
        builder.ignore(false);
        builder.parents(false);
        builder.follow_links(follow_symlinks);
        if !recursive {
            builder.max_depth(Some(1));
        }
        builder.threads(walk_thread_count);

        let walker = builder.build_parallel();
        walker.run(|| {
            // Per-worker clones.
            let by_size = Arc::clone(&by_size_shared);
            let scanned_files = Arc::clone(&scanned_files_atomic);
            let extension_filter = extension_filter.clone();
            let skip_set = Arc::clone(&indexed_paths_skip);
            let progress_app = progress_app.clone();
            Box::new(move |entry_result| {
                // Cheap cancel check at the top of every entry.
                if duplicate_scan_cancelled() {
                    return ignore::WalkState::Quit;
                }
                let entry = match entry_result {
                    Ok(value) => value,
                    Err(_) => return ignore::WalkState::Continue,
                };
                let file_type = match entry.file_type() {
                    Some(ft) => ft,
                    None => return ignore::WalkState::Continue,
                };
                if !file_type.is_file() {
                    return ignore::WalkState::Continue;
                }

                let path = entry.path();

                // Option B: already preloaded from the filename index?
                // Skip — its size is already in the by_size map.
                let path_lower = path.to_string_lossy().to_lowercase();
                if let Ok(skip) = skip_set.lock() {
                    if skip.contains(&path_lower) {
                        return ignore::WalkState::Continue;
                    }
                }

                let new_total = scanned_files.fetch_add(1, Ordering::Relaxed) + 1;
                // Quality Pass Wave 1 user feedback (2026-05-29): live
                // progress every 500 entries. Cheap modulus check; the
                // event itself is rate-limited at the OS by IPC pipeline.
                if new_total.is_multiple_of(500) {
                    emit_progress(progress_app.as_ref(), new_total, 0, "walking");
                }

                // DF-3 extension filter.
                if let Some(filter_set) = &extension_filter {
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|s| s.to_ascii_lowercase())
                        .unwrap_or_default();
                    if !filter_set.contains(&ext) {
                        return ignore::WalkState::Continue;
                    }
                }
                let metadata = match entry.metadata() {
                    Ok(m) => m,
                    Err(_) => return ignore::WalkState::Continue,
                };
                let len = metadata.len();
                if len < min_size {
                    return ignore::WalkState::Continue;
                }

                // Lock briefly to push into the size-group map.
                if let Ok(mut map) = by_size.lock() {
                    map.entry(len).or_default().push(path.to_path_buf());
                }
                ignore::WalkState::Continue
            })
        });
    }

    let scanned_files = scanned_files_atomic.load(Ordering::Relaxed);
    if duplicate_scan_cancelled() {
        return duplicate_cancelled_result(scanned_files, 0);
    }
    let by_size: HashMap<u64, Vec<PathBuf>> = match Arc::try_unwrap(by_size_shared) {
        Ok(mutex) => mutex.into_inner().unwrap_or_default(),
        Err(arc) => arc.lock().map(|guard| guard.clone()).unwrap_or_default(),
    };

    // Quality Pass Wave 1 / DF-1 (2026-05-29): perceptual-image branch.
    // When the user picks "Similar images" mode, we ignore the size
    // grouping entirely — two visually identical photos at different
    // resolutions have completely different file sizes — and instead
    // fingerprint every image with dHash. Hamming distance ≤ threshold
    // collapses into "looks alike" groups via Union-Find.
    if options.hash_strategy == "perceptual" {
        return run_perceptual_image_scan(
            by_size,
            scanned_files,
            options.similarity_threshold,
            options.max_threads,
            indexed_state_dir.as_deref(),
            progress_app.as_ref(),
        );
    }

    let mut candidate_files: Vec<(u64, Vec<PathBuf>)> = by_size
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .collect();

    // Largest groups/files first gives the user useful results sooner and improves cache locality
    // before the parallel hashing phase starts.
    candidate_files.sort_by(|(a_size, a_paths), (b_size, b_paths)| {
        b_size
            .cmp(a_size)
            .then_with(|| b_paths.len().cmp(&a_paths.len()))
    });

    if duplicate_scan_cancelled() {
        return duplicate_cancelled_result(scanned_files, 0);
    }

    // Fast prefilter: for same-size files, compare a small fingerprint made from
    // the beginning, middle and end of the file. Only files that still collide
    // on this fingerprint need a full-file hash. Final duplicate results remain
    // exact because full hashes are still used before grouping duplicates.
    let partial_jobs: Vec<(u64, PathBuf)> = candidate_files
        .into_iter()
        .flat_map(|(size, paths)| paths.into_iter().map(move |path| (size, path)))
        .collect();

    let thread_count = duplicate_thread_count(options.max_threads);
    let pool = ThreadPoolBuilder::new().num_threads(thread_count).build();

    let partial_records: Vec<(u64, String, PathBuf)> = match pool {
        Ok(pool) => pool.install(|| {
            partial_jobs
                .par_iter()
                .filter_map(|(size, path)| {
                    if duplicate_scan_cancelled() {
                        return None;
                    }
                    partial_fingerprint(path, *size)
                        .ok()
                        .map(|partial| (*size, partial, path.clone()))
                })
                .collect()
        }),
        Err(_) => partial_jobs
            .par_iter()
            .filter_map(|(size, path)| {
                if duplicate_scan_cancelled() {
                    return None;
                }
                partial_fingerprint(path, *size)
                    .ok()
                    .map(|partial| (*size, partial, path.clone()))
            })
            .collect(),
    };

    if duplicate_scan_cancelled() {
        return duplicate_cancelled_result(scanned_files, 0);
    }

    let mut by_partial: HashMap<(u64, String), Vec<PathBuf>> = HashMap::new();
    for (size, partial, path) in partial_records {
        by_partial.entry((size, partial)).or_default().push(path);
    }

    let full_hash_jobs: Vec<(u64, PathBuf)> = by_partial
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .flat_map(|((size, _partial), paths)| paths.into_iter().map(move |path| (size, path)))
        .collect();

    let hash_algorithm = options.hash_algorithm.clone();
    // Quality Pass Wave 1 / Option C (2026-05-28): persistent
    // (path, mtime) -> hash cache. Re-scans on the same library skip
    // the full-file read + hash entirely for any file whose bytes
    // haven't changed since last time. Cache lives at state_dir/
    // duplicate_hash_cache.redb (sibling of the OCR cache and search
    // index). None when no state dir is available — the hash phase
    // then runs exactly as before, recomputing from scratch.
    let cache_ctx = indexed_state_dir
        .as_ref()
        .map(|state_dir| super::duplicate_cache::DuplicateCacheCtx::new(state_dir));
    // Quality Pass Wave 1 user feedback (2026-05-29): hash-phase live
    // progress. Each rayon worker bumps this counter and emits an
    // event every 50 hashes, so the UI keeps ticking through the
    // potentially long full-hash phase instead of looking frozen.
    let hashed_counter = Arc::new(AtomicUsize::new(0));
    let total_scanned = scanned_files;
    let hashed_records: Vec<(u64, String, PathBuf)> =
        match ThreadPoolBuilder::new().num_threads(thread_count).build() {
            Ok(pool) => pool.install(|| {
                full_hash_jobs
                    .par_iter()
                    .filter_map(|(size, path)| {
                        if duplicate_scan_cancelled() {
                            return None;
                        }
                        let hash = compute_or_lookup_hash(
                            path,
                            *size,
                            &hash_algorithm,
                            cache_ctx.as_ref(),
                        )?;
                        let n = hashed_counter.fetch_add(1, Ordering::Relaxed) + 1;
                        // Quality Pass Wave 1 user feedback (2026-05-29):
                        // lowered from 50 to 10 so the counter ticks
                        // smoothly on slow hash phases (~30 hashes/sec
                        // → ~3 updates/sec instead of one every 1.5 s).
                        if n.is_multiple_of(10) {
                            emit_progress(progress_app.as_ref(), total_scanned, n, "hashing");
                        }
                        Some((*size, hash, path.clone()))
                    })
                    .collect()
            }),
            Err(_) => full_hash_jobs
                .par_iter()
                .filter_map(|(size, path)| {
                    if duplicate_scan_cancelled() {
                        return None;
                    }
                    let hash = compute_or_lookup_hash(
                        path,
                        *size,
                        &hash_algorithm,
                        cache_ctx.as_ref(),
                    )?;
                    let n = hashed_counter.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of(50) {
                        emit_progress(progress_app.as_ref(), total_scanned, n, "hashing");
                    }
                    Some((*size, hash, path.clone()))
                })
                .collect(),
        };

    let hashed_files = hashed_records.len();

    if duplicate_scan_cancelled() {
        return duplicate_cancelled_result(scanned_files, hashed_files);
    }

    let mut by_hash: HashMap<(u64, String), Vec<PathBuf>> = HashMap::new();
    for (size, hash, path) in hashed_records {
        by_hash.entry((size, hash)).or_default().push(path);
    }

    let mut duplicate_groups: Vec<DuplicateGroup> = by_hash
        .into_iter()
        .filter_map(|((size, hash), mut paths)| {
            if paths.len() < 2 {
                return None;
            }

            paths.sort_by(|a, b| format_path(a).cmp(&format_path(b)));

            let files: Vec<DuplicateFileEntry> = paths
                .iter()
                .filter_map(|path| {
                    let metadata = fs::metadata(path).ok()?;
                    let modified_ms = metadata
                        .modified()
                        .ok()
                        .and_then(|v| v.duration_since(UNIX_EPOCH).ok())
                        .map(|v| v.as_millis());

                    Some(DuplicateFileEntry {
                        path: format_path(path),
                        file_name: file_name(path),
                        size: metadata.len(),
                        modified_ms,
                        extension: extension_lower(path),
                    })
                })
                .collect();

            if files.len() < 2 {
                return None;
            }

            Some(DuplicateGroup {
                hash,
                size,
                count: files.len(),
                wasted_bytes: size.saturating_mul((files.len() - 1) as u64),
                files,
            })
        })
        .collect();

    duplicate_groups.sort_by(|a, b| {
        b.wasted_bytes
            .cmp(&a.wasted_bytes)
            .then(a.size.cmp(&b.size))
    });

    let duplicate_files = duplicate_groups
        .iter()
        .map(|g| g.count.saturating_sub(1))
        .sum();

    let reclaimable_bytes = duplicate_groups.iter().map(|g| g.wasted_bytes).sum();

    // Quality Pass Wave 1 / Option C: GC the persistent hash cache
    // after every successful scan. Non-fatal — logs only.
    if let Some(ctx) = cache_ctx.as_ref() {
        match super::duplicate_cache::gc(
            &ctx.db_path,
            super::duplicate_cache::DEFAULT_TTL_DAYS,
            super::duplicate_cache::DEFAULT_MAX_BYTES,
        ) {
            Ok(stats) if stats.entries_before > 0 => {
                eprintln!(
                    "duplicate_cache: GC kept {}/{} entries ({:.1} MB / {:.1} MB), dropped {} TTL + {} LRU",
                    stats.entries_after,
                    stats.entries_before,
                    stats.bytes_after as f64 / 1_048_576.0,
                    stats.bytes_before as f64 / 1_048_576.0,
                    stats.ttl_dropped,
                    stats.lru_dropped,
                );
            }
            Ok(_) => {}
            Err(e) => eprintln!("duplicate_cache: GC failed (non-fatal): {e}"),
        }
    }

    DuplicateScanResult {
        success: true,
        canceled: false,
        error: None,
        scanned_files,
        hashed_files,
        duplicate_groups,
        duplicate_files,
        reclaimable_bytes,
    }
}

#[tauri::command]
pub async fn move_duplicate_files(
    app: AppHandle,
    options: MoveDuplicatesOptions,
) -> MoveDuplicatesResponse {
    // Security gate: each source path must be a real, non-system file
    // and destination_dir must not target a forbidden location. Return
    // the error inline via MoveFileResult so the existing per-file error
    // rendering on the frontend catches it.
    for path in &options.paths {
        if let Err(e) = crate::core::safe_path::validate_user_path(path) {
            return MoveDuplicatesResponse {
                results: options
                    .paths
                    .iter()
                    .map(|p| MoveFileResult {
                        source_path: p.clone(),
                        output_path: String::new(),
                        success: false,
                        error: Some(e.clone()),
                    })
                    .collect(),
                recovery: None,
            };
        }
    }
    if let Err(e) = crate::core::safe_path::forbid_system_path(&options.destination_dir) {
        return MoveDuplicatesResponse {
            results: options
                .paths
                .iter()
                .map(|p| MoveFileResult {
                    source_path: p.clone(),
                    output_path: String::new(),
                    success: false,
                    error: Some(e.clone()),
                })
                .collect(),
            recovery: None,
        };
    }

    tauri::async_runtime::spawn_blocking(move || {
        let destination = PathBuf::from(options.destination_dir);
        if let Err(e) = fs::create_dir_all(&destination) {
            return MoveDuplicatesResponse {
                results: options
                    .paths
                    .iter()
                    .map(|path| MoveFileResult {
                        source_path: path.clone(),
                        output_path: String::new(),
                        success: false,
                        error: Some(format!("Cannot create destination folder: {e}")),
                    })
                    .collect(),
                recovery: None,
            };
        }

        let mut results = Vec::new();
        let mut recovery_entries = Vec::new();

        for path in &options.paths {
            let source = PathBuf::from(path);
            if !source.exists() {
                results.push(MoveFileResult {
                    source_path: path.clone(),
                    output_path: String::new(),
                    success: false,
                    error: Some("Source file no longer exists".into()),
                });
                continue;
            }
            if !source.is_file() {
                results.push(MoveFileResult {
                    source_path: path.clone(),
                    output_path: String::new(),
                    success: false,
                    error: Some("Only files can be moved by duplicate finder".into()),
                });
                continue;
            }

            let target = unique_destination_path(&destination, &source);
            if path_too_long(&target) {
                results.push(MoveFileResult {
                    source_path: path.clone(),
                    output_path: format_path(&target),
                    success: false,
                    error: Some(
                        "Destination path is too long for reliable Windows file handling".into(),
                    ),
                });
                continue;
            }

            match move_file_preserving_single_copy(&source, &target) {
                Ok(_) => {
                    recovery_entries.push(FileRecoveryEntry {
                        original_path: path.clone(),
                        current_path: format_path(&target),
                        is_dir: false,
                    });
                    results.push(MoveFileResult {
                        source_path: path.clone(),
                        output_path: format_path(&target),
                        success: true,
                        error: None,
                    });
                }
                Err(error) => {
                    results.push(MoveFileResult {
                        source_path: path.clone(),
                        output_path: String::new(),
                        success: false,
                        error: Some(error),
                    });
                }
            }
        }

        let recovery = if recovery_entries.is_empty() {
            None
        } else {
            let transaction = FileRecoveryTransaction {
                id: format!(
                    "duplicate_move_{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|v| v.as_millis())
                        .unwrap_or(0)
                ),
                kind: "duplicate_move".into(),
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|v| v.as_millis())
                    .unwrap_or(0),
                entry_count: recovery_entries.len(),
                description: format!(
                    "Moved {} duplicate file{}",
                    recovery_entries.len(),
                    if recovery_entries.len() == 1 { "" } else { "s" }
                ),
                entries: recovery_entries,
            };
            persist_transaction(&app, transaction).ok()
        };

        MoveDuplicatesResponse { results, recovery }
    })
    .await
    .unwrap_or_else(|e| MoveDuplicatesResponse {
        results: vec![MoveFileResult {
            source_path: String::new(),
            output_path: String::new(),
            success: false,
            error: Some(format!("Move worker failed: {e}")),
        }],
        recovery: None,
    })
}

// ============================================================
// Bulk Rename
// ============================================================

#[derive(Deserialize, Clone)]
pub struct BulkRenameOptions {
    pub roots: Vec<String>,
    pub recursive: bool,
    pub include_files: bool,
    pub include_dirs: bool,
    pub include_hidden: bool,
    pub find: String,
    pub replace: String,
    /// Quality Pass Wave 1 / BR-1 (2026-05-28): when true, `find` is
    /// interpreted as a regex pattern (Rust's `regex` crate dialect)
    /// and `replace` as the substitution string (with `$1` / `$name`
    /// capture-group references). Default false keeps the literal
    /// find/replace behavior for backward compatibility.
    #[serde(default)]
    pub use_regex: bool,
    pub prefix: String,
    pub suffix: String,
    pub case_mode: String, // none | lower | upper | title
    pub numbering_enabled: bool,
    pub numbering_start: u32,
    pub numbering_padding: u8,
    pub numbering_separator: String,
    pub extension_case: String, // keep | lower | upper
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RenamePreviewItem {
    pub original_path: String,
    pub new_path: String,
    pub original_name: String,
    pub new_name: String,
    pub is_dir: bool,
    pub status: String, // ready | unchanged | conflict | invalid
    pub reason: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct BulkRenamePreviewResult {
    pub success: bool,
    pub error: Option<String>,
    pub scanned_items: usize,
    pub ready_count: usize,
    pub conflict_count: usize,
    pub unchanged_count: usize,
    pub invalid_count: usize,
    pub items: Vec<RenamePreviewItem>,
}

#[derive(Deserialize)]
pub struct ApplyBulkRenameOptions {
    pub items: Vec<RenamePreviewItem>,
}

#[derive(Serialize, Clone)]
pub struct RenameApplyResult {
    pub original_path: String,
    pub new_path: String,
    pub success: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn preview_bulk_rename(options: BulkRenameOptions) -> BulkRenamePreviewResult {
    // Security gate: each scan root must resolve to a real, non-system
    // path. Rejects `..` traversal and protected system folders before
    // we walk anything.
    for root in &options.roots {
        if let Err(e) = crate::core::safe_path::validate_user_path(root) {
            return BulkRenamePreviewResult {
                success: false,
                error: Some(e),
                scanned_items: 0,
                ready_count: 0,
                conflict_count: 0,
                unchanged_count: 0,
                invalid_count: 0,
                items: vec![],
            };
        }
    }

    tauri::async_runtime::spawn_blocking(move || preview_bulk_rename_blocking(options))
        .await
        .unwrap_or_else(|e| BulkRenamePreviewResult {
            success: false,
            error: Some(format!("Rename preview worker failed: {e}")),
            scanned_items: 0,
            ready_count: 0,
            conflict_count: 0,
            unchanged_count: 0,
            invalid_count: 0,
            items: vec![],
        })
}

fn preview_bulk_rename_blocking(options: BulkRenameOptions) -> BulkRenamePreviewResult {
    if options.roots.is_empty() {
        return BulkRenamePreviewResult {
            success: false,
            error: Some("Choose files or folders first".into()),
            scanned_items: 0,
            ready_count: 0,
            conflict_count: 0,
            unchanged_count: 0,
            invalid_count: 0,
            items: vec![],
        };
    }

    let paths = match collect_paths(
        &options.roots,
        options.recursive,
        options.include_files,
        options.include_dirs,
        options.include_hidden,
        false,
        None,
        // BulkRename keeps the historical default cap — typical rename
        // workloads don't justify multi-GB of path strings in memory.
        Some(MAX_COLLECTED_PATHS),
    ) {
        Ok(paths) => paths,
        Err(error) => {
            return BulkRenamePreviewResult {
                success: false,
                error: Some(error),
                scanned_items: 0,
                ready_count: 0,
                conflict_count: 0,
                unchanged_count: 0,
                invalid_count: 0,
                items: vec![],
            };
        }
    };

    // Quality Pass Wave 1 / BR-1: when regex mode is on, compile the
    // pattern ONCE up-front. A bad pattern fails the whole preview
    // cleanly with a single human error rather than marking every
    // matched file as invalid. Empty find is allowed in both modes
    // (= "no find/replace pass").
    let compiled_regex = if options.use_regex && !options.find.is_empty() {
        match regex::Regex::new(&options.find) {
            Ok(r) => Some(r),
            Err(e) => {
                return BulkRenamePreviewResult {
                    success: false,
                    error: Some(format!("Invalid regex: {e}")),
                    scanned_items: 0,
                    ready_count: 0,
                    conflict_count: 0,
                    unchanged_count: 0,
                    invalid_count: 0,
                    items: vec![],
                };
            }
        }
    } else {
        None
    };

    let mut items: Vec<RenamePreviewItem> = paths
        .iter()
        .enumerate()
        .map(|(idx, path)| build_rename_preview_item(path, idx, &options, compiled_regex.as_ref()))
        .collect();

    mark_batch_conflicts(&mut items);
    summarize_preview(items)
}

/// Quality Pass Wave 1 / BR-2 (2026-05-29): per-file context for the
/// token expander. Holds everything `expand_tokens` needs to substitute
/// `{n}`, `{name}`, `{ext}`, `{parent}`, `{date}`, `{modified}` etc.
struct TokenContext<'a> {
    /// 0-based index of this file in the batch — `numbering_start +
    /// index` is the value `{n}` resolves to.
    index: u32,
    /// Original file stem (without extension).
    name: &'a str,
    /// Original file extension (without dot).
    ext: &'a str,
    /// Immediate parent folder name.
    parent: &'a str,
    /// File's modification time in unix nanoseconds, used by
    /// `{modified}` / `{modified:fmt}`. None falls back to current time.
    modified_nanos: Option<i128>,
    numbering_start: u32,
    numbering_padding: u8,
}

/// Quality Pass Wave 1 / BR-2 (2026-05-29): substitute templated tokens
/// in a string. Supported tokens (case-sensitive):
///   * `{n}` — current numbering value, padded by the global setting
///   * `{n:NNN}` — N-digit zero-padded counter (overrides the global)
///   * `{name}` — original file stem
///   * `{ext}` — original file extension (no dot)
///   * `{parent}` — immediate parent folder name
///   * `{date}` / `{date:fmt}` — current date (default `YYYY-MM-DD`)
///   * `{modified}` / `{modified:fmt}` — file's mtime (same default)
///
/// Format strings honor `YYYY YY MM DD HH mm ss` substitution tokens.
/// Unknown tokens render as literal `{whatever}` so users can spot
/// typos in the preview without crashing the rename.
fn expand_tokens(template: &str, ctx: &TokenContext<'_>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '{' {
            out.push(c);
            continue;
        }
        // Collect until '}'.
        let mut token = String::new();
        let mut closed = false;
        for next in chars.by_ref() {
            if next == '}' {
                closed = true;
                break;
            }
            token.push(next);
        }
        if !closed {
            // Unclosed brace — treat as literal.
            out.push('{');
            out.push_str(&token);
            continue;
        }
        out.push_str(&expand_single_token(&token, ctx));
    }
    out
}

fn expand_single_token(token: &str, ctx: &TokenContext<'_>) -> String {
    let (name, arg) = match token.split_once(':') {
        Some((n, a)) => (n, Some(a)),
        None => (token, None),
    };
    match name {
        "n" => {
            let num = ctx.numbering_start.saturating_add(ctx.index);
            let pad = arg.map(|s| s.len()).unwrap_or(ctx.numbering_padding as usize);
            format!("{:0width$}", num, width = pad.max(1))
        }
        "name" => ctx.name.to_string(),
        "ext" => ctx.ext.to_string(),
        "parent" => ctx.parent.to_string(),
        "date" => {
            let now = time::OffsetDateTime::now_local()
                .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
            format_token_date(arg.unwrap_or("YYYY-MM-DD"), now)
        }
        "modified" => {
            let dt = ctx
                .modified_nanos
                .and_then(|ns| time::OffsetDateTime::from_unix_timestamp_nanos(ns).ok())
                .map(|utc| utc.to_offset(time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC)))
                .unwrap_or_else(|| {
                    time::OffsetDateTime::now_local()
                        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
                });
            format_token_date(arg.unwrap_or("YYYY-MM-DD"), dt)
        }
        _ => format!("{{{token}}}"),
    }
}

fn format_token_date(fmt: &str, dt: time::OffsetDateTime) -> String {
    // Naive token-by-token substitution. Replaces longest tokens first
    // so YYYY doesn't get half-eaten by YY's substitution.
    let mut out = fmt.to_string();
    out = out.replace("YYYY", &format!("{:04}", dt.year()));
    out = out.replace("YY", &format!("{:02}", (dt.year() % 100).unsigned_abs()));
    out = out.replace("MM", &format!("{:02}", dt.month() as u8));
    out = out.replace("DD", &format!("{:02}", dt.day()));
    out = out.replace("HH", &format!("{:02}", dt.hour()));
    out = out.replace("mm", &format!("{:02}", dt.minute()));
    out = out.replace("ss", &format!("{:02}", dt.second()));
    out
}

fn build_rename_preview_item(
    path: &Path,
    index: usize,
    options: &BulkRenameOptions,
    // Quality Pass Wave 1 / BR-1: pre-compiled find regex (Some only
    // when `options.use_regex` is true AND the pattern compiled). None
    // = literal find/replace path. Passed by reference so we don't
    // re-compile per file.
    compiled_regex: Option<&regex::Regex>,
) -> RenamePreviewItem {
    let original_name = file_name(path);
    let is_dir = path.is_dir();

    if original_name.is_empty() {
        return RenamePreviewItem {
            original_path: format_path(path),
            new_path: format_path(path),
            original_name,
            new_name: String::new(),
            is_dir,
            status: "invalid".into(),
            reason: Some("Cannot rename this path".into()),
        };
    }

    let (stem, ext) = if is_dir {
        (original_name.clone(), String::new())
    } else {
        let stem = path
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or(&original_name)
            .to_string();
        let ext = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_string();
        (stem, ext)
    };

    // Quality Pass Wave 1 / BR-2 (2026-05-29): build the per-file
    // token context once, use it to expand prefix / suffix / replace.
    // Parent name = the immediate folder above this file.
    let parent_name: String = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    // Modified time in unix nanoseconds. Some FSes can return absurd
    // values — `from_unix_timestamp_nanos` rejects them at the
    // OffsetDateTime layer and {modified} silently falls back to now.
    let modified_nanos: Option<i128> = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|st| st.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i128);
    let token_ctx = TokenContext {
        index: index as u32,
        name: &stem,
        ext: &ext,
        parent: &parent_name,
        modified_nanos,
        numbering_start: options.numbering_start,
        numbering_padding: options.numbering_padding,
    };

    // Expand tokens in the replacement string (literal mode only —
    // regex mode uses Regex's `$1` / `$name` capture-group syntax and
    // shouldn't be double-expanded).
    let expanded_replace = if compiled_regex.is_some() {
        options.replace.clone()
    } else {
        expand_tokens(&options.replace, &token_ctx)
    };

    let mut new_stem = stem.clone();

    if !options.find.is_empty() {
        // Quality Pass Wave 1 / BR-1: regex path uses Regex::replace_all
        // which honors capture-group syntax (`$1`, `$name`) in the
        // replacement string. Literal path keeps str::replace semantics.
        new_stem = if let Some(re) = compiled_regex {
            re.replace_all(&new_stem, expanded_replace.as_str()).into_owned()
        } else {
            new_stem.replace(&options.find, &expanded_replace)
        };
    }

    new_stem = match options.case_mode.as_str() {
        "lower" => new_stem.to_lowercase(),
        "upper" => new_stem.to_uppercase(),
        "title" => title_case(&new_stem),
        _ => new_stem,
    };

    if options.numbering_enabled {
        let number = options.numbering_start.saturating_add(index as u32);
        let width = options.numbering_padding.clamp(1, 12) as usize;
        let serial = format!("{number:0width$}");
        new_stem = format!("{serial}{}{}", options.numbering_separator, new_stem);
    }

    // Quality Pass Wave 1 / BR-2: expand tokens in prefix + suffix.
    // Token expansion happens AFTER find/replace + casing + numbering
    // so the user can still reference {n} and {date} regardless of
    // upstream transforms.
    let expanded_prefix = expand_tokens(&options.prefix, &token_ctx);
    let expanded_suffix = expand_tokens(&options.suffix, &token_ctx);

    new_stem = format!("{expanded_prefix}{new_stem}{expanded_suffix}");
    new_stem = new_stem.trim().to_string();

    let new_ext = match options.extension_case.as_str() {
        "lower" => ext.to_lowercase(),
        "upper" => ext.to_uppercase(),
        _ => ext,
    };

    let new_name = if is_dir || new_ext.is_empty() {
        new_stem.clone()
    } else {
        format!("{new_stem}.{new_ext}")
    };

    let mut status = "ready".to_string();
    let mut reason = None;

    if new_stem.is_empty() || new_name.is_empty() {
        status = "invalid".into();
        reason = Some("New name cannot be empty".into());
    } else if filename_too_long(&new_name) {
        status = "invalid".into();
        reason = Some("New name is too long for reliable Windows file handling".into());
    } else if contains_path_separator(&new_name) {
        status = "invalid".into();
        reason = Some("New name cannot contain path separators".into());
    } else if has_invalid_windows_name(&new_name) {
        status = "invalid".into();
        reason = Some("New name contains characters invalid on Windows".into());
    }

    let new_path = path
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(&new_name);

    if status == "ready" && path_too_long(&new_path) {
        status = "invalid".into();
        reason = Some("Resulting path is too long for reliable Windows file handling".into());
    }

    if status == "ready" && same_path_string(path, &new_path) {
        status = "unchanged".into();
        reason = Some("Name is unchanged".into());
    }

    if status == "ready" && new_path.exists() {
        status = "conflict".into();
        reason = Some("A file or folder with this name already exists".into());
    }

    RenamePreviewItem {
        original_path: format_path(path),
        new_path: format_path(&new_path),
        original_name,
        new_name,
        is_dir,
        status,
        reason,
    }
}

fn summarize_preview(items: Vec<RenamePreviewItem>) -> BulkRenamePreviewResult {
    let scanned_items = items.len();
    let ready_count = items.iter().filter(|i| i.status == "ready").count();
    let conflict_count = items.iter().filter(|i| i.status == "conflict").count();
    let unchanged_count = items.iter().filter(|i| i.status == "unchanged").count();
    let invalid_count = items.iter().filter(|i| i.status == "invalid").count();

    BulkRenamePreviewResult {
        success: true,
        error: None,
        scanned_items,
        ready_count,
        conflict_count,
        unchanged_count,
        invalid_count,
        items,
    }
}

fn mark_batch_conflicts(items: &mut [RenamePreviewItem]) {
    let mut targets: HashMap<String, usize> = HashMap::new();
    let mut duplicated_targets = HashSet::new();

    for item in items.iter().filter(|i| i.status == "ready") {
        let normalized = item.new_path.to_lowercase();
        if targets.insert(normalized.clone(), 1).is_some() {
            duplicated_targets.insert(normalized);
        }
    }

    if !duplicated_targets.is_empty() {
        for item in items.iter_mut().filter(|i| i.status == "ready") {
            if duplicated_targets.contains(&item.new_path.to_lowercase()) {
                item.status = "conflict".into();
                item.reason = Some("Multiple selected items would get the same name".into());
            }
        }
    }

    let selected_paths: Vec<PathBuf> = items
        .iter()
        .filter(|i| i.status == "ready" || i.status == "unchanged")
        .map(|i| PathBuf::from(&i.original_path))
        .collect();

    for item in items.iter_mut().filter(|i| i.status == "ready" && i.is_dir) {
        let current = PathBuf::from(&item.original_path);
        let has_selected_child = selected_paths
            .iter()
            .any(|candidate| candidate != &current && candidate.starts_with(&current));
        if has_selected_child {
            item.status = "conflict".into();
            item.reason =
                Some("Cannot rename a folder and its nested items in the same batch".into());
        }
    }
}

#[tauri::command]
pub async fn apply_bulk_rename(
    app: AppHandle,
    options: ApplyBulkRenameOptions,
) -> RenameApplyResponse {
    // Security gate: validate every original/new path the user is about
    // to rename. Sources must exist and be non-system; targets must not
    // land in a forbidden location. Bail out atomically (no partial
    // renames) if any single path is invalid.
    for item in &options.items {
        if let Err(e) = crate::core::safe_path::validate_user_path(&item.original_path) {
            return RenameApplyResponse {
                results: options
                    .items
                    .iter()
                    .map(|it| RenameApplyResult {
                        original_path: it.original_path.clone(),
                        new_path: it.new_path.clone(),
                        success: false,
                        error: Some(e.clone()),
                    })
                    .collect(),
                recovery: None,
            };
        }
        if let Err(e) = crate::core::safe_path::forbid_system_path(&item.new_path) {
            return RenameApplyResponse {
                results: options
                    .items
                    .iter()
                    .map(|it| RenameApplyResult {
                        original_path: it.original_path.clone(),
                        new_path: it.new_path.clone(),
                        success: false,
                        error: Some(e.clone()),
                    })
                    .collect(),
                recovery: None,
            };
        }
    }

    tauri::async_runtime::spawn_blocking(move || {
        let (results, recovery_entries) = apply_bulk_rename_blocking(options.items);
        let recovery = if recovery_entries.is_empty() {
            None
        } else {
            let created_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|v| v.as_millis())
                .unwrap_or(0);
            let transaction = FileRecoveryTransaction {
                id: format!("bulk_rename_{created_at}"),
                kind: "bulk_rename".into(),
                created_at,
                entry_count: recovery_entries.len(),
                description: format!(
                    "Renamed {} item{}",
                    recovery_entries.len(),
                    if recovery_entries.len() == 1 { "" } else { "s" }
                ),
                entries: recovery_entries,
            };
            persist_transaction(&app, transaction).ok()
        };

        RenameApplyResponse { results, recovery }
    })
    .await
    .unwrap_or_else(|e| RenameApplyResponse {
        results: vec![RenameApplyResult {
            original_path: String::new(),
            new_path: String::new(),
            success: false,
            error: Some(format!("Rename worker failed: {e}")),
        }],
        recovery: None,
    })
}

fn apply_bulk_rename_blocking(
    items: Vec<RenamePreviewItem>,
) -> (Vec<RenameApplyResult>, Vec<FileRecoveryEntry>) {
    let ready: Vec<RenamePreviewItem> = items
        .into_iter()
        .filter(|item| item.status == "ready")
        .collect();

    if ready.is_empty() {
        return (vec![], vec![]);
    }

    let mut temp_pairs: Vec<(RenamePreviewItem, PathBuf)> = Vec::new();
    let batch_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_millis())
        .unwrap_or(0);

    let mut results = Vec::new();
    let mut recovery_entries = Vec::new();

    for (index, item) in ready.iter().enumerate() {
        let source = PathBuf::from(&item.original_path);
        let target = PathBuf::from(&item.new_path);

        if !source.exists() {
            results.push(RenameApplyResult {
                original_path: item.original_path.clone(),
                new_path: item.new_path.clone(),
                success: false,
                error: Some("Source no longer exists".into()),
            });
            continue;
        }

        if target.exists() {
            results.push(RenameApplyResult {
                original_path: item.original_path.clone(),
                new_path: item.new_path.clone(),
                success: false,
                error: Some("Target already exists".into()),
            });
            continue;
        }

        let temp = source.with_file_name(format!(".keepitlocal_rename_{batch_id}_{index}.tmp"));
        if temp.exists() {
            results.push(RenameApplyResult {
                original_path: item.original_path.clone(),
                new_path: item.new_path.clone(),
                success: false,
                error: Some("Temporary rename path already exists".into()),
            });
            continue;
        }

        match fs::rename(&source, &temp) {
            Ok(_) => temp_pairs.push((item.clone(), temp)),
            Err(e) => results.push(RenameApplyResult {
                original_path: item.original_path.clone(),
                new_path: item.new_path.clone(),
                success: false,
                error: Some(describe_io_error("Prepare rename", &source, &e)),
            }),
        }
    }

    for (item, temp) in temp_pairs {
        let target = PathBuf::from(&item.new_path);
        match fs::rename(&temp, &target) {
            Ok(_) => {
                recovery_entries.push(FileRecoveryEntry {
                    original_path: item.original_path.clone(),
                    current_path: item.new_path.clone(),
                    is_dir: item.is_dir,
                });
                results.push(RenameApplyResult {
                    original_path: item.original_path,
                    new_path: item.new_path,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                let rollback_result = fs::rename(&temp, &item.original_path);
                let rollback_note = if let Err(rollback_err) = rollback_result {
                    format!(
                        " Rollback failed: {}",
                        describe_io_error(
                            "Rollback",
                            Path::new(&item.original_path),
                            &rollback_err
                        )
                    )
                } else {
                    String::new()
                };
                results.push(RenameApplyResult {
                    original_path: item.original_path,
                    new_path: item.new_path,
                    success: false,
                    error: Some(format!(
                        "{}.{rollback_note}",
                        describe_io_error("Finish rename", &target, &e)
                    )),
                });
            }
        }
    }

    (results, recovery_entries)
}

fn title_case(value: &str) -> String {
    let mut output = String::new();
    let mut capitalize_next = true;

    for ch in value.chars() {
        if ch.is_alphanumeric() {
            if capitalize_next {
                for c in ch.to_uppercase() {
                    output.push(c);
                }
                capitalize_next = false;
            } else {
                for c in ch.to_lowercase() {
                    output.push(c);
                }
            }
        } else {
            output.push(ch);
            capitalize_next = true;
        }
    }

    output
}

fn contains_path_separator(value: &str) -> bool {
    value.contains('/') || value.contains('\\')
}

fn has_invalid_windows_name(value: &str) -> bool {
    let invalid_chars = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    if value
        .chars()
        .any(|c| invalid_chars.contains(&c) || c.is_control())
    {
        return true;
    }

    let stem = value.split('.').next().unwrap_or("").trim().to_uppercase();

    matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    ) || value.ends_with('.')
        || value.ends_with(' ')
}

fn same_path_string(a: &Path, b: &Path) -> bool {
    format_path(a).to_lowercase() == format_path(b).to_lowercase()
}

fn filename_too_long(value: &str) -> bool {
    value.chars().count() > 240
}

fn undo_recovery_entries(
    entries: &[FileRecoveryEntry],
) -> (Vec<MoveFileResult>, Vec<FileRecoveryEntry>) {
    let mut results = Vec::new();
    let mut remaining = Vec::new();

    for entry in entries.iter().rev() {
        let current = PathBuf::from(&entry.current_path);
        let original = PathBuf::from(&entry.original_path);

        if !current.exists() {
            results.push(MoveFileResult {
                source_path: entry.current_path.clone(),
                output_path: entry.original_path.clone(),
                success: false,
                error: Some("Current file no longer exists, so KeepItLocal cannot restore it".into()),
            });
            continue;
        }

        if original.exists() {
            results.push(MoveFileResult {
                source_path: entry.current_path.clone(),
                output_path: entry.original_path.clone(),
                success: false,
                error: Some(
                    "Original path already exists, so restore would overwrite another file".into(),
                ),
            });
            remaining.push(entry.clone());
            continue;
        }

        let restore_result = if entry.is_dir {
            fs::rename(&current, &original)
                .map_err(|error| describe_io_error("Restore", &current, &error))
        } else {
            move_file_preserving_single_copy(&current, &original)
        };

        match restore_result {
            Ok(_) => results.push(MoveFileResult {
                source_path: entry.current_path.clone(),
                output_path: entry.original_path.clone(),
                success: true,
                error: None,
            }),
            Err(error) => {
                results.push(MoveFileResult {
                    source_path: entry.current_path.clone(),
                    output_path: entry.original_path.clone(),
                    success: false,
                    error: Some(error),
                });
                remaining.push(entry.clone());
            }
        }
    }

    (results, remaining)
}

#[tauri::command]
pub fn get_file_recovery_state(app: AppHandle) -> Result<FileRecoveryState, String> {
    let log = read_recovery_log(&app)?;
    Ok(FileRecoveryState {
        bulk_rename: latest_transaction(&log, "bulk_rename").map(summarize_recovery),
        duplicate_move: latest_transaction(&log, "duplicate_move").map(summarize_recovery),
    })
}

#[tauri::command]
pub async fn undo_bulk_rename(app: AppHandle, transaction_id: String) -> RenameApplyResponse {
    tauri::async_runtime::spawn_blocking(move || {
        let log = match read_recovery_log(&app) {
            Ok(log) => log,
            Err(error) => {
                return RenameApplyResponse {
                    results: vec![RenameApplyResult {
                        original_path: String::new(),
                        new_path: String::new(),
                        success: false,
                        error: Some(error),
                    }],
                    recovery: None,
                };
            }
        };

        let Some(transaction) = log
            .transactions
            .iter()
            .find(|transaction| {
                transaction.id == transaction_id && transaction.kind == "bulk_rename"
            })
            .cloned()
        else {
            return RenameApplyResponse {
                results: vec![RenameApplyResult {
                    original_path: String::new(),
                    new_path: String::new(),
                    success: false,
                    error: Some("Rename recovery entry was not found".into()),
                }],
                recovery: None,
            };
        };

        let (move_results, remaining_entries) = undo_recovery_entries(&transaction.entries);
        let results = move_results
            .into_iter()
            .map(|result| RenameApplyResult {
                original_path: result.source_path,
                new_path: result.output_path,
                success: result.success,
                error: result.error,
            })
            .collect::<Vec<_>>();
        let recovery = update_transaction_after_undo(&app, &transaction_id, remaining_entries)
            .ok()
            .flatten();
        RenameApplyResponse { results, recovery }
    })
    .await
    .unwrap_or_else(|e| RenameApplyResponse {
        results: vec![RenameApplyResult {
            original_path: String::new(),
            new_path: String::new(),
            success: false,
            error: Some(format!("Undo worker failed: {e}")),
        }],
        recovery: None,
    })
}

#[tauri::command]
pub async fn undo_duplicate_move(app: AppHandle, transaction_id: String) -> MoveDuplicatesResponse {
    tauri::async_runtime::spawn_blocking(move || {
        let log = match read_recovery_log(&app) {
            Ok(log) => log,
            Err(error) => {
                return MoveDuplicatesResponse {
                    results: vec![MoveFileResult {
                        source_path: String::new(),
                        output_path: String::new(),
                        success: false,
                        error: Some(error),
                    }],
                    recovery: None,
                };
            }
        };

        let Some(transaction) = log
            .transactions
            .iter()
            .find(|transaction| {
                transaction.id == transaction_id && transaction.kind == "duplicate_move"
            })
            .cloned()
        else {
            return MoveDuplicatesResponse {
                results: vec![MoveFileResult {
                    source_path: String::new(),
                    output_path: String::new(),
                    success: false,
                    error: Some("Duplicate move recovery entry was not found".into()),
                }],
                recovery: None,
            };
        };

        let (results, remaining_entries) = undo_recovery_entries(&transaction.entries);
        let recovery = update_transaction_after_undo(&app, &transaction_id, remaining_entries)
            .ok()
            .flatten();
        MoveDuplicatesResponse { results, recovery }
    })
    .await
    .unwrap_or_else(|e| MoveDuplicatesResponse {
        results: vec![MoveFileResult {
            source_path: String::new(),
            output_path: String::new(),
            success: false,
            error: Some(format!("Undo worker failed: {e}")),
        }],
        recovery: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dangerous_scan_roots_block_system_locations() {
        assert!(is_dangerous_scan_root(Path::new("C:\\Windows")));
        assert!(is_dangerous_scan_root(Path::new("/usr")));
        assert!(is_dangerous_scan_root(Path::new("C:")));
    }

    #[test]
    fn dangerous_scan_roots_allow_normal_user_locations() {
        assert!(!is_dangerous_scan_root(Path::new(
            "C:\\Users\\neo\\Downloads"
        )));
        assert!(!is_dangerous_scan_root(Path::new("/home/neo/projects")));
    }
}

fn file_operations_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot resolve app data directory: {e}"))?
        .join(FILE_OPERATIONS_DIR);
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Cannot create file operations directory: {e}"))?;
    Ok(dir)
}

fn file_recovery_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(file_operations_dir(app)?.join(FILE_RECOVERY_FILE))
}

fn file_operations_db_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(local_db::database_path_for_dir(&file_operations_dir(app)?))
}

fn quarantine_corrupt_json(path: &Path) -> Result<(), String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_millis())
        .unwrap_or(0);
    let quarantine = path.with_extension(format!("corrupt-{timestamp}.json"));
    fs::rename(path, quarantine).map_err(|e| format!("Cannot quarantine corrupt recovery log: {e}"))
}

fn default_recovery_log() -> FileRecoveryLog {
    FileRecoveryLog {
        version: FILE_RECOVERY_VERSION,
        transactions: Vec::new(),
    }
}

fn read_recovery_log(app: &AppHandle) -> Result<FileRecoveryLog, String> {
    let db_path = file_operations_db_path(app)?;
    if let Some(mut log) = local_db::read_json::<FileRecoveryLog>(&db_path, FILE_RECOVERY_FILE)? {
        if log.version == 0 {
            log.version = FILE_RECOVERY_VERSION;
        }
        return Ok(log);
    }

    let path = file_recovery_path(app)?;
    if !path.exists() {
        let log = default_recovery_log();
        local_db::write_json(&db_path, FILE_RECOVERY_FILE, &log)?;
        return Ok(log);
    }

    let raw = fs::read_to_string(&path).map_err(|e| format!("Cannot read recovery log: {e}"))?;
    let mut log: FileRecoveryLog = match serde_json::from_str(&raw) {
        Ok(log) => log,
        Err(_) => {
            quarantine_corrupt_json(&path)?;
            let log = default_recovery_log();
            local_db::write_json(&db_path, FILE_RECOVERY_FILE, &log)?;
            return Ok(log);
        }
    };
    if log.version == 0 {
        log.version = FILE_RECOVERY_VERSION;
    }
    local_db::write_json(&db_path, FILE_RECOVERY_FILE, &log)?;
    Ok(log)
}

fn write_recovery_log(app: &AppHandle, log: &FileRecoveryLog) -> Result<(), String> {
    let mut cleaned = log.clone();
    cleaned.version = FILE_RECOVERY_VERSION;
    if cleaned.transactions.len() > MAX_FILE_TRANSACTIONS {
        cleaned.transactions.truncate(MAX_FILE_TRANSACTIONS);
    }

    let db_path = file_operations_db_path(app)?;
    local_db::write_json(&db_path, FILE_RECOVERY_FILE, &cleaned)
}

fn summarize_recovery(transaction: &FileRecoveryTransaction) -> RecoveryTransactionSummary {
    RecoveryTransactionSummary {
        id: transaction.id.clone(),
        kind: transaction.kind.clone(),
        created_at: transaction.created_at,
        entry_count: transaction.entry_count,
        description: transaction.description.clone(),
    }
}

fn persist_transaction(
    app: &AppHandle,
    transaction: FileRecoveryTransaction,
) -> Result<RecoveryTransactionSummary, String> {
    let mut log = read_recovery_log(app)?;
    log.transactions
        .retain(|existing| existing.id != transaction.id);
    log.transactions.insert(0, transaction.clone());
    if log.transactions.len() > MAX_FILE_TRANSACTIONS {
        log.transactions.truncate(MAX_FILE_TRANSACTIONS);
    }
    write_recovery_log(app, &log)?;
    Ok(summarize_recovery(&transaction))
}

fn latest_transaction<'a>(
    log: &'a FileRecoveryLog,
    kind: &str,
) -> Option<&'a FileRecoveryTransaction> {
    log.transactions
        .iter()
        .find(|transaction| transaction.kind == kind)
}

fn update_transaction_after_undo(
    app: &AppHandle,
    transaction_id: &str,
    remaining_entries: Vec<FileRecoveryEntry>,
) -> Result<Option<RecoveryTransactionSummary>, String> {
    let mut log = read_recovery_log(app)?;
    let index = match log
        .transactions
        .iter()
        .position(|transaction| transaction.id == transaction_id)
    {
        Some(index) => index,
        None => return Ok(None),
    };

    if remaining_entries.is_empty() {
        log.transactions.remove(index);
        write_recovery_log(app, &log)?;
        return Ok(None);
    }

    log.transactions[index].entry_count = remaining_entries.len();
    log.transactions[index].entries = remaining_entries;
    let summary = summarize_recovery(&log.transactions[index]);
    write_recovery_log(app, &log)?;
    Ok(Some(summary))
}

/// One immediate child of a folder, for the command-palette folder preview.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FolderChild {
    name: String,
    is_dir: bool,
    /// File size in bytes (0 for directories).
    size: u64,
    /// Last-modified time in ms since epoch (0 if unavailable).
    modified_ms: i64,
}

/// List a folder's immediate children for the preview pane. Best-effort:
/// entries that error are skipped, and we cap the listing so a huge dir
/// (downloads, node_modules) can't stall the preview. Sorted dirs-first,
/// then case-insensitive by name.
#[tauri::command(async)]
pub fn list_folder_children(path: String) -> Result<Vec<FolderChild>, String> {
    const MAX_CHILDREN: usize = 250;

    let read_dir = fs::read_dir(&path).map_err(|e| format!("Cannot read folder: {e}"))?;

    let mut children: Vec<FolderChild> = Vec::new();
    for entry in read_dir.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let is_dir = file_type.is_dir();

        // Metadata is only needed for files (size) and the modified time.
        let metadata = entry.metadata().ok();
        let size = if is_dir {
            0
        } else {
            metadata.as_ref().map(|m| m.len()).unwrap_or(0)
        };
        let modified_ms = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        children.push(FolderChild {
            name,
            is_dir,
            size,
            modified_ms,
        });
    }

    // Dirs first, then files; within each group, case-insensitive by name.
    children.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    children.truncate(MAX_CHILDREN);

    Ok(children)
}
