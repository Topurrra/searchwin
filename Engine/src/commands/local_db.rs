use redb::{Database, DatabaseError, StorageError, TableDefinition, TableError};
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use crate::core::dpapi;

pub const LOCAL_DB_FILE: &str = "keepitlocal.redb";

const JSON_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("json");

/// Process-wide cache of opened redb handles, keyed by database path.
///
/// redb forbids two `Database` instances open on the same file at once (it
/// locks the file), so the old code re-opened the database on EVERY call and
/// serialized those opens behind a single global mutex. Worse, `open_db` ran
/// a fresh `begin_write` + `commit` — an fsync — on every call (even reads)
/// just to ensure the table existed. Under a busy disk / AV scan that fsync
/// could stall for hundreds of ms while the global lock was held, parking
/// every window's pending IPC together (the intermittent both-windows hang).
///
/// We now open each database ONCE, cache the handle, and let redb's own MVCC
/// do the concurrency: many concurrent readers + a single serialized writer,
/// with readers seeing a consistent snapshot even mid-write. The mutex below
/// guards only the cache map (a fast HashMap lookup, plus the one-time open
/// per path); it is never held across a database read/write transaction, so a
/// reader no longer blocks behind an unrelated write's fsync.
static DB_CACHE: LazyLock<Mutex<HashMap<PathBuf, Arc<Database>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Databases other processes open too (the file index's state, which its
/// worker processes write), as the `_shared` calls met them. Those open and
/// close them each time; `get_db` refuses them, since its cached handle
/// would keep the file locked for the life of the process and every other
/// process's open would fail.
static SHARED_DBS: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Set to `true` by `quarantine_corrupt_db` when it has to move the
/// corrupt database aside and create a fresh one. The flag is
/// intentionally a `swap`-based latch — once the frontend has read it
/// via `take_db_corruption_recovered()` it resets to `false` so
/// subsequent calls return `false` (i.e., the banner shows once).
static DB_CORRUPTION_RECOVERED: AtomicBool = AtomicBool::new(false);

/// Consume and return the "DB was corrupted and auto-recovered" flag.
///
/// Returns `true` once (the first call after a corruption event) then
/// `false` for all subsequent calls until the next corruption event.
/// Called by the `take_db_corruption_notice` Tauri command in `lib.rs`
/// so the frontend can show a one-time dismissable warning banner.
pub fn take_db_corruption_recovered() -> bool {
    DB_CORRUPTION_RECOVERED.swap(false, Ordering::Relaxed)
}

/// Wrap a JSON payload with DPAPI on Windows. On non-Windows builds we
/// fall through to plaintext — the cfg-gating mirrors the dpapi module's
/// own `#![cfg(windows)]`. Centralizing the gate here means every call
/// site reads as if encryption always happens, which is correct for the
/// only platform we ship on today.
fn encrypt_for_db(plaintext: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        dpapi::protect(plaintext)
    }
    #[cfg(not(windows))]
    {
        Ok(plaintext.to_vec())
    }
}

/// Reverse of `encrypt_for_db`. On Windows the dpapi module already
/// transparently handles legacy plaintext (returns input unchanged if it
/// lacks the version byte), so reads of a pre-encryption database keep
/// working — the next write will re-protect the value automatically.
fn decrypt_from_db(stored: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        dpapi::unprotect(stored)
    }
    #[cfg(not(windows))]
    {
        Ok(stored.to_vec())
    }
}

pub fn database_path_for_dir(dir: &Path) -> PathBuf {
    dir.join(LOCAL_DB_FILE)
}

/// How long a database the engine needs (settings, the index's state) is
/// waited for while another process has it.
pub const BUSY_WAIT: Duration = Duration::from_secs(5);
/// How long a cache is: a busy cache is a miss, not a stalled search.
pub const CACHE_WAIT: Duration = Duration::from_millis(100);

/// Open a redb file, creating it when it's missing (or empty: a process that
/// stopped right after creating it). redb locks the whole file while a
/// `Database` is open, and the engine's index workers are other processes
/// using the same files, so a file another process holds is waited for, up
/// to `wait` — it is busy, not damaged. Only a file redb can't read is
/// corrupt: graceful recovery moves it aside as
/// `<name>.corrupt-<timestamp>.redb` and starts a fresh one (its data is
/// lost, the app keeps working: "service degraded" over "service down").
/// Returns the database and whether it was recovered.
pub fn open_redb(path: &Path, wait: Duration) -> Result<(Database, bool), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Cannot create database directory: {error}"))?;
    }
    let deadline = Instant::now() + wait;
    loop {
        let has_data = fs::metadata(path).map(|meta| meta.len() > 0).unwrap_or(false);
        let opened = if has_data { Database::open(path) } else { Database::create(path) };
        match opened {
            Ok(db) => return Ok((db, false)),
            Err(DatabaseError::DatabaseAlreadyOpen) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(DatabaseError::DatabaseAlreadyOpen) => {
                return Err(format!("{} is in use by another process", path.display()));
            }
            Err(error) if has_data && is_corruption(&error) => {
                eprintln!("local_db: {} is unreadable ({error}); quarantining and starting fresh.", path.display());
                quarantine_corrupt_db(path)?;
                let db = Database::create(path)
                    .map_err(|e| format!("Cannot create fresh database after quarantine: {e}"))?;
                return Ok((db, true));
            }
            Err(error) => return Err(format!("Cannot open {}: {error}", path.display())),
        }
    }
}

/// Whether an open failed because of what's in the file, rather than who
/// else has it (`DatabaseAlreadyOpen`) or a passing I/O error.
fn is_corruption(error: &DatabaseError) -> bool {
    match error {
        DatabaseError::Storage(StorageError::Corrupted(_)) | DatabaseError::UpgradeRequired(_) => true,
        DatabaseError::Storage(StorageError::Io(io)) => {
            matches!(io.kind(), ErrorKind::InvalidData | ErrorKind::UnexpectedEof)
        }
        _ => false,
    }
}

fn open_db(path: &Path) -> Result<Database, String> {
    let (db, recovered) = open_redb(path, BUSY_WAIT)?;
    if recovered {
        // Signal the frontend that a corruption recovery happened. The flag
        // is consumed by `take_db_corruption_notice` (lib.rs) once the main
        // window's layout polls it on mount — the banner then shows once.
        DB_CORRUPTION_RECOVERED.store(true, Ordering::Relaxed);
    }

    let write_txn = db
        .begin_write()
        .map_err(|error| format!("Cannot initialize local database: {error}"))?;
    {
        let _table = write_txn
            .open_table(JSON_TABLE)
            .map_err(|error| format!("Cannot initialize local database table: {error}"))?;
    }
    write_txn
        .commit()
        .map_err(|error| format!("Cannot commit local database initialization: {error}"))?;

    Ok(db)
}

/// Return the process-wide cached handle for `path`, opening and
/// one-time-initializing the database on first access. Subsequent calls are a
/// fast cache hit — no file re-open and, crucially, no fsync. The cache mutex
/// is held only for the lookup (and the rare first open per path); the redb
/// read/write transactions in the callers below run on the returned handle
/// WITHOUT any app-level lock, so a reader never blocks behind an unrelated
/// writer's fsync. A cached handle keeps the file locked for the life of the
/// process, so a database other processes use goes through the `_shared`
/// functions instead, and is refused here.
fn get_db(path: &Path) -> Result<Arc<Database>, String> {
    if is_shared(path)? {
        return Err(format!("{} is shared with other processes: use the _shared calls", path.display()));
    }
    let mut cache = DB_CACHE
        .lock()
        .map_err(|_| "Local database cache lock was poisoned".to_string())?;
    if let Some(db) = cache.get(path) {
        return Ok(db.clone());
    }
    let db = Arc::new(open_db(path)?);
    cache.insert(path.to_path_buf(), db.clone());
    Ok(db)
}

/// Move a corrupt redb file aside so a fresh DB can be created in its
/// place. The quarantined file gets a `.corrupt-<timestamp-ms>.redb`
/// suffix so users can recover the data manually if they really need
/// to (open it from a backup, etc.) and so we keep evidence for support
/// if the corruption was due to a KeepItLocal bug rather than disk damage.
fn quarantine_corrupt_db(path: &Path) -> Result<(), String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0);
    let quarantine = path.with_extension(format!("corrupt-{timestamp}.redb"));
    fs::rename(path, quarantine)
        .map_err(|error| format!("Cannot quarantine corrupt database: {error}"))
}

pub fn read_json_or_default<T>(db_path: &Path, key: &str, fallback: T) -> Result<T, String>
where
    T: Clone + DeserializeOwned + Serialize,
{
    let db = get_db(db_path)?;
    let read_txn = db
        .begin_read()
        .map_err(|error| format!("Cannot read local database: {error}"))?;
    // The redb value may be DPAPI-protected (post-encryption installs) or
    // raw JSON (legacy installs from before encryption shipped). We always
    // route through `decrypt_from_db` so the DPAPI module's version-byte
    // check transparently handles both cases — protected blobs come back
    // as plaintext, legacy blobs pass through unchanged.
    {
        let table = read_txn
            .open_table(JSON_TABLE)
            .map_err(|error| format!("Cannot read local database table: {error}"))?;
        if let Some(value) = table
            .get(key)
            .map_err(|error| format!("Cannot read local database value: {error}"))?
        {
            if let Ok(plaintext) = decrypt_from_db(value.value()) {
                if let Ok(parsed) = serde_json::from_slice::<T>(&plaintext) {
                    return Ok(parsed);
                }
            }
        }
    }
    drop(read_txn);

    // No value in the DB yet — seed it with the default. (Legacy loose-JSON
    // files are no longer read/migrated: all config has lived in this encrypted
    // redb database for a while, so there's nothing left to import.)
    let data = serde_json::to_vec_pretty(&fallback)
        .map_err(|error| format!("Cannot serialize local database value: {error}"))?;
    write_json_bytes_to_db(&db, key, &data)?;
    Ok(fallback)
}

pub fn read_json<T>(db_path: &Path, key: &str) -> Result<Option<T>, String>
where
    T: DeserializeOwned,
{
    if !db_path.exists() {
        return Ok(None);
    }

    let db = get_db(db_path)?;
    let read_txn = db
        .begin_read()
        .map_err(|error| format!("Cannot read local database: {error}"))?;
    let table = read_txn
        .open_table(JSON_TABLE)
        .map_err(|error| format!("Cannot read local database table: {error}"))?;
    let Some(value) = table
        .get(key)
        .map_err(|error| format!("Cannot read local database value: {error}"))?
    else {
        return Ok(None);
    };
    // Same DPAPI-transparent path as `read_json_or_default` — the
    // decrypt routine handles both protected blobs and legacy plaintext.
    let plaintext = decrypt_from_db(value.value())?;
    let parsed = serde_json::from_slice::<T>(&plaintext)
        .map_err(|error| format!("Cannot parse local database value: {error}"))?;
    Ok(Some(parsed))
}

pub fn write_json<T>(db_path: &Path, key: &str, value: &T) -> Result<(), String>
where
    T: Serialize,
{
    let data = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Cannot serialize local database value: {error}"))?;
    write_json_bytes(db_path, key, &data)
}

/// `read_json` for a database other processes open too (the file index's
/// state, which its worker processes write): opened for this one read and
/// closed again, never cached, so no process holds the lock between calls.
pub fn read_json_shared<T>(db_path: &Path, key: &str) -> Result<Option<T>, String>
where
    T: DeserializeOwned,
{
    mark_shared(db_path)?;
    if !db_path.exists() {
        return Ok(None);
    }
    let (db, _) = open_redb(db_path, BUSY_WAIT)?;
    let read_txn = db
        .begin_read()
        .map_err(|error| format!("Cannot read local database: {error}"))?;
    let table = match read_txn.open_table(JSON_TABLE) {
        Ok(table) => table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(format!("Cannot read local database table: {error}")),
    };
    let Some(value) = table
        .get(key)
        .map_err(|error| format!("Cannot read local database value: {error}"))?
    else {
        return Ok(None);
    };
    let plaintext = decrypt_from_db(value.value())?;
    serde_json::from_slice::<T>(&plaintext)
        .map(Some)
        .map_err(|error| format!("Cannot parse local database value: {error}"))
}

/// `write_json` for a database other processes open too; see `read_json_shared`.
pub fn write_json_shared<T>(db_path: &Path, key: &str, value: &T) -> Result<(), String>
where
    T: Serialize,
{
    mark_shared(db_path)?;
    let data = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Cannot serialize local database value: {error}"))?;
    let (db, _) = open_redb(db_path, BUSY_WAIT)?;
    write_json_bytes_to_db(&db, key, &data)
}

/// Record `path` as shared with other processes. One this process already
/// holds open (`get_db`'s cache) is an error, not a 5 s wait for itself.
fn mark_shared(path: &Path) -> Result<(), String> {
    SHARED_DBS
        .lock()
        .map_err(|_| "Local database cache lock was poisoned".to_string())?
        .insert(path.to_path_buf());
    let held = DB_CACHE
        .lock()
        .map_err(|_| "Local database cache lock was poisoned".to_string())?
        .contains_key(path);
    if held {
        return Err(format!("{} is shared with other processes but held open here", path.display()));
    }
    Ok(())
}

fn is_shared(path: &Path) -> Result<bool, String> {
    Ok(SHARED_DBS
        .lock()
        .map_err(|_| "Local database cache lock was poisoned".to_string())?
        .contains(path))
}

pub fn write_json_bytes(db_path: &Path, key: &str, data: &[u8]) -> Result<(), String> {
    let db = get_db(db_path)?;
    write_json_bytes_to_db(&db, key, data)
}

/// Writes the value through DPAPI on Windows so all values land in redb
/// encrypted. Callers don't see the encryption — they pass plain JSON
/// bytes and get back a Result, identical to before.
fn write_json_bytes_to_db(db: &Database, key: &str, data: &[u8]) -> Result<(), String> {
    let encrypted = encrypt_for_db(data)?;
    let write_txn = db
        .begin_write()
        .map_err(|error| format!("Cannot write local database: {error}"))?;
    {
        let mut table = write_txn
            .open_table(JSON_TABLE)
            .map_err(|error| format!("Cannot write local database table: {error}"))?;
        table
            .insert(key, encrypted.as_slice())
            .map_err(|error| format!("Cannot write local database value: {error}"))?;
    }
    write_txn
        .commit()
        .map_err(|error| format!("Cannot commit local database value: {error}"))
}

/// Remove a single key from the local DB. Used by features that legitimately
/// drop individual values (e.g. Time Tracker retention pruning + wipe). No-op
/// when the DB file or the key is absent.
pub fn remove_json(db_path: &Path, key: &str) -> Result<(), String> {
    if !db_path.exists() {
        return Ok(());
    }
    let db = get_db(db_path)?;
    let write_txn = db
        .begin_write()
        .map_err(|error| format!("Cannot write local database: {error}"))?;
    {
        let mut table = write_txn
            .open_table(JSON_TABLE)
            .map_err(|error| format!("Cannot open local database table: {error}"))?;
        table
            .remove(key)
            .map_err(|error| format!("Cannot remove local database value: {error}"))?;
    }
    write_txn
        .commit()
        .map_err(|error| format!("Cannot commit local database removal: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file redb can't read is still set aside and replaced, so the engine
    /// keeps working: being busy is no longer mistaken for this, but this
    /// must still be recognised.
    #[test]
    fn an_unreadable_database_is_set_aside_and_replaced() {
        let dir = std::env::temp_dir().join(format!(
            "kil-local-db-test-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = database_path_for_dir(&dir);
        fs::write(&path, vec![0x5au8; 8192]).unwrap();

        write_json_shared(&path, "k", &7u32).unwrap();
        let read: Option<u32> = read_json_shared(&path, "k").unwrap();

        let set_aside = fs::read_dir(&dir)
            .unwrap()
            .filter(|entry| {
                let name = entry.as_ref().unwrap().file_name().to_string_lossy().into_owned();
                name.starts_with("keepitlocal.corrupt-") && name.ends_with(".redb")
            })
            .count();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(read, Some(7));
        assert_eq!(set_aside, 1);
    }

    /// A database the `_shared` calls use (other processes open it too) is
    /// refused by the cached calls, whose handle would hold it for good.
    #[test]
    fn a_shared_database_is_never_held_open() {
        let dir = std::env::temp_dir().join(format!(
            "kil-local-db-shared-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        let path = database_path_for_dir(&dir);
        write_json_shared(&path, "k", &1u32).unwrap();

        let cached = write_json(&path, "k", &2u32);
        let other_process = Database::open(&path).map(drop);
        let _ = fs::remove_dir_all(&dir);
        assert!(cached.is_err());
        assert!(other_process.is_ok(), "{other_process:?}");
    }
}
