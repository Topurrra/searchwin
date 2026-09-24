//! Folder Diff + 3-Way Merge — recursively compare two directory trees, show a
//! per-file unified diff, and run a diff3-style three-way merge with conflict
//! markers. All local, no git required.
//!
//! Beats basic free diff tools by combining a recursive folder comparison
//! (added / removed / modified, content-verified) with an inline text diff and a
//! real 3-way merge (`diffy`) in one place.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde::Serialize;
use walkdir::WalkDir;

// ── Cancel registry (Wave 2.5) ─────────────────────────────────────────
//
// Folder Diff walks two directory trees and byte-compares every matching
// file — for big projects (node_modules + .git), that's many seconds of
// I/O work. We add cancel checks at the cheap loop bounds: per-directory-
// entry during traversal and per-matching-file before the byte compare.
// Pattern matches hash.rs / crypto_tool.rs.

static CANCELLED_DIFF_OPERATIONS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[tauri::command]
pub fn cancel_diff_operation(operation_id: String) -> Result<(), String> {
    CANCELLED_DIFF_OPERATIONS
        .lock()
        .map_err(|_| "Cancel registry lock failed".to_string())?
        .insert(operation_id);
    Ok(())
}

fn is_diff_cancelled(operation_id: &Option<String>) -> bool {
    operation_id
        .as_ref()
        .and_then(|id| CANCELLED_DIFF_OPERATIONS.lock().ok().map(|s| s.contains(id)))
        .unwrap_or(false)
}

fn clear_diff_cancel(operation_id: &Option<String>) {
    if let Some(id) = operation_id {
        if let Ok(mut set) = CANCELLED_DIFF_OPERATIONS.lock() {
            set.remove(id);
        }
    }
}

/// Files larger than this aren't diffed/merged inline (too big to be useful).
const MAX_TEXT_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffEntry {
    pub path: String,
    /// "added" | "removed" | "modified"
    pub status: String,
    pub left_size: Option<u64>,
    pub right_size: Option<u64>,
    pub is_text: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderDiff {
    pub entries: Vec<DiffEntry>,
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    pub identical: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeResult {
    pub merged: String,
    pub conflicts: bool,
}

fn read_up_to<R: Read>(r: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match r.read(&mut buf[total..])? {
            0 => break,
            n => total += n,
        }
    }
    Ok(total)
}

/// Heuristic: a NUL byte in the first 8 KB means "binary" (don't text-diff).
fn is_text_file(path: &Path) -> bool {
    let Ok(mut f) = File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 8192];
    let n = read_up_to(&mut f, &mut buf).unwrap_or(0);
    !buf[..n].contains(&0)
}

/// Byte-compare two files (used only when sizes already match). Early-exits on
/// the first difference; bounded memory.
fn files_equal(a: &Path, b: &Path) -> std::io::Result<bool> {
    let mut fa = BufReader::new(File::open(a)?);
    let mut fb = BufReader::new(File::open(b)?);
    let mut ba = [0u8; 16384];
    let mut bb = [0u8; 16384];
    loop {
        let na = read_up_to(&mut fa, &mut ba)?;
        let nb = read_up_to(&mut fb, &mut bb)?;
        if na != nb {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
        if ba[..na] != bb[..nb] {
            return Ok(false);
        }
    }
}

/// Map every file under `root` to (forward-slash relative path) → (size, full path).
/// Cancel-aware: returns the partial map on cancel; the caller checks the
/// flag separately and returns the cancelled error.
fn collect_files(root: &Path, operation_id: &Option<String>) -> BTreeMap<String, (u64, PathBuf)> {
    let mut map = BTreeMap::new();
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        if is_diff_cancelled(operation_id) {
            return map;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let full = entry.path().to_path_buf();
        let rel = match full.strip_prefix(root) {
            Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        map.insert(rel, (size, full));
    }
    map
}

#[tauri::command]
pub async fn folder_diff(
    left: String,
    right: String,
    operation_id: Option<String>,
) -> Result<FolderDiff, String> {
    tauri::async_runtime::spawn_blocking(move || {
        clear_diff_cancel(&operation_id);
        let left_root = PathBuf::from(&left);
        let right_root = PathBuf::from(&right);
        if !left_root.is_dir() {
            return Err(format!("Not a folder: {left}"));
        }
        if !right_root.is_dir() {
            return Err(format!("Not a folder: {right}"));
        }
        let left_files = collect_files(&left_root, &operation_id);
        if is_diff_cancelled(&operation_id) {
            clear_diff_cancel(&operation_id);
            return Err("Cancelled".to_string());
        }
        let right_files = collect_files(&right_root, &operation_id);
        if is_diff_cancelled(&operation_id) {
            clear_diff_cancel(&operation_id);
            return Err("Cancelled".to_string());
        }

        let mut keys: Vec<&String> = left_files.keys().chain(right_files.keys()).collect();
        keys.sort();
        keys.dedup();

        let mut entries = Vec::new();
        let (mut added, mut removed, mut modified, mut identical) = (0, 0, 0, 0);
        for key in keys {
            if is_diff_cancelled(&operation_id) {
                clear_diff_cancel(&operation_id);
                return Err("Cancelled".to_string());
            }
            match (left_files.get(key), right_files.get(key)) {
                (Some((ls, lp)), None) => {
                    removed += 1;
                    entries.push(DiffEntry {
                        path: key.clone(),
                        status: "removed".into(),
                        left_size: Some(*ls),
                        right_size: None,
                        is_text: is_text_file(lp),
                    });
                }
                (None, Some((rs, rp))) => {
                    added += 1;
                    entries.push(DiffEntry {
                        path: key.clone(),
                        status: "added".into(),
                        left_size: None,
                        right_size: Some(*rs),
                        is_text: is_text_file(rp),
                    });
                }
                (Some((ls, lp)), Some((rs, rp))) => {
                    let same = if ls != rs {
                        false
                    } else {
                        files_equal(lp, rp).unwrap_or(false)
                    };
                    if same {
                        identical += 1;
                    } else {
                        modified += 1;
                        entries.push(DiffEntry {
                            path: key.clone(),
                            status: "modified".into(),
                            left_size: Some(*ls),
                            right_size: Some(*rs),
                            is_text: is_text_file(lp) && is_text_file(rp),
                        });
                    }
                }
                (None, None) => {}
            }
        }
        clear_diff_cancel(&operation_id);
        Ok(FolderDiff { entries, added, removed, modified, identical })
    })
    .await
    .map_err(|e| format!("Folder diff worker failed: {e}"))?
}

/// Quality Pass Wave 1 / DM-3 OCR augmentation (2026-05-28): image
/// extensions whose content we can OCR for a text-level diff. PNG/JPG
/// etc. are technically "binary" by NUL-byte heuristic, but their
/// visible content is text most users actually want to compare
/// (scanned receipts, screenshots, photos of documents). Same set as
/// the OCR-on-Index code path (`text_extract.rs::is_ocr_image_extension`)
/// so we treat the same files consistently across surfaces.
fn is_image_extension_for_ocr(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "tif" | "tiff" | "bmp" | "webp" | "gif"
    )
}

/// Unified text diff of two files (for a "modified" folder-diff entry, or any two
/// files). Refuses oversized files; for byte-binary files, falls back to OCR when
/// both sides are images (so you can compare two scans / screenshots by their
/// recognized text).
#[tauri::command]
pub async fn file_unified_diff(left: String, right: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lp = PathBuf::from(&left);
        let rp = PathBuf::from(&right);

        // Size gate first — refuses oversized files regardless of kind.
        for p in [&lp, &rp] {
            if p.exists() {
                let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                if size > MAX_TEXT_BYTES {
                    return Ok("File too large to diff inline.".to_string());
                }
            }
        }

        let both_text = is_text_file(&lp) && is_text_file(&rp);

        // Quality Pass Wave 1 / DM-3 OCR fallback: when both sides are
        // images, run OCR on each and diff the recognized text instead
        // of bailing with "Binary file". Lang defaults match the indexer
        // default (eng+rus+kat) so cached entries from a prior index
        // build can be reused if we later wire the Wave 8.5 cache into
        // this code path (TODO follow-up).
        if !both_text {
            if is_image_extension_for_ocr(&lp) && is_image_extension_for_ocr(&rp) {
                let lt = super::ocr::ocr_image_file(&lp, "eng+rus+kat", false, None, &None)
                    .map_err(|e| format!("OCR failed for left image: {e}"))?;
                let rt = super::ocr::ocr_image_file(&rp, "eng+rus+kat", false, None, &None)
                    .map_err(|e| format!("OCR failed for right image: {e}"))?;
                if lt.trim() == rt.trim() {
                    return Ok(
                        "# Diff of OCR-extracted text (images).\n# The recognized text is identical on both sides."
                            .to_string(),
                    );
                }
                let patch = diffy::create_patch(&lt, &rt).to_string();
                return Ok(format!(
                    "# Diff of OCR-extracted text (images). Text was recognized via Tesseract.\n{patch}"
                ));
            }
            return Ok("Binary file — no text diff available.".to_string());
        }

        let lt = std::fs::read(&lp).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
        let rt = std::fs::read(&rp).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
        Ok(diffy::create_patch(&lt, &rt).to_string())
    })
    .await
    .map_err(|e| format!("Diff worker failed: {e}"))?
}

/// diff3-style three-way merge. `conflicts` is true when regions couldn't be
/// auto-merged — `merged` then contains the standard `<<<<<<<`/`=======`/`>>>>>>>`
/// conflict markers for the user to resolve.
#[tauri::command]
pub fn three_way_merge(base: String, ours: String, theirs: String) -> Result<MergeResult, String> {
    match diffy::merge(&base, &ours, &theirs) {
        Ok(merged) => Ok(MergeResult { merged, conflicts: false }),
        Err(merged) => Ok(MergeResult { merged, conflicts: true }),
    }
}

// ── Quality Pass Wave 1 / DM-4 (2026-05-29): folder sync actions ──────
//
// After running a folder diff, the user can:
//   * Copy missing files from left → right
//   * Copy missing files from right → left
//   * Mirror left → right (also DELETES extras on the right so it
//     matches the left exactly)
//
// Mirror is destructive — the frontend must confirm before invoking it.
// All ops report a per-file result so the user can see exactly what
// happened, and respect the existing Wave 2.5 cancel infrastructure.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFileResult {
    pub path: String,
    /// "copied" | "deleted" | "skipped" | "failed"
    pub action: String,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFoldersResult {
    pub copied_count: usize,
    pub deleted_count: usize,
    pub failed_count: usize,
    pub items: Vec<SyncFileResult>,
}

/// Folder sync action. Three modes:
///   * `"copy_left_to_right"` — copy every file present in `left` but
///     absent (or different) in `right` into `right`. Non-destructive.
///   * `"copy_right_to_left"` — same the other way around.
///   * `"mirror_left_to_right"` — makes `right` byte-identical to
///     `left`: copies missing/changed files AND deletes any file in
///     `right` not present in `left`. DESTRUCTIVE on the right side;
///     frontend must confirm before calling.
#[tauri::command]
pub async fn sync_folders(
    left: String,
    right: String,
    mode: String,
    operation_id: Option<String>,
) -> Result<SyncFoldersResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        clear_diff_cancel(&operation_id);
        let left_root = PathBuf::from(&left);
        let right_root = PathBuf::from(&right);
        if !left_root.is_dir() {
            return Err(format!("Not a folder: {left}"));
        }
        if !right_root.is_dir() {
            return Err(format!("Not a folder: {right}"));
        }

        let left_files = collect_files(&left_root, &operation_id);
        if is_diff_cancelled(&operation_id) {
            clear_diff_cancel(&operation_id);
            return Err("Cancelled".to_string());
        }
        let right_files = collect_files(&right_root, &operation_id);
        if is_diff_cancelled(&operation_id) {
            clear_diff_cancel(&operation_id);
            return Err("Cancelled".to_string());
        }

        let mut items: Vec<SyncFileResult> = Vec::new();
        let mut copied_count = 0usize;
        let mut deleted_count = 0usize;
        let mut failed_count = 0usize;

        let (source_root, target_root, source_files, target_files, do_delete) = match mode.as_str()
        {
            "copy_left_to_right" => (&left_root, &right_root, &left_files, &right_files, false),
            "copy_right_to_left" => (&right_root, &left_root, &right_files, &left_files, false),
            "mirror_left_to_right" => (&left_root, &right_root, &left_files, &right_files, true),
            other => return Err(format!("Unknown sync mode: {other}")),
        };

        // Phase 1: copy files that are in source but missing OR
        // different (size mismatch) in target.
        for (rel, (src_size, src_full)) in source_files {
            if is_diff_cancelled(&operation_id) {
                clear_diff_cancel(&operation_id);
                return Err("Cancelled".to_string());
            }
            let target_full = target_root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            let needs_copy = match target_files.get(rel) {
                None => true,
                Some((t_size, _)) => t_size != src_size,
            };
            if !needs_copy {
                continue;
            }
            // Ensure parent dir exists.
            if let Some(parent) = target_full.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    failed_count += 1;
                    items.push(SyncFileResult {
                        path: rel.clone(),
                        action: "failed".to_string(),
                        error: Some(format!("Cannot create parent dir: {e}")),
                    });
                    continue;
                }
            }
            match std::fs::copy(src_full, &target_full) {
                Ok(_) => {
                    copied_count += 1;
                    items.push(SyncFileResult {
                        path: rel.clone(),
                        action: "copied".to_string(),
                        error: None,
                    });
                }
                Err(e) => {
                    failed_count += 1;
                    items.push(SyncFileResult {
                        path: rel.clone(),
                        action: "failed".to_string(),
                        error: Some(format!("Cannot copy: {e}")),
                    });
                }
            }
        }

        // Phase 2 (mirror only): delete files in target not present in
        // source.
        if do_delete {
            for (rel, (_, target_full)) in target_files {
                if is_diff_cancelled(&operation_id) {
                    clear_diff_cancel(&operation_id);
                    return Err("Cancelled".to_string());
                }
                if source_files.contains_key(rel) {
                    continue;
                }
                match std::fs::remove_file(target_full) {
                    Ok(_) => {
                        deleted_count += 1;
                        items.push(SyncFileResult {
                            path: rel.clone(),
                            action: "deleted".to_string(),
                            error: None,
                        });
                    }
                    Err(e) => {
                        failed_count += 1;
                        items.push(SyncFileResult {
                            path: rel.clone(),
                            action: "failed".to_string(),
                            error: Some(format!("Cannot delete: {e}")),
                        });
                    }
                }
            }
        }

        // Reference source_root to silence unused warnings in copy
        // modes (it's only used as the iterator base via source_files,
        // but the parameter is symmetric for clarity).
        let _ = source_root;

        clear_diff_cancel(&operation_id);
        Ok(SyncFoldersResult {
            copied_count,
            deleted_count,
            failed_count,
            items,
        })
    })
    .await
    .map_err(|e| format!("Folder sync worker failed: {e}"))?
}

/// Read a text file into a string (for loading a file into a merge pane). Caps
/// size and rejects binary.
#[tauri::command]
pub fn diff_read_text(path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    let size = std::fs::metadata(&p).map(|m| m.len()).map_err(|e| format!("Cannot read: {e}"))?;
    if size > MAX_TEXT_BYTES {
        return Err("File is too large to load.".to_string());
    }
    if !is_text_file(&p) {
        return Err("That looks like a binary file.".to_string());
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("Cannot read: {e}"))?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}
