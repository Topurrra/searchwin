//! Dual-pane File Manager backend.
//!
//! Provides directory listing, copy / move / delete-to-recycle, rename,
//! make-dir, and a robocopy-wrapped backup. Every write operation is guarded
//! against OS-critical / whole-drive locations via `is_dangerous_scan_root`
//! (shared with the rest of the file tools) so the manager can never be
//! pointed at `C:\Windows`, `Program Files`, a drive root, etc.
//!
//! Long-running ops (copy / move / backup) run on a worker thread
//! (`#[tauri::command(async)]`), emit throttled `fm-progress` events, and are
//! cancellable through a per-operation id flag (`fm_cancel`).
//!
//! Recycle-bin deletion uses the Windows `IFileOperation` shell API so files
//! are recoverable (sent to the bin, not unlinked). All Windows-specific code
//! is gated with `#[cfg(windows)]`; non-Windows builds get `Err` stubs so the
//! crate still compiles cross-platform.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Instant, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::commands::files::{describe_io_error, is_dangerous_scan_root};

// ---------------------------------------------------------------------------
// Shared types (serde camelCase — must match the JS command contract exactly).
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FmEntry {
    name: String,
    /// Full absolute path.
    path: String,
    is_dir: bool,
    size: u64,
    modified_ms: i64,
    hidden: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FmListing {
    entries: Vec<FmEntry>,
    truncated: bool,
    total: usize,
    /// Parent directory path, or `None` at a drive root.
    parent: Option<String>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct FmOpResult {
    copied: usize,
    skipped: usize,
    failed: usize,
    errors: Vec<String>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct FmDeleteResult {
    deleted: usize,
    errors: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FmProgress {
    operation_id: String,
    done: u64,
    total: u64,
    current_path: String,
}

const MAX_ENTRIES: usize = 5000;

// ---------------------------------------------------------------------------
// Per-operation cancel registry.
// ---------------------------------------------------------------------------

/// Operation ids the frontend has asked to cancel. A running op polls this set
/// by its `operation_id`; `fm_cancel` inserts an id. Mirrors the shredder's
/// CancelFlag idea but keyed per-operation so several ops can run at once.
static CANCELLED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

fn is_cancelled(operation_id: &str) -> bool {
    CANCELLED
        .lock()
        .map(|set| set.contains(operation_id))
        .unwrap_or(false)
}

/// Clear an op's cancel flag (call at the start so a stale id from a previous
/// run can't insta-cancel a new op that happens to reuse the id).
fn clear_cancel(operation_id: &str) {
    if let Ok(mut set) = CANCELLED.lock() {
        set.remove(operation_id);
    }
}

#[tauri::command]
pub fn fm_cancel(operation_id: String) {
    if let Ok(mut set) = CANCELLED.lock() {
        set.insert(operation_id);
    }
}

// ---------------------------------------------------------------------------
// Throttled progress emitter (~150ms, like cleaner.rs).
// ---------------------------------------------------------------------------

struct ProgressEmitter<'a> {
    app: &'a AppHandle,
    operation_id: &'a str,
    last_emit: Instant,
}

impl<'a> ProgressEmitter<'a> {
    fn new(app: &'a AppHandle, operation_id: &'a str) -> Self {
        Self {
            app,
            operation_id,
            // Force the first emit immediately.
            last_emit: Instant::now() - std::time::Duration::from_millis(1000),
        }
    }

    fn emit(&mut self, done: u64, total: u64, current_path: &str, force: bool) {
        if !force && self.last_emit.elapsed().as_millis() < 150 {
            return;
        }
        self.last_emit = Instant::now();
        let _ = self.app.emit(
            "fm-progress",
            FmProgress {
                operation_id: self.operation_id.to_string(),
                done,
                total,
                current_path: current_path.to_string(),
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Guards.
// ---------------------------------------------------------------------------

/// Reject a write target that is, or sits under, an OS-critical / whole-drive
/// location. Used for every destination, delete target, rename target, and
/// make-dir parent.
fn guard_write(path: &Path) -> Result<(), String> {
    // Reject any unresolved `..` segment up front: the shared guard is lexical,
    // so `C:\Users\Public\..\..\Windows` would slip past the c:\users allowlist
    // while the OS still resolves the `..` to a system dir.
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!(
            "Refusing a path with '..' segments: {}. Use a direct folder path.",
            path.display()
        ));
    }
    // Check the CANONICAL path too (resolves symlinks/junctions, stripping the
    // \\?\ prefix Windows adds) so a junction under the user profile pointing at
    // a system dir can't bypass the lexical guard. Falls back to the raw path
    // when it doesn't exist yet.
    let resolved = std::fs::canonicalize(path)
        .map(|p| {
            let s = p.to_string_lossy().to_string();
            PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
        })
        .unwrap_or_else(|_| path.to_path_buf());
    if is_dangerous_scan_root(path) || is_dangerous_scan_root(&resolved) {
        return Err(format!(
            "Refusing to modify a system or whole-drive location: {}. Choose a normal user folder instead.",
            path.display()
        ));
    }
    Ok(())
}

/// True if `inner` is the same as, or nested inside, `outer` (both resolved).
/// Stops a copy/move that would write a folder into itself or a descendant —
/// which would otherwise recurse forever into the growing copy.
fn path_within_or_equal(inner: &Path, outer: &Path) -> bool {
    let ci = std::fs::canonicalize(inner).unwrap_or_else(|_| inner.to_path_buf());
    let co = std::fs::canonicalize(outer).unwrap_or_else(|_| outer.to_path_buf());
    ci == co || ci.starts_with(&co)
}

// ---------------------------------------------------------------------------
// 1. fm_list_dir
// ---------------------------------------------------------------------------

#[tauri::command(async)]
pub fn fm_list_dir(
    path: String,
    show_hidden: bool,
    sort_by: String,
) -> Result<FmListing, String> {
    let dir = PathBuf::from(&path);
    let read_dir =
        std::fs::read_dir(&dir).map_err(|e| describe_io_error("Listing folder", &dir, &e))?;

    let mut entries: Vec<FmEntry> = Vec::new();
    let mut total: usize = 0;

    for entry in read_dir.flatten() {
        let entry_path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();

        // file_type() is cheap (no extra stat on Windows for the dir flag).
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let is_dir = file_type.is_dir();
        let metadata = entry.metadata().ok();

        let hidden = is_hidden(&name, metadata.as_ref());
        if hidden && !show_hidden {
            // Hidden entries that the user hasn't opted into are excluded from
            // the listing AND from the total count, so "total" reflects what
            // would have been shown.
            continue;
        }

        total += 1;
        if entries.len() >= MAX_ENTRIES {
            // Keep counting (for an accurate `total`) but stop collecting.
            continue;
        }

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

        entries.push(FmEntry {
            name,
            path: entry_path.to_string_lossy().into_owned(),
            is_dir,
            size,
            modified_ms,
            hidden,
        });
    }

    sort_entries(&mut entries, &sort_by);

    let truncated = total > entries.len();
    let parent = dir
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty());

    Ok(FmListing {
        entries,
        truncated,
        total,
        parent,
    })
}

// ---------------------------------------------------------------------------
// 1b. fm_dir_size — recursive folder size (background, cancellable)
// ---------------------------------------------------------------------------

/// Total size (bytes) of a folder's contents, walked recursively. Runs async
/// (off the UI thread) and bails promptly when `operation_id` is cancelled via
/// `fm_cancel` — the File Manager computes folder sizes in the background AFTER a
/// listing (so the pane never blocks) and cancels the whole batch on navigation.
/// Symlinks are NOT followed (avoids cycles + double-counting); unreadable
/// subfolders are skipped, not fatal.
#[tauri::command(async)]
pub fn fm_dir_size(path: String, operation_id: String) -> Result<u64, String> {
    let root = PathBuf::from(&path);
    let meta = std::fs::symlink_metadata(&root)
        .map_err(|e| describe_io_error("Sizing folder", &root, &e))?;
    if meta.file_type().is_symlink() {
        return Ok(0); // never traverse a symlinked directory
    }
    if !meta.is_dir() {
        return Ok(meta.len());
    }
    Ok(walk_dir_size(&root, &operation_id))
}

/// Iterative (stack-based) recursive byte sum. Iterative avoids a deep-recursion
/// stack overflow on pathological trees; the cancel flag is polled both per
/// directory and every few thousand entries so a huge folder stops quickly once
/// the user navigates away.
fn walk_dir_size(root: &Path, operation_id: &str) -> u64 {
    let mut total: u64 = 0;
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    let mut counter: u64 = 0;
    while let Some(dir) = stack.pop() {
        if is_cancelled(operation_id) {
            break;
        }
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue; // permission denied / vanished — skip this subtree
        };
        for entry in read_dir.flatten() {
            counter += 1;
            if counter % 4096 == 0 && is_cancelled(operation_id) {
                return total;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue; // never follow symlinks (cycles + double-counting)
            }
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if let Ok(m) = entry.metadata() {
                total = total.saturating_add(m.len());
            }
        }
    }
    total
}

// ---------------------------------------------------------------------------
// 1c. fm_path_suggestions — autocomplete for the path bar
// ---------------------------------------------------------------------------

/// Folder autocomplete for the path input: given a (possibly partial) path,
/// return matching child DIRECTORY paths (the bar navigates folders only). The
/// input is split into a parent dir + a partial leaf; the parent is listed and
/// the leaf prefix-matched case-insensitively. One shallow `read_dir`, no
/// recursion — cheap enough to call on each keystroke. Empty on any error.
#[tauri::command(async)]
pub fn fm_path_suggestions(input: String) -> Vec<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    // A trailing separator means "list this dir's children"; otherwise the last
    // segment is the prefix to match within the parent.
    let (dir, prefix): (PathBuf, String) = if trimmed.ends_with('\\') || trimmed.ends_with('/') {
        (PathBuf::from(trimmed), String::new())
    } else {
        let p = PathBuf::from(trimmed);
        match p.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => (
                parent.to_path_buf(),
                p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            ),
            _ => return Vec::new(), // bare drive ("C:") / no parent yet
        }
    };

    let prefix_lower = prefix.to_lowercase();
    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for entry in read_dir.flatten() {
        if out.len() >= 200 {
            break; // bound work on huge directories
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue; // only folders navigate from the path bar
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if prefix_lower.is_empty() || name.to_lowercase().starts_with(&prefix_lower) {
            out.push(entry.path().to_string_lossy().into_owned());
        }
    }
    out.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    out.truncate(12);
    out
}

/// Dirs always first; then by the requested key. Name is the tiebreak.
fn sort_entries(entries: &mut [FmEntry], sort_by: &str) {
    entries.sort_by(|a, b| {
        // Directories before files.
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| match sort_by {
                "size" => a.size.cmp(&b.size),
                "modified" => a.modified_ms.cmp(&b.modified_ms),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

#[cfg(windows)]
fn is_hidden(name: &str, metadata: Option<&std::fs::Metadata>) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    if name.starts_with('.') {
        return true;
    }
    metadata
        .map(|m| m.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn is_hidden(name: &str, _metadata: Option<&std::fs::Metadata>) -> bool {
    name.starts_with('.')
}

// ---------------------------------------------------------------------------
// Conflict resolution helpers.
// ---------------------------------------------------------------------------

/// Produce a non-colliding destination path by appending " (2)", " (3)"…
/// before the extension, matching Explorer's rename-on-conflict behaviour.
fn rename_destination(dest: &Path) -> PathBuf {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    let stem = dest
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = dest.extension().map(|e| e.to_string_lossy().into_owned());

    for n in 2..10_000 {
        let candidate_name = match &ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = parent.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    // Pathological fallback — extremely unlikely to be reached.
    parent.join(format!("{stem} (copy)"))
}

/// Decide the destination for a source given the conflict policy.
/// Returns `Ok(None)` to mean "skip this source".
fn resolve_conflict(dest: &Path, on_conflict: &str) -> Result<Option<PathBuf>, String> {
    if !dest.exists() {
        return Ok(Some(dest.to_path_buf()));
    }
    match on_conflict {
        "skip" => Ok(None),
        "overwrite" => Ok(Some(dest.to_path_buf())),
        "rename" => Ok(Some(rename_destination(dest))),
        other => Err(format!("Unknown conflict policy: {other}")),
    }
}

/// Count the files under a set of sources, for a progress denominator.
/// Best-effort: unreadable entries are simply not counted.
fn count_files(sources: &[String]) -> u64 {
    let mut total: u64 = 0;
    for s in sources {
        let p = PathBuf::from(s);
        if p.is_dir() {
            for entry in walkdir::WalkDir::new(&p).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    total += 1;
                }
            }
        } else if p.exists() {
            total += 1;
        }
    }
    total.max(1)
}

// ---------------------------------------------------------------------------
// 2. fm_copy
// ---------------------------------------------------------------------------

#[tauri::command(async)]
pub fn fm_copy(
    app: AppHandle,
    sources: Vec<String>,
    dest_dir: String,
    on_conflict: String,
    operation_id: String,
) -> Result<FmOpResult, String> {
    clear_cancel(&operation_id);
    let dest_root = PathBuf::from(&dest_dir);
    guard_write(&dest_root)?;
    if !dest_root.is_dir() {
        return Err("Destination is not a folder.".into());
    }

    let total = count_files(&sources);
    let mut emitter = ProgressEmitter::new(&app, &operation_id);
    let mut done: u64 = 0;
    let mut result = FmOpResult::default();

    for source in &sources {
        if is_cancelled(&operation_id) {
            return Err("Cancelled".into());
        }
        let src = PathBuf::from(source);
        let Some(name) = src.file_name() else {
            result.failed += 1;
            result.errors.push(format!("Invalid source path: {source}"));
            continue;
        };
        // Don't copy a folder into itself or a descendant (runaway recursion).
        if path_within_or_equal(&dest_root, &src) {
            result.failed += 1;
            result
                .errors
                .push(format!("Cannot copy \"{source}\" into itself."));
            continue;
        }
        let dest = dest_root.join(name);

        match resolve_conflict(&dest, &on_conflict) {
            Ok(None) => {
                result.skipped += 1;
                continue;
            }
            Ok(Some(target)) => {
                copy_one(
                    &src,
                    &target,
                    &operation_id,
                    &mut emitter,
                    &mut done,
                    total,
                    &mut result,
                )?;
            }
            Err(e) => {
                result.failed += 1;
                result.errors.push(e);
            }
        }
    }

    emitter.emit(done, total, "", true);
    clear_cancel(&operation_id);
    Ok(result)
}

/// Copy a single source (file or dir, recursive) to an already-resolved
/// destination path. Updates `result` and `done`. Returns `Err("Cancelled")`
/// only if the user cancelled mid-walk.
fn copy_one(
    src: &Path,
    dest: &Path,
    operation_id: &str,
    emitter: &mut ProgressEmitter,
    done: &mut u64,
    total: u64,
    result: &mut FmOpResult,
) -> Result<(), String> {
    let meta = match std::fs::symlink_metadata(src) {
        Ok(m) => m,
        Err(e) => {
            result.failed += 1;
            result
                .errors
                .push(describe_io_error("Copy", src, &e));
            return Ok(());
        }
    };

    if meta.is_dir() {
        copy_dir_recursive(src, dest, operation_id, emitter, done, total, result)
    } else {
        if is_cancelled(operation_id) {
            return Err("Cancelled".into());
        }
        emitter.emit(*done, total, &src.to_string_lossy(), false);
        match std::fs::copy(src, dest) {
            Ok(_) => {
                result.copied += 1;
                *done += 1;
            }
            Err(e) => {
                result.failed += 1;
                result.errors.push(describe_io_error("Copy", src, &e));
            }
        }
        Ok(())
    }
}

fn copy_dir_recursive(
    src: &Path,
    dest: &Path,
    operation_id: &str,
    emitter: &mut ProgressEmitter,
    done: &mut u64,
    total: u64,
    result: &mut FmOpResult,
) -> Result<(), String> {
    if let Err(e) = std::fs::create_dir_all(dest) {
        result.failed += 1;
        result
            .errors
            .push(describe_io_error("Create folder", dest, &e));
        return Ok(());
    }

    let read = match std::fs::read_dir(src) {
        Ok(r) => r,
        Err(e) => {
            result.failed += 1;
            result.errors.push(describe_io_error("Read folder", src, &e));
            return Ok(());
        }
    };

    for entry in read.flatten() {
        if is_cancelled(operation_id) {
            return Err("Cancelled".into());
        }
        let child = entry.path();
        let child_dest = dest.join(entry.file_name());
        let Ok(ft) = entry.file_type() else { continue };

        if ft.is_dir() {
            copy_dir_recursive(&child, &child_dest, operation_id, emitter, done, total, result)?;
        } else {
            emitter.emit(*done, total, &child.to_string_lossy(), false);
            match std::fs::copy(&child, &child_dest) {
                Ok(_) => {
                    result.copied += 1;
                    *done += 1;
                }
                Err(e) => {
                    result.failed += 1;
                    result.errors.push(describe_io_error("Copy", &child, &e));
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. fm_move
// ---------------------------------------------------------------------------

#[tauri::command(async)]
pub fn fm_move(
    app: AppHandle,
    sources: Vec<String>,
    dest_dir: String,
    on_conflict: String,
    operation_id: String,
) -> Result<FmOpResult, String> {
    clear_cancel(&operation_id);
    let dest_root = PathBuf::from(&dest_dir);
    guard_write(&dest_root)?;
    if !dest_root.is_dir() {
        return Err("Destination is not a folder.".into());
    }

    let total = count_files(&sources);
    let mut emitter = ProgressEmitter::new(&app, &operation_id);
    let mut done: u64 = 0;
    let mut result = FmOpResult::default();

    for source in &sources {
        if is_cancelled(&operation_id) {
            return Err("Cancelled".into());
        }
        let src = PathBuf::from(source);

        // Moving a source means deleting the source — guard it too.
        if let Err(e) = guard_write(&src) {
            result.failed += 1;
            result.errors.push(e);
            continue;
        }

        let Some(name) = src.file_name() else {
            result.failed += 1;
            result.errors.push(format!("Invalid source path: {source}"));
            continue;
        };
        // Don't move a folder into itself or a descendant.
        if path_within_or_equal(&dest_root, &src) {
            result.failed += 1;
            result
                .errors
                .push(format!("Cannot move \"{source}\" into itself."));
            continue;
        }
        let dest = dest_root.join(name);

        let target = match resolve_conflict(&dest, &on_conflict) {
            Ok(None) => {
                result.skipped += 1;
                continue;
            }
            Ok(Some(t)) => t,
            Err(e) => {
                result.failed += 1;
                result.errors.push(e);
                continue;
            }
        };

        emitter.emit(done, total, &src.to_string_lossy(), false);

        // Fast path: same-volume rename. On overwrite we must clear the target
        // first because fs::rename won't replace a non-empty dir / differing
        // type. Try rename; if it fails with a cross-device error, fall back to
        // copy-then-delete.
        if on_conflict == "overwrite" && target.exists() {
            let _ = remove_path(&target);
        }

        // Count the source's files BEFORE the rename moves it away (afterwards
        // the path no longer exists and count would read 0).
        let move_count = count_files(std::slice::from_ref(source));

        match std::fs::rename(&src, &target) {
            Ok(_) => {
                result.copied += 1;
                done += move_count;
            }
            Err(e) => {
                // Cross-volume (ERROR_NOT_SAME_DEVICE = 17) → copy then delete.
                let cross_volume = e.raw_os_error() == Some(17)
                    || e.kind() == std::io::ErrorKind::CrossesDevices;
                if cross_volume {
                    let before_failed = result.failed;
                    copy_one(
                        &src,
                        &target,
                        &operation_id,
                        &mut emitter,
                        &mut done,
                        total,
                        &mut result,
                    )?;
                    // Only delete the source if the copy didn't add failures.
                    if result.failed == before_failed {
                        if let Err(del) = remove_path(&src) {
                            result
                                .errors
                                .push(describe_io_error("Remove source after move", &src, &del));
                        }
                    }
                } else {
                    result.failed += 1;
                    result.errors.push(describe_io_error("Move", &src, &e));
                }
            }
        }
    }

    emitter.emit(done, total, "", true);
    clear_cancel(&operation_id);
    Ok(result)
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

// ---------------------------------------------------------------------------
// 4. fm_delete_to_recycle  (Windows Recycle Bin via IFileOperation)
// ---------------------------------------------------------------------------

#[tauri::command(async)]
pub fn fm_delete_to_recycle(paths: Vec<String>) -> Result<FmDeleteResult, String> {
    let mut result = FmDeleteResult::default();

    // Guard every target before touching the shell API.
    for p in &paths {
        let path = PathBuf::from(p);
        if let Err(e) = guard_write(&path) {
            result.errors.push(e);
        }
    }
    if !result.errors.is_empty() {
        return Err(result.errors.join("\n"));
    }

    recycle_paths(&paths, &mut result);
    Ok(result)
}

#[cfg(windows)]
fn recycle_paths(paths: &[String], result: &mut FmDeleteResult) {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
        FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_SILENT,
        FOF_WANTNUKEWARNING,
    };

    unsafe {
        // Per-thread STA init; pair with CoUninitialize at the end. RPC_E_CHANGED_MODE
        // (already initialized in another apartment) is non-fatal — the shell
        // operation works regardless, and we only CoUninitialize if WE initialized.
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let we_initialized = init.is_ok();

        let mut perform = || -> windows::core::Result<()> {
            let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
            // The page already asked "to the Recycle Bin?". An item that
            // can't go there (too big, or a drive without one) would be
            // destroyed silently under FOF_NOCONFIRMATION; the nuke warning
            // asks again for exactly those, as Explorer does.
            op.SetOperationFlags(
                FOF_ALLOWUNDO
                    | FOFX_RECYCLEONDELETE
                    | FOF_NOCONFIRMATION
                    | FOF_WANTNUKEWARNING
                    | FOF_SILENT,
            )?;

            let mut queued = 0usize;
            for p in paths {
                let wide = HSTRING::from(p.as_str());
                match SHCreateItemFromParsingName::<_, _, IShellItem>(&wide, None) {
                    Ok(item) => match op.DeleteItem(&item, None) {
                        Ok(_) => queued += 1,
                        Err(e) => result.errors.push(format!("Cannot delete {p}: {e}")),
                    },
                    Err(e) => result.errors.push(format!("Cannot locate {p}: {e}")),
                }
            }

            if queued > 0 {
                op.PerformOperations()?;
            }
            // Count successfully queued items as deleted; PerformOperations is
            // all-or-nothing per the batch, and a failure throws above.
            result.deleted += queued;
            Ok(())
        };

        if let Err(e) = perform() {
            result
                .errors
                .push(format!("Recycle operation failed: {e}"));
        }

        if we_initialized {
            CoUninitialize();
        }
    }
}

#[cfg(not(windows))]
fn recycle_paths(_paths: &[String], result: &mut FmDeleteResult) {
    result
        .errors
        .push("Recycle Bin deletion is only supported on Windows.".into());
}

// ---------------------------------------------------------------------------
// 5. fm_rename
// ---------------------------------------------------------------------------

#[tauri::command(async)]
pub fn fm_rename(path: String, new_name: String) -> Result<String, String> {
    let src = PathBuf::from(&path);
    guard_write(&src)?;

    let trimmed = new_name.trim();
    if trimmed.is_empty() {
        return Err("Name cannot be empty.".into());
    }
    // Reject path separators / traversal — a rename stays in the same folder.
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed == "." || trimmed == ".." {
        return Err("Name contains invalid path characters.".into());
    }

    let parent = src
        .parent()
        .ok_or_else(|| "Cannot rename a drive root.".to_string())?;
    let target = parent.join(trimmed);

    if target.exists() {
        return Err(format!("An item named \"{trimmed}\" already exists here."));
    }

    std::fs::rename(&src, &target).map_err(|e| describe_io_error("Rename", &src, &e))?;
    Ok(target.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------
// 6. fm_make_dir
// ---------------------------------------------------------------------------

#[tauri::command(async)]
pub fn fm_make_dir(parent_dir: String, name: String) -> Result<String, String> {
    let parent = PathBuf::from(&parent_dir);
    guard_write(&parent)?;
    if !parent.is_dir() {
        return Err("Parent is not a folder.".into());
    }

    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Folder name cannot be empty.".into());
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed == "." || trimmed == ".." {
        return Err("Folder name contains invalid path characters.".into());
    }

    let target = parent.join(trimmed);
    if target.exists() {
        return Err(format!("An item named \"{trimmed}\" already exists here."));
    }

    std::fs::create_dir(&target).map_err(|e| describe_io_error("Create folder", &target, &e))?;
    Ok(target.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------
// 7. fm_backup  (robocopy wrapper — local copy / mirror)
// ---------------------------------------------------------------------------

#[tauri::command(async)]
pub fn fm_backup(
    app: AppHandle,
    source_dir: String,
    dest_dir: String,
    mirror: bool,
    operation_id: String,
) -> Result<String, String> {
    clear_cancel(&operation_id);
    let src = PathBuf::from(&source_dir);
    let dst = PathBuf::from(&dest_dir);

    // The destination is where files are written / (in mirror mode) deleted.
    guard_write(&dst)?;
    if !src.is_dir() {
        return Err("Backup source is not a folder.".into());
    }

    run_backup(&app, &src, &dst, mirror, &operation_id)
}

#[cfg(windows)]
fn run_backup(
    app: &AppHandle,
    src: &Path,
    dst: &Path,
    mirror: bool,
    operation_id: &str,
) -> Result<String, String> {
    use std::io::{BufRead, BufReader};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut cmd = Command::new("robocopy");
    cmd.arg(src).arg(dst);
    if mirror {
        // /MIR = mirror (exact replica; deletes extras in dest).
        cmd.arg("/MIR");
    } else {
        // /E = copy all subdirs incl. empty ones (additive).
        cmd.arg("/E");
    }
    // Quiet logging so stdout stays parseable: no file/dir list, no header,
    // no per-file percent line.
    cmd.args(["/NFL", "/NDL", "/NJH", "/NP"]);
    cmd.creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Could not start robocopy: {e}"))?;

    let mut emitter = ProgressEmitter::new(app, operation_id);
    let mut files_done: u64 = 0;

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if is_cancelled(operation_id) {
                let _ = child.kill();
                clear_cancel(operation_id);
                return Err("Cancelled".into());
            }
            // robocopy emits one indented line per copied file (after the
            // size column). A rough heuristic: a tab-led data line that names
            // a file. We don't have a true total, so report a moving count.
            let trimmed = line.trim();
            if !trimmed.is_empty()
                && (line.starts_with('\t') || line.starts_with("  "))
                && !trimmed.starts_with("Total")
            {
                files_done += 1;
                emitter.emit(files_done, 0, trimmed, false);
            }
        }
    }

    let status = child
        .wait()
        .map_err(|e| format!("robocopy did not finish cleanly: {e}"))?;

    emitter.emit(files_done, files_done, "", true);
    clear_cancel(operation_id);

    // robocopy exit codes: 0-7 = success (bit flags), >=8 = error.
    let code = status.code().unwrap_or(-1);
    if (0..=7).contains(&code) {
        let mode = if mirror { "Mirrored" } else { "Copied" };
        Ok(format!(
            "{mode} {} item(s) from {} to {} (robocopy code {code}).",
            files_done,
            src.display(),
            dst.display()
        ))
    } else {
        Err(format!(
            "Backup failed: robocopy exit code {code} (>=8 means error)."
        ))
    }
}

#[cfg(not(windows))]
fn run_backup(
    _app: &AppHandle,
    _src: &Path,
    _dst: &Path,
    _mirror: bool,
    _operation_id: &str,
) -> Result<String, String> {
    Err("Backup (robocopy) is only supported on Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_destination_appends_numbered_suffix() {
        let dir = std::env::temp_dir().join("kil-fm-rename-test");
        std::fs::create_dir_all(&dir).unwrap();
        let original = dir.join("report.txt");
        std::fs::write(&original, b"x").unwrap();

        let renamed = rename_destination(&original);
        assert_eq!(renamed.file_name().unwrap().to_string_lossy(), "report (2).txt");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rename_destination_handles_no_extension() {
        let dir = std::env::temp_dir().join("kil-fm-rename-test2");
        std::fs::create_dir_all(&dir).unwrap();
        let original = dir.join("folder");
        std::fs::create_dir_all(&original).unwrap();

        let renamed = rename_destination(&original);
        assert_eq!(renamed.file_name().unwrap().to_string_lossy(), "folder (2)");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sort_puts_dirs_first_then_by_name() {
        let mut entries = vec![
            FmEntry { name: "b.txt".into(), path: String::new(), is_dir: false, size: 10, modified_ms: 0, hidden: false },
            FmEntry { name: "Apples".into(), path: String::new(), is_dir: true, size: 0, modified_ms: 0, hidden: false },
            FmEntry { name: "a.txt".into(), path: String::new(), is_dir: false, size: 20, modified_ms: 0, hidden: false },
            FmEntry { name: "zeta".into(), path: String::new(), is_dir: true, size: 0, modified_ms: 0, hidden: false },
        ];
        sort_entries(&mut entries, "name");
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Apples", "zeta", "a.txt", "b.txt"]);
    }

    #[test]
    fn cancel_flag_roundtrips() {
        let id = "test-op-cancel-roundtrip";
        clear_cancel(id);
        assert!(!is_cancelled(id));
        fm_cancel(id.to_string());
        assert!(is_cancelled(id));
        clear_cancel(id);
        assert!(!is_cancelled(id));
    }
}
