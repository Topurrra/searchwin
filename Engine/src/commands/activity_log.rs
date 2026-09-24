//! Append-only operation history.
//!
//! Every meaningful tool action — "shredded 12 files", "merged 3 PDFs into
//! report.pdf", "stripped metadata from 5 images" — gets a row in this log
//! so users can answer the question "what did KeepItLocal do to my files
//! this week?"
//!
//! Why this matters: trust. Without an audit trail, users can't verify
//! that a privacy-tool actually did what it claimed. With one, the
//! Settings → Activity panel becomes a tangible "this is what I have
//! evidence of" surface — a stronger pitch for "local-first" than just
//! the word.
//!
//! Storage: a single redb key `"activity_log_v1"` holding a Vec of
//! entries, capped at MAX_ENTRIES so the log doesn't grow unbounded.
//! Encrypted at rest by the local_db DPAPI layer like every other value.
//!
//! Frontend logs entries via `record_activity`. Settings reads them via
//! `list_activity`. Users can wipe the log with `clear_activity`.

use super::local_db;
use super::preferences;
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

const ACTIVITY_KEY: &str = "activity_log_v1";

/// Hard cap on entries. Older entries get evicted FIFO when this is
/// exceeded. 500 is enough for several months of normal use without
/// the redb file ballooning, and any user investigating a specific
/// past action is overwhelmingly looking at recent activity.
const MAX_ENTRIES: usize = 500;

/// One activity entry. Designed as a flat schema so the redb row stays
/// small and serializes cheaply. `outcome` differentiates "the
/// operation completed successfully" from "user cancelled" or "failed".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: u64,
    pub timestamp_ms: i64,
    /// The tool that performed the action — e.g. "privacy-sanitizer",
    /// "pdf-merge", "file-shredder". Matches the screen id in
    /// appScreens.ts so the frontend can deep-link from the log into
    /// the originating tool.
    pub tool_id: String,
    /// Human-readable verb + summary, e.g. "Merged 3 PDFs",
    /// "Shredded 12 files". Localized at log time by the caller.
    pub summary: String,
    /// Optional structured details — file count, byte count, etc.
    /// Free-form so different tools can include what makes sense.
    #[serde(default)]
    pub details: Option<String>,
    /// "success" / "cancelled" / "failed". Drives the badge color in
    /// the UI.
    pub outcome: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivityFile {
    #[serde(default)]
    entries: Vec<ActivityEntry>,
    #[serde(default)]
    next_id: u64,
}

static ACTIVITY_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn db_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(local_db::database_path_for_dir(&preferences::preferences_dir(app)?))
}

fn load(app: &AppHandle) -> Result<ActivityFile, String> {
    let path = db_path(app)?;
    let stored: Option<ActivityFile> = local_db::read_json(&path, ACTIVITY_KEY)?;
    Ok(stored.unwrap_or_default())
}

fn save(app: &AppHandle, file: &ActivityFile) -> Result<(), String> {
    let path = db_path(app)?;
    local_db::write_json(&path, ACTIVITY_KEY, file)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityRecordInput {
    pub tool_id: String,
    pub summary: String,
    pub details: Option<String>,
    pub outcome: String,
}

/// Log a new activity entry. Lightweight — single redb write inside the
/// global activity mutex. Best-effort: failures don't bubble (the
/// caller's actual operation already succeeded; losing one log line
/// shouldn't make their tool feel broken).
#[tauri::command]
pub fn record_activity(app: AppHandle, input: ActivityRecordInput) -> Result<(), String> {
    let _guard = ACTIVITY_LOCK
        .lock()
        .map_err(|_| "Activity log lock poisoned".to_string())?;
    let mut file = load(&app)?;

    let id = file.next_id.max(1);
    file.next_id = id + 1;
    let entry = ActivityEntry {
        id,
        timestamp_ms: now_ms(),
        tool_id: input.tool_id.trim().to_string(),
        summary: input.summary.trim().to_string(),
        details: input.details.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        outcome: input.outcome.trim().to_string(),
    };
    file.entries.push(entry);

    // FIFO eviction to MAX_ENTRIES. Drop the oldest entries (front of
    // the vec) so recent activity survives.
    if file.entries.len() > MAX_ENTRIES {
        let drop_count = file.entries.len() - MAX_ENTRIES;
        file.entries.drain(0..drop_count);
    }

    save(&app, &file)
}

/// Return all entries, newest first. Frontend can paginate or filter
/// client-side; the cap of 500 makes that trivial.
#[tauri::command]
pub fn list_activity(app: AppHandle) -> Result<Vec<ActivityEntry>, String> {
    let file = load(&app)?;
    let mut entries = file.entries;
    entries.reverse();
    Ok(entries)
}

/// Wipe the entire log. Surfaced as an explicit user action in
/// Settings → Activity for users who want a clean slate (selling the
/// machine, handing it over, etc.). Different from reset_all_data
/// because this only wipes the audit trail, leaving everything else.
///
/// Implementation note: we DON'T `load()` first. The previous version
/// did, which meant a deserialize error in the existing log (schema
/// drift, partial corruption) surfaced as "Clear failed: Cannot parse
/// local database value: …" — but the user's intent ("wipe everything")
/// was perfectly satisfied by writing an empty file directly. So now
/// we skip the read entirely and just overwrite with a fresh
/// `ActivityFile::default()`. `next_id` resets to 0; that's fine for
/// a clear-everything semantic (post-clear entries start fresh too).
#[tauri::command]
pub fn clear_activity(app: AppHandle) -> Result<(), String> {
    let _guard = ACTIVITY_LOCK
        .lock()
        .map_err(|_| "Activity log lock poisoned".to_string())?;
    let empty = ActivityFile::default();
    save(&app, &empty)
}
