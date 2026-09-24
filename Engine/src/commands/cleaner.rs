use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fs;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use walkdir::WalkDir;

use crate::core::resources;

// CREATE_NO_WINDOW — suppress the cmd console flash when the File
// Cleaner / Analyzer runs `reg query` (startup items). Without this,
// opening the Cleaner page or pressing Refresh briefly pops a black
// console window — both bad UX and at odds with the "no surprising
// console output" expectation. (RAM is now read via the in-process
// `core::resources` sysinfo probe — no more PowerShell spawn.)
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const MB: u64 = 1024 * 1024;
const GB: u64 = 1024 * MB;
const DEFAULT_OLD_FILE_DAYS: u64 = 365;
const DEFAULT_MAX_OLD_FILES: usize = 60;
const OLD_FILE_MIN_BYTES: u64 = 50 * MB;

// ─── Adaptive scan: progress + throttle (no hard entry caps) ──────────
//
// The old `MAX_ANALYZER_WALK_ENTRIES` / `MAX_OLD_FILE_SCAN_ENTRIES` caps are
// gone. Memory is bounded by streaming (bounded top-N heap for old files;
// fixed per-dir / per-type accumulators for the maps), responsiveness by
// RAM-sized walk concurrency (`core::resources`) + CPU throttle + user Cancel.

/// Stage count for the coarse determinate progress bar (the frontend renders
/// stageIndex/stageTotal). Stages: drives+memory → cache → old files →
/// disk-usage map → file-type breakdown.
const CLEANER_STAGE_TOTAL: u32 = 5;
/// Emit a progress event at most this often (ms) — mirrors the search indexer's
/// 700 ms throttle so the UI updates smoothly without flooding IPC.
const CLEANER_PROGRESS_MIN_INTERVAL_MS: u128 = 700;
/// Sleep `throttle_ms` every N walked entries to keep a full-drive scan gentle
/// on low-end CPUs.
const CLEANER_THROTTLE_EVERY: u64 = 2_500;
const CLEANER_THROTTLE_SLEEP_MS: u64 = 6;
const CLEANER_PROGRESS_EVENT: &str = "cleaner-analyze-progress";

#[derive(Debug, Clone)]
struct KnownCleanupTarget {
    id: String,
    label: String,
    category: String,
    path: PathBuf,
    description: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerAnalyzeOptions {
    #[serde(default = "default_true")]
    pub include_cache_scan: bool,
    #[serde(default = "default_true")]
    pub include_old_file_review: bool,
    pub old_file_days: Option<u64>,
    pub max_old_files: Option<usize>,
    /// Per-scan id so the frontend can cancel a long analysis mid-run
    /// (`cancel_cleaner_operation`) and correlate progress events. `None`
    /// disables cancellation + progress for that call (e.g. a headless caller).
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupTargetSummary {
    pub id: String,
    pub label: String,
    pub category: String,
    pub path: String,
    pub description: String,
    pub bytes: u64,
    pub file_count: u64,
    pub directory_count: u64,
    pub safe_to_clean: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OldLargeFileItem {
    pub path: String,
    pub file_name: String,
    pub bytes: u64,
    pub modified_ms: Option<u128>,
    pub accessed_ms: Option<u128>,
    pub days_since_modified: Option<u64>,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    pub name: String,
    pub source: String,
    pub command_or_path: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskSummary {
    pub path: String,
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub used_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySummary {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub used_percent: Option<f64>,
    pub source: String,
}

/// Quality Pass Wave 1 / CA-1 (2026-05-29): one row of the treemap
/// visualization on the Cleaner page. The frontend renders these as
/// proportionally-sized tiles ("what's eating my disk") so users can
/// see at a glance whether their biggest user folder is Documents,
/// Pictures, Downloads, or a sub-folder under one of those.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopUserDirEntry {
    pub path: String,
    /// Display name — the top-level folder name on its drive (e.g.
    /// "Program Files", "Users", "Steam").
    pub label: String,
    /// The drive root this folder lives on (e.g. "C:\"), so the UI can render
    /// "Program Files · C:\" and group rows by drive.
    pub parent_label: String,
    pub bytes: u64,
    pub file_count: u64,
}

/// Quality Pass Wave 1 / CA-2 (2026-05-29): one row of the file-type
/// breakdown panel. The frontend renders these as labeled bars so the
/// user can answer "is it videos, photos, or code chewing my disk?"
/// at a glance — complements the directory-level treemap (CA-1) which
/// answers "where is it?".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTypeBreakdownEntry {
    /// Bucket label — "Videos", "Images", "Audio", "Documents",
    /// "Code", "Archives", "Installers", "Disk images", "Other".
    pub category: String,
    pub bytes: u64,
    pub file_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerAnalyzeReport {
    pub generated_at_ms: u128,
    pub cleanup_targets: Vec<CleanupTargetSummary>,
    pub old_large_files: Vec<OldLargeFileItem>,
    pub startup_items: Vec<StartupItem>,
    pub disk_summaries: Vec<DiskSummary>,
    pub memory_summary: MemorySummary,
    /// CA-1 (whole-drive): the largest top-level folders across ALL fixed
    /// drives, sorted by total bytes desc and capped to top 40. Each row's
    /// `parent_label` is the drive root (e.g. "C:\"). Empty only if no fixed
    /// drive is enumerable.
    pub top_user_dirs: Vec<TopUserDirEntry>,
    /// Quality Pass Wave 1 / CA-2: file-type rollup across the user's home
    /// content folders ("my content"). Sorted by bytes desc; empty when no
    /// home folder is accessible. Categories are coarse and fixed (see
    /// `classify_extension`).
    pub file_type_breakdown: Vec<FileTypeBreakdownEntry>,
    pub total_cleanable_bytes: u64,
    pub tips: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerCleanPayload {
    pub target_ids: Vec<String>,
    pub confirm: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerCleanResultItem {
    pub id: String,
    pub label: String,
    pub path: String,
    pub deleted_bytes: u64,
    pub deleted_entries: u64,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanerCleanResult {
    pub success: bool,
    pub deleted_bytes: u64,
    pub deleted_entries: u64,
    pub results: Vec<CleanerCleanResultItem>,
}

#[derive(Default)]
struct DirSummary {
    bytes: u64,
    file_count: u64,
    directory_count: u64,
}

fn default_true() -> bool {
    true
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0)
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_string()
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

// ─── Cancellation (canonical static-set idiom, see hash.rs) ───────────

/// Operation ids the frontend asked to cancel. The analyze walk checks this
/// every ~256 entries and between stages, and bails promptly.
static CANCELLED_CLEANER_OPS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Cancel a running Cleaner analysis. Inserting the id makes the in-flight walk
/// return `Err("Cancelled")` at its next checkpoint. No-op if the scan already
/// finished. Sync + cheap.
#[tauri::command]
pub fn cancel_cleaner_operation(operation_id: String) -> Result<(), String> {
    CANCELLED_CLEANER_OPS
        .lock()
        .map_err(|_| "Cancel registry lock failed".to_string())?
        .insert(operation_id);
    Ok(())
}

fn is_cleaner_op_cancelled(op: &Option<String>) -> bool {
    op.as_ref()
        .and_then(|id| CANCELLED_CLEANER_OPS.lock().ok().map(|set| set.contains(id)))
        .unwrap_or(false)
}

fn clear_cleaner_op(op: &Option<String>) {
    if let Some(id) = op {
        if let Ok(mut set) = CANCELLED_CLEANER_OPS.lock() {
            set.remove(id);
        }
    }
}

// ─── Scan context: RAM-sized concurrency, throttle, cancel, progress ──

/// One coarse progress tick for the analyze walk. Stage-based (total entries
/// are unknown up front) plus a live `entriesScanned` counter + current path,
/// so the UI can show an honest determinate-by-stage bar with a live tail.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CleanerProgressEvent {
    operation_id: String,
    stage: String,
    stage_index: u32,
    stage_total: u32,
    entries_scanned: u64,
    current_path: String,
}

/// Shared state threaded through the analyze walks: the app handle (for
/// progress events), the op id (for cancellation), a shared entry counter, a
/// throttled last-emit timestamp, the per-entry CPU throttle, and the
/// RAM-sized worker count.
struct ScanCtx {
    app: AppHandle,
    op_id: Option<String>,
    entries: Arc<AtomicU64>,
    last_emit: Arc<Mutex<u128>>,
    throttle_ms: u64,
    workers: usize,
}

impl ScanCtx {
    fn cancelled(&self) -> bool {
        is_cleaner_op_cancelled(&self.op_id)
    }

    /// Call once per walked entry. Increments the counter, applies the periodic
    /// CPU throttle, and — every 256 entries — checks cancellation and emits a
    /// throttled progress event. Returns `true` when the caller should stop
    /// (the op was cancelled).
    fn on_entry(&self, stage: &str, stage_index: u32, current_path: &Path) -> bool {
        let n = self.entries.fetch_add(1, Ordering::Relaxed) + 1;
        if self.throttle_ms > 0 && n.is_multiple_of(CLEANER_THROTTLE_EVERY) {
            thread::sleep(Duration::from_millis(self.throttle_ms));
        }
        if n.is_multiple_of(256) {
            if self.cancelled() {
                return true;
            }
            self.maybe_emit(stage, stage_index, n, current_path);
        }
        false
    }

    /// Throttled progress emit (≥ 700 ms apart). Only stringifies the path when
    /// it actually emits, so the hot walk path allocates nothing per entry.
    fn maybe_emit(&self, stage: &str, stage_index: u32, entries: u64, current_path: &Path) {
        let Some(op) = self.op_id.clone() else {
            return;
        };
        let now = now_ms();
        let mut should = false;
        if let Ok(mut last) = self.last_emit.lock() {
            if now.saturating_sub(*last) >= CLEANER_PROGRESS_MIN_INTERVAL_MS {
                *last = now;
                should = true;
            }
        }
        if should {
            let _ = self.app.emit(
                CLEANER_PROGRESS_EVENT,
                CleanerProgressEvent {
                    operation_id: op,
                    stage: stage.to_string(),
                    stage_index,
                    stage_total: CLEANER_STAGE_TOTAL,
                    entries_scanned: entries,
                    current_path: current_path.to_string_lossy().into_owned(),
                },
            );
        }
    }

    /// Immediate stage marker (start of each stage) so the determinate bar
    /// advances even for fast stages. Bypasses the interval throttle.
    fn emit_stage(&self, stage: &str, stage_index: u32) {
        let Some(op) = self.op_id.clone() else {
            return;
        };
        let _ = self.app.emit(
            CLEANER_PROGRESS_EVENT,
            CleanerProgressEvent {
                operation_id: op,
                stage: stage.to_string(),
                stage_index,
                stage_total: CLEANER_STAGE_TOTAL,
                entries_scanned: self.entries.load(Ordering::Relaxed),
                current_path: String::new(),
            },
        );
    }
}

fn push_existing_target(
    targets: &mut Vec<KnownCleanupTarget>,
    seen: &mut HashSet<String>,
    id: &str,
    label: &str,
    category: &str,
    path: PathBuf,
    description: &str,
) {
    if !path.exists() || !path.is_dir() {
        return;
    }

    let key = fs::canonicalize(&path)
        .map(|value| path_string(&value))
        .unwrap_or_else(|_| path_string(&path))
        .to_lowercase();
    if !seen.insert(key) {
        return;
    }

    targets.push(KnownCleanupTarget {
        id: id.to_string(),
        label: label.to_string(),
        category: category.to_string(),
        path,
        description: description.to_string(),
    });
}

fn known_cleanup_targets() -> Vec<KnownCleanupTarget> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();

    push_existing_target(
        &mut targets,
        &mut seen,
        "system-temp",
        "Temporary files",
        "Windows / system",
        std::env::temp_dir(),
        "Current user temporary files. Locked files are skipped.",
    );

    if let Some(local_app_data) = env_path("LOCALAPPDATA") {
        push_existing_target(
            &mut targets,
            &mut seen,
            "local-temp",
            "Local app temp",
            "Windows / system",
            local_app_data.join("Temp"),
            "Per-user app temporary files.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "windows-inet-cache",
            "Windows internet cache",
            "Windows / system",
            local_app_data
                .join("Microsoft")
                .join("Windows")
                .join("INetCache"),
            "Windows web/cache files.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "crash-dumps",
            "Crash dumps",
            "Diagnostics",
            local_app_data.join("CrashDumps"),
            "Application crash dump files.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "d3d-cache",
            "DirectX shader cache",
            "Graphics cache",
            local_app_data.join("D3DSCache"),
            "Regeneratable DirectX shader cache.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "chrome-cache",
            "Chrome cache",
            "Browser cache",
            local_app_data
                .join("Google")
                .join("Chrome")
                .join("User Data")
                .join("Default")
                .join("Cache"),
            "Chrome page cache only; profile data is not touched.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "chrome-code-cache",
            "Chrome code cache",
            "Browser cache",
            local_app_data
                .join("Google")
                .join("Chrome")
                .join("User Data")
                .join("Default")
                .join("Code Cache"),
            "Chrome script cache only; profile data is not touched.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "edge-cache",
            "Edge cache",
            "Browser cache",
            local_app_data
                .join("Microsoft")
                .join("Edge")
                .join("User Data")
                .join("Default")
                .join("Cache"),
            "Edge page cache only; profile data is not touched.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "edge-code-cache",
            "Edge code cache",
            "Browser cache",
            local_app_data
                .join("Microsoft")
                .join("Edge")
                .join("User Data")
                .join("Default")
                .join("Code Cache"),
            "Edge script cache only; profile data is not touched.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "nvidia-dx-cache",
            "NVIDIA DX cache",
            "Graphics cache",
            local_app_data.join("NVIDIA").join("DXCache"),
            "Regeneratable NVIDIA DirectX shader cache.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "nvidia-gl-cache",
            "NVIDIA GL cache",
            "Graphics cache",
            local_app_data.join("NVIDIA").join("GLCache"),
            "Regeneratable NVIDIA OpenGL shader cache.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "amd-dx-cache",
            "AMD DX cache",
            "Graphics cache",
            local_app_data.join("AMD").join("DxCache"),
            "Regeneratable AMD DirectX shader cache.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "amd-gl-cache",
            "AMD GL cache",
            "Graphics cache",
            local_app_data.join("AMD").join("GLCache"),
            "Regeneratable AMD OpenGL shader cache.",
        );

        // Quality Pass Wave 1 / CA-3 (2026-05-28): dev/work app caches.
        // Each entry is independently and safely regeneratable — the
        // host app rebuilds the cache on next launch. Slack / Teams /
        // Discord caches are the noisiest "where did all my disk go?"
        // offenders on a typical work machine.
        push_existing_target(
            &mut targets,
            &mut seen,
            "vscode-cache",
            "VS Code cache",
            "Dev tools",
            local_app_data
                .join("Microsoft VS Code")
                .join("Code Cache"),
            "VS Code script + page cache. Regenerates on next launch.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "vscode-shader-cache",
            "VS Code shader cache",
            "Dev tools",
            local_app_data
                .join("Microsoft VS Code")
                .join("GPUCache"),
            "VS Code GPU shader cache. Regenerates on next launch.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "pip-cache",
            "pip cache",
            "Dev tools",
            local_app_data.join("pip").join("Cache"),
            "Python pip wheel/HTTP cache. Re-downloads on next install.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "yarn-cache",
            "Yarn cache",
            "Dev tools",
            local_app_data.join("Yarn").join("Cache"),
            "Yarn package cache. Re-downloads on next install.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "jetbrains-cache",
            "JetBrains IDE caches",
            "Dev tools",
            local_app_data.join("JetBrains"),
            "IntelliJ / PyCharm / Rider / WebStorm shared cache root. Regenerates on next launch.",
        );
    }

    if let Some(appdata) = env_path("APPDATA") {
        // Roaming app data lives in %APPDATA% (vs %LOCALAPPDATA%) for
        // these chat / npm / Code caches.
        push_existing_target(
            &mut targets,
            &mut seen,
            "slack-cache",
            "Slack cache",
            "Chat / collab",
            appdata.join("Slack").join("Cache"),
            "Slack desktop cache. Re-downloads images and avatars on next launch.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "discord-cache",
            "Discord cache",
            "Chat / collab",
            appdata.join("discord").join("Cache"),
            "Discord desktop cache. Regenerates on next launch.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "teams-cache",
            "Microsoft Teams cache",
            "Chat / collab",
            appdata.join("Microsoft").join("Teams").join("Cache"),
            "Teams desktop cache. Regenerates on next launch.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "npm-cache",
            "npm cache",
            "Dev tools",
            appdata.join("npm-cache"),
            "Node.js npm package cache. Re-downloads on next install.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "vscode-user-cache",
            "VS Code user cache",
            "Dev tools",
            appdata.join("Code").join("Cache"),
            "VS Code user-data cache. Regenerates on next launch.",
        );
    }

    if let Some(home) = home_dir() {
        // CA-3: language-toolchain caches under ~/. — these can grow
        // into multi-GB and rebuild from registries on next use.
        push_existing_target(
            &mut targets,
            &mut seen,
            "cargo-registry-cache",
            "Cargo registry cache",
            "Dev tools",
            home.join(".cargo").join("registry").join("cache"),
            "Rust cargo registry downloads. Re-downloaded on next build.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "gradle-caches",
            "Gradle caches",
            "Dev tools",
            home.join(".gradle").join("caches"),
            "Gradle build cache. Rebuilt on next Android/Java build.",
        );

        push_existing_target(
            &mut targets,
            &mut seen,
            "home-cache",
            "User cache",
            "User cache",
            home.join(".cache"),
            "Linux-style user cache folder. User documents are not touched.",
        );
        push_existing_target(
            &mut targets,
            &mut seen,
            "macos-user-cache",
            "User library caches",
            "User cache",
            home.join("Library").join("Caches"),
            "macOS user cache folder. User documents are not touched.",
        );
    }

    targets
}

/// Recursively sum bytes/files/dirs under `path`. Streaming: holds only the
/// running totals (three `u64`s), never the entry list — so memory is constant
/// regardless of how large the tree is. When `ctx` is `Some`, every entry ticks
/// the throttle/progress/cancel machinery; a cancelled scan stops early and
/// returns the partial total (the caller discards it).
fn summarize_directory_tracked(
    path: &Path,
    ctx: Option<&ScanCtx>,
    stage: &str,
    stage_index: u32,
) -> DirSummary {
    let mut summary = DirSummary::default();

    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if let Some(ctx) = ctx {
            if ctx.on_entry(stage, stage_index, entry.path()) {
                break;
            }
        }
        if entry.path() == path {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() {
            summary.bytes = summary.bytes.saturating_add(metadata.len());
            summary.file_count += 1;
        } else if metadata.is_dir() {
            summary.directory_count += 1;
        }
    }

    summary
}

/// Untracked sizing for paths off the analyze hot path (e.g. computing a cache
/// target's size just before deleting it in `clean_system_cache`).
fn summarize_directory(path: &Path) -> DirSummary {
    summarize_directory_tracked(path, None, "", 0)
}

/// True for a reparse point (symlink/junction/mount). Skipped at drive-root
/// enumeration so the treemap never double-counts a junction (e.g. the legacy
/// `C:\Documents and Settings` → `C:\Users`) or follows it into a cycle.
#[cfg(windows)]
fn is_reparse_point(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    fs::symlink_metadata(path)
        .map(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn is_reparse_point(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

fn home_dir() -> Option<PathBuf> {
    env_path("USERPROFILE").or_else(|| env_path("HOME"))
}

/// Quality Pass Wave 1 / CA-2 (2026-05-29): coarse file-type
/// classifier. Returns a stable bucket name for the per-file rollup;
/// "Other" is the catch-all for extensions we don't recognize and for
/// files with no extension at all.
fn classify_extension(ext: &str) -> &'static str {
    match ext {
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" | "flv" | "m4v" | "mpg" | "mpeg"
        | "ts" | "vob" | "3gp" => "Videos",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "tif" | "tiff" | "bmp" | "ico" | "svg"
        | "heic" | "heif" | "raw" | "cr2" | "nef" | "arw" | "dng" => "Images",
        "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" | "opus" | "wma" | "alac" | "aiff" => {
            "Audio"
        }
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "odt"
        | "ods" | "odp" | "rtf" | "csv" | "tsv" | "epub" | "mobi" => "Documents",
        // Source code + web assets. "ts" above clashes with MPEG ts;
        // the Videos arm gets first match, so .ts here would never be
        // hit anyway. Code .ts files are still classified as code via
        // the tsx / jsx / py / rs etc. siblings most repos contain.
        "rs" | "py" | "java" | "kt" | "swift" | "c" | "h" | "cpp" | "hpp" | "cc" | "cxx"
        | "cs" | "rb" | "php" | "go" | "js" | "jsx" | "tsx" | "vue" | "svelte" | "html"
        | "htm" | "css" | "scss" | "sass" | "json" | "toml" | "yaml" | "yml" | "xml"
        | "lua" | "sh" | "ps1" | "bat" | "cmd" | "sql" | "ipynb" => "Code",
        "zip" | "tar" | "gz" | "7z" | "rar" | "xz" | "bz2" | "zst" | "lz" | "lzma" => {
            "Archives"
        }
        "exe" | "msi" | "msu" | "appx" | "msix" | "deb" | "rpm" | "dmg" | "pkg"
        | "appimage" => "Installers",
        "iso" | "img" | "vhd" | "vhdx" | "vmdk" | "qcow2" => "Disk images",
        _ => "Other",
    }
}

/// Quality Pass Wave 1 / CA-2 (2026-05-29): walk the user's home content
/// folders and aggregate file sizes by coarse file-type bucket — answers "is it
/// videos, photos, or code chewing my disk?". Streaming: holds only one
/// `(bytes, count)` pair per fixed category bucket, so memory is constant
/// regardless of tree size. Ticks `ctx` per entry for throttle/progress/cancel.
///
/// Scope is the home content folders (the "my content" question), not all
/// drives — the all-drive walk is the treemap's job (stage 4), and walking
/// every drive twice would double the cost for little extra signal.
fn enumerate_file_type_breakdown(ctx: &ScanCtx) -> Vec<FileTypeBreakdownEntry> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    const TOP_FOLDERS: &[&str] = &[
        "Desktop",
        "Documents",
        "Downloads",
        "Pictures",
        "Videos",
        "Music",
        "OneDrive",
        "Dropbox",
    ];

    let mut sums: std::collections::HashMap<&'static str, (u64, u64)> =
        std::collections::HashMap::new();

    'folders: for folder_name in TOP_FOLDERS {
        let folder_path = home.join(folder_name);
        if !folder_path.exists() || !folder_path.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&folder_path)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if ctx.on_entry("Categorizing file types", 5, entry.path()) {
                break 'folders;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            let len = metadata.len();
            let ext = entry
                .path()
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            let category = classify_extension(&ext);
            let bucket = sums.entry(category).or_insert((0, 0));
            bucket.0 = bucket.0.saturating_add(len);
            bucket.1 = bucket.1.saturating_add(1);
        }
    }

    let mut out: Vec<FileTypeBreakdownEntry> = sums
        .into_iter()
        .map(|(category, (bytes, file_count))| FileTypeBreakdownEntry {
            category: category.to_string(),
            bytes,
            file_count,
        })
        .filter(|entry| entry.bytes > 0)
        .collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    out
}

/// Quality Pass Wave 1 / CA-1 — now whole-drive (2026-06): enumerate the
/// biggest folders **across all fixed drives** for the "what's eating my disk"
/// treemap. For each fixed drive (via `search::list_logical_drives()`), lists
/// its immediate child directories and sums each recursively, emitting one row
/// per `(drive, top-level folder)` with the drive as the parent label
/// (e.g. "Program Files · C:\"). Returns up to `max_count` rows, largest first.
///
/// No admin, no MFT — pure WalkDir. Streaming: only the running per-folder
/// `u64` sum is held during each summarize; the only accumulation is the
/// bounded result list (truncated to `max_count`). Reparse points (junctions /
/// symlinks) at the drive root are skipped so a legacy junction can't
/// double-count or cause a walk cycle. Ticks `ctx` per entry for
/// throttle/progress/cancel; returns whatever it has so far if cancelled.
fn enumerate_top_directories(max_count: usize, ctx: &ScanCtx) -> Vec<TopUserDirEntry> {
    let mut entries: Vec<TopUserDirEntry> = Vec::new();

    for drive in crate::commands::search::list_logical_drives() {
        if ctx.cancelled() {
            break;
        }
        let drive_path = PathBuf::from(&drive);
        let Ok(read_dir) = fs::read_dir(&drive_path) else {
            continue;
        };
        for child in read_dir.flatten() {
            if ctx.cancelled() {
                break;
            }
            let Ok(file_type) = child.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let subdir = child.path();
            // Skip hidden dotfolders and system roots ($Recycle.Bin,
            // $WinREAgent, System Volume Information starts with neither so it
            // is handled by the reparse/permission skips below).
            if let Some(name) = subdir.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') || name.starts_with('$') {
                    continue;
                }
            }
            // Never descend a junction/symlink at the drive root.
            if is_reparse_point(&subdir) {
                continue;
            }
            let summary = summarize_directory_tracked(&subdir, Some(ctx), "Mapping disk usage", 4);
            if summary.bytes == 0 {
                continue;
            }
            entries.push(TopUserDirEntry {
                path: path_string(&subdir),
                label: file_name(&subdir),
                // Drive root as the context label, e.g. "C:\".
                parent_label: drive.clone(),
                bytes: summary.bytes,
                file_count: summary.file_count,
            });
        }
    }

    // Largest first, then cap.
    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    entries.truncate(max_count);
    entries
}

fn user_review_roots() -> Vec<PathBuf> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    ["Desktop", "Downloads", "Documents", "Pictures", "Videos"]
        .iter()
        .map(|name| home.join(name))
        .filter(|path| path.exists() && path.is_dir())
        .collect()
}

fn system_roots() -> Vec<PathBuf> {
    // Wave 4.3a (2026-05-27): one entry per accessible logical drive,
    // via the existing list_logical_drives() enumerator (search.rs's
    // A-Z probe). The previous implementation mixed SystemDrive +
    // home + cwd as proxies for "user's drive" — that was a workaround
    // for not enumerating drives at all, and produced duplicate rows
    // (C:\, C:\Users\<user>, …) once we DO enumerate. Drop the
    // home/cwd proxies; users care about per-drive disk usage rows,
    // not per-folder. Empty optical / disconnected drive letters are
    // skipped by the enumerator (root must be a readable directory).
    crate::commands::search::list_logical_drives()
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect()
}

fn disk_summaries() -> Vec<DiskSummary> {
    system_roots()
        .into_iter()
        .map(|path| {
            let total = fs2::total_space(&path).ok();
            let available = fs2::available_space(&path).ok();
            let used_percent = match (total, available) {
                (Some(total), Some(available)) if total > 0 => {
                    Some(((total.saturating_sub(available)) as f64 / total as f64) * 100.0)
                }
                _ => None,
            };
            DiskSummary {
                path: path_string(&path),
                total_bytes: total,
                available_bytes: available,
                used_percent,
            }
        })
        .collect()
}

/// RAM summary via the shared `core::resources` sysinfo probe — cross-platform,
/// in-process, and instant (no PowerShell-CIM spawn / `/proc` parse). A failed
/// live-RAM probe reports `available = None` (the source labels it so).
fn memory_summary() -> MemorySummary {
    let total = resources::device_profile().total_ram_bytes;
    let avail = resources::available_ram_bytes();
    let total_bytes = (total > 0).then_some(total);
    // `available_ram_bytes()` returns `u64::MAX` when the probe failed.
    let available_bytes = (avail != u64::MAX).then_some(avail);
    let used_percent = match (total_bytes, available_bytes) {
        (Some(total), Some(available)) if total > 0 => {
            Some(((total.saturating_sub(available)) as f64 / total as f64) * 100.0)
        }
        _ => None,
    };
    MemorySummary {
        total_bytes,
        available_bytes,
        used_percent,
        source: "sysinfo".to_string(),
    }
}

fn ms_from_system_time(value: SystemTime) -> Option<u128> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|time| time.as_millis())
}

/// Roots to walk for the "old large files" review. On the system
/// drive we keep the focused 5-home-subfolder walk (Desktop / Downloads /
/// Documents / Pictures / Videos) — walking the whole C:\ would surface
/// noise from Windows + Program Files that isn't useful "review me"
/// content. On non-system drives we walk the entire drive root since
/// those are typically user-content drives (D:\Downloads, E:\Backups,
/// etc.); the per-walk scan cap prevents a giant drive from spinning
/// forever.
fn old_files_scan_roots() -> Vec<PathBuf> {
    let mut roots = user_review_roots();

    // System-drive prefix in upper-case (`"C:"`). Used to skip the
    // system drive from the full-tree walk pass — the focused
    // home-subfolder walk above already covers it.
    let system_prefix = std::env::var("SystemDrive")
        .ok()
        .map(|s| s.to_uppercase())
        .unwrap_or_else(|| "C:".to_string());

    for drive in crate::commands::search::list_logical_drives() {
        let upper = drive.to_uppercase();
        if upper.starts_with(&system_prefix) {
            continue;
        }
        roots.push(PathBuf::from(drive));
    }
    roots
}

/// Ordering wrapper so the old-large-files walk can keep a bounded min-heap of
/// the top-N largest candidates. Orders by `(bytes, path)` — the smallest
/// (least bytes, then lexically-first path) sorts first, so `BinaryHeap`'s
/// `Reverse` min-heap pops exactly the entry to evict when over capacity.
struct OldFileCandidate(OldLargeFileItem);

impl PartialEq for OldFileCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.0.bytes == other.0.bytes && self.0.path == other.0.path
    }
}
impl Eq for OldFileCandidate {}
impl PartialOrd for OldFileCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OldFileCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .bytes
            .cmp(&other.0.bytes)
            .then_with(|| self.0.path.cmp(&other.0.path))
    }
}

/// Find the largest files older than `days`, across all review roots, keeping
/// only the top `limit`. Streaming + bounded: each walker holds a min-heap of
/// at most `limit` items (evicting the smallest once full), so total memory is
/// `workers × limit` items — independent of drive size, no matter how many
/// millions of files are walked. Walk concurrency is sized to live free RAM
/// (`ctx.workers`); per-entry throttle/progress/cancel via `ctx`.
fn find_old_large_files(days: u64, limit: usize, ctx: &ScanCtx) -> Vec<OldLargeFileItem> {
    let now = SystemTime::now();
    let cutoff_ms = now_ms().saturating_sub(days as u128 * 24 * 60 * 60 * 1000);
    let roots = old_files_scan_roots();

    let walk = || {
        roots
            .par_iter()
            .map(|root| {
                let mut heap: BinaryHeap<Reverse<OldFileCandidate>> = BinaryHeap::new();
                for entry in WalkDir::new(root)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(Result::ok)
                {
                    if ctx.on_entry("Scanning large old files", 3, entry.path()) {
                        break;
                    }
                    let Ok(metadata) = entry.metadata() else {
                        continue;
                    };
                    if !metadata.is_file() || metadata.len() < OLD_FILE_MIN_BYTES {
                        continue;
                    }
                    let modified_ms = metadata.modified().ok().and_then(ms_from_system_time);
                    if modified_ms.map(|value| value > cutoff_ms).unwrap_or(true) {
                        continue;
                    }
                    let accessed_ms = metadata.accessed().ok().and_then(ms_from_system_time);
                    let days_since_modified = metadata
                        .modified()
                        .ok()
                        .and_then(|modified| now.duration_since(modified).ok())
                        .map(|duration| duration.as_secs() / 86_400);

                    heap.push(Reverse(OldFileCandidate(OldLargeFileItem {
                        path: path_string(entry.path()),
                        file_name: file_name(entry.path()),
                        bytes: metadata.len(),
                        modified_ms,
                        accessed_ms,
                        days_since_modified,
                        recommendation:
                            "Review only. KeepItLocal will not delete user files from Cleaner."
                                .to_string(),
                    })));
                    if heap.len() > limit {
                        heap.pop(); // evict the smallest — keep the top `limit`
                    }
                }
                heap.into_iter().map(|Reverse(c)| c.0).collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };

    // Size walk concurrency by live free RAM — only ever reduces under pressure.
    // Fall back to the global pool if a dedicated pool can't be built.
    let per_root: Vec<Vec<OldLargeFileItem>> = match rayon::ThreadPoolBuilder::new()
        .num_threads(ctx.workers.max(1))
        .build()
    {
        Ok(pool) => pool.install(walk),
        Err(_) => walk(),
    };

    let mut files: Vec<OldLargeFileItem> = per_root.into_iter().flatten().collect();
    files.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    files.truncate(limit);
    files
}

fn startup_folder_items() -> Vec<StartupItem> {
    let mut items = Vec::new();
    let mut folders = Vec::new();
    if let Some(app_data) = env_path("APPDATA") {
        folders.push((
            "Current user Startup folder",
            app_data
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Startup"),
        ));
    }
    if let Some(program_data) = env_path("PROGRAMDATA") {
        folders.push((
            "All users Startup folder",
            program_data
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Startup"),
        ));
    }

    for (source, folder) in folders {
        let Ok(read_dir) = fs::read_dir(folder) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            items.push(StartupItem {
                name: file_name(&path),
                source: source.to_string(),
                command_or_path: path_string(&path),
                recommendation: "Review in Windows Startup Apps before disabling.".to_string(),
            });
        }
    }

    items
}

#[cfg(windows)]
fn startup_registry_items() -> Vec<StartupItem> {
    let mut items = Vec::new();
    for (source, key) in [
        (
            "HKCU Run",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
        ),
        (
            "HKLM Run",
            r"HKLM\Software\Microsoft\Windows\CurrentVersion\Run",
        ),
    ] {
        let output = Command::new("reg")
            .args(["query", key])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        let Ok(output) = output else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let raw = String::from_utf8_lossy(&output.stdout);
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("HKEY_") {
                continue;
            }
            let parts = line
                .split("    ")
                .filter(|part| !part.trim().is_empty())
                .collect::<Vec<_>>();
            if parts.len() < 3 {
                continue;
            }
            items.push(StartupItem {
                name: parts[0].trim().to_string(),
                source: source.to_string(),
                command_or_path: parts[2..].join(" ").trim().to_string(),
                recommendation: "Review in Windows Startup Apps before disabling.".to_string(),
            });
        }
    }
    items
}

#[cfg(not(windows))]
fn startup_registry_items() -> Vec<StartupItem> {
    Vec::new()
}

/// Combined startup inventory (Startup folders + HKCU/HKLM Run keys). Public so
/// the Privacy Audit can reuse it for its startup-programs scan rather than
/// re-enumerating the same sources.
pub fn startup_items() -> Vec<StartupItem> {
    let mut items = startup_folder_items();
    items.extend(startup_registry_items());
    items.sort_by(|a, b| a.source.cmp(&b.source).then_with(|| a.name.cmp(&b.name)));
    items
}

fn build_tips(
    cleanup_targets: &[CleanupTargetSummary],
    old_files: &[OldLargeFileItem],
    startup_items: &[StartupItem],
    disks: &[DiskSummary],
    memory: &MemorySummary,
) -> Vec<String> {
    let mut tips = Vec::new();
    let cleanable: u64 = cleanup_targets.iter().map(|target| target.bytes).sum();
    if cleanable > GB {
        tips.push("Caches are using more than 1 GB. Preview selected cache groups, close browsers/apps, then clean with CONFIRM.".to_string());
    } else {
        tips.push(
            "Cache cleanup looks modest. Cleaning is optional unless you need quick space."
                .to_string(),
        );
    }
    if old_files.len() >= 10 {
        tips.push("Several large files have not been modified for a long time. Review them manually; KeepItLocal will not delete user files here.".to_string());
    }
    if startup_items.len() >= 8 {
        tips.push("Startup list is busy. Review startup apps in Windows Settings to reduce boot time and background RAM usage.".to_string());
    }
    if disks
        .iter()
        .any(|disk| disk.used_percent.unwrap_or(0.0) >= 85.0)
    {
        tips.push("One disk is above 85% used. Prioritize large old downloads, videos, installers, and cache cleanup.".to_string());
    }
    if memory.used_percent.unwrap_or(0.0) >= 85.0 {
        tips.push("RAM usage is high right now. Check startup apps and heavy background apps before deleting files.".to_string());
    }
    tips
}

#[tauri::command]
pub async fn analyze_system_cleaner(
    app: AppHandle,
    options: CleanerAnalyzeOptions,
) -> Result<CleanerAnalyzeReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let op_id = options.operation_id.clone();

        // Size walk concurrency to live free RAM (only ever reduces under
        // pressure). Shared with the rayon old-files pool via `ctx.workers`.
        let workers = resources::recommend_workers_with(
            resources::cpu_cores(),
            resources::available_ram_bytes(),
            resources::WALK_WORKER_RAM_BYTES,
        );
        let ctx = ScanCtx {
            app,
            op_id: op_id.clone(),
            entries: Arc::new(AtomicU64::new(0)),
            last_emit: Arc::new(Mutex::new(0)),
            throttle_ms: CLEANER_THROTTLE_SLEEP_MS,
            workers,
        };

        // Cancel checkpoint helper — clears the op id and returns the canonical
        // cancelled error so the frontend treats it as a cancel, not a failure.
        macro_rules! bail_if_cancelled {
            () => {
                if is_cleaner_op_cancelled(&op_id) {
                    clear_cleaner_op(&op_id);
                    return Err("Cancelled".to_string());
                }
            };
        }

        // ── Stage 1: drives + memory ──────────────────────────────────
        ctx.emit_stage("Reading drives & memory", 1);
        bail_if_cancelled!();
        let disk_summaries = disk_summaries();
        let memory_summary = memory_summary();

        // ── Stage 2: cache / temp sizing ──────────────────────────────
        ctx.emit_stage("Sizing cache & temp", 2);
        let cleanup_targets = if options.include_cache_scan {
            known_cleanup_targets()
                .into_iter()
                .map(|target| {
                    let summary = summarize_directory_tracked(
                        &target.path,
                        Some(&ctx),
                        "Sizing cache & temp",
                        2,
                    );
                    CleanupTargetSummary {
                        id: target.id,
                        label: target.label,
                        category: target.category,
                        path: path_string(&target.path),
                        description: target.description,
                        bytes: summary.bytes,
                        file_count: summary.file_count,
                        directory_count: summary.directory_count,
                        safe_to_clean: true,
                    }
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        bail_if_cancelled!();

        // ── Stage 3: large old files (streamed, bounded heap) ─────────
        ctx.emit_stage("Scanning large old files", 3);
        let old_large_files = if options.include_old_file_review {
            find_old_large_files(
                options
                    .old_file_days
                    .unwrap_or(DEFAULT_OLD_FILE_DAYS)
                    .max(30),
                options
                    .max_old_files
                    .unwrap_or(DEFAULT_MAX_OLD_FILES)
                    .clamp(10, 200),
                &ctx,
            )
        } else {
            Vec::new()
        };
        bail_if_cancelled!();

        // ── Stage 4: all-fixed-drive disk-usage map ───────────────────
        ctx.emit_stage("Mapping disk usage", 4);
        // 40 rows is the sweet spot between "see everything" and "tile labels
        // become illegible" once the treemap spans every drive.
        let top_user_dirs = enumerate_top_directories(40, &ctx);
        bail_if_cancelled!();

        // ── Stage 5: file-type breakdown (home content) ───────────────
        ctx.emit_stage("Categorizing file types", 5);
        let file_type_breakdown = enumerate_file_type_breakdown(&ctx);
        bail_if_cancelled!();

        let startup_items = startup_items();
        let total_cleanable_bytes = cleanup_targets.iter().map(|target| target.bytes).sum();
        let tips = build_tips(
            &cleanup_targets,
            &old_large_files,
            &startup_items,
            &disk_summaries,
            &memory_summary,
        );

        clear_cleaner_op(&op_id);
        Ok(CleanerAnalyzeReport {
            generated_at_ms: now_ms(),
            cleanup_targets,
            old_large_files,
            startup_items,
            disk_summaries,
            memory_summary,
            top_user_dirs,
            file_type_breakdown,
            total_cleanable_bytes,
            tips,
        })
    })
    .await
    .map_err(|error| format!("Cleaner analysis worker failed: {error}"))?
}

fn delete_child(path: &Path) -> Result<(u64, u64), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Cannot inspect cleanup entry: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err("Skipped symlink or junction inside cleanup target.".to_string());
    }

    let bytes = if metadata.is_dir() {
        summarize_directory(path).bytes
    } else {
        metadata.len()
    };

    if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(|error| format!("Cannot remove directory: {error}"))?;
    } else {
        fs::remove_file(path).map_err(|error| format!("Cannot remove file: {error}"))?;
    }

    Ok((bytes, 1))
}

fn clean_target_contents(target: &KnownCleanupTarget) -> CleanerCleanResultItem {
    let mut deleted_bytes = 0u64;
    let mut deleted_entries = 0u64;
    let mut first_error = None;

    let read_dir = match fs::read_dir(&target.path) {
        Ok(value) => value,
        Err(error) => {
            return CleanerCleanResultItem {
                id: target.id.clone(),
                label: target.label.clone(),
                path: path_string(&target.path),
                deleted_bytes: 0,
                deleted_entries: 0,
                success: false,
                error: Some(format!("Cannot read cleanup target: {error}")),
            };
        }
    };

    for entry in read_dir.flatten() {
        match delete_child(&entry.path()) {
            Ok((bytes, entries)) => {
                deleted_bytes = deleted_bytes.saturating_add(bytes);
                deleted_entries = deleted_entries.saturating_add(entries);
            }
            Err(error) => {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }

    CleanerCleanResultItem {
        id: target.id.clone(),
        label: target.label.clone(),
        path: path_string(&target.path),
        deleted_bytes,
        deleted_entries,
        success: first_error.is_none(),
        error: first_error,
    }
}

#[tauri::command]
pub async fn clean_system_cache(
    payload: CleanerCleanPayload,
) -> Result<CleanerCleanResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if payload.confirm.trim() != "CONFIRM" {
            return Err("Type CONFIRM before cleaning cache targets.".to_string());
        }

        let requested = payload.target_ids.into_iter().collect::<HashSet<_>>();
        if requested.is_empty() {
            return Err("Select at least one cleanup target.".to_string());
        }

        let catalog = known_cleanup_targets()
            .into_iter()
            .map(|target| (target.id.clone(), target))
            .collect::<HashMap<_, _>>();

        let mut results = Vec::new();
        for target_id in requested {
            let Some(target) = catalog.get(&target_id) else {
                results.push(CleanerCleanResultItem {
                    id: target_id,
                    label: "Unknown target".to_string(),
                    path: String::new(),
                    deleted_bytes: 0,
                    deleted_entries: 0,
                    success: false,
                    error: Some("Cleanup target is not allowed.".to_string()),
                });
                continue;
            };
            results.push(clean_target_contents(target));
        }

        let deleted_bytes = results.iter().map(|item| item.deleted_bytes).sum();
        let deleted_entries = results.iter().map(|item| item.deleted_entries).sum();
        let success = results.iter().all(|item| item.success);

        Ok(CleanerCleanResult {
            success,
            deleted_bytes,
            deleted_entries,
            results,
        })
    })
    .await
    .map_err(|error| format!("Cleaner worker failed: {error}"))?
}
