//! Quality Pass Wave 1 / Option C (2026-05-28): persistent hash cache
//! for DuplicateFinder.
//!
//! Same shape as the Wave 8.5 OCR cache (`commands/ocr_cache.rs`) —
//! when the user re-runs the duplicate scanner on a tree they've
//! scanned before, we'd otherwise recompute the BLAKE3 (or SHA-256)
//! of every same-size candidate file from scratch. For a 500 k-photo
//! library that's gigabytes of disk reads per scan; caching them by
//! `(path, mtime)` makes the second-pass scan near-instant for any
//! file whose bytes haven't changed.
//!
//! Cache key: lowercase canonical path. Cache value is the packed
//! tuple `(mtime, hash_algo_id, hash_bytes, last_used_at, created_at)`.
//! On lookup we compare the caller's `mtime` to the stored one — if
//! they match, we trust the hash. If they don't, we treat it as a miss
//! and the caller will recompute + store.
//!
//! Lives in its own redb file `duplicate_hash_cache.redb` under the
//! same state dir as the search index, so it nukes cleanly with a
//! user-initiated data wipe.
//!
//! GC runs at the end of every successful duplicate scan:
//!   1. drop entries last touched more than `ttl_days` ago, then
//!   2. trim oldest entries (LRU) to fit under `max_bytes`.
//!
//! All cache errors are swallowed at the call sites — a cache miss /
//! write failure must never fail the scan. Worst case is one
//! scan's worth of duplicate hashing work.

use redb::{Database, ReadableTable, TableDefinition};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DUPLICATE_CACHE_FILE: &str = "duplicate_hash_cache.redb";

/// 64-byte slot — enough for BLAKE3 (32) or SHA-256 (32). Variable
/// length in the wire format below; the type alias documents intent.
pub type FileHash = Vec<u8>;

const DUPLICATE_CACHE_TABLE: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("duplicate_hash_cache_v1");

/// Default TTL: entries not touched in 90 days are dropped at the next
/// GC pass. Plenty for "scan every month" rhythms.
pub const DEFAULT_TTL_DAYS: u64 = 90;
/// Default total cache cap: 100 MB ≈ ~1 M entries of 32-byte hashes
/// + metadata. Larger libraries trigger an LRU trim.
pub const DEFAULT_MAX_BYTES: u64 = 100 * 1024 * 1024;

/// Serialize cache-file access at the process level. Mirrors the
/// `LOCAL_DB_LOCK` and `OCR_CACHE_LOCK` patterns.
static DUPLICATE_CACHE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

pub fn cache_path_for_dir(dir: &Path) -> PathBuf {
    dir.join(DUPLICATE_CACHE_FILE)
}

/// Hand to the duplicate-scan worker. `None` (when no state dir is
/// available, e.g. a build without app data) disables the cache —
/// the scanner runs exactly as before Option C.
#[derive(Clone, Debug)]
pub struct DuplicateCacheCtx {
    pub db_path: PathBuf,
}

impl DuplicateCacheCtx {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            db_path: cache_path_for_dir(state_dir),
        }
    }
}

/// Pack the cache value. Layout (little-endian):
///   [u64 last_used_at_secs][u64 created_at_secs]
///   [u64 mtime_ms][u8 algo_id][u32 hash_len][hash bytes]
///
/// `algo_id` is 0 for BLAKE3, 1 for SHA-256 (matches the existing
/// `DuplicateScanOptions::hash_algorithm` string). Future algorithms
/// add ids without breaking older entries.
fn pack_value(mtime_ms: u64, algo_id: u8, hash: &[u8], last_used: u64, created: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + 8 + 8 + 1 + 4 + hash.len());
    out.extend_from_slice(&last_used.to_le_bytes());
    out.extend_from_slice(&created.to_le_bytes());
    out.extend_from_slice(&mtime_ms.to_le_bytes());
    out.push(algo_id);
    out.extend_from_slice(&(hash.len() as u32).to_le_bytes());
    out.extend_from_slice(hash);
    out
}

/// Reverse of `pack_value`. Returns `(mtime_ms, algo_id, hash, last_used, created)`
/// or `None` on malformed bytes (older format, truncation) — caller
/// should treat as miss and overwrite.
fn unpack_value(bytes: &[u8]) -> Option<(u64, u8, Vec<u8>, u64, u64)> {
    if bytes.len() < 29 {
        return None;
    }
    let last_used = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let created = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let mtime = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
    let algo = bytes[24];
    let hash_len = u32::from_le_bytes(bytes[25..29].try_into().ok()?) as usize;
    if bytes.len() != 29 + hash_len {
        return None;
    }
    Some((mtime, algo, bytes[29..].to_vec(), last_used, created))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn algo_id(algorithm: &str) -> u8 {
    match algorithm {
        "sha256" => 1,
        _ => 0, // default BLAKE3
    }
}

fn open_db(path: &Path) -> Result<Database, String> {
    let (db, _) = super::local_db::open_redb(path)?;
    let write_txn = db
        .begin_write()
        .map_err(|e| format!("Cannot initialize duplicate cache: {e}"))?;
    {
        let _ = write_txn
            .open_table(DUPLICATE_CACHE_TABLE)
            .map_err(|e| format!("Cannot initialize duplicate cache table: {e}"))?;
    }
    write_txn
        .commit()
        .map_err(|e| format!("Cannot commit duplicate cache init: {e}"))?;
    Ok(db)
}

/// Cache lookup. `Some(hash)` on hit (mtime + algorithm match),
/// `None` on miss / mismatch / error. Safe to ignore all errors — a
/// miss falls through to "compute and store" in the caller.
pub fn lookup(
    ctx: &DuplicateCacheCtx,
    path_lower: &[u8],
    mtime_ms: u64,
    algorithm: &str,
) -> Option<FileHash> {
    let _guard = DUPLICATE_CACHE_LOCK.lock().ok()?;
    let db = open_db(&ctx.db_path).ok()?;
    let value_bytes = {
        let read_txn = db.begin_read().ok()?;
        let table = read_txn.open_table(DUPLICATE_CACHE_TABLE).ok()?;
        let entry = table.get(path_lower).ok()??;
        entry.value().to_vec()
    };
    let (stored_mtime, stored_algo, stored_hash, last_used, created) =
        unpack_value(&value_bytes)?;
    if stored_algo != algo_id(algorithm) {
        return None;
    }
    if stored_mtime != mtime_ms {
        return None;
    }

    // Day-grained touch — avoids hammering redb with one write per
    // hit on cache-heavy scans.
    let now = now_secs();
    if now.saturating_sub(last_used) > 86_400 {
        let touched = pack_value(stored_mtime, stored_algo, &stored_hash, now, created);
        if let Ok(write_txn) = db.begin_write() {
            if let Ok(mut table) = write_txn.open_table(DUPLICATE_CACHE_TABLE) {
                let _ = table.insert(path_lower, touched.as_slice());
            }
            let _ = write_txn.commit();
        }
    }
    Some(stored_hash)
}

/// Cache store. Silent on error — a write failure shouldn't fail the
/// scan. Worst case is one scan's worth of duplicate hashing work.
pub fn store(
    ctx: &DuplicateCacheCtx,
    path_lower: &[u8],
    mtime_ms: u64,
    algorithm: &str,
    hash: &[u8],
) {
    let _guard = match DUPLICATE_CACHE_LOCK.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let db = match open_db(&ctx.db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("duplicate_cache: cannot open for store ({e})");
            return;
        }
    };
    let now = now_secs();
    let value = pack_value(mtime_ms, algo_id(algorithm), hash, now, now);
    let write_txn = match db.begin_write() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("duplicate_cache: cannot begin write for store ({e})");
            return;
        }
    };
    {
        if let Ok(mut table) = write_txn.open_table(DUPLICATE_CACHE_TABLE) {
            let _ = table.insert(path_lower, value.as_slice());
        }
    }
    let _ = write_txn.commit();
}

#[derive(Debug, Clone, Default)]
pub struct GcStats {
    pub entries_before: usize,
    pub entries_after: usize,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub ttl_dropped: usize,
    pub lru_dropped: usize,
}

/// TTL + LRU GC. Called at the end of every successful duplicate scan.
/// Non-fatal — errors are logged.
pub fn gc(db_path: &Path, ttl_days: u64, max_bytes: u64) -> Result<GcStats, String> {
    let _guard = DUPLICATE_CACHE_LOCK
        .lock()
        .map_err(|_| "Duplicate cache lock was poisoned".to_string())?;
    if !db_path.exists() {
        return Ok(GcStats::default());
    }
    let db = open_db(db_path)?;
    let now = now_secs();
    let ttl_cutoff = now.saturating_sub(ttl_days.saturating_mul(86_400));

    let mut survivors: Vec<(Vec<u8>, u64, u64)> = Vec::new();
    let mut ttl_drops: Vec<Vec<u8>> = Vec::new();
    let mut entries_before = 0usize;
    let mut bytes_before = 0u64;
    {
        let read_txn = db
            .begin_read()
            .map_err(|e| format!("Cannot begin duplicate-cache GC read: {e}"))?;
        let table = read_txn
            .open_table(DUPLICATE_CACHE_TABLE)
            .map_err(|e| format!("Cannot open table for duplicate-cache GC: {e}"))?;
        for entry in table
            .iter()
            .map_err(|e| format!("Cannot iterate table for duplicate-cache GC: {e}"))?
        {
            let entry = entry
                .map_err(|e| format!("Cannot read entry for duplicate-cache GC: {e}"))?;
            entries_before += 1;
            let key = entry.0.value().to_vec();
            let val = entry.1.value();
            let val_len = val.len() as u64;
            bytes_before += val_len;
            match unpack_value(val) {
                Some((_mtime, _algo, _hash, last_used, _created))
                    if last_used >= ttl_cutoff =>
                {
                    survivors.push((key, last_used, val_len));
                }
                _ => ttl_drops.push(key),
            }
        }
    }
    let ttl_dropped = ttl_drops.len();
    if !ttl_drops.is_empty() {
        let write_txn = db
            .begin_write()
            .map_err(|e| format!("Cannot begin TTL drop write: {e}"))?;
        {
            let mut table = write_txn
                .open_table(DUPLICATE_CACHE_TABLE)
                .map_err(|e| format!("Cannot open table for TTL drop: {e}"))?;
            for key in &ttl_drops {
                let _ = table.remove(key.as_slice());
            }
        }
        write_txn
            .commit()
            .map_err(|e| format!("Cannot commit TTL drop: {e}"))?;
    }

    let mut bytes_after: u64 = survivors.iter().map(|(_, _, len)| *len).sum();
    let mut lru_drops: Vec<Vec<u8>> = Vec::new();
    if max_bytes > 0 && bytes_after > max_bytes {
        survivors.sort_by_key(|(_, last_used, _)| *last_used);
        let mut bytes_to_drop = bytes_after - max_bytes;
        for (key, _last_used, len) in &survivors {
            if bytes_to_drop == 0 {
                break;
            }
            lru_drops.push(key.clone());
            bytes_to_drop = bytes_to_drop.saturating_sub(*len);
            bytes_after = bytes_after.saturating_sub(*len);
        }
        let write_txn = db
            .begin_write()
            .map_err(|e| format!("Cannot begin LRU drop write: {e}"))?;
        {
            let mut table = write_txn
                .open_table(DUPLICATE_CACHE_TABLE)
                .map_err(|e| format!("Cannot open table for LRU drop: {e}"))?;
            for key in &lru_drops {
                let _ = table.remove(key.as_slice());
            }
        }
        write_txn
            .commit()
            .map_err(|e| format!("Cannot commit LRU drop: {e}"))?;
    }

    let entries_after = entries_before
        .saturating_sub(ttl_dropped)
        .saturating_sub(lru_drops.len());
    Ok(GcStats {
        entries_before,
        entries_after,
        bytes_before,
        bytes_after,
        ttl_dropped,
        lru_dropped: lru_drops.len(),
    })
}
