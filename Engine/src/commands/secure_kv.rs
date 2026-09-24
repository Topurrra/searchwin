//! Generic DPAPI-encrypted key/value store for frontend data that is
//! sensitive but historically lived in plaintext localStorage (e.g. the
//! user-defined commands, which can carry shell payloads and file paths).
//!
//! Values land in the SAME shared encrypted redb as snippets + preferences
//! (`local_db` wraps every write with DPAPI on Windows), so they get the same
//! at-rest protection — without each store needing its own DB file. Keys are
//! namespaced so they can't collide with the other consumers of that redb.

use super::local_db;
use super::preferences;
use std::path::PathBuf;
use tauri::AppHandle;

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = preferences::preferences_dir(app)?;
    Ok(local_db::database_path_for_dir(&dir))
}

fn namespaced(key: &str) -> String {
    format!("securekv:{key}")
}

/// Read a previously-stored string, or `None` if the key was never set.
#[tauri::command]
pub fn secure_kv_get(app: AppHandle, key: String) -> Result<Option<String>, String> {
    let path = db_path(&app)?;
    local_db::read_json::<String>(&path, &namespaced(&key))
}

/// Store a string value, DPAPI-encrypted at rest.
#[tauri::command]
pub fn secure_kv_set(app: AppHandle, key: String, value: String) -> Result<(), String> {
    let path = db_path(&app)?;
    local_db::write_json(&path, &namespaced(&key), &value)
}
