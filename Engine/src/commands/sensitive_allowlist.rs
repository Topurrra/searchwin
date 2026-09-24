//! Sensitive findings allowlist — Phase 6.5-2 (2026-05-27).
//!
//! Lets the user dismiss a false positive once and never see it flagged
//! again on the same string. Powers the "Mark as not sensitive" action
//! on every finding in the Secret Leak Scanner, the Clipboard sensitive
//! badge, and the Privacy Audit Unencrypted Secrets section.
//!
//! ## What's actually stored
//!
//! NEVER the raw secret. Storing `password123` on disk to remember "the
//! user said this isn't really a password" would defeat the entire
//! point of a privacy-first product. Instead we store
//!
//! ```text
//! BLAKE3(device_salt || normalized_match_text)
//! ```
//!
//! as a hex string. The salt is generated once on first use and lives
//! in the same DPAPI-wrapped redb table — so even an attacker who reads
//! the redb file can't precompute a rainbow table against common
//! passwords, and the hashes aren't portable across machines (different
//! salt). Normalization is `.trim()` only — case + spacing matter for
//! tokens (`abc` and `ABC` are different secrets).
//!
//! ## Why a per-device salt, not a per-user pepper
//!
//! Per-device is sufficient: an attacker who can read the redb file
//! already has at least filesystem-level access to the user's account
//! (or has copied the file off the machine). They could also read the
//! salt from the same DB. The salt's only job is to defeat off-device
//! rainbow-table attacks (e.g. someone who exfiltrates just the hash
//! list and not the salt). DPAPI then protects the salt + hashes
//! together at rest, so this is belt-and-braces.
//!
//! ## API surface
//!
//! Three Tauri commands + one library function:
//!
//!   • `dismiss_finding(text)` — mark `text` as a dismissed FP.
//!   • `restore_allowlist()` — clear every dismissal (one-click reset).
//!   • `count_dismissed_findings()` — drives the Settings badge ("12
//!     items in your allowlist").
//!   • `apply_allowlist(findings, source_text)` — library helper used
//!     by `scan_text_for_findings` to filter out dismissed findings
//!     before returning to the frontend. Pure function; takes the
//!     source text so it can slice each finding's span.

use crate::commands::local_db;
use crate::commands::preferences;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use tauri::AppHandle;

use super::sensitive_scan::Finding;

/// Storage key inside the shared `keepitlocal.redb` JSON table.
const ALLOWLIST_KEY: &str = "sensitive_allowlist_v1";

/// In-memory cache so we don't hit redb + DPAPI for every finding the
/// scanner produces. Read-on-first-access, invalidated on every write.
/// Wrapped in `Mutex<Option<…>>` so the lazy fill is concurrency-safe.
static CACHE: LazyLock<Mutex<Option<AllowlistFile>>> = LazyLock::new(|| Mutex::new(None));

/// Persistent shape — versioned so future changes (per-org allowlists,
/// expiration timestamps, etc.) can add fields without a migration step.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct AllowlistFile {
    /// 32 bytes of device-local salt, hex-encoded for JSON friendliness.
    /// Generated on first dismissal; never rotated automatically (rotating
    /// would invalidate every existing dismissal — the user can hit
    /// "Restore allowlist" if they want a clean slate).
    #[serde(default)]
    salt_hex: String,
    /// Hex-encoded BLAKE3 hashes of dismissed match texts. Sorted +
    /// deduplicated on every write so reads can binary-search later
    /// if the list ever grows large.
    #[serde(default)]
    hashes: Vec<String>,
}

// ─── Internal helpers ───────────────────────────────────────────────

fn allowlist_db_path(app: &AppHandle) -> Result<PathBuf, String> {
    // Reuse preferences dir + shared redb — same recipe every other
    // persistent feature (snippets, time tracker, …) uses.
    let dir = preferences::preferences_dir(app)?;
    Ok(local_db::database_path_for_dir(&dir))
}

fn load(app: &AppHandle) -> Result<AllowlistFile, String> {
    if let Some(cached) = CACHE.lock().map_err(lock_err)?.clone() {
        return Ok(cached);
    }
    let path = allowlist_db_path(app)?;
    let stored: Option<AllowlistFile> = local_db::read_json(&path, ALLOWLIST_KEY)?;
    let file = stored.unwrap_or_default();
    *CACHE.lock().map_err(lock_err)? = Some(file.clone());
    Ok(file)
}

fn save(app: &AppHandle, file: &AllowlistFile) -> Result<(), String> {
    let path = allowlist_db_path(app)?;
    local_db::write_json(&path, ALLOWLIST_KEY, file)?;
    *CACHE.lock().map_err(lock_err)? = Some(file.clone());
    Ok(())
}

fn lock_err<E>(_: E) -> String {
    "Allowlist cache lock was poisoned".to_string()
}

/// Lazily provision a 32-byte device salt. Stored hex-encoded so it
/// round-trips through JSON without base64 padding noise. Once
/// generated it's never changed — see module docs.
fn ensure_salt(file: &mut AllowlistFile) {
    if file.salt_hex.len() == 64 {
        return; // Already provisioned.
    }
    let mut bytes = [0u8; 32];
    // `getrandom` via `blake3::Hasher::new_keyed` would also work, but
    // the explicit `rand::thread_rng` keeps the source of entropy
    // legible — and `rand` is already a direct dep.
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut bytes);
    file.salt_hex = hex::encode(bytes);
}

/// Hash `match_text` with the device salt. Returns 64-char hex.
fn hash_with_salt(salt_hex: &str, match_text: &str) -> String {
    let salt_bytes = hex::decode(salt_hex).unwrap_or_default();
    let mut hasher = blake3::Hasher::new();
    hasher.update(&salt_bytes);
    hasher.update(match_text.trim().as_bytes());
    hex::encode(hasher.finalize().as_bytes())
}

// ─── Library helpers (used by other backend code) ───────────────────

/// True when the given match text has been dismissed by the user.
/// Cheap — O(log n) into a sorted hash list, plus one BLAKE3 hash per
/// call (microseconds). The cache means we don't hit redb either.
///
/// Returns `false` on any error (failure to load, hash mismatch, etc.)
/// — failing OPEN here means a momentary cache miss surfaces a finding
/// the user previously dismissed, which is recoverable; failing CLOSED
/// would silently hide real findings the user never approved.
pub fn is_dismissed(app: &AppHandle, match_text: &str) -> bool {
    let Ok(file) = load(app) else {
        return false;
    };
    if file.salt_hex.is_empty() || file.hashes.is_empty() {
        return false;
    }
    let h = hash_with_salt(&file.salt_hex, match_text);
    file.hashes.binary_search(&h).is_ok()
}

/// Filter `findings` so any dismissed match is excluded. Used by the
/// `scan_text_for_findings` Tauri command before returning to the
/// frontend. Takes the original `source_text` so each finding's span
/// can be sliced to recover the matched text for hashing — we don't
/// store the text on the Finding itself (the frontend can derive it).
pub fn apply_allowlist(
    app: &AppHandle,
    findings: Vec<Finding>,
    source_text: &str,
) -> Vec<Finding> {
    let Ok(file) = load(app) else {
        return findings;
    };
    if file.salt_hex.is_empty() || file.hashes.is_empty() {
        return findings;
    }
    findings
        .into_iter()
        .filter(|f| {
            // Defensive bounds: a malformed finding (start > end, or end
            // > source.len()) shouldn't drop ALL findings — just include
            // it as-is so the user sees something.
            if f.end > source_text.len() || f.start >= f.end {
                return true;
            }
            let slice = &source_text[f.start..f.end];
            let h = hash_with_salt(&file.salt_hex, slice);
            file.hashes.binary_search(&h).is_err()
        })
        .collect()
}

// ─── Tauri commands ─────────────────────────────────────────────────

/// Mark `text` as a dismissed false positive. The string itself is
/// hashed before being written — the secret never lands on disk.
/// Idempotent: dismissing the same text twice is a no-op.
#[tauri::command]
pub async fn dismiss_finding(app: AppHandle, text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("Cannot dismiss empty text.".to_string());
        }
        let mut file = load(&app)?;
        ensure_salt(&mut file);
        let hash = hash_with_salt(&file.salt_hex, trimmed);
        if file.hashes.binary_search(&hash).is_err() {
            // Keep the list sorted so `is_dismissed` can binary-search.
            file.hashes.push(hash);
            file.hashes.sort_unstable();
        }
        save(&app, &file)
    })
    .await
    .map_err(|e| format!("Allowlist worker failed: {e}"))?
}

/// Remove a previously dismissed finding so it surfaces again. Mirrors
/// `dismiss_finding`'s shape (same hash + lookup recipe).
#[tauri::command]
pub async fn undismiss_finding(app: AppHandle, text: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let mut file = load(&app)?;
        if file.salt_hex.is_empty() {
            return Ok(()); // Nothing was ever dismissed.
        }
        let hash = hash_with_salt(&file.salt_hex, trimmed);
        if let Ok(pos) = file.hashes.binary_search(&hash) {
            file.hashes.remove(pos);
            save(&app, &file)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Allowlist worker failed: {e}"))?
}

/// Clear every dismissal. One-click reset surfaced under
/// Settings → Privacy → Sensitive findings allowlist. Returns the
/// number removed so the UI can confirm ("Restored 12 findings.").
#[tauri::command]
pub async fn restore_allowlist(app: AppHandle) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut file = load(&app)?;
        let removed = file.hashes.len();
        file.hashes.clear();
        // Keep the salt — rotating it would invalidate future dismissals
        // for no benefit. Clearing only the hash list is the cheap +
        // correct reset.
        save(&app, &file)?;
        Ok(removed)
    })
    .await
    .map_err(|e| format!("Allowlist worker failed: {e}"))?
}

/// Count of dismissed findings — drives the badge in Settings + the
/// "Allowlist N items" label next to the Restore button.
#[tauri::command]
pub async fn count_dismissed_findings(app: AppHandle) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || load(&app).map(|f| f.hashes.len()))
        .await
        .map_err(|e| format!("Allowlist worker failed: {e}"))?
}

/// Check a single string from the frontend (e.g. a Clipboard preview
/// the user is about to act on). Useful for UI that wants to render
/// the "dismissed" state on individual findings without re-running the
/// scanner. Cheap — see `is_dismissed` notes.
#[tauri::command]
pub async fn is_finding_dismissed(app: AppHandle, text: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || Ok(is_dismissed(&app, &text)))
        .await
        .map_err(|e| format!("Allowlist worker failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hash function is deterministic given a fixed salt. Regression
    /// guard: if anyone refactors the hashing recipe (e.g. forgets to
    /// trim) every existing dismissal silently invalidates.
    #[test]
    fn hash_is_deterministic_and_salt_sensitive() {
        let salt_a = "00".repeat(32);
        let salt_b = "ff".repeat(32);
        let h_a1 = hash_with_salt(&salt_a, "secret");
        let h_a2 = hash_with_salt(&salt_a, "secret");
        let h_b = hash_with_salt(&salt_b, "secret");
        assert_eq!(h_a1, h_a2, "same salt + text must produce same hash");
        assert_ne!(h_a1, h_b, "different salt must produce different hash");
        assert_eq!(h_a1.len(), 64, "BLAKE3 256-bit hash is 64 hex chars");
    }

    #[test]
    fn hash_normalizes_via_trim_only() {
        // Trimming is the ONLY normalization — case + interior spaces
        // matter because tokens are case-sensitive.
        let salt = "00".repeat(32);
        assert_eq!(
            hash_with_salt(&salt, "abc"),
            hash_with_salt(&salt, "  abc  "),
            "trim should normalize whitespace at the edges"
        );
        assert_ne!(
            hash_with_salt(&salt, "abc"),
            hash_with_salt(&salt, "ABC"),
            "case must matter — 'abc' and 'ABC' are different tokens"
        );
        assert_ne!(
            hash_with_salt(&salt, "a b c"),
            hash_with_salt(&salt, "abc"),
            "interior whitespace must matter"
        );
    }

    #[test]
    fn ensure_salt_provisions_only_once() {
        let mut file = AllowlistFile::default();
        ensure_salt(&mut file);
        let salt_first = file.salt_hex.clone();
        ensure_salt(&mut file);
        assert_eq!(salt_first, file.salt_hex, "salt should not rotate on repeat call");
        assert_eq!(salt_first.len(), 64);
    }
}
