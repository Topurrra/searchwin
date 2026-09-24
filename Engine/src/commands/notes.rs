//! Notes — local, open-format note storage.
//!
//! Notes are plain `.ki` files (UTF-8 Markdown + optional YAML frontmatter)
//! living in one flat folder: `Documents/KeepItLocal Notes`. No database, no
//! sync — the user owns the files, and the content-search pillar can index the
//! folder directly. Frontmatter holds metadata (title, created, updated,
//! pinned, tags); the body is Markdown the TipTap editor round-trips.
//!
//! At rest: notes are stored as **plaintext** `.ki` files — deliberately. The
//! point of an open `.ki` format in the user's own Documents folder is
//! portability and longevity ("yours forever, openable in any editor"), which
//! app-level encryption would defeat. Unlike the redb-backed stores (clipboard,
//! snippets, OCR cache — all DPAPI-encrypted), note bodies rely on OS disk
//! encryption (BitLocker) for at-rest protection. Optional per-note encryption
//! could be a future opt-in, but the default stays open. (Encrypt-at-rest
//! audit, 2026-06-21.)
//!
//! The frontend store owns the canonical frontmatter serialize/parse; Rust does
//! a LENIENT read-only parse here purely to build the note list (title/preview/
//! pinned/tags) without the frontend having to read every body. Notes may live
//! in user subfolders; every caller-supplied path is validated component-by-
//! component against the notes dir (only "normal" name parts allowed), so `..`,
//! absolute escapes, and drive prefixes are impossible — traversal stays out.

use fs2::FileExt;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

const NOTES_FOLDER: &str = "KeepItLocal Notes";
const TRASH_FOLDER: &str = ".trash";
/// Template store. A DOTFOLDER on purpose: `collect_notes`, `collect_folders`
/// and the folder-name guard all already skip `.`-prefixed dirs, so templates
/// stay out of the note list, the folder chips, and search with zero changes to
/// any walker. A plain `templates/` would show up as a normal folder full of
/// normal notes, and reserving that name would break anyone who already has one.
const TEMPLATES_FOLDER: &str = ".templates";
/// Visible, ordinary folder for one deterministic daily note per local day.
const DAILY_FOLDER: &str = "Daily";
/// Per-note revisions live beside their note in a hidden sidecar directory.
/// Keeping this next to the file means normal folder moves and renames carry
/// history without a second path mapping or database.
const HISTORY_SUFFIX: &str = ".history";
const MAX_NOTE_REVISIONS: usize = 30;
const TRASH_METADATA_SUFFIX: &str = ".trash.json";
const PREVIEW_LEN: usize = 160;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    /// Absolute path to the `.ki` file.
    pub path: String,
    pub title: String,
    pub preview: String,
    pub modified_ms: i64,
    pub pinned: bool,
    pub tags: Vec<String>,
    /// Relative folder under the notes dir, forward-slash separated. "" = root,
    /// "Work" or "Work/Specs" for subfolders. Notes can live in subfolders.
    pub folder: String,
}

/// Raw `.ki` content plus a content revision for optimistic saves.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteFile {
    pub content: String,
    pub revision: String,
}

/// Outcome of a revision-checked note write.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteWriteResult {
    pub saved: bool,
    /// The revision on disk after a save, or the revision that conflicted.
    pub revision: Option<String>,
    pub modified_ms: Option<i64>,
    /// `changed` means another writer updated the file. `missing` means it
    /// disappeared before this write.
    pub conflict: Option<String>,
}

/// A bounded, local snapshot of a note before a successful save.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteRevision {
    /// Opaque id, scoped to the requested note. It is never a filesystem path.
    pub id: String,
    pub created_ms: i64,
    pub bytes: u64,
}

/// Snapshot contents for the revision picker and diff view.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteRevisionContent {
    pub id: String,
    pub created_ms: i64,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrashMetadata {
    original_path: String,
    kind: String,
    deleted_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrashItemKind {
    Note,
    Folder,
}

impl TrashItemKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Folder => "folder",
        }
    }
}

#[derive(Debug, Clone)]
struct TrashItem {
    path: PathBuf,
    kind: TrashItemKind,
    original_path: PathBuf,
    deleted_ms: i64,
}

/// A direct body-search hit, including a localized body excerpt.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteBodyMatch {
    pub path: String,
    pub snippet: String,
}

/// Raw candidate note bodies used only while the Notes context drawer is open.
/// The frontend runs the shared Markdown-aware wikilink parser before showing a
/// backlink, so code examples such as `[[literal]]` never become false links.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteLinkSource {
    pub path: String,
    pub body: String,
}

/// A safe, local attachment path for the Notes editor. The frontend may pass a
/// note-authored relative link, so this command is the chokepoint before that
/// link reaches the OS opener.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteAttachment {
    pub path: String,
    pub name: String,
    pub bytes: u64,
}

/// Resolve (and create) the notes folder: `Documents/KeepItLocal Notes`.
fn notes_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let documents = app
        .path()
        .document_dir()
        .map_err(|e| format!("Cannot resolve Documents directory: {e}"))?;
    let dir = documents.join(NOTES_FOLDER);
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create notes directory: {e}"))?;
    let documents = fs::canonicalize(&documents)
        .map_err(|e| format!("Cannot resolve Documents directory: {e}"))?;
    let dir = fs::canonicalize(&dir).map_err(|e| format!("Cannot resolve notes directory: {e}"))?;
    if !paths_match(&dir, &documents.join(NOTES_FOLDER)) {
        return Err("Notes folder must not be a link".to_string());
    }
    Ok(dir)
}

#[cfg(windows)]
fn paths_match(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(not(windows))]
fn paths_match(left: &Path, right: &Path) -> bool {
    left == right
}

fn is_inside(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok()
}

/// Validate a RELATIVE path component-by-component and rebuild it. Only "normal"
/// name components survive — `.` is dropped, and `..`, root, and drive prefixes
/// are rejected outright — so the result can ONLY ever resolve inside the notes
/// dir it's later joined to. The single chokepoint behind every notes path.
fn safe_relative_path(rel: &Path) -> Result<PathBuf, String> {
    let mut out = PathBuf::new();
    for comp in rel.components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            _ => return Err("Invalid note path".into()),
        }
    }
    if out.as_os_str().is_empty() {
        return Err("Empty note path".into());
    }
    Ok(out)
}

/// Resolve an input underneath the physical notes root. Every existing path
/// component must remain exactly where its lexical path says it is, so both
/// symlinks and Windows junctions are rejected even when they point back into
/// the notes tree. Missing final paths are allowed only after their existing
/// parents have passed that check.
fn resolve_notes_path(root: &Path, input: &str, allow_missing: bool) -> Result<PathBuf, String> {
    let raw = Path::new(input);
    let relative = if raw.is_absolute() {
        match raw.strip_prefix(root) {
            Ok(relative) => safe_relative_path(relative)?,
            Err(_) if raw.exists() => {
                let canonical = fs::canonicalize(raw)
                    .map_err(|error| format!("Cannot resolve note path: {error}"))?;
                let relative = canonical
                    .strip_prefix(root)
                    .map_err(|_| "Note path is outside the notes folder".to_string())?;
                safe_relative_path(relative)?
            }
            Err(_) => return Err("Note path is outside the notes folder".to_string()),
        }
    } else {
        safe_relative_path(raw)?
    };

    let mut expected = root.to_path_buf();
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("Invalid note path".to_string());
        };
        expected.push(name);
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err("Note path must not contain a link".to_string());
                }
                let canonical = fs::canonicalize(&current)
                    .map_err(|error| format!("Cannot resolve note path: {error}"))?;
                if !is_inside(root, &canonical) || !paths_match(&canonical, &expected) {
                    return Err("Note path must not contain a link".to_string());
                }
                current = canonical;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && allow_missing => {
                return Ok(root.join(&relative));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err("Note no longer exists".to_string());
            }
            Err(error) => return Err(format!("Cannot access note path: {error}")),
        }
    }
    Ok(current)
}

fn visible_note_relative(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Note path is outside the notes folder".to_string())?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("Invalid note path".to_string());
        };
        let name = name.to_string_lossy();
        if name == TRASH_FOLDER || name.eq_ignore_ascii_case("attachments") || name.starts_with('.') {
            return Err("That path is reserved for KeepItLocal".to_string());
        }
    }
    Ok(relative.to_path_buf())
}

/// Read directory entries without following a link or junction. Every Notes
/// tree walker uses this instead of `Path::is_dir`, which follows links and can
/// otherwise escape the user-selected Notes root or recurse forever.
fn safe_tree_metadata(root: &Path, path: &Path) -> Option<fs::Metadata> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() {
        return None;
    }
    let canonical = fs::canonicalize(path).ok()?;
    if !is_inside(root, &canonical) || !paths_match(&canonical, path) {
        return None;
    }
    Some(metadata)
}

/// Resolve a caller-supplied note path (absolute under the notes dir, or a
/// relative subpath) to a SAFE `.ki` file inside the notes folder. Absolute
/// inputs are stripped back to their relative part first, then validated, so
/// notes in subfolders work while traversal stays impossible.
fn note_path(app: &AppHandle, file_arg: &str) -> Result<PathBuf, String> {
    let dir = notes_dir(app)?;
    let p = resolve_notes_path(&dir, file_arg, true)?;
    visible_note_relative(&dir, &p)?;
    if !p
        .extension()
        .map(|e| e.eq_ignore_ascii_case("ki"))
        .unwrap_or(false)
    {
        return Err("Not a .ki note file".into());
    }
    Ok(p)
}

/// Resolve a caller-supplied FOLDER path to a safe directory inside the notes
/// folder. Same validation as `note_path`, plus the reserved folders (`.trash`,
/// `attachments`, any dotfolder) are off-limits to folder operations.
fn folder_path(app: &AppHandle, folder_arg: &str) -> Result<PathBuf, String> {
    let dir = notes_dir(app)?;
    let p = resolve_notes_path(&dir, folder_arg, true)?;
    let safe = p
        .strip_prefix(&dir)
        .map_err(|_| "Folder is outside the notes folder".to_string())?;
    for comp in safe.components() {
        if let Component::Normal(c) = comp {
            let s = c.to_string_lossy();
            if s == TRASH_FOLDER || s.eq_ignore_ascii_case("attachments") || s.starts_with('.') {
                return Err("That folder name is reserved".into());
            }
        }
    }
    Ok(p)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn modified_ms(path: &Path) -> i64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Slug for a new note's file name: lowercase, alphanumerics + single dashes,
/// capped. Empty input → "untitled".
fn slug(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;
    for ch in input.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    let capped: String = trimmed.chars().take(48).collect();
    if capped.is_empty() {
        "untitled".to_string()
    } else {
        capped
    }
}

/// Lenient frontmatter split. If `content` opens with a `---` fence, returns
/// (title, pinned, tags, body). Title falls back to the first Markdown heading
/// then None. Handles `\n` and `\r\n`.
fn parse(content: &str) -> (Option<String>, bool, Vec<String>, String) {
    let normalized = content.replace("\r\n", "\n");
    let mut title: Option<String> = None;
    let mut pinned = false;
    let mut tags: Vec<String> = Vec::new();

    let body: &str = if let Some(rest) = normalized.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let yaml = &rest[..end];
            for line in yaml.lines() {
                let line = line.trim();
                if let Some(v) = line.strip_prefix("title:") {
                    let v = unquote(v.trim());
                    if !v.is_empty() {
                        title = Some(v);
                    }
                } else if let Some(v) = line.strip_prefix("pinned:") {
                    pinned = v.trim() == "true";
                } else if let Some(v) = line.strip_prefix("tags:") {
                    tags = parse_tags(v.trim());
                }
            }
            // Body = everything after the closing fence line.
            let after = &rest[end + "\n---".len()..];
            after.trim_start_matches('\n').trim_start_matches('\n')
        } else {
            normalized.as_str()
        }
    } else {
        normalized.as_str()
    };

    if title.is_none() {
        // First Markdown ATX heading, else None.
        for line in body.lines() {
            let t = line.trim_start();
            if let Some(h) = t.strip_prefix("# ") {
                title = Some(h.trim().to_string());
                break;
            }
            if !t.is_empty() {
                break;
            }
        }
    }

    (title, pinned, tags, body.to_string())
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if (v.starts_with('"') && v.ends_with('"') && v.len() >= 2)
        || (v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2)
    {
        v[1..v.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        v.to_string()
    }
}

/// Parse `tags: [a, b, "c d"]` (inline flow form — the form the frontend writes).
fn parse_tags(v: &str) -> Vec<String> {
    let v = v.trim();
    let inner = v.strip_prefix('[').and_then(|s| s.strip_suffix(']'));
    let body = match inner {
        Some(b) => b,
        None => return Vec::new(),
    };
    body.split(',')
        .map(|t| unquote(t.trim()))
        .filter(|t| !t.is_empty())
        .collect()
}

/// First ~PREVIEW_LEN chars of the body, with Markdown noise flattened to a
/// single line so the list reads cleanly.
fn preview_of(body: &str) -> String {
    let mut flat = String::new();
    for raw in body.lines() {
        let line = raw
            .trim_start_matches(['#', '>', '-', '*', ' ', '\t'])
            .trim();
        if line.is_empty() {
            continue;
        }
        if !flat.is_empty() {
            flat.push(' ');
        }
        flat.push_str(line);
        if flat.chars().count() >= PREVIEW_LEN {
            break;
        }
    }
    flat.chars().take(PREVIEW_LEN).collect()
}

fn summarize(path: &Path) -> Option<NoteSummary> {
    let content = fs::read_to_string(path).ok()?;
    let (title, pinned, tags, body) = parse(&content);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled")
        .to_string();
    Some(NoteSummary {
        path: path.to_string_lossy().to_string(),
        title: title.unwrap_or(stem),
        preview: preview_of(&body),
        modified_ms: modified_ms(path),
        pinned,
        tags,
        // Filled in by the recursive lister (`collect_notes`); root by default.
        folder: String::new(),
    })
}

fn note_revision(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex().to_string()
}

fn read_note_file(path: &Path) -> Result<NoteFile, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Cannot read note: {e}"))?;
    Ok(NoteFile {
        revision: note_revision(&content),
        content,
    })
}

fn history_path_for_note(root: &Path, note: &Path) -> Result<PathBuf, String> {
    let relative = note
        .strip_prefix(root)
        .map_err(|_| "Note path is outside the notes folder".to_string())?;
    let file_name = relative
        .file_name()
        .ok_or_else(|| "Note path is invalid".to_string())?
        .to_string_lossy();
    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let candidate = root
        .join(parent)
        .join(format!(".{file_name}{HISTORY_SUFFIX}"));
    resolve_notes_path(root, &candidate.to_string_lossy(), true)
}

fn existing_history_dir(root: &Path, note: &Path) -> Result<Option<PathBuf>, String> {
    let history = history_path_for_note(root, note)?;
    match fs::symlink_metadata(&history) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Cannot read note history: {error}")),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err("Note history is not a regular directory".to_string())
        }
        Ok(_) => Ok(Some(resolve_notes_path(
            root,
            &history.to_string_lossy(),
            false,
        )?)),
    }
}

fn ensure_history_dir(root: &Path, note: &Path) -> Result<PathBuf, String> {
    let history = history_path_for_note(root, note)?;
    fs::create_dir_all(&history).map_err(|error| format!("Cannot create note history: {error}"))?;
    existing_history_dir(root, note)?.ok_or_else(|| "Cannot create note history".to_string())
}

fn valid_revision_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 96
        && id.chars().all(|character| character.is_ascii_alphanumeric() || character == '-')
}

fn revision_from_path(path: &Path) -> Option<NoteRevision> {
    let id = path.file_stem()?.to_str()?;
    let (created_ms, _) = id.split_once('-')?;
    let created_ms = created_ms.parse::<i64>().ok()?;
    if !valid_revision_id(id) {
        return None;
    }
    Some(NoteRevision {
        id: id.to_string(),
        created_ms,
        bytes: fs::metadata(path).ok()?.len(),
    })
}

fn revision_path(root: &Path, note: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_revision_id(id) {
        return Err("Invalid note revision".to_string());
    }
    let history = existing_history_dir(root, note)?
        .ok_or_else(|| "That note has no saved revisions".to_string())?;
    let path = history.join(format!("{id}.ki"));
    let path = resolve_notes_path(root, &path.to_string_lossy(), false)?;
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| "That note revision no longer exists".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("That note revision is invalid".to_string());
    }
    Ok(path)
}

fn list_note_revisions_for_path(root: &Path, note: &Path) -> Result<Vec<NoteRevision>, String> {
    let Some(history) = existing_history_dir(root, note)? else {
        return Ok(Vec::new());
    };
    let mut revisions = Vec::new();
    let entries = fs::read_dir(&history).map_err(|error| format!("Cannot read note history: {error}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.eq_ignore_ascii_case("ki"))
            != Some(true)
        {
            continue;
        }
        let Ok(path) = resolve_notes_path(root, &path.to_string_lossy(), false) else {
            continue;
        };
        if let Some(revision) = revision_from_path(&path) {
            revisions.push(revision);
        }
    }
    revisions.sort_by(|left, right| {
        right
            .created_ms
            .cmp(&left.created_ms)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(revisions)
}

fn save_note_snapshot(root: &Path, note: &Path, content: &str) -> Result<(), String> {
    let history = ensure_history_dir(root, note)?;
    let hash = note_revision(content);
    for attempt in 0..1000 {
        let id = if attempt == 0 {
            format!("{}-{}", now_ms(), &hash[..12])
        } else {
            format!("{}-{}-{attempt}", now_ms(), &hash[..12])
        };
        let path = history.join(format!("{id}.ki"));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create note revision: {error}")),
        };
        if let Err(error) = file.write_all(content.as_bytes()).and_then(|_| file.sync_all()) {
            let _ = fs::remove_file(&path);
            return Err(format!("Cannot save note revision: {error}"));
        }
        let revisions = list_note_revisions_for_path(root, note)?;
        for revision in revisions.into_iter().skip(MAX_NOTE_REVISIONS) {
            let old = history.join(format!("{}.ki", revision.id));
            let _ = fs::remove_file(old);
        }
        return Ok(());
    }
    Err("Cannot allocate a unique note revision".to_string())
}

fn write_temp_note(path: &Path, content: &str) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Note path has no parent directory".to_string())?;
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("note.ki");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for attempt in 0..1000 {
        let temp = parent.join(format!(".{name}.{nonce}-{attempt}.tmp"));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create temporary note: {error}")),
        };
        if let Err(error) = file.write_all(content.as_bytes()).and_then(|_| file.sync_all()) {
            let _ = fs::remove_file(&temp);
            return Err(format!("Cannot save temporary note: {error}"));
        }
        return Ok(temp);
    }
    Err("Cannot allocate a temporary note file".to_string())
}

fn sync_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        if let Ok(directory) = OpenOptions::new().read(true).open(parent) {
            let _ = directory.sync_all();
        }
    }
}

fn write_note_if_current(
    root: &Path,
    path: &Path,
    content: &str,
    expected_revision: Option<String>,
) -> Result<NoteWriteResult, String> {
    let history = ensure_history_dir(root, path)?;
    let lock_path = history.join(".lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&lock_path)
        .map_err(|error| format!("Cannot open note save lock: {error}"))?;
    lock.lock_exclusive()
        .map_err(|error| format!("Cannot lock note for saving: {error}"))?;

    let current = match fs::read_to_string(path) {
        Ok(content) => Some(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Cannot read note before saving: {error}")),
    };

    let Some(current) = current else {
        if expected_revision.is_some() {
            return Ok(NoteWriteResult {
                saved: false,
                revision: None,
                modified_ms: None,
                conflict: Some("missing".to_string()),
            });
        }

        let mut created = match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Ok(NoteWriteResult {
                    saved: false,
                    revision: read_note_file(path).ok().map(|note| note.revision),
                    modified_ms: None,
                    conflict: Some("changed".to_string()),
                });
            }
            Err(error) => return Err(format!("Cannot restore note: {error}")),
        };
        if let Err(error) = created.write_all(content.as_bytes()).and_then(|_| created.sync_all()) {
            let _ = fs::remove_file(path);
            return Err(format!("Cannot restore note: {error}"));
        }
        sync_parent(path);
        return Ok(NoteWriteResult {
            saved: true,
            revision: Some(note_revision(content)),
            modified_ms: Some(modified_ms(path)),
            conflict: None,
        });
    };

    let current_revision = note_revision(&current);
    if expected_revision.as_deref() != Some(current_revision.as_str()) {
        return Ok(NoteWriteResult {
            saved: false,
            revision: Some(current_revision),
            modified_ms: None,
            conflict: Some("changed".to_string()),
        });
    }

    save_note_snapshot(root, path, &current)?;
    let latest = fs::read_to_string(path).map_err(|error| format!("Cannot recheck note before saving: {error}"))?;
    if note_revision(&latest) != current_revision {
        return Ok(NoteWriteResult {
            saved: false,
            revision: Some(note_revision(&latest)),
            modified_ms: None,
            conflict: Some("changed".to_string()),
        });
    }

    let temp = write_temp_note(path, content)?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(format!("Cannot replace note safely: {error}"));
    }
    sync_parent(path);

    Ok(NoteWriteResult {
        saved: true,
        revision: Some(note_revision(content)),
        modified_ms: Some(modified_ms(path)),
        conflict: None,
    })
}

fn create_note_in_dir(
    dir: &Path,
    file_base: &str,
    content: &str,
    folder: String,
) -> Result<NoteSummary, String> {
    let base = slug(file_base);
    for n in 1..=9999 {
        let name = if n == 1 {
            format!("{base}.ki")
        } else {
            format!("{base}-{n}.ki")
        };
        let path = dir.join(name);
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create note: {error}")),
        };
        if let Err(error) = file.write_all(content.as_bytes()).and_then(|_| file.sync_all()) {
            let _ = fs::remove_file(&path);
            return Err(format!("Cannot create note: {error}"));
        }
        let mut summary = summarize(&path)
            .ok_or_else(|| "Note created but could not be read back".to_string())?;
        summary.folder = folder;
        return Ok(summary);
    }
    Err("Too many notes with this name".to_string())
}

fn is_daily_note_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

fn open_or_create_daily_note_in_dir(
    dir: &Path,
    date_key: &str,
    content: &str,
    folder: String,
) -> Result<NoteSummary, String> {
    let path = dir.join(format!("{date_key}.ki"));
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(content.as_bytes())
                .map_err(|error| format!("Cannot create daily note: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("Cannot finish creating daily note: {error}"))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(format!("Cannot open daily note: {error}")),
    }

    let mut summary =
        summarize(&path).ok_or_else(|| "Daily note could not be read back".to_string())?;
    summary.folder = folder;
    Ok(summary)
}

#[tauri::command]
pub fn get_notes_dir(app: AppHandle) -> Result<String, String> {
    let path = notes_dir(&app)?.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
            return Ok(format!(r"\\{rest}"));
        }
        return Ok(path.strip_prefix(r"\\?\").unwrap_or(&path).to_string());
    }
    #[cfg(not(windows))]
    Ok(path)
}

/// Paths of every note whose BODY contains `query` (case-insensitive substring).
///
/// Exists because the note list's own search only ever saw `preview` — the
/// first `PREVIEW_LEN` (160) chars of the body — so anything past roughly the
/// first paragraph was unfindable from the Notes tool. `summarize` already
/// reads each full body to build that preview and then drops it; this walks the
/// same tree and actually matches against it.
///
/// Deliberately NOT routed through the Tantivy content index, even though
/// `update_notes_index` keeps notes in it: that path silently no-ops until the
/// content index has been built, which would make the notes app's own search
/// quietly depend on an unrelated subsystem's state. A direct scan over a few
/// hundred plaintext files has no such coupling and no cold-start.
///
/// Returns paths and source excerpts. Title/preview/tag matching stays instant
/// and client-side; the excerpt lets a result explain a deep body match.
///
/// `async` (spawn_blocking) because the notes folder is frequently
/// OneDrive-synced / AV-scanned (see `write_note`), so a cold read can stall.
#[tauri::command(async)]
pub fn search_note_bodies(app: AppHandle, query: String) -> Result<Vec<NoteBodyMatch>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    // Escape the user's text so a note query is always literal, then use
    // regex's Unicode-aware source offsets to build a safe excerpt.
    let escaped = regex::escape(query);
    let matcher = RegexBuilder::new(&escaped)
        .case_insensitive(true)
        .unicode(true)
        .build()
        .map_err(|e| format!("Cannot prepare note search: {e}"))?;
    let dir = notes_dir(&app)?;
    let mut out = Vec::new();
    collect_body_matches(&dir, &dir, &matcher, &mut out);
    Ok(out)
}

/// Candidate note bodies that contain the wikilink opener. This stays an
/// on-demand direct scan instead of a second index. The frontend parses the
/// candidates with the same Markdown rule the editor uses before it shows any
/// backlink.
#[tauri::command(async)]
pub fn list_note_link_sources(app: AppHandle) -> Result<Vec<NoteLinkSource>, String> {
    let dir = notes_dir(&app)?;
    let mut out = Vec::new();
    collect_note_link_sources(&dir, &dir, &mut out);
    Ok(out)
}

/// Names of the user's note templates — the `.ki` file stems in `.templates/`.
///
/// A template is just a note: no schema, no templating language. The folder is
/// created on demand, so an empty list simply means the user has none.
#[tauri::command(async)]
pub fn list_note_templates(app: AppHandle) -> Result<Vec<String>, String> {
    let root = notes_dir(&app)?;
    let requested = root.join(TEMPLATES_FOLDER);
    let dir = match resolve_notes_path(&root, &requested.to_string_lossy(), false) {
        Ok(dir) => dir,
        Err(_) => return Ok(Vec::new()),
    };
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        // Missing folder = no templates, not an error worth a dialog.
        Err(_) => return Ok(Vec::new()),
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|path| safe_tree_metadata(&root, path).is_some_and(|metadata| metadata.is_file()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("ki"))
                == Some(true)
        })
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    Ok(names)
}

fn note_template_path(root: &Path, name: &str) -> Result<PathBuf, String> {
    let name = safe_relative_path(Path::new(name))?;
    if name.components().count() != 1
        || name
            .file_name()
            .is_none_or(|component| component.to_string_lossy().starts_with('.'))
    {
        return Err("Invalid template name".to_string());
    }
    let path = root
        .join(TEMPLATES_FOLDER)
        .join(format!("{}.ki", name.to_string_lossy()));
    let path = resolve_notes_path(root, &path.to_string_lossy(), false)?;
    let metadata = fs::symlink_metadata(&path).map_err(|_| "Template no longer exists".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Template is not a regular file".to_string());
    }
    Ok(path)
}

/// A template's body, with snippet variables resolved.
///
/// Reuses the snippet system's `expand_template_full` rather than growing a
/// second variable syntax: it's a pure string function with no coupling to
/// snippet storage or the keyboard hook, and users already know `{{date}}`
/// from snippets. Unknown `{{...}}` are left verbatim by that function, so a
/// template can still contain literal braces.
///
/// `name` is a stem, never a path — it's rejoined under `.templates/` and run
/// through `note_path`, so `../` can't escape the notes folder.
#[tauri::command(async)]
pub fn read_note_template(app: AppHandle, name: String) -> Result<String, String> {
    let root = notes_dir(&app)?;
    let path = note_template_path(&root, &name)?;
    let content = fs::read_to_string(&path).map_err(|e| format!("Cannot read template: {e}"))?;
    let (_, _, _, body) = parse(&content);
    let vars = std::collections::HashMap::new();
    Ok(crate::commands::snippets::expand_template_full(
        &body, None, None, &vars,
    ))
}

/// Create `.templates/` and reveal it, so "I have no templates" has an obvious
/// next step instead of requiring the user to know the folder name.
/// Mirrors `open_notes_folder`'s explorer shell-out rather than adding a
/// different mechanism for the same job.
#[tauri::command(async)]
pub fn open_note_templates_folder(app: AppHandle) -> Result<(), String> {
    let root = notes_dir(&app)?;
    let dir = root.join(TEMPLATES_FOLDER);
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create templates folder: {e}"))?;
    let dir = resolve_notes_path(&root, &dir.to_string_lossy(), false)?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("Cannot open templates folder: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        let _ = &dir;
    }
    Ok(())
}

/// Walk mirroring `collect_notes`' skip rules (trash / attachments / dotfolders)
/// so search can never surface a note the list itself won't show.
fn collect_body_matches(root: &Path, dir: &Path, matcher: &Regex, out: &mut Vec<NoteBodyMatch>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(metadata) = safe_tree_metadata(root, &path) else {
            continue;
        };
        if metadata.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == TRASH_FOLDER
                || name.eq_ignore_ascii_case("attachments")
                || name.starts_with('.')
            {
                continue;
            }
            collect_body_matches(root, &path, matcher, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("ki"))
            == Some(true)
        {
            // Unreadable note = skipped, not fatal: one bad file must not
            // break search across the whole folder.
            if let Ok(content) = fs::read_to_string(&path) {
                let (_, _, _, body) = parse(&content);
                if let Some(found) = matcher.find(&body) {
                    out.push(NoteBodyMatch {
                        path: path.to_string_lossy().to_string(),
                        snippet: body_match_snippet(&body, found.start(), found.end()),
                    });
                }
            }
        }
    }
}

fn body_match_snippet(body: &str, start_byte: usize, end_byte: usize) -> String {
    const CONTEXT_CHARS: usize = 72;

    // Regex byte offsets always fall on source character boundaries. Convert
    // only the crop points to character counts so Georgian, emoji, and other
    // multi-byte text cannot be cut in half.
    let start = body[..start_byte]
        .chars()
        .count()
        .saturating_sub(CONTEXT_CHARS);
    let match_end = body[..end_byte].chars().count();
    let total = body.chars().count();
    let end = (match_end + CONTEXT_CHARS).min(total);
    let excerpt: String = body.chars().skip(start).take(end - start).collect();
    let excerpt = excerpt.split_whitespace().collect::<Vec<_>>().join(" ");
    let prefix = if start > 0 { "..." } else { "" };
    let suffix = if end < total { "..." } else { "" };
    format!("{prefix}{excerpt}{suffix}")
}

/// Walks the same visible note tree as list/search. `[[` is just a cheap
/// candidate filter; precise link parsing lives with the TipTap Markdown rule.
fn collect_note_link_sources(root: &Path, dir: &Path, out: &mut Vec<NoteLinkSource>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(metadata) = safe_tree_metadata(root, &path) else {
            continue;
        };
        if metadata.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == TRASH_FOLDER
                || name.eq_ignore_ascii_case("attachments")
                || name.starts_with('.')
            {
                continue;
            }
            collect_note_link_sources(root, &path, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("ki"))
            == Some(true)
        {
            if let Ok(content) = fs::read_to_string(&path) {
                let (_, _, _, body) = parse(&content);
                if body.contains("[[") {
                    out.push(NoteLinkSource {
                        path: path.to_string_lossy().to_string(),
                        body,
                    });
                }
            }
        }
    }
}

/// Copy an asset into the notes folder's `attachments/` subfolder and return
/// its path RELATIVE to the notes folder (`attachments/<name>`), so the note's
/// Markdown stays portable — move the whole notes folder and the links still
/// resolve. Only the source file-NAME is used (no directory part is trusted),
/// and exclusive creation prevents a concurrent import from overwriting it.
#[tauri::command(async)]
pub fn copy_note_asset(app: AppHandle, source_path: String) -> Result<String, String> {
    let dir = notes_dir(&app)?;
    let requested_attachments = dir.join("attachments");
    std::fs::create_dir_all(&requested_attachments)
        .map_err(|e| format!("Cannot create attachments folder: {e}"))?;
    let attachments = attachment_dir_in_notes(&dir)?;

    let src = PathBuf::from(&source_path);
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("attachment");
    let safe_stem: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    let safe_stem = if safe_stem.is_empty() {
        "attachment".to_string()
    } else {
        safe_stem
    };
    let ext: String = src
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("bin")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let ext = if ext.is_empty() {
        "bin".to_string()
    } else {
        ext
    };

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut source = fs::File::open(&src).map_err(|e| format!("Cannot read attachment: {e}"))?;

    for attempt in 0..1000 {
        let suffix = if attempt == 0 {
            nanos.to_string()
        } else {
            format!("{nanos}-{attempt}")
        };
        let name = format!("{safe_stem}-{suffix}.{ext}");
        let dest = attachments.join(&name);
        let mut destination = match OpenOptions::new().write(true).create_new(true).open(&dest) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create attachment: {error}")),
        };

        if let Err(error) = std::io::copy(&mut source, &mut destination)
            .and_then(|_| destination.flush())
        {
            let _ = fs::remove_file(&dest);
            return Err(format!("Cannot copy attachment: {error}"));
        }
        return Ok(format!("attachments/{name}"));
    }

    Err("Cannot allocate a unique attachment name".to_string())
}

/// Return the physical attachment directory only when it is the direct child
/// of the physical notes directory. This rejects a symlink or Windows junction
/// named `attachments` that would otherwise make a note-authored link reach an
/// unrelated location through the OS opener.
fn attachment_dir_in_notes(dir: &Path) -> Result<PathBuf, String> {
    let notes_root = fs::canonicalize(dir)
        .map_err(|error| format!("Cannot resolve notes folder: {error}"))?;
    let requested_attachments = dir.join("attachments");
    let metadata = fs::symlink_metadata(&requested_attachments)
        .map_err(|error| format!("Cannot read attachments folder: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Attachments folder is not a regular directory".to_string());
    }

    let attachments = fs::canonicalize(&requested_attachments)
        .map_err(|error| format!("Cannot resolve attachments folder: {error}"))?;
    let relative = attachments
        .strip_prefix(&notes_root)
        .map_err(|_| "Attachments folder is outside the notes folder".to_string())?;
    if relative != Path::new("attachments") {
        return Err("Attachments folder is outside the notes folder".to_string());
    }
    Ok(attachments)
}

fn note_attachment_in_dir(dir: &Path, relative: &str) -> Result<NoteAttachment, String> {
    let safe = safe_relative_path(Path::new(relative))?;
    let mut components = safe.components();
    let is_attachment = matches!(components.next(), Some(Component::Normal(name)) if name == "attachments")
        && matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none();
    if !is_attachment {
        return Err("Attachment must be a direct file in attachments".to_string());
    }

    let attachment_name = safe
        .file_name()
        .ok_or_else(|| "Attachment name is invalid".to_string())?;
    let attachments = attachment_dir_in_notes(dir)?;
    let path = attachments.join(attachment_name);
    let metadata = fs::symlink_metadata(&path).map_err(|error| format!("Cannot read attachment: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Attachment is not a regular file".to_string());
    }
    let path = fs::canonicalize(&path).map_err(|error| format!("Cannot resolve attachment: {error}"))?;
    if path.parent() != Some(attachments.as_path()) {
        return Err("Attachment is outside the notes folder".to_string());
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Attachment name is invalid".to_string())?
        .to_string();
    Ok(NoteAttachment {
        path: path.to_string_lossy().to_string(),
        name,
        bytes: metadata.len(),
    })
}

#[tauri::command(async)]
pub fn get_note_attachment(app: AppHandle, path: String) -> Result<NoteAttachment, String> {
    note_attachment_in_dir(&notes_dir(&app)?, &path)
}

/// Reveal the notes folder in the OS file manager.
#[tauri::command]
pub fn open_notes_folder(app: AppHandle) -> Result<(), String> {
    let dir = notes_dir(&app)?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("Cannot open notes folder: {e}"))?;
    }
    #[cfg(not(windows))]
    {
        let _ = &dir;
    }
    Ok(())
}

/// All notes, newest first (pinned first). Reads one file at a time and keeps
/// only the summary — never the full set of bodies in memory at once. The
/// `.trash` subfolder is skipped automatically (we only collect top-level
/// `.ki` files, not directories).
/// Walk the notes folder recursively, collecting every `.ki` file tagged with
/// its relative folder. Skips the trash, the attachments dir, and any dotfolder.
fn collect_notes(base: &Path, dir: &Path, out: &mut Vec<NoteSummary>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(metadata) = safe_tree_metadata(base, &path) else {
            continue;
        };
        if metadata.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == TRASH_FOLDER
                || name.eq_ignore_ascii_case("attachments")
                || name.starts_with('.')
            {
                continue;
            }
            collect_notes(base, &path, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("ki"))
            == Some(true)
        {
            if let Some(mut summary) = summarize(&path) {
                summary.folder = path
                    .parent()
                    .and_then(|p| p.strip_prefix(base).ok())
                    .map(|r| r.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                out.push(summary);
            }
        }
    }
}

#[tauri::command(async)]
pub fn list_notes(app: AppHandle) -> Result<Vec<NoteSummary>, String> {
    let dir = notes_dir(&app)?;
    let mut out: Vec<NoteSummary> = Vec::new();
    collect_notes(&dir, &dir, &mut out);
    // Pinned first, then most-recently-modified.
    out.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.modified_ms.cmp(&a.modified_ms))
    });
    Ok(out)
}

/// Raw note content plus a revision. The frontend keeps the revision and sends
/// it back with each save, so a concurrent external edit cannot be overwritten.
#[tauri::command(async)]
pub fn read_note(app: AppHandle, path: String) -> Result<NoteFile, String> {
    let p = note_path(&app, &path)?;
    read_note_file(&p)
}

/// Revision-checked note write. `(async)` so the blocking disk work runs on a
/// worker thread, NOT the Tauri main/event-loop thread: autosave flushes often
/// and notes live under Documents (frequently OneDrive-synced / AV-scanned),
/// where a main-thread write could stall the event loop and freeze the whole UI
/// while the user is typing or renaming. The write stages a synced temporary
/// file, then replaces the live note only after revision and snapshot checks.
#[tauri::command(async)]
pub fn write_note(
    app: AppHandle,
    path: String,
    content: String,
    expected_revision: Option<String>,
) -> Result<NoteWriteResult, String> {
    let root = notes_dir(&app)?;
    let p = note_path(&app, &path)?;
    write_note_if_current(&root, &p, &content, expected_revision)
}

#[tauri::command(async)]
pub fn list_note_revisions(app: AppHandle, path: String) -> Result<Vec<NoteRevision>, String> {
    let root = notes_dir(&app)?;
    let note = note_path(&app, &path)?;
    list_note_revisions_for_path(&root, &note)
}

#[tauri::command(async)]
pub fn read_note_revision(
    app: AppHandle,
    path: String,
    id: String,
) -> Result<NoteRevisionContent, String> {
    let root = notes_dir(&app)?;
    let note = note_path(&app, &path)?;
    let revision = revision_path(&root, &note, &id)?;
    let entry = revision_from_path(&revision)
        .ok_or_else(|| "That note revision is invalid".to_string())?;
    let content = fs::read_to_string(&revision)
        .map_err(|error| format!("Cannot read note revision: {error}"))?;
    Ok(NoteRevisionContent {
        id: entry.id,
        created_ms: entry.created_ms,
        content,
    })
}

#[tauri::command(async)]
pub fn restore_note_revision(
    app: AppHandle,
    path: String,
    id: String,
    expected_revision: Option<String>,
) -> Result<NoteWriteResult, String> {
    let root = notes_dir(&app)?;
    let note = note_path(&app, &path)?;
    let revision = revision_path(&root, &note, &id)?;
    let content = fs::read_to_string(&revision)
        .map_err(|error| format!("Cannot read note revision: {error}"))?;
    write_note_if_current(&root, &note, &content, expected_revision)
}

#[tauri::command(async)]
pub fn diff_note_revision(app: AppHandle, path: String, id: String) -> Result<String, String> {
    let root = notes_dir(&app)?;
    let note = note_path(&app, &path)?;
    let revision = revision_path(&root, &note, &id)?;
    let previous = fs::read_to_string(&revision)
        .map_err(|error| format!("Cannot read note revision: {error}"))?;
    let current = fs::read_to_string(&note).map_err(|error| format!("Cannot read note: {error}"))?;
    Ok(diffy::create_patch(&previous, &current).to_string())
}

/// Open the canonical daily note for `YYYY-MM-DD`, creating it exactly once
/// in the visible `Daily/` folder. The caller supplies fully serialized `.ki`
/// content so note frontmatter remains owned by the frontend store.
#[tauri::command(async)]
pub fn open_or_create_daily_note(
    app: AppHandle,
    date_key: String,
    content: String,
) -> Result<NoteSummary, String> {
    if !is_daily_note_key(&date_key) {
        return Err("Invalid daily note date".to_string());
    }
    let root = notes_dir(&app)?;
    let dir = folder_path(&app, DAILY_FOLDER)?;
    fs::create_dir_all(&dir).map_err(|error| format!("Cannot create Daily folder: {error}"))?;
    let dir = resolve_notes_path(&root, &dir.to_string_lossy(), false)?;
    open_or_create_daily_note_in_dir(&dir, &date_key, &content, DAILY_FOLDER.to_string())
}

/// Create a new note. The frontend supplies the full initial `content` (so the
/// frontmatter format stays single-sourced there) and a `file_base` for the
/// file name. An optional `target_folder` keeps automatic captures as ordinary
/// notes in a user-visible folder.
#[tauri::command(async)]
pub fn create_note(
    app: AppHandle,
    file_base: String,
    content: String,
    target_folder: Option<String>,
) -> Result<NoteSummary, String> {
    let root = notes_dir(&app)?;
    let dir = match target_folder.filter(|folder| !folder.trim().is_empty()) {
        Some(folder) => folder_path(&app, &folder)?,
        None => root.clone(),
    };
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create folder: {e}"))?;
    let dir = if paths_match(&dir, &root) {
        root.clone()
    } else {
        resolve_notes_path(&root, &dir.to_string_lossy(), false)?
    };
    let folder = dir
        .strip_prefix(&root)
        .ok()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    create_note_in_dir(&dir, &file_base, &content, folder)
}

fn is_absent(path: &Path) -> bool {
    matches!(fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
}

fn ensure_trash_dir(root: &Path) -> Result<PathBuf, String> {
    let trash = root.join(TRASH_FOLDER);
    fs::create_dir_all(&trash).map_err(|error| format!("Cannot create trash folder: {error}"))?;
    let trash = resolve_notes_path(root, &trash.to_string_lossy(), false)?;
    let metadata = fs::symlink_metadata(&trash)
        .map_err(|error| format!("Cannot read trash folder: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Trash folder is not a regular directory".to_string());
    }
    Ok(trash)
}

fn trash_metadata_path(item: &Path) -> Result<PathBuf, String> {
    let parent = item
        .parent()
        .ok_or_else(|| "Trash item has no parent directory".to_string())?;
    let name = item
        .file_name()
        .ok_or_else(|| "Trash item has no name".to_string())?
        .to_string_lossy();
    Ok(parent.join(format!(".{name}{TRASH_METADATA_SUFFIX}")))
}

fn write_trash_metadata(item: &Path, metadata: &TrashMetadata) -> Result<(), String> {
    let path = trash_metadata_path(item)?;
    let body = serde_json::to_vec(metadata)
        .map_err(|error| format!("Cannot serialize trash metadata: {error}"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("Cannot create trash metadata: {error}"))?;
    if let Err(error) = file.write_all(&body).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&path);
        return Err(format!("Cannot save trash metadata: {error}"));
    }
    Ok(())
}

fn read_trash_metadata(item: &Path) -> Option<TrashMetadata> {
    let path = trash_metadata_path(item).ok()?;
    let body = fs::read_to_string(path).ok()?;
    serde_json::from_str(&body).ok()
}

fn direct_trash_item_path(root: &Path, input: &str) -> Result<PathBuf, String> {
    let path = resolve_notes_path(root, input, false)?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "Trash item is outside the notes folder".to_string())?;
    let mut components = relative.components();
    let is_trash = matches!(components.next(), Some(Component::Normal(name)) if name == TRASH_FOLDER);
    let name = components.next();
    if !is_trash
        || !matches!(name, Some(Component::Normal(_)))
        || components.next().is_some()
        || path
            .file_name()
            .is_none_or(|name| name.to_string_lossy().starts_with('.'))
    {
        return Err("Trash item must be directly inside the trash".to_string());
    }
    Ok(path)
}

fn validated_trash_original(root: &Path, value: &str, kind: TrashItemKind) -> Option<PathBuf> {
    let relative = safe_relative_path(Path::new(value)).ok()?;
    let relative = visible_note_relative(root, &root.join(relative)).ok()?;
    match kind {
        TrashItemKind::Note
            if relative
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.eq_ignore_ascii_case("ki"))
                == Some(true) => Some(relative),
        TrashItemKind::Folder if relative.file_name().is_some() => Some(relative),
        _ => None,
    }
}

fn trash_item(root: &Path, path: &Path) -> Result<TrashItem, String> {
    let path = direct_trash_item_path(root, &path.to_string_lossy())?;
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| "That trash item no longer exists".to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("Trash item must not be a link".to_string());
    }
    let kind = if metadata.is_dir() {
        TrashItemKind::Folder
    } else if metadata.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.eq_ignore_ascii_case("ki"))
            == Some(true)
    {
        TrashItemKind::Note
    } else {
        return Err("That trash item is not a note or folder".to_string());
    };
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let (legacy_deleted_ms, legacy_original) = split_trash_name(name);
    let stored = read_trash_metadata(&path).filter(|stored| stored.kind == kind.as_str());
    let original_path = stored
        .as_ref()
        .and_then(|stored| validated_trash_original(root, &stored.original_path, kind))
        .or_else(|| validated_trash_original(root, &legacy_original, kind))
        .ok_or_else(|| "Trash item has an invalid original path".to_string())?;
    let deleted_ms = stored
        .map(|stored| stored.deleted_ms)
        .filter(|stamp| *stamp > 0)
        .or_else(|| (legacy_deleted_ms > 0).then_some(legacy_deleted_ms))
        .unwrap_or_else(|| modified_ms(&path));
    Ok(TrashItem {
        path,
        kind,
        original_path,
        deleted_ms,
    })
}

fn list_trash_items(root: &Path) -> Result<Vec<TrashItem>, String> {
    let requested = root.join(TRASH_FOLDER);
    if is_absent(&requested) {
        return Ok(Vec::new());
    }
    let trash = resolve_notes_path(root, &requested.to_string_lossy(), false)?;
    let entries = fs::read_dir(&trash).map_err(|error| format!("Cannot read trash folder: {error}"))?;
    let mut items = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        if let Ok(item) = trash_item(root, &path) {
            items.push(item);
        }
    }
    items.sort_by(|left, right| right.deleted_ms.cmp(&left.deleted_ms));
    Ok(items)
}

fn history_path_is_available(root: &Path, note: &Path) -> Result<bool, String> {
    Ok(is_absent(&history_path_for_note(root, note)?))
}

fn move_note_with_history(root: &Path, source: &Path, destination: &Path) -> Result<(), String> {
    if !is_absent(destination) || !history_path_is_available(root, destination)? {
        return Err("A note or its history already exists at that destination".to_string());
    }
    let source_history = existing_history_dir(root, source)?;
    let destination_history = history_path_for_note(root, destination)?;
    fs::rename(source, destination).map_err(|error| format!("Cannot move note: {error}"))?;
    if let Some(source_history) = source_history {
        if let Err(error) = fs::rename(&source_history, &destination_history) {
            let _ = fs::rename(destination, source);
            return Err(format!("Cannot move note history: {error}"));
        }
    }
    Ok(())
}

fn allocate_trash_path(
    root: &Path,
    trash: &Path,
    name: &str,
    kind: TrashItemKind,
) -> Result<PathBuf, String> {
    let stamp = now_ms();
    for attempt in 0..1000 {
        let file_name = if attempt == 0 {
            format!("{stamp}-{name}")
        } else {
            format!("{stamp}-{attempt}-{name}")
        };
        let candidate = trash.join(file_name);
        if !is_absent(&candidate) || !is_absent(&trash_metadata_path(&candidate)?) {
            continue;
        }
        if kind == TrashItemKind::Note && !history_path_is_available(root, &candidate)? {
            continue;
        }
        return Ok(candidate);
    }
    Err("Cannot allocate a unique trash item".to_string())
}

fn move_to_trash(
    root: &Path,
    source: &Path,
    original_path: PathBuf,
    kind: TrashItemKind,
) -> Result<(), String> {
    let trash = ensure_trash_dir(root)?;
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Note path is invalid".to_string())?;
    let destination = allocate_trash_path(root, &trash, name, kind)?;
    match kind {
        TrashItemKind::Note => move_note_with_history(root, source, &destination)?,
        TrashItemKind::Folder => fs::rename(source, &destination)
            .map_err(|error| format!("Cannot move folder to trash: {error}"))?,
    }
    let metadata = TrashMetadata {
        original_path: original_path.to_string_lossy().replace('\\', "/"),
        kind: kind.as_str().to_string(),
        deleted_ms: now_ms(),
    };
    if let Err(error) = write_trash_metadata(&destination, &metadata) {
        match kind {
            TrashItemKind::Note => {
                let _ = move_note_with_history(root, &destination, source);
            }
            TrashItemKind::Folder => {
                let _ = fs::rename(&destination, source);
            }
        }
        return Err(error);
    }
    Ok(())
}

fn restore_destination(root: &Path, original: &Path, kind: TrashItemKind) -> Result<PathBuf, String> {
    let parent_relative = original.parent().unwrap_or_else(|| Path::new(""));
    let parent = if parent_relative.as_os_str().is_empty() {
        root.to_path_buf()
    } else {
        let requested_parent = root.join(parent_relative);
        fs::create_dir_all(&requested_parent)
            .map_err(|error| format!("Cannot recreate original folder: {error}"))?;
        resolve_notes_path(root, &requested_parent.to_string_lossy(), false)?
    };
    let name = original
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Original note path is invalid".to_string())?;
    for attempt in 1..=9999 {
        let candidate_name = if attempt == 1 {
            name.to_string()
        } else if kind == TrashItemKind::Note {
            let stem = Path::new(name)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("note");
            let extension = Path::new(name)
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("ki");
            format!("{stem}-{attempt}.{extension}")
        } else {
            format!("{name}-{attempt}")
        };
        let candidate = parent.join(candidate_name);
        if !is_absent(&candidate) {
            continue;
        }
        if kind == TrashItemKind::Note && !history_path_is_available(root, &candidate)? {
            continue;
        }
        return Ok(candidate);
    }
    Err("Cannot find an available restore name".to_string())
}

fn collect_restored_note_paths(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(metadata) = safe_tree_metadata(root, &path) else {
            continue;
        };
        if metadata.is_dir() {
            if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
            {
                continue;
            }
            collect_restored_note_paths(root, &path, out);
        } else if metadata.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.eq_ignore_ascii_case("ki"))
                == Some(true)
        {
            out.push(path.to_string_lossy().to_string());
        }
    }
}

fn restore_trash_item(root: &Path, source: &Path) -> Result<Vec<String>, String> {
    let item = trash_item(root, source)?;
    let destination = restore_destination(root, &item.original_path, item.kind)?;
    match item.kind {
        TrashItemKind::Note => move_note_with_history(root, &item.path, &destination)?,
        TrashItemKind::Folder => fs::rename(&item.path, &destination)
            .map_err(|error| format!("Cannot restore folder: {error}"))?,
    }
    if let Ok(metadata) = trash_metadata_path(&item.path) {
        let _ = fs::remove_file(metadata);
    }
    let mut restored = Vec::new();
    match item.kind {
        TrashItemKind::Note => restored.push(destination.to_string_lossy().to_string()),
        TrashItemKind::Folder => collect_restored_note_paths(root, &destination, &mut restored),
    }
    Ok(restored)
}

fn delete_trash_item(root: &Path, source: &Path) -> Result<(), String> {
    let item = trash_item(root, source)?;
    match item.kind {
        TrashItemKind::Note => {
            let history = existing_history_dir(root, &item.path)?;
            if let Some(history) = history {
                fs::remove_dir_all(history)
                    .map_err(|error| format!("Cannot delete note history: {error}"))?;
            }
            fs::remove_file(&item.path).map_err(|error| format!("Cannot delete note: {error}"))?;
        }
        TrashItemKind::Folder => {
            fs::remove_dir_all(&item.path).map_err(|error| format!("Cannot delete folder: {error}"))?;
        }
    }
    if let Ok(metadata) = trash_metadata_path(&item.path) {
        let _ = fs::remove_file(metadata);
    }
    Ok(())
}

/// Move a note into the `.trash` subfolder (reversible — not a hard delete).
#[tauri::command(async)]
pub fn delete_note(app: AppHandle, path: String) -> Result<(), String> {
    let root = notes_dir(&app)?;
    let note = note_path(&app, &path)?;
    if is_absent(&note) {
        return Ok(());
    }
    let original = visible_note_relative(&root, &note)?;
    move_to_trash(&root, &note, original, TrashItemKind::Note)
}

/// A note sitting in `.trash`, for the restore UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashedNote {
    /// Absolute path to the trashed `.ki` file.
    pub path: String,
    /// Title from frontmatter, else the original file stem.
    pub title: String,
    /// When it was trashed — parsed from the filename's timestamp prefix,
    /// falling back to the file's mtime if that prefix is missing.
    pub deleted_ms: i64,
    /// "note" or "folder". Folder items restore their entire visible tree.
    pub kind: String,
}

/// Split a trashed filename (`<ms>-<original>.ki`) back into its parts.
/// Returns (deleted_ms, original_file_name). Falls back to (0, whole name)
/// when the prefix isn't a timestamp — e.g. a file the user dropped in by hand.
fn split_trash_name(file_name: &str) -> (i64, String) {
    match file_name.split_once('-') {
        Some((stamp, rest)) if !stamp.is_empty() && stamp.chars().all(|c| c.is_ascii_digit()) => {
            (stamp.parse::<i64>().unwrap_or(0), rest.to_string())
        }
        _ => (0, file_name.to_string()),
    }
}

/// Everything currently in the trash, newest first.
///
/// The trash was write-only until now: `delete_note` moved notes in and
/// nothing ever listed, restored, or emptied it, so it grew without bound and
/// recovery meant opening Explorer.
#[tauri::command(async)]
pub fn list_trashed_notes(app: AppHandle) -> Result<Vec<TrashedNote>, String> {
    let root = notes_dir(&app)?;
    let mut out = Vec::new();
    for item in list_trash_items(&root)? {
        let title = match item.kind {
            TrashItemKind::Note => fs::read_to_string(&item.path)
                .ok()
                .and_then(|content| parse(&content).0)
                .unwrap_or_else(|| {
                    item.original_path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("Untitled")
                        .to_string()
                }),
            TrashItemKind::Folder => item
                .original_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Untitled folder")
                .to_string(),
        };
        out.push(TrashedNote {
            path: item.path.to_string_lossy().to_string(),
            title,
            deleted_ms: item.deleted_ms,
            kind: item.kind.as_str().to_string(),
        });
    }
    Ok(out)
}

/// Restore a trashed note or folder to its recorded original location. Returns
/// every restored visible note path so callers can refresh their indexes.
#[tauri::command(async)]
pub fn restore_trashed_note(app: AppHandle, path: String) -> Result<Vec<String>, String> {
    let root = notes_dir(&app)?;
    restore_trash_item(&root, Path::new(&path))
}

/// Permanently delete ONE trashed note. Irreversible.
#[tauri::command(async)]
pub fn delete_trashed_note(app: AppHandle, path: String) -> Result<(), String> {
    let root = notes_dir(&app)?;
    delete_trash_item(&root, Path::new(&path))
}

/// Permanently delete everything in the trash. Returns how many were removed.
#[tauri::command(async)]
pub fn empty_note_trash(app: AppHandle) -> Result<usize, String> {
    let root = notes_dir(&app)?;
    let items = list_trash_items(&root)?;
    for item in &items {
        delete_trash_item(&root, &item.path)?;
    }
    Ok(items.len())
}

/// Recursively collect every user subfolder (relative, forward-slash) under the
/// notes dir, skipping trash/attachments/dotfolders.
fn collect_folders(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(metadata) = safe_tree_metadata(base, &path) else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == TRASH_FOLDER || name.eq_ignore_ascii_case("attachments") || name.starts_with('.') {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(base) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
        collect_folders(base, &path, out);
    }
}

/// All user note subfolders, sorted — for the folder tree / move-to picker.
#[tauri::command(async)]
pub fn list_note_folders(app: AppHandle) -> Result<Vec<String>, String> {
    let dir = notes_dir(&app)?;
    let mut out: Vec<String> = Vec::new();
    collect_folders(&dir, &dir, &mut out);
    out.sort();
    Ok(out)
}

/// Create a (possibly nested) note subfolder.
#[tauri::command(async)]
pub fn create_note_folder(app: AppHandle, folder: String) -> Result<(), String> {
    let root = notes_dir(&app)?;
    let p = folder_path(&app, &folder)?;
    fs::create_dir_all(&p).map_err(|e| format!("Cannot create folder: {e}"))?;
    let _ = resolve_notes_path(&root, &p.to_string_lossy(), false)?;
    Ok(())
}

/// Rename / move a note subfolder (and everything inside it).
#[tauri::command(async)]
pub fn rename_note_folder(app: AppHandle, from: String, to: String) -> Result<(), String> {
    let root = notes_dir(&app)?;
    let src = folder_path(&app, &from)?;
    let dest = folder_path(&app, &to)?;
    if is_absent(&src) {
        return Err("Folder no longer exists".into());
    }
    if !is_absent(&dest) {
        return Err("A folder with that name already exists".into());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("Cannot create folder: {error}"))?;
        if !paths_match(parent, &root) {
            let _ = resolve_notes_path(&root, &parent.to_string_lossy(), false)?;
        }
    }
    fs::rename(&src, &dest).map_err(|e| format!("Cannot rename folder: {e}"))?;
    Ok(())
}

/// Move a note subfolder (and its notes) into the `.trash` — reversible, like
/// `delete_note`. Never a hard delete.
#[tauri::command(async)]
pub fn delete_note_folder(app: AppHandle, folder: String) -> Result<(), String> {
    let root = notes_dir(&app)?;
    let src = folder_path(&app, &folder)?;
    if is_absent(&src) {
        return Ok(());
    }
    let original = visible_note_relative(&root, &src)?;
    move_to_trash(&root, &src, original, TrashItemKind::Folder)
}

/// Move a note into `target_folder` (empty = root). Returns the note's new
/// absolute path. Auto-renames on a name collision in the destination.
#[tauri::command(async)]
pub fn move_note(app: AppHandle, path: String, target_folder: String) -> Result<String, String> {
    let root = notes_dir(&app)?;
    let src = note_path(&app, &path)?;
    if is_absent(&src) {
        return Err("Note no longer exists".into());
    }
    let dest_dir = if target_folder.trim().is_empty() {
        root.clone()
    } else {
        folder_path(&app, &target_folder)?
    };
    fs::create_dir_all(&dest_dir).map_err(|e| format!("Cannot create folder: {e}"))?;
    let dest_dir = if paths_match(&dest_dir, &root) {
        root.clone()
    } else {
        resolve_notes_path(&root, &dest_dir.to_string_lossy(), false)?
    };
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("note").to_string();
    for n in 1..=9999 {
        let dest = if n == 1 {
            dest_dir.join(format!("{stem}.ki"))
        } else {
            dest_dir.join(format!("{stem}-{n}.ki"))
        };
        if paths_match(&src, &dest) {
            return Ok(src.to_string_lossy().to_string());
        }
        if !is_absent(&dest) || !history_path_is_available(&root, &dest)? {
            continue;
        }
        move_note_with_history(&root, &src, &dest)?;
        return Ok(dest.to_string_lossy().to_string());
    }
    Err("Too many notes with this name".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this whole feature exists for: `preview` stops at
    /// PREVIEW_LEN (160) chars, so text past it used to be unfindable from
    /// the Notes search box even though the note was sitting right there.
    #[test]
    fn finds_text_past_the_160_char_preview() {
        let dir = temp_notes_dir("past-preview");

        // Padding pushes the needle well beyond PREVIEW_LEN, so a match here
        // can ONLY come from reading the real body.
        let padding = "lorem ipsum dolor sit amet ".repeat(20);
        assert!(padding.len() > PREVIEW_LEN, "fixture must exceed the preview");
        write_note_file(
            &dir.join("meeting.ki"),
            "---\ntitle: Meeting notes\n---\n",
            &format!("{padding}\n\ninvoice #4471 is still unpaid\n"),
        );

        let hits = matches_in(&dir, "4471");
        assert_eq!(hits.len(), 1, "needle past the preview must be found");
        assert!(hits[0].path.ends_with("meeting.ki"));
        assert!(hits[0].snippet.contains("invoice #4471"));

        // Sanity: the preview genuinely does NOT contain it, so this test
        // would have failed before the fix rather than passing by accident.
        let body = fs::read_to_string(dir.join("meeting.ki")).unwrap();
        let (_, _, _, parsed) = parse(&body);
        assert!(!preview_of(&parsed).contains("4471"));

        cleanup(&dir);
    }

    /// Matching is case-insensitive both ways, like the client-side
    /// title/tag filter it merges with.
    #[test]
    fn body_match_is_case_insensitive() {
        let dir = temp_notes_dir("case");
        write_note_file(&dir.join("a.ki"), "", "The Quarterly BUDGET is due\n");

        assert_eq!(matches_in(&dir, "budget").len(), 1);
        assert_eq!(matches_in(&dir, "QUARTERLY").len(), 1);
        assert_eq!(matches_in(&dir, "nonexistent").len(), 0);

        cleanup(&dir);
    }

    #[test]
    fn body_search_keeps_unicode_snippets_and_treats_queries_as_literals() {
        let dir = temp_notes_dir("unicode-search");
        write_note_file(
            &dir.join("unicode.ki"),
            "",
            "Before 🙂 ქართული context C++ [x] after\n",
        );

        let unicode_hits = matches_in(&dir, "🙂 ქართული");
        assert_eq!(unicode_hits.len(), 1);
        assert!(unicode_hits[0].snippet.contains("🙂 ქართული"));

        let literal_hits = matches_in(&dir, "C++ [x]");
        assert_eq!(literal_hits.len(), 1);
        assert!(literal_hits[0].snippet.contains("C++ [x]"));

        cleanup(&dir);
    }

    /// Search must never surface a note the list itself won't show, or a
    /// user gets a result they cannot click.
    #[test]
    fn skips_trash_attachments_and_dotfolders_but_recurses_real_folders() {
        let dir = temp_notes_dir("skips");
        let needle = "findme";

        for hidden in [TRASH_FOLDER, "attachments", ".git"] {
            let sub = dir.join(hidden);
            fs::create_dir_all(&sub).unwrap();
            write_note_file(&sub.join("h.ki"), "", "findme in a hidden place\n");
        }
        // A real subfolder MUST still be searched — the skip list can't be
        // so broad it stops recursing.
        let work = dir.join("Work");
        fs::create_dir_all(&work).unwrap();
        write_note_file(&work.join("real.ki"), "", "findme in a real folder\n");
        // Non-.ki files are not notes.
        fs::write(dir.join("note.txt"), "findme in a txt").unwrap();

        let hits = matches_in(&dir, needle);
        assert_eq!(hits.len(), 1, "only the real subfolder note should match");
        assert!(hits[0].path.ends_with("real.ki"));

        cleanup(&dir);
    }

    #[test]
    fn link_sources_stay_in_the_visible_note_tree() {
        let dir = temp_notes_dir("link-sources");
        write_note_file(&dir.join("linked.ki"), "", "See [[Roadmap]]\n");
        write_note_file(&dir.join("plain.ki"), "", "No links here\n");

        let hidden = dir.join(TRASH_FOLDER);
        fs::create_dir_all(&hidden).unwrap();
        write_note_file(&hidden.join("hidden.ki"), "", "See [[Roadmap]]\n");

        let sources = link_sources_in(&dir);
        assert_eq!(sources.len(), 1);
        assert!(sources[0].path.ends_with("linked.ki"));

        cleanup(&dir);
    }

    #[test]
    fn creates_in_the_requested_folder_with_its_own_collision_space() {
        let root = temp_notes_dir("create-in-folder");
        let inbox = root.join("Inbox");
        fs::create_dir_all(&inbox).unwrap();
        fs::write(inbox.join("capture.ki"), "existing").unwrap();

        let note = create_note_in_dir(&inbox, "Capture", "body", "Inbox".to_string()).unwrap();

        assert!(note.path.ends_with("capture-2.ki"));
        assert_eq!(note.folder, "Inbox");
        let content = fs::read_to_string(inbox.join("capture-2.ki")).unwrap();
        assert_eq!(content, "body");

        cleanup(&root);
    }

    #[test]
    fn attachment_metadata_stays_inside_the_direct_attachments_folder() {
        let root = temp_notes_dir("attachment-metadata");
        let attachments = root.join("attachments");
        fs::create_dir_all(&attachments).unwrap();
        fs::write(attachments.join("report.pdf"), b"abc").unwrap();
        fs::create_dir_all(attachments.join("nested")).unwrap();

        let asset = note_attachment_in_dir(&root, "attachments/report.pdf").unwrap();
        assert_eq!(asset.name, "report.pdf");
        assert_eq!(asset.bytes, 3);
        assert!(Path::new(&asset.path).ends_with(Path::new("attachments").join("report.pdf")));
        assert!(note_attachment_in_dir(&root, "report.pdf").is_err());
        assert!(note_attachment_in_dir(&root, "attachments/nested/file.txt").is_err());
        assert!(note_attachment_in_dir(&root, "attachments/nested").is_err());

        cleanup(&root);
    }

    #[test]
    fn daily_note_key_accepts_only_a_safe_iso_day() {
        assert!(is_daily_note_key("2026-07-30"));
        assert!(!is_daily_note_key("2026/07/30"));
        assert!(!is_daily_note_key("2026-7-30"));
        assert!(!is_daily_note_key("../2026-07-30"));
    }

    #[test]
    fn daily_note_creation_is_idempotent() {
        let root = temp_notes_dir("daily-note");
        let daily = root.join(DAILY_FOLDER);
        fs::create_dir_all(&daily).unwrap();

        let first = open_or_create_daily_note_in_dir(
            &daily,
            "2026-07-30",
            "first content",
            DAILY_FOLDER.to_string(),
        )
        .unwrap();
        let second = open_or_create_daily_note_in_dir(
            &daily,
            "2026-07-30",
            "second content",
            DAILY_FOLDER.to_string(),
        )
        .unwrap();

        assert_eq!(first.path, second.path);
        assert_eq!(second.folder, DAILY_FOLDER);
        assert_eq!(fs::read_to_string(&daily.join("2026-07-30.ki")).unwrap(), "first content");

        cleanup(&root);
    }

    #[test]
    fn revision_checked_write_preserves_an_external_change() {
        let dir = temp_notes_dir("revision-conflict");
        let path = dir.join("note.ki");
        fs::write(&path, "first version").unwrap();
        let opened = read_note_file(&path).unwrap();

        fs::write(&path, "changed elsewhere").unwrap();
        let conflict = write_note_if_current(&dir, &path, "my local version", Some(opened.revision)).unwrap();

        assert!(!conflict.saved);
        assert_eq!(conflict.conflict.as_deref(), Some("changed"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "changed elsewhere");

        let saved = write_note_if_current(&dir, &path, "my local version", conflict.revision.clone()).unwrap();
        assert!(saved.saved);
        assert_eq!(fs::read_to_string(&path).unwrap(), "my local version");

        cleanup(&dir);
    }

    #[test]
    fn revision_checked_write_can_restore_a_missing_note_only_on_explicit_retry() {
        let dir = temp_notes_dir("revision-missing");
        let path = dir.join("note.ki");
        fs::write(&path, "first version").unwrap();
        let opened = read_note_file(&path).unwrap();
        fs::remove_file(&path).unwrap();

        let missing = write_note_if_current(&dir, &path, "my local version", Some(opened.revision)).unwrap();
        assert!(!missing.saved);
        assert_eq!(missing.conflict.as_deref(), Some("missing"));
        assert!(!path.exists());

        let restored = write_note_if_current(&dir, &path, "my local version", None).unwrap();
        assert!(restored.saved);
        assert_eq!(fs::read_to_string(&path).unwrap(), "my local version");

        cleanup(&dir);
    }

    #[test]
    fn missing_recovery_never_overwrites_an_existing_empty_note() {
        let dir = temp_notes_dir("missing-recovery-existing");
        let path = dir.join("note.ki");
        fs::write(&path, "").unwrap();

        let blocked = write_note_if_current(&dir, &path, "my local version", None).unwrap();

        assert!(!blocked.saved);
        assert_eq!(blocked.conflict.as_deref(), Some("changed"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "");

        cleanup(&dir);
    }

    #[test]
    fn local_revisions_are_capped_and_can_restore_a_snapshot() {
        let root = temp_notes_dir("revision-history");
        let note = root.join("note.ki");
        fs::write(&note, "version 0").unwrap();
        let mut opened = read_note_file(&note).unwrap();

        for version in 1..=(MAX_NOTE_REVISIONS + 4) {
            let result = write_note_if_current(
                &root,
                &note,
                &format!("version {version}"),
                Some(opened.revision),
            )
            .unwrap();
            assert!(result.saved);
            opened = read_note_file(&note).unwrap();
        }

        let revisions = list_note_revisions_for_path(&root, &note).unwrap();
        assert_eq!(revisions.len(), MAX_NOTE_REVISIONS);
        let selected = revisions.last().unwrap();
        let snapshot = fs::read_to_string(revision_path(&root, &note, &selected.id).unwrap()).unwrap();
        let restored = write_note_if_current(&root, &note, &snapshot, Some(opened.revision)).unwrap();
        assert!(restored.saved);
        assert_eq!(fs::read_to_string(&note).unwrap(), snapshot);

        cleanup(&root);
    }

    #[test]
    fn trash_restores_a_root_note_with_its_history() {
        let root = temp_notes_dir("trash-root-note");
        let note = root.join("roadmap.ki");
        fs::write(&note, "first").unwrap();
        let opened = read_note_file(&note).unwrap();
        assert!(write_note_if_current(&root, &note, "second", Some(opened.revision))
            .unwrap()
            .saved);

        move_to_trash(
            &root,
            &note,
            visible_note_relative(&root, &note).unwrap(),
            TrashItemKind::Note,
        )
        .unwrap();
        let items = list_trash_items(&root).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, TrashItemKind::Note);
        assert!(!note.exists());

        let restored = restore_trash_item(&root, &items[0].path).unwrap();
        assert_eq!(restored, vec![note.to_string_lossy().to_string()]);
        assert_eq!(fs::read_to_string(&note).unwrap(), "second");
        let revisions = list_note_revisions_for_path(&root, &note).unwrap();
        assert_eq!(revisions.len(), 1);
        assert_eq!(
            fs::read_to_string(revision_path(&root, &note, &revisions[0].id).unwrap()).unwrap(),
            "first"
        );

        cleanup(&root);
    }

    #[test]
    fn trash_lifecycle_restores_and_purges_folder_items() {
        let root = temp_notes_dir("trash-folder");
        let folder = root.join("Work");
        let note = folder.join("plan.ki");
        fs::create_dir_all(&folder).unwrap();
        fs::write(&note, "plan").unwrap();

        move_to_trash(
            &root,
            &folder,
            visible_note_relative(&root, &folder).unwrap(),
            TrashItemKind::Folder,
        )
        .unwrap();
        let item = list_trash_items(&root).unwrap().pop().unwrap();
        assert_eq!(item.kind, TrashItemKind::Folder);
        let restored = restore_trash_item(&root, &item.path).unwrap();
        assert_eq!(restored, vec![note.to_string_lossy().to_string()]);
        assert!(note.exists());

        move_to_trash(
            &root,
            &folder,
            visible_note_relative(&root, &folder).unwrap(),
            TrashItemKind::Folder,
        )
        .unwrap();
        let item = list_trash_items(&root).unwrap().pop().unwrap();
        delete_trash_item(&root, &item.path).unwrap();
        assert!(list_trash_items(&root).unwrap().is_empty());
        assert!(!folder.exists());

        cleanup(&root);
    }

    #[test]
    fn restore_rejects_live_paths_outside_trash() {
        let root = temp_notes_dir("trash-path-guard");
        let live = root.join("live.ki");
        fs::write(&live, "keep me").unwrap();

        assert!(restore_trash_item(&root, &live).is_err());
        assert_eq!(fs::read_to_string(&live).unwrap(), "keep me");

        cleanup(&root);
    }

    #[test]
    fn concurrent_note_creation_never_overwrites_a_collision() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let root = temp_notes_dir("create-race");
        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for body in ["first", "second"] {
            let dir = root.clone();
            let barrier = Arc::clone(&barrier);
            workers.push(thread::spawn(move || {
                barrier.wait();
                create_note_in_dir(&dir, "Capture", body, String::new()).unwrap()
            }));
        }
        let created = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert_ne!(created[0].path, created[1].path);
        let mut contents = created
            .iter()
            .map(|note| fs::read_to_string(&note.path).unwrap())
            .collect::<Vec<_>>();
        contents.sort();
        assert_eq!(contents, vec!["first", "second"]);

        cleanup(&root);
    }

    #[cfg(unix)]
    #[test]
    fn walkers_skip_symlinked_note_directories() {
        use std::os::unix::fs::symlink;

        let root = temp_notes_dir("link-root");
        let outside = temp_notes_dir("link-outside");
        fs::write(outside.join("outside.ki"), "not a note").unwrap();
        symlink(&outside, root.join("escape")).unwrap();

        let mut notes = Vec::new();
        collect_notes(&root, &root, &mut notes);
        assert!(notes.is_empty());

        cleanup(&root);
        cleanup(&outside);
    }

    #[cfg(windows)]
    #[test]
    fn walkers_skip_symlinked_note_directories() {
        use std::os::windows::fs::symlink_dir;

        let root = temp_notes_dir("link-root");
        let outside = temp_notes_dir("link-outside");
        fs::write(outside.join("outside.ki"), "not a note").unwrap();
        if symlink_dir(&outside, root.join("escape")).is_err() {
            cleanup(&root);
            cleanup(&outside);
            return;
        }

        let mut notes = Vec::new();
        collect_notes(&root, &root, &mut notes);
        assert!(notes.is_empty());

        cleanup(&root);
        cleanup(&outside);
    }

    /// Trashed files are `<ms>-<original>.ki`. Restore has to recover the
    /// original name — and must not mangle a name that merely contains a
    /// hyphen, or restoring "my-note.ki" would yield "note.ki".
    #[test]
    fn split_trash_name_recovers_the_original_file_name() {
        assert_eq!(
            split_trash_name("1700000000000-meeting.ki"),
            (1_700_000_000_000, "meeting.ki".to_string())
        );
        // Hyphens in the original name survive — only the FIRST segment is
        // eaten, and only when it's all digits.
        assert_eq!(
            split_trash_name("1700000000000-my-long-note.ki"),
            (1_700_000_000_000, "my-long-note.ki".to_string())
        );
        // No timestamp prefix (hand-dropped file): keep the whole name, and
        // signal 0 so the caller falls back to mtime.
        assert_eq!(
            split_trash_name("my-note.ki"),
            (0, "my-note.ki".to_string())
        );
        assert_eq!(split_trash_name("plain.ki"), (0, "plain.ki".to_string()));
    }

    // ── helpers ──────────────────────────────────────────────────────────

    fn matches_in(dir: &Path, needle: &str) -> Vec<NoteBodyMatch> {
        let escaped = regex::escape(needle);
        let matcher = RegexBuilder::new(&escaped)
            .case_insensitive(true)
            .unicode(true)
            .build()
            .unwrap();
        let mut out = Vec::new();
        collect_body_matches(dir, dir, &matcher, &mut out);
        out
    }

    fn link_sources_in(dir: &Path) -> Vec<NoteLinkSource> {
        let mut out = Vec::new();
        collect_note_link_sources(dir, dir, &mut out);
        out
    }

    fn write_note_file(path: &Path, frontmatter: &str, body: &str) {
        fs::write(path, format!("{frontmatter}{body}")).unwrap();
    }

    /// Unique per test + per run so the three tests can run concurrently and
    /// a crashed run can't poison the next one.
    fn temp_notes_dir(tag: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("kil-notes-test-{tag}-{unique}"));
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(dir).unwrap()
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }
}
