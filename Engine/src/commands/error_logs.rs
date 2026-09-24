//! Local diagnostic log — append-only file of errors + warnings the
//! app + its frontend encounter.
//!
//! Why this matters: when a user hits a problem we can't reproduce
//! locally, the difference between "give us a screenshot" and "open
//! the log folder, attach the file" is enormous. Bug reports that
//! include the actual error trail are 10× faster to triage. Without
//! this, we're guessing.
//!
//! Privacy posture:
//!   - Logs stay on the user's machine. Nothing auto-uploads.
//!   - File paths in error messages may appear (e.g. "Cannot read
//!     C:\Users\jane\Documents\..."). File CONTENTS never appear —
//!     we only log error metadata.
//!   - Settings → Diagnostics surfaces the log + a "Copy / Save /
//!     Open folder" toolbar so the user explicitly chooses what to
//!     share.
//!
//! Storage:
//!   - One JSON-lines file per UTC day under
//!     %APPDATA%\KeepItLocal\logs\keepitlocal-YYYY-MM-DD.log
//!   - Max retention 7 days; older files get pruned on each write.
//!   - Per-file soft cap of 1 MB; once exceeded we still append (we
//!     never lose a recent error) but stop emitting new files for
//!     that day.
//!
//! Format: one JSON object per line. Easy to grep, easy to parse:
//!   {"timestampMs":1723500000000,"level":"error","source":"frontend",
//!    "message":"Cannot save: ...","details":null}

use super::preferences;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

/// Hard cap on entries returned to the frontend in one `list` call.
/// 500 is plenty for a viewer; anyone wanting more should export to
/// file. Keeps the JSON payload bounded.
const MAX_LIST_ENTRIES: usize = 500;

/// Days of log files to keep on disk. Anything older is pruned on
/// the next write. 7 covers a typical "I noticed a bug a few days
/// ago, what happened?" investigation window.
const RETENTION_DAYS: i64 = 7;

/// Folder name under preferences_dir() where logs land. Co-located
/// with settings.json + activity_log so users have ONE place to look
/// for KeepItLocal's local data.
const LOG_FOLDER_NAME: &str = "logs";

/// Single mutex serializes file I/O so the JSON-lines stay one-per-line
/// even when both backend and frontend log concurrently.
static LOG_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// Unix epoch milliseconds, UTC.
    pub timestamp_ms: i64,
    /// "error" | "warn" | "info". Drives the badge color in the UI.
    pub level: String,
    /// Where the entry came from — typically "frontend",
    /// "rust:<command-name>", "voice", etc. Free-form; not indexed.
    pub source: String,
    /// Short human message — what went wrong. Surfaced verbatim.
    pub message: String,
    /// Optional longer context — stack trace, file path, payload
    /// shape. Shown in the expandable detail row of the UI.
    #[serde(default)]
    pub details: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogInput {
    pub level: String,
    pub source: String,
    pub message: String,
    pub details: Option<String>,
}

/// Redact absolute Windows paths from a log string.
///
/// Replaces every occurrence of `X:\...` (drive letter + colon + backslash)
/// with `<path>/<filename>` so the file name stays visible for debugging
/// but the full directory tree (which can expose the Windows user account
/// name, organisation name, project paths, etc.) is stripped out.
///
/// Example:
///   "Cannot read C:\Users\jane\Documents\report.pdf"
///   → "Cannot read <path>/report.pdf"
///
/// The walk is byte-level so it handles both `\` and `/` separators and
/// works correctly on strings that contain multiple paths.
fn sanitize_paths(s: &str) -> String {
    if s.is_empty() {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0usize;
    while i < len {
        // Look for a drive letter followed by `:\` or `:/`
        // Pattern: ASCII letter at position i, `:` at i+1, `\` or `/` at i+2
        if i + 2 < len
            && bytes[i].is_ascii_alphabetic()
            && bytes[i + 1] == b':'
            && (bytes[i + 2] == b'\\' || bytes[i + 2] == b'/')
        {
            // Consume the full path: everything up to but not including the
            // next whitespace, null, quote, or common non-path delimiter.
            let path_start = i;
            let mut j = i + 3;
            while j < len {
                let b = bytes[j];
                // Stop at whitespace, null, quotes, angle brackets, parens,
                // semicolons, commas — characters that can't be in a Windows
                // path name and reliably mark the end of the path token.
                if b == b' '
                    || b == b'\t'
                    || b == b'\n'
                    || b == b'\r'
                    || b == b'\0'
                    || b == b'"'
                    || b == b'\''
                    || b == b'<'
                    || b == b'>'
                    || b == b'('
                    || b == b')'
                    || b == b';'
                    || b == b','
                {
                    break;
                }
                j += 1;
            }
            let path_token = &s[path_start..j];
            // Extract just the filename component (last segment after `\` or `/`).
            let filename = path_token
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or(path_token);
            if filename.is_empty() {
                out.push_str("<path>");
            } else {
                out.push_str("<path>/");
                out.push_str(filename);
            }
            i = j;
        } else {
            // Fast path: copy one char (which may be multi-byte UTF-8).
            // We know bytes[i] is not in the middle of a surrogate because
            // Rust strings are valid UTF-8, so we can safely iterate chars.
            let ch = s[i..].chars().next().unwrap_or('\0');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Convert a unix epoch millisecond stamp into a `YYYY-MM-DD` UTC date
/// string for the per-day filename. Done by hand so we don't drag in
/// chrono just for one date format.
fn date_string_from_ms(ms: i64) -> String {
    // Days since unix epoch (1970-01-01).
    let days = ms / (24 * 60 * 60 * 1000);
    // Convert via the proleptic Gregorian calendar. Algorithm from
    // Howard Hinnant's date library — handles leap years correctly
    // for any reasonable date.
    let z = days + 719468; // shift epoch to 0000-03-01 era
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = y + (if m <= 2 { 1 } else { 0 });
    format!("{:04}-{:02}-{:02}", year, m, d)
}

fn log_folder(app: &AppHandle) -> Result<PathBuf, String> {
    let folder = preferences::preferences_dir(app)?.join(LOG_FOLDER_NAME);
    fs::create_dir_all(&folder)
        .map_err(|e| format!("Cannot create logs folder: {e}"))?;
    Ok(folder)
}

fn log_file_for_today(app: &AppHandle) -> Result<PathBuf, String> {
    let date = date_string_from_ms(now_ms());
    Ok(log_folder(app)?.join(format!("keepitlocal-{date}.log")))
}

/// Drop log files older than RETENTION_DAYS. Called opportunistically
/// from `log_event` so retention enforcement happens whenever the log
/// grows — no scheduler needed. Failures are silent (one bad readdir
/// shouldn't break logging).
fn prune_old_logs(app: &AppHandle) {
    let Ok(folder) = log_folder(app) else { return };
    let cutoff_ms = now_ms() - RETENTION_DAYS * 24 * 60 * 60 * 1000;
    let cutoff_date = date_string_from_ms(cutoff_ms);
    let Ok(entries) = fs::read_dir(&folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.starts_with("keepitlocal-") || !name.ends_with(".log") {
            continue;
        }
        // The middle YYYY-MM-DD part is sortable as a string, so a
        // string comparison is exactly the right correctness call.
        let date_part = &name["keepitlocal-".len()..name.len() - ".log".len()];
        if date_part < cutoff_date.as_str() {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Append one entry to today's log file. Best-effort: failures here
/// don't propagate (we can't very well log the failure to log
/// something). The caller's primary action already succeeded.
#[tauri::command]
pub fn log_event(app: AppHandle, input: LogInput) -> Result<(), String> {
    let _guard = LOG_LOCK
        .lock()
        .map_err(|_| "Log lock was poisoned".to_string())?;

    prune_old_logs(&app);

    let entry = LogEntry {
        timestamp_ms: now_ms(),
        level: input.level.trim().to_lowercase(),
        source: input.source.trim().to_string(),
        // Strip absolute Windows paths before writing so log files don't
        // accidentally capture the user's account name, organisation, or
        // project structure. The filename component is preserved for
        // debuggability; only the directory prefix is redacted.
        message: sanitize_paths(input.message.trim()),
        details: input
            .details
            .map(|s| sanitize_paths(s.trim()))
            .filter(|s| !s.is_empty()),
    };
    let line = serde_json::to_string(&entry)
        .map_err(|e| format!("Cannot serialize log entry: {e}"))?;

    let path = log_file_for_today(&app)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("Cannot open log file: {e}"))?;
    writeln!(file, "{line}").map_err(|e| format!("Cannot write log entry: {e}"))?;
    Ok(())
}

/// Read the most recent N entries across all retained log files.
/// Returns newest-first. Always capped at MAX_LIST_ENTRIES regardless
/// of `limit` to keep the IPC payload bounded.
#[tauri::command]
pub fn list_log_entries(
    app: AppHandle,
    limit: Option<usize>,
) -> Result<Vec<LogEntry>, String> {
    let _guard = LOG_LOCK
        .lock()
        .map_err(|_| "Log lock was poisoned".to_string())?;

    let cap = limit
        .unwrap_or(MAX_LIST_ENTRIES)
        .min(MAX_LIST_ENTRIES);

    let folder = log_folder(&app)?;
    // Collect log files, newest first by name (ISO date sorts
    // lexicographically — same answer as parsing).
    let mut files: Vec<PathBuf> = fs::read_dir(&folder)
        .map_err(|e| format!("Cannot read log folder: {e}"))?
        .filter_map(|r| r.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.starts_with("keepitlocal-") && n.ends_with(".log"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files.reverse();

    let mut out: Vec<LogEntry> = Vec::with_capacity(cap);
    'files: for path in files {
        let Ok(file) = OpenOptions::new().read(true).open(&path) else {
            continue;
        };
        // Read the file then iterate IN REVERSE so we get newest-
        // first within a day. JSON-lines are append-only, so the
        // last line is the most recent entry that day.
        let lines: Vec<String> = BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .collect();
        for line in lines.iter().rev() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<LogEntry>(trimmed) {
                out.push(entry);
                if out.len() >= cap {
                    break 'files;
                }
            }
        }
    }

    Ok(out)
}

/// Wipe every retained log file. Surfaced as an explicit user action
/// in Settings → Diagnostics for the same reason `clear_activity`
/// exists: hand-the-machine-over moments and "I want a clean slate
/// before reporting a bug" workflows.
#[tauri::command]
pub fn clear_log_entries(app: AppHandle) -> Result<(), String> {
    let _guard = LOG_LOCK
        .lock()
        .map_err(|_| "Log lock was poisoned".to_string())?;

    let folder = log_folder(&app)?;
    let entries = fs::read_dir(&folder)
        .map_err(|e| format!("Cannot read log folder: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_log = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|n| n.starts_with("keepitlocal-") && n.ends_with(".log"))
            .unwrap_or(false);
        if is_log {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

/// Return the absolute path to the log folder so the UI can open it
/// in Explorer (or copy the path for support emails).
#[tauri::command]
pub fn get_log_folder(app: AppHandle) -> Result<String, String> {
    let folder = log_folder(&app)?;
    Ok(folder.to_string_lossy().to_string())
}

/// Build a single text blob of all retained log entries — newest
/// first, human-readable timestamps. Used by Settings →
/// Diagnostics → "Copy logs" so users can paste straight into
/// support emails / GitHub issues without dealing with JSON.
#[tauri::command]
pub fn export_log_text(app: AppHandle) -> Result<String, String> {
    let entries = list_log_entries(app, Some(MAX_LIST_ENTRIES))?;
    let mut out = String::new();
    for entry in entries {
        out.push_str(&format!(
            "[{}] {:5} {} — {}\n",
            format_timestamp(entry.timestamp_ms),
            entry.level.to_uppercase(),
            entry.source,
            entry.message
        ));
        if let Some(details) = entry.details {
            for line in details.lines() {
                out.push_str("  ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    if out.is_empty() {
        out.push_str("(no entries)\n");
    }
    Ok(out)
}

/// Render a unix-ms stamp as YYYY-MM-DD HH:MM:SS UTC for the export
/// view. Same hand-rolled date arithmetic as date_string_from_ms.
fn format_timestamp(ms: i64) -> String {
    let date = date_string_from_ms(ms);
    let secs_today = ((ms.rem_euclid(24 * 60 * 60 * 1000)) / 1000) as u32;
    let h = secs_today / 3600;
    let m = (secs_today % 3600) / 60;
    let s = secs_today % 60;
    format!("{date} {h:02}:{m:02}:{s:02} UTC")
}
