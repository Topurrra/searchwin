//! Quality Pass Wave 1 / DF-1 (2026-05-29): persistent perceptual-hash
//! cache for DuplicateFinder's "Similar images" mode.
//!
//! Same shape as `duplicate_cache.rs` (the exact-byte hash cache) and
//! the older Wave 8.5 OCR cache. Perceptual hashing reads + decodes the
//! whole image — expensive on cold cache (50–500 ms per file depending
//! on dimensions). Caching by `(path, mtime)` makes a re-scan over the
//! same photo library take ~constant time per file regardless of size,
//! so the second scan is "fingerprint-only" speed.
//!
//! Cache key: lowercase canonical path. Cache value: the packed tuple
//! `(mtime_ms, algo_id, phash_u64, last_used_at, created_at)`.
//! On lookup we compare the caller's mtime + algo_id; mismatch ⇒ miss
//! and the caller will recompute + store.
//!
//! `algo_id` lets us extend the algorithm later (16×16 pHash, four-way
//! rotation, etc.) without invalidating the whole cache file. v1 ships
//! with `algo_id = 0` ≡ "dHash 8×8 gradient + DCT preprocess".
//!
//! Lives in its own redb file (`perceptual_hash_cache.redb`) under the
//! same state dir as the search index — disappears cleanly with a
//! user-initiated data wipe.
//!
//! GC pattern matches the OCR / duplicate caches: TTL drop, then LRU
//! trim to fit `max_bytes`. All cache errors are swallowed at the call
//! sites — a cache miss / write failure must never fail the scan.

use redb::{Database, ReadableTable, TableDefinition};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use super::perceptual_hash::PerceptualHash;

pub const PERCEPTUAL_CACHE_FILE: &str = "perceptual_hash_cache.redb";

/// Wire id for the currently-shipping algorithm: dHash 8×8 gradient +
/// DCT preprocess. Future variants (pHash, 16×16, rotation-stable)
/// claim new ids without breaking older entries — older entries with
/// unknown ids are treated as misses and overwritten.
pub const ALGO_DHASH_8X8_DCT: u8 = 0;

const PERCEPTUAL_CACHE_TABLE: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("perceptual_hash_cache_v1");

/// Default TTL: 90 days — same rhythm as the other caches. A user who
/// re-runs the "Similar images" pass quarterly keeps all their work.
pub const DEFAULT_TTL_DAYS: u64 = 90;
/// Default cap: 50 MB ≈ ~1.5 M entries (~33 bytes / entry packed).
/// Larger libraries trigger an LRU trim.
pub const DEFAULT_MAX_BYTES: u64 = 50 * 1024 * 1024;

/// Serialize cache-file access at the process level. Mirrors
/// `DUPLICATE_CACHE_LOCK` / `OCR_CACHE_LOCK`.
static PERCEPTUAL_CACHE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

pub fn cache_path_for_dir(dir: &Path) -> PathBuf {
    dir.join(PERCEPTUAL_CACHE_FILE)
}

/// Hand to the similar-image-scan worker. `None` (no state dir) makes
/// the scanner skip the cache entirely and hash every file fresh.
#[derive(Clone, Debug)]
pub struct PerceptualCacheCtx {
    pub db_path: PathBuf,
}

impl PerceptualCacheCtx {
    pub fn new(state_dir: &Path) -> Self {
        Self {
            db_path: cache_path_for_dir(state_dir),
        }
    }
}

/// Pack the cache value. Layout (little-endian):
///   [u64 last_used_at_secs][u64 created_at_secs]
///   [u64 mtime_ms][u8 algo_id][u64 phash]
/// Fixed-size (33 bytes) — no length prefix needed since the hash is
/// always 64 bits.
fn pack_value(
    mtime_ms: u64,
    algo_id: u8,
    phash: PerceptualHash,
    last_used: u64,
    created: u64,
) -> [u8; 33] {
    let mut out = [0u8; 33];
    out[0..8].copy_from_slice(&last_used.to_le_bytes());
    out[8..16].copy_from_slice(&created.to_le_bytes());
    out[16..24].copy_from_slice(&mtime_ms.to_le_bytes());
    out[24] = algo_id;
    out[25..33].copy_from_slice(&phash.to_le_bytes());
    out
}

/// Reverse of `pack_value`. Returns `None` on malformed / wrong-length
/// bytes — caller treats as miss and overwrites.
fn unpack_value(bytes: &[u8]) -> Option<(u64, u8, PerceptualHash, u64, u64)> {
    if bytes.len() != 33 {
        return None;
    }
    let last_used = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let created = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    let mtime = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
    let algo = bytes[24];
    let phash = u64::from_le_bytes(bytes[25..33].try_into().ok()?);
    Some((mtime, algo, phash, last_used, created))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn open_db(path: &Path) -> Result<Database, String> {
    let (db, _) = super::local_db::open_redb(path)?;
    let write_txn = db
        .begin_write()
        .map_err(|e| format!("Cannot initialize perceptual cache: {e}"))?;
    {
        let _ = write_txn
            .open_table(PERCEPTUAL_CACHE_TABLE)
            .map_err(|e| format!("Cannot initialize perceptual cache table: {e}"))?;
    }
    write_txn
        .commit()
        .map_err(|e| format!("Cannot commit perceptual cache init: {e}"))?;
    Ok(db)
}

/// Cache lookup. `Some(phash)` on hit (mtime + algo match), `None`
/// otherwise or on any error. Safe to ignore errors here — a miss
/// falls through to recompute + store in the caller.
pub fn lookup(
    ctx: &PerceptualCacheCtx,
    path_lower: &[u8],
    mtime_ms: u64,
    algo_id: u8,
) -> Option<PerceptualHash> {
    let _guard = PERCEPTUAL_CACHE_LOCK.lock().ok()?;
    let db = open_db(&ctx.db_path).ok()?;
    let value_bytes = {
        let read_txn = db.begin_read().ok()?;
        let table = read_txn.open_table(PERCEPTUAL_CACHE_TABLE).ok()?;
        let entry = table.get(path_lower).ok()??;
        entry.value().to_vec()
    };
    let (stored_mtime, stored_algo, phash, last_used, created) = unpack_value(&value_bytes)?;
    if stored_algo != algo_id {
        return None;
    }
    if stored_mtime != mtime_ms {
        return None;
    }

    // Day-grained touch — avoids hammering redb with one write per hit
    // on cache-heavy scans of a stable photo library.
    let now = now_secs();
    if now.saturating_sub(last_used) > 86_400 {
        let touched = pack_value(stored_mtime, stored_algo, phash, now, created);
        if let Ok(write_txn) = db.begin_write() {
            if let Ok(mut table) = write_txn.open_table(PERCEPTUAL_CACHE_TABLE) {
                let _ = table.insert(path_lower, touched.as_slice());
            }
            let _ = write_txn.commit();
        }
    }
    Some(phash)
}

/// Cache store. Silent on error — a write failure shouldn't fail the
/// scan. Worst case is one scan's worth of duplicate perceptual hashing.
pub fn store(
    ctx: &PerceptualCacheCtx,
    path_lower: &[u8],
    mtime_ms: u64,
    algo_id: u8,
    phash: PerceptualHash,
) {
    let _guard = match PERCEPTUAL_CACHE_LOCK.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let db = match open_db(&ctx.db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("perceptual_cache: cannot open for store ({e})");
            return;
        }
    };
    let now = now_secs();
    let value = pack_value(mtime_ms, algo_id, phash, now, now);
    let write_txn = match db.begin_write() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("perceptual_cache: cannot begin write for store ({e})");
            return;
        }
    };
    {
        if let Ok(mut table) = write_txn.open_table(PERCEPTUAL_CACHE_TABLE) {
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

/// TTL + LRU GC. Called at the end of every successful similar-image
/// scan. Non-fatal — errors are logged.
pub fn gc(db_path: &Path, ttl_days: u64, max_bytes: u64) -> Result<GcStats, String> {
    let _guard = PERCEPTUAL_CACHE_LOCK
        .lock()
        .map_err(|_| "Perceptual cache lock was poisoned".to_string())?;
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
            .map_err(|e| format!("Cannot begin perceptual-cache GC read: {e}"))?;
        let table = read_txn
            .open_table(PERCEPTUAL_CACHE_TABLE)
            .map_err(|e| format!("Cannot open table for perceptual-cache GC: {e}"))?;
        for entry in table
            .iter()
            .map_err(|e| format!("Cannot iterate table for perceptual-cache GC: {e}"))?
        {
            let entry = entry
                .map_err(|e| format!("Cannot read entry for perceptual-cache GC: {e}"))?;
            entries_before += 1;
            let key = entry.0.value().to_vec();
            let val = entry.1.value();
            let val_len = val.len() as u64;
            bytes_before += val_len;
            match unpack_value(val) {
                Some((_mtime, _algo, _phash, last_used, _created)) if last_used >= ttl_cutoff => {
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
                .open_table(PERCEPTUAL_CACHE_TABLE)
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
                .open_table(PERCEPTUAL_CACHE_TABLE)
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
