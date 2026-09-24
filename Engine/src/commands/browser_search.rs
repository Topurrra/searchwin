//! Browser bookmarks + history search — the one "connector" allowed in v1.
//!
//! Searches bookmarks and (optionally) history across every installed
//! Chromium-family browser and Firefox, across every profile, and surfaces the
//! hits in the command palette next to files/notes/apps. No browser window
//! needed, no network, nothing stored.
//!
//! THREE INVARIANTS, enforced structurally rather than by discipline:
//!
//! 1. **We never write to the user's browser files.** The only operations
//!    against a browser profile are `fs::copy` (reads) and `Path::exists`.
//!    Every `Connection::open` receives a path inside OUR temp dir.
//! 2. **No Tauri command here accepts a filesystem path.** The frontend cannot
//!    name a file, so there is no untrusted path to sanitise — a stronger
//!    property than sanitising one.
//! 3. **Nothing is persisted.** No redb, no Tantivy, no cache file. This is
//!    deliberate and load-bearing: an index of browsing history would SURVIVE
//!    the user clearing their browsing data, silently retaining what they
//!    explicitly deleted. For a privacy-first product that is disqualifying.
//!    Results live in RAM for one query; snapshots are deleted on TTL, on
//!    palette close, and swept at startup.
//!
//! WHY COPY THE DB INSTEAD OF OPENING IT: Chromium holds `History` open with a
//! write lock while running, so opening the original usually fails with
//! SQLITE_BUSY — in the COMMON case (browser open), not a rare one. Opening
//! read-only doesn't help: `mode=ro` still takes a shared lock, and on a
//! WAL-mode DB it needs to create the `-shm` file, which means WRITING into the
//! user's profile folder. `immutable=1` avoids locking but skips WAL recovery,
//! silently dropping the most recent browsing — exactly what people search for.
//! Copying sidesteps all of it: `fs::copy` succeeds on a live file (Chromium
//! opens with FILE_SHARE_READ), and the copy is ours to open normally.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::commands::rank;

/// How long a snapshot stays warm before it's rebuilt.
const SNAPSHOT_TTL_MS: u128 = 90_000;
/// Skip any single DB larger than this — copying a 1GB history to answer a
/// palette query is not acceptable on the 4-8GB hardware target.
const MAX_DB_BYTES: u64 = 500 * 1024 * 1024;
/// Total copy budget across all profiles.
const MAX_SNAPSHOT_BYTES: u64 = 1024 * 1024 * 1024;
/// Per-source SQL cap. The palette shows a handful; this bounds the work.
const QUERY_LIMIT: i64 = 60;

// ─── Result shapes ───────────────────────────────────────────────────────

/// Mirrors the field style of `search.rs`'s `FileSearchResultItem`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHit {
    /// Full URL — used for opening only, never rendered verbatim.
    pub url: String,
    pub title: String,
    /// Host + path with the query string STRIPPED. URLs genuinely carry
    /// access tokens and reset nonces; the palette may be screen-shared.
    pub display_url: String,
    /// "bookmark" | "history"
    pub kind: String,
    pub browser: String,
    pub profile: String,
    /// Bookmark folder path ("Bookmarks bar/Dev"); "" for history.
    pub folder: String,
    pub visit_count: u32,
    /// Unix ms; 0 = unknown.
    pub last_visit_ms: u64,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSearchResult {
    pub results: Vec<BrowserHit>,
    pub took_ms: u64,
    pub profiles_scanned: usize,
    /// Locked, corrupt, or over budget — surfaced so the UI can be honest
    /// rather than silently returning less.
    pub profiles_skipped: usize,
    pub history_included: bool,
}

// ─── Profile discovery ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct BrowserProfile {
    browser: String,
    profile: String,
    /// Chromium family (History + Bookmarks) vs Firefox (places.sqlite).
    chromium: bool,
    dir: PathBuf,
}

/// Chromium browsers and their %LOCALAPPDATA%-relative User Data dirs.
/// Copied (4 rows) from `privacy_audit.rs`'s browser-extension audit rather
/// than lifted to a shared module: it's a small const in a private `mod ext`
/// carrying a third field we don't need. Rule of three — extract on the third
/// consumer.
const CHROMIUM_BROWSERS: &[(&str, &str)] = &[
    ("Chrome", r"Google\Chrome\User Data"),
    ("Edge", r"Microsoft\Edge\User Data"),
    ("Brave", r"BraveSoftware\Brave-Browser\User Data"),
    ("Vivaldi", r"Vivaldi\User Data"),
];

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

/// Every installed browser profile we can read. Missing browsers are skipped
/// silently — "not installed" is the normal case, not an error.
fn profiles() -> Vec<BrowserProfile> {
    let mut out = Vec::new();

    if let Some(local) = env_path("LOCALAPPDATA") {
        for (browser, rel) in CHROMIUM_BROWSERS {
            let user_data = local.join(rel);
            if !user_data.is_dir() {
                continue;
            }
            let Ok(entries) = fs::read_dir(&user_data) else {
                continue;
            };
            for entry in entries.flatten() {
                let dir = entry.path();
                if !dir.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                // Chromium's profile naming: "Default", "Profile 1", ...
                if name != "Default" && !name.starts_with("Profile ") {
                    continue;
                }
                out.push(BrowserProfile {
                    browser: (*browser).to_string(),
                    profile: name,
                    chromium: true,
                    dir,
                });
            }
        }
    }

    // Firefox: enumerate profile dirs that actually contain a places.sqlite.
    // Simpler AND more robust than parsing profiles.ini for the common case —
    // it auto-handles .default / .default-release / .dev-edition-default
    // naming. Ceiling: misses profiles stored outside the default dir
    // (profiles.ini IsRelative=0); parse profiles.ini if that's ever reported.
    if let Some(appdata) = env_path("APPDATA") {
        let root = appdata.join(r"Mozilla\Firefox\Profiles");
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let dir = entry.path();
                if dir.is_dir() && dir.join("places.sqlite").is_file() {
                    out.push(BrowserProfile {
                        browser: "Firefox".to_string(),
                        profile: entry.file_name().to_string_lossy().to_string(),
                        chromium: false,
                        dir,
                    });
                }
            }
        }
    }

    out
}

// ─── Snapshot ────────────────────────────────────────────────────────────

struct Snapshot {
    root: PathBuf,
    built_ms: u128,
    /// (profile, copied db path) for each profile we successfully snapshotted.
    dbs: Vec<(BrowserProfile, PathBuf)>,
    skipped: usize,
}

static SNAPSHOT: LazyLock<Mutex<Option<Snapshot>>> = LazyLock::new(|| Mutex::new(None));

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn snapshot_prefix() -> String {
    "kil-browser-".to_string()
}

/// Delete every `kil-browser-*` dir in temp.
///
/// MANDATORY at startup. A crash mid-session otherwise leaves a complete copy
/// of the user's browsing history sitting in temp indefinitely — the single
/// highest-severity risk in this feature, and this sweep is its only
/// mitigation.
pub fn sweep_orphan_snapshots() {
    let temp = std::env::temp_dir();
    let Ok(entries) = fs::read_dir(&temp) else {
        return;
    };
    let prefix = snapshot_prefix();
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&prefix) {
            let _ = fs::remove_dir_all(&p);
        }
    }
}

fn drop_snapshot_locked(slot: &mut Option<Snapshot>) {
    if let Some(s) = slot.take() {
        let _ = fs::remove_dir_all(&s.root);
    }
}

/// Copy one DB plus its `-wal` sidecar into `dest_dir`. Returns the copied
/// main-DB path.
///
/// The `-wal` is NOT optional. In WAL mode, committed transactions live only
/// in the `-wal` until a checkpoint — copy just the main file and you get the
/// DB as of the last checkpoint, silently missing hours of browsing. "The page
/// I looked at 20 minutes ago" is the #1 query for this feature.
///
/// The `-shm` is deliberately NOT copied: it's a pure in-memory index into the
/// WAL, rebuilt on first open. A stale one only risks confusing recovery.
///
/// Main is copied BEFORE the WAL: if a checkpoint lands between the two, we
/// get a newer main + an emptied WAL (harmless — the data is already in main).
fn copy_db(src: &Path, dest_dir: &Path, budget: &mut u64) -> Option<PathBuf> {
    let meta = fs::metadata(src).ok()?;
    let size = meta.len();
    if size > MAX_DB_BYTES || size > *budget {
        return None;
    }

    fs::create_dir_all(dest_dir).ok()?;
    let file_name = src.file_name()?;
    let dest = dest_dir.join(file_name);
    fs::copy(src, &dest).ok()?;
    *budget = budget.saturating_sub(size);

    // Sidecar uses SQLite's default `<dbname>-wal` convention, so for a file
    // literally named `History` it is `History-wal`.
    let wal_src = src.with_file_name(format!("{}-wal", file_name.to_string_lossy()));
    if wal_src.is_file() {
        if let Ok(wal_meta) = fs::metadata(&wal_src) {
            let wal_size = wal_meta.len();
            if wal_size <= *budget {
                let wal_dest =
                    dest_dir.join(format!("{}-wal", file_name.to_string_lossy()));
                if fs::copy(&wal_src, &wal_dest).is_ok() {
                    *budget = budget.saturating_sub(wal_size);
                }
            }
        }
    }

    Some(dest)
}

/// Build (or reuse) the snapshot. Returns how many profiles were skipped.
fn ensure_snapshot(include_history: bool) -> Result<(), String> {
    let mut slot = SNAPSHOT.lock().map_err(|_| "snapshot lock poisoned")?;

    if let Some(s) = slot.as_ref() {
        if now_ms().saturating_sub(s.built_ms) < SNAPSHOT_TTL_MS {
            return Ok(());
        }
    }
    drop_snapshot_locked(&mut slot);

    let root = std::env::temp_dir().join(format!(
        "{}{}-{}",
        snapshot_prefix(),
        std::process::id(),
        now_ms()
    ));
    fs::create_dir_all(&root).map_err(|e| format!("Cannot create snapshot dir: {e}"))?;

    let mut budget = MAX_SNAPSHOT_BYTES;
    let mut dbs = Vec::new();
    let mut skipped = 0usize;

    for profile in profiles() {
        let dest_dir = root.join(format!("{}-{}", profile.browser, profile.profile));
        // Chromium bookmarks are JSON, read directly (atomic writes, never
        // locked) — only the history DB needs copying. Firefox's places.sqlite
        // holds BOTH, so it's copied whether or not history is on.
        let db_src = if profile.chromium {
            if !include_history {
                // Bookmarks-only: no DB to copy, but the profile still counts
                // as scannable via its Bookmarks JSON.
                dbs.push((profile, PathBuf::new()));
                continue;
            }
            profile.dir.join("History")
        } else {
            profile.dir.join("places.sqlite")
        };

        if !db_src.is_file() {
            // Chromium profile with no History yet is normal, not a failure.
            if profile.chromium {
                dbs.push((profile, PathBuf::new()));
            } else {
                skipped += 1;
            }
            continue;
        }

        match copy_db(&db_src, &dest_dir, &mut budget) {
            Some(copied) => dbs.push((profile, copied)),
            None => skipped += 1,
        }
    }

    *slot = Some(Snapshot {
        root,
        built_ms: now_ms(),
        dbs,
        skipped,
    });
    Ok(())
}

// ─── Timestamps — the two epochs DIFFER ──────────────────────────────────

/// Days from 1601-01-01 to 1970-01-01 = 134,774; x 86,400 s.
const CHROMIUM_EPOCH_OFFSET_MS: i64 = 11_644_473_600_000;

/// Chromium: microseconds since 1601-01-01 UTC (the Windows FILETIME epoch,
/// but microseconds rather than 100ns ticks). Returns 0 for "unknown", which
/// feeds `rank::recency_score`'s `modified_ms == 0` short-circuit directly.
fn chromium_us_to_unix_ms(us: i64) -> u64 {
    if us <= 0 {
        return 0;
    }
    let ms = us / 1_000 - CHROMIUM_EPOCH_OFFSET_MS;
    if ms <= 0 {
        0
    } else {
        ms as u64
    }
}

/// Firefox PRTime: microseconds since the UNIX epoch — NO offset. Mixing this
/// up with the Chromium epoch above yields dates ~369 years out.
fn firefox_us_to_unix_ms(us: i64) -> u64 {
    if us <= 0 {
        0
    } else {
        (us / 1_000) as u64
    }
}

// ─── URL handling ────────────────────────────────────────────────────────

/// Only these ever reach the frontend. Bookmarks routinely contain
/// `javascript:` bookmarklets and `chrome://settings` entries, which
/// `open_external_url` correctly rejects — filtering here keeps that security
/// boundary intact instead of widening its allowlist for a cosmetic feature.
fn is_openable(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("file://")
}

/// Host + path, query string and fragment stripped. Matching still runs
/// against the full URL and the full URL is what gets opened — only the
/// RENDERED string is trimmed, because URLs carry access tokens and
/// password-reset nonces and the palette may be screen-shared.
fn display_url(url: &str) -> String {
    let without_scheme = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url);
    let cut = without_scheme
        .find(['?', '#'])
        .unwrap_or(without_scheme.len());
    let trimmed = &without_scheme[..cut];
    trimmed.trim_end_matches('/').to_string()
}

/// Escape LIKE wildcards so a query containing `%` doesn't match every row
/// and return arbitrary noise up to the LIMIT.
fn escape_like(q: &str) -> String {
    q.replace('\\', r"\\").replace('%', r"\%").replace('_', r"\_")
}

// ─── Ranking ─────────────────────────────────────────────────────────────

/// Score a hit with the SAME fused scorer the file search uses, so browser
/// rows sit on a comparable scale rather than inventing a second ranking.
fn score_hit(query: &str, title: &str, url: &str, visit_count: u32, last_visit_ms: u64, is_bookmark: bool) -> f32 {
    let q = query.trim().to_lowercase();
    let title_l = title.to_lowercase();
    let url_l = url.to_lowercase();

    let keywords: Vec<&str> = q.split_whitespace().filter(|k| !k.is_empty()).collect();
    let mut exact = 0usize;
    let mut prefix = 0usize;
    for kw in &keywords {
        if title_l.split_whitespace().any(|t| t == *kw) || url_l.contains(&format!("/{kw}")) {
            exact += 1;
        } else if title_l.contains(kw) || url_l.contains(kw) {
            prefix += 1;
        }
    }

    // Title is the "name" axis, URL the "path" axis — the same distinction the
    // file ranker draws, so its 0.55/0.30 phrase weighting does the right
    // thing: a title match outranks a URL-substring match.
    let hits = rank::LexicalHits {
        full_query_in_name: !q.is_empty() && title_l.contains(&q),
        full_query_in_path: !q.is_empty() && url_l.contains(&q),
        exact_keyword_hits: exact,
        prefix_keyword_hits: prefix,
        fuzzy_keyword_hits: 0,
        phrase_name_hits: 0,
        phrase_path_hits: 0,
        total_keywords: keywords.len(),
    };

    let signals = rank::RankSignals {
        bm25: 0.0, // no Tantivy index here
        lexical: rank::lexical_quality(&hits),
        // Bookmarks are curated, so they get a flat boost rather than a visit
        // count they don't have.
        frecency_boost: if is_bookmark { 3.0 } else { visit_count as f32 },
        modified_ms: last_visit_ms,
        filter_bonus: if is_bookmark { 1.0 } else { 0.0 },
        semantic: 0.0,
    };
    rank::fuse(&signals, now_ms())
}

// ─── Chromium bookmarks (JSON, not SQLite) ───────────────────────────────

/// Flatten Chromium's bookmark tree.
///
/// Two traps this handles: `date_added` is a JSON *string* holding the
/// microsecond value (not a number), and `roots` can contain non-node keys
/// (e.g. `sync_transaction_version`) — so anything without a `"type"` is
/// skipped rather than assumed to be a node.
fn flatten_chromium_bookmarks(json: &serde_json::Value, out: &mut Vec<(String, String, String, u64)>) {
    fn walk(
        node: &serde_json::Value,
        folder: &str,
        out: &mut Vec<(String, String, String, u64)>,
    ) {
        let Some(kind) = node.get("type").and_then(|t| t.as_str()) else {
            return;
        };
        let name = node.get("name").and_then(|n| n.as_str()).unwrap_or("");
        match kind {
            "url" => {
                let Some(url) = node.get("url").and_then(|u| u.as_str()) else {
                    return;
                };
                let added = node
                    .get("date_added")
                    .and_then(|d| d.as_str())
                    .and_then(|s| s.parse::<i64>().ok())
                    .map(chromium_us_to_unix_ms)
                    .unwrap_or(0);
                out.push((name.to_string(), url.to_string(), folder.to_string(), added));
            }
            "folder" => {
                let child_folder = if folder.is_empty() {
                    name.to_string()
                } else {
                    format!("{folder}/{name}")
                };
                if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
                    for child in children {
                        walk(child, &child_folder, out);
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(roots) = json.get("roots").and_then(|r| r.as_object()) {
        for value in roots.values() {
            walk(value, "", out);
        }
    }
}

// ─── Readers ─────────────────────────────────────────────────────────────

fn read_chromium_bookmarks(
    profile: &BrowserProfile,
    needle: &str,
    out: &mut Vec<BrowserHit>,
) {
    let path = profile.dir.join("Bookmarks");
    // Read directly — Chrome writes this atomically (temp + rename), so it is
    // never locked and needs no copy.
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let mut flat = Vec::new();
    flatten_chromium_bookmarks(&json, &mut flat);

    for (title, url, folder, added) in flat {
        if !is_openable(&url) {
            continue;
        }
        if !title.to_lowercase().contains(needle) && !url.to_lowercase().contains(needle) {
            continue;
        }
        let display = display_url(&url);
        let shown_title = if title.trim().is_empty() {
            display.clone()
        } else {
            title
        };
        out.push(BrowserHit {
            score: score_hit(needle, &shown_title, &url, 0, added, true),
            url,
            title: shown_title,
            display_url: display,
            kind: "bookmark".into(),
            browser: profile.browser.clone(),
            profile: profile.profile.clone(),
            folder,
            visit_count: 0,
            last_visit_ms: added,
        });
    }
}

fn read_chromium_history(
    profile: &BrowserProfile,
    db: &Path,
    query: &str,
    needle: &str,
    out: &mut Vec<BrowserHit>,
) -> Result<(), rusqlite::Error> {
    let conn = rusqlite::Connection::open(db)?;
    let like = format!("%{}%", escape_like(query));
    let mut stmt = conn.prepare(
        "SELECT url, title, visit_count, last_visit_time \
         FROM urls \
         WHERE hidden = 0 AND last_visit_time > 0 \
           AND (title LIKE ?1 ESCAPE '\\' OR url LIKE ?1 ESCAPE '\\') \
         ORDER BY last_visit_time DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![like, QUERY_LIMIT], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;

    for row in rows.flatten() {
        let (url, title, visits, ts) = row;
        if !is_openable(&url) {
            continue;
        }
        let display = display_url(&url);
        let shown_title = title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| display.clone());
        let last = chromium_us_to_unix_ms(ts);
        out.push(BrowserHit {
            score: score_hit(needle, &shown_title, &url, visits.max(0) as u32, last, false),
            url,
            title: shown_title,
            display_url: display,
            kind: "history".into(),
            browser: profile.browser.clone(),
            profile: profile.profile.clone(),
            folder: String::new(),
            visit_count: visits.max(0) as u32,
            last_visit_ms: last,
        });
    }
    Ok(())
}

fn read_firefox(
    profile: &BrowserProfile,
    db: &Path,
    query: &str,
    needle: &str,
    include_history: bool,
    out: &mut Vec<BrowserHit>,
) -> Result<(), rusqlite::Error> {
    let conn = rusqlite::Connection::open(db)?;
    let like = format!("%{}%", escape_like(query));

    // Bookmarks: moz_bookmarks.type 1 = bookmark, 2 = folder, 3 = separator.
    // b.title is nullable (Firefox falls back to the page title).
    {
        let mut stmt = conn.prepare(
            "SELECT b.title, p.title, p.url, b.dateAdded \
             FROM moz_bookmarks b JOIN moz_places p ON p.id = b.fk \
             WHERE b.type = 1 \
               AND (b.title LIKE ?1 ESCAPE '\\' OR p.url LIKE ?1 ESCAPE '\\') \
             ORDER BY b.dateAdded DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![like, QUERY_LIMIT], |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?;
        for row in rows.flatten() {
            let (btitle, ptitle, url, added) = row;
            if !is_openable(&url) {
                continue;
            }
            let display = display_url(&url);
            let shown_title = btitle
                .filter(|t| !t.trim().is_empty())
                .or(ptitle.filter(|t| !t.trim().is_empty()))
                .unwrap_or_else(|| display.clone());
            let added_ms = firefox_us_to_unix_ms(added.unwrap_or(0));
            out.push(BrowserHit {
                score: score_hit(needle, &shown_title, &url, 0, added_ms, true),
                url,
                title: shown_title,
                display_url: display,
                kind: "bookmark".into(),
                browser: profile.browser.clone(),
                profile: profile.profile.clone(),
                folder: String::new(),
                visit_count: 0,
                last_visit_ms: added_ms,
            });
        }
    }

    if include_history {
        let mut stmt = conn.prepare(
            "SELECT url, title, visit_count, last_visit_date \
             FROM moz_places \
             WHERE hidden = 0 AND last_visit_date IS NOT NULL \
               AND (title LIKE ?1 ESCAPE '\\' OR url LIKE ?1 ESCAPE '\\') \
             ORDER BY last_visit_date DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![like, QUERY_LIMIT], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?;
        for row in rows.flatten() {
            let (url, title, visits, ts) = row;
            if !is_openable(&url) {
                continue;
            }
            let display = display_url(&url);
            let shown_title = title
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| display.clone());
            let v = visits.unwrap_or(0).max(0) as u32;
            let last = firefox_us_to_unix_ms(ts.unwrap_or(0));
            out.push(BrowserHit {
                score: score_hit(needle, &shown_title, &url, v, last, false),
                url,
                title: shown_title,
                display_url: display,
                kind: "history".into(),
                browser: profile.browser.clone(),
                profile: profile.profile.clone(),
                folder: String::new(),
                visit_count: v,
                last_visit_ms: last,
            });
        }
    }
    Ok(())
}

// ─── Commands ────────────────────────────────────────────────────────────

/// Build the snapshot ahead of the first keystroke, so the copy cost is paid
/// while the user is still typing. Called when the palette's Web chip is
/// activated. Best-effort — a failure here just means the first query pays.
#[tauri::command(async)]
pub fn warm_browser_search(include_history: bool) -> Result<(), String> {
    ensure_snapshot(include_history)
}

/// Drop the snapshot and delete the copied files. Called on palette close and
/// when the feature is switched off.
#[tauri::command(async)]
pub fn clear_browser_snapshot() -> Result<(), String> {
    let mut slot = SNAPSHOT.lock().map_err(|_| "snapshot lock poisoned")?;
    drop_snapshot_locked(&mut slot);
    Ok(())
}

/// Search bookmarks (always) and history (when `include_history`).
#[tauri::command(async)]
pub fn search_browser(
    query: String,
    include_history: bool,
) -> Result<BrowserSearchResult, String> {
    let started = now_ms();
    let trimmed = query.trim().to_string();
    if trimmed.len() < 2 {
        return Ok(BrowserSearchResult {
            results: Vec::new(),
            took_ms: 0,
            profiles_scanned: 0,
            profiles_skipped: 0,
            history_included: include_history,
        });
    }
    let needle = trimmed.to_lowercase();

    ensure_snapshot(include_history)?;
    let slot = SNAPSHOT.lock().map_err(|_| "snapshot lock poisoned")?;
    let Some(snap) = slot.as_ref() else {
        return Err("Snapshot unavailable".into());
    };

    let mut results = Vec::new();
    let mut skipped = snap.skipped;

    for (profile, db) in &snap.dbs {
        if profile.chromium {
            read_chromium_bookmarks(profile, &needle, &mut results);
            if include_history && !db.as_os_str().is_empty() {
                if read_chromium_history(profile, db, &trimmed, &needle, &mut results).is_err() {
                    // A torn copy (browser wrote mid-copy) yields a corrupt
                    // COPY — never the user's file. Count and move on.
                    skipped += 1;
                }
            }
        } else if !db.as_os_str().is_empty()
            && read_firefox(profile, db, &trimmed, &needle, include_history, &mut results).is_err()
        {
            skipped += 1;
        }
    }

    // Same URL can appear in several profiles/browsers — keep the best.
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let mut seen = std::collections::HashSet::new();
    results.retain(|h| seen.insert(h.url.clone()));
    results.truncate(QUERY_LIMIT as usize);

    Ok(BrowserSearchResult {
        results,
        took_ms: now_ms().saturating_sub(started) as u64,
        profiles_scanned: snap.dbs.len(),
        profiles_skipped: skipped,
        history_included: include_history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two epochs differ by ~369 years. Getting this wrong makes every
    /// recency score meaningless, and it's invisible in the UI until someone
    /// notices the sort order is nonsense.
    #[test]
    fn timestamp_epochs_are_not_mixed_up() {
        // The Chromium epoch value for the unix epoch itself -> 0 ms.
        assert_eq!(chromium_us_to_unix_ms(11_644_473_600_000_000), 0);
        assert_eq!(chromium_us_to_unix_ms(0), 0);
        assert_eq!(chromium_us_to_unix_ms(-5), 0);
        // A real-ish Chromium stamp lands in a sane range.
        let ms = chromium_us_to_unix_ms(13_350_000_000_000_000);
        assert!(ms > 1_600_000_000_000, "expected a post-2020 ms, got {ms}");

        // Firefox is plain unix microseconds — no offset.
        assert_eq!(firefox_us_to_unix_ms(1_700_000_000_000_000), 1_700_000_000_000);
        assert_eq!(firefox_us_to_unix_ms(0), 0);
    }

    /// A query containing `%` must not match everything.
    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like("100%"), r"100\%");
        assert_eq!(escape_like("a_b"), r"a\_b");
        assert_eq!(escape_like(r"back\slash"), r"back\\slash");
        assert_eq!(escape_like("plain"), "plain");
    }

    /// Guards the `open_external_url` allowlist boundary — bookmarklets and
    /// browser-internal pages must never reach the frontend.
    #[test]
    fn only_openable_schemes_pass() {
        assert!(is_openable("https://example.com"));
        assert!(is_openable("http://example.com"));
        assert!(is_openable("file:///C:/x.pdf"));
        assert!(!is_openable("javascript:alert(1)"));
        assert!(!is_openable("chrome://settings"));
        assert!(!is_openable("edge://flags"));
        assert!(!is_openable("about:config"));
    }

    /// The query string is where access tokens and reset nonces live.
    #[test]
    fn display_url_strips_query_and_fragment() {
        assert_eq!(
            display_url("https://mail.example.com/inbox?access_token=SECRET"),
            "mail.example.com/inbox"
        );
        assert_eq!(display_url("https://a.com/b#frag"), "a.com/b");
        assert_eq!(display_url("https://a.com/"), "a.com");
        // The full URL is untouched — only the rendered string is trimmed.
        let full = "https://a.com/b?t=1";
        assert!(display_url(full) != full);
    }

    /// Covers both documented traps: `date_added` as a STRING, and a `roots`
    /// entry with no `"type"` (real Chromium files carry such keys).
    #[test]
    fn chromium_bookmarks_flatten_with_folders_and_string_dates() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
              "roots": {
                "bookmark_bar": {
                  "type": "folder", "name": "Bookmarks bar",
                  "children": [
                    { "type": "url", "name": "Rust", "url": "https://rust-lang.org",
                      "date_added": "13350000000000000" },
                    { "type": "folder", "name": "Dev", "children": [
                       { "type": "url", "name": "Docs", "url": "https://docs.rs" },
                       { "type": "url", "name": "Bad", "url": "javascript:alert(1)" }
                    ]}
                  ]
                },
                "sync_transaction_version": "42"
              }
            }"#,
        )
        .unwrap();

        let mut out = Vec::new();
        flatten_chromium_bookmarks(&json, &mut out);

        assert_eq!(out.len(), 3, "javascript: is filtered later, not here");
        let rust = out.iter().find(|(n, _, _, _)| n == "Rust").unwrap();
        assert_eq!(rust.2, "Bookmarks bar", "folder path");
        assert!(rust.3 > 1_600_000_000_000, "string date_added parsed");
        let docs = out.iter().find(|(n, _, _, _)| n == "Docs").unwrap();
        assert_eq!(docs.2, "Bookmarks bar/Dev", "nested folder path");
        // The non-node `roots` key must not panic or produce an entry.
        assert!(out.iter().all(|(n, _, _, _)| n != "42"));
    }

    /// The sweep is the crash-safety net for leaving a copy of someone's
    /// browsing history in temp. It must remove ours and nothing else.
    #[test]
    fn orphan_sweep_removes_only_our_dirs() {
        let temp = std::env::temp_dir();
        let ours = temp.join(format!("{}test-{}", snapshot_prefix(), std::process::id()));
        let theirs = temp.join(format!("unrelated-{}", std::process::id()));
        fs::create_dir_all(&ours).unwrap();
        fs::create_dir_all(&theirs).unwrap();
        fs::write(ours.join("History"), b"x").unwrap();

        sweep_orphan_snapshots();

        assert!(!ours.exists(), "our snapshot dir must be swept");
        assert!(theirs.exists(), "unrelated temp dirs must be left alone");
        let _ = fs::remove_dir_all(&theirs);
    }
}
