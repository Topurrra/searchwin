//! Time Tracker — a local, private, automatic activity tracker.
//!
//! A background thread samples the foreground window (app + title) and the
//! system idle time, coalesces consecutive samples of the same window into
//! **sessions**, categorizes them, and persists one day-blob per local day into
//! the encrypted local DB (`local_db`, DPAPI on Windows — the same at-rest
//! encryption as preferences / clipboard history). Nothing ever leaves the
//! machine.
//!
//! Design vs. free trackers (ActivityWatch / RescueTime): native + light (one
//! poll thread, no server), beautiful dashboard (the frontend), encrypted at
//! rest, and **privacy-first** — capture is master-switchable, pausable, has a
//! per-app exclude list (excluded apps record as "(private)" with no title), a
//! retention cap, and a one-click wipe.
//!
//! Local dates come from the OS (`GetLocalTime`) so we never touch the `time`
//! crate's multithread-unsound local-offset path; the frontend (which knows the
//! user's locale) passes the explicit list of `YYYY-MM-DD` dates to aggregate.

use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::commands::{local_db, preferences};

/// How often the background thread samples the foreground window + idle state.
const POLL_SECS: u64 = 3;
/// How often the in-memory day buffer is flushed to disk (bounds crash loss).
const FLUSH_SECS: i64 = 30;
/// How often the cached config is re-read from disk (so Settings edits apply).
const CONFIG_REFRESH_SECS: i64 = 10;

const CONFIG_KEY: &str = "time-tracker/config";
const DAYS_INDEX_KEY: &str = "time-tracker/days";
fn day_key(day: &str) -> String {
    format!("time-tracker/day/{day}")
}

// ── Persisted model ─────────────────────────────────────────────────────────

/// One coalesced session: the same foreground window (or idle/private state)
/// held continuously. Times are UTC epoch ms; the owning local day is the blob
/// it lives in.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    pub start_ms: i64,
    pub end_ms: i64,
    pub app: String,
    pub title: String,
    pub category: String,
    pub idle: bool,
}

impl ActivityEvent {
    /// Two samples belong to the same session when the window identity matches.
    fn same_window(&self, app: &str, title: &str, idle: bool) -> bool {
        self.idle == idle && self.app == app && self.title == title
    }
}

/// A user-defined category with a display color and a productivity kind that
/// drives the focus score.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryDef {
    pub name: String,
    pub color: String,
    /// "productive" | "neutral" | "distracting"
    pub kind: String,
}

/// A categorization rule. Rules are checked in order and the first match wins;
/// they override the built-in app map.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryRule {
    /// "app" | "title"
    pub field: String,
    /// "is" | "contains"
    pub op: String,
    pub pattern: String,
    pub category: String,
}

/// A daily time goal for a category.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub category: String,
    pub daily_target_mins: u32,
}

/// All tracker settings — persisted as one JSON blob, read by both the
/// background thread and the dashboard.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerConfig {
    pub enabled: bool,
    pub capture_titles: bool,
    pub idle_threshold_secs: u32,
    pub retention_days: u32,
    pub paused_until_ms: i64,
    pub excluded_apps: Vec<String>,
    pub categories: Vec<CategoryDef>,
    pub rules: Vec<CategoryRule>,
    pub goals: Vec<Goal>,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        let cat = |name: &str, color: &str, kind: &str| CategoryDef {
            name: name.to_string(),
            color: color.to_string(),
            kind: kind.to_string(),
        };
        TrackerConfig {
            enabled: true,
            capture_titles: true,
            idle_threshold_secs: 180,
            retention_days: 90,
            paused_until_ms: 0,
            excluded_apps: Vec::new(),
            categories: vec![
                cat("Development", "#10b981", "productive"),
                cat("Office", "#3b82f6", "productive"),
                cat("Design", "#8b5cf6", "productive"),
                cat("Communication", "#f59e0b", "neutral"),
                cat("Browsing", "#06b6d4", "neutral"),
                cat("System", "#64748b", "neutral"),
                cat("Media", "#ec4899", "distracting"),
                cat("Games", "#ef4444", "distracting"),
                cat("Other", "#94a3b8", "neutral"),
                cat("Idle", "#475569", "neutral"),
                cat("Private", "#475569", "neutral"),
            ],
            rules: Vec::new(),
            goals: Vec::new(),
        }
    }
}

// ── Categorization ──────────────────────────────────────────────────────────

const CATEGORY_IDLE: &str = "Idle";
const CATEGORY_PRIVATE: &str = "Private";
const CATEGORY_OTHER: &str = "Other";

/// Built-in app → category map (lowercased exe base names, no `.exe`). User
/// rules take precedence over this.
fn builtin_category(app_lower: &str) -> Option<&'static str> {
    const DEV: &[&str] = &[
        "code", "cursor", "devenv", "idea", "idea64", "pycharm", "pycharm64", "webstorm",
        "rustrover", "goland", "clion", "rider", "rider64", "studio64", "sublime_text", "atom",
        "eclipse", "windowsterminal", "wt", "powershell", "pwsh", "cmd",
    ];
    const BROWSE: &[&str] =
        &["chrome", "firefox", "msedge", "brave", "opera", "vivaldi", "arc", "iexplore"];
    const OFFICE: &[&str] = &[
        "winword", "excel", "powerpnt", "onenote", "outlook", "acrobat", "acrord32",
        "foxitpdfreader", "wps", "et", "wpp",
    ];
    const COMMS: &[&str] = &[
        "slack", "discord", "teams", "ms-teams", "zoom", "telegram", "whatsapp", "skype",
        "thunderbird",
    ];
    const MEDIA: &[&str] = &[
        "spotify", "vlc", "wmplayer", "mpc-hc", "mpc-hc64", "foobar2000", "itunes", "music",
        "potplayermini64",
    ];
    const GAMES: &[&str] = &["steam", "epicgameslauncher", "riotclientux", "leagueclient"];
    const SYSTEM: &[&str] = &["explorer", "taskmgr", "conhost", "systemsettings", "searchapp"];
    const DESIGN: &[&str] = &[
        "photoshop", "illustrator", "figma", "gimp", "inkscape", "blender", "krita", "afdesign",
        "afphoto",
    ];
    for (apps, name) in [
        (DEV, "Development"),
        (BROWSE, "Browsing"),
        (OFFICE, "Office"),
        (COMMS, "Communication"),
        (MEDIA, "Media"),
        (GAMES, "Games"),
        (SYSTEM, "System"),
        (DESIGN, "Design"),
    ] {
        if apps.contains(&app_lower) {
            return Some(name);
        }
    }
    None
}

/// Resolve a window's category: user rules first (in order), then the built-in
/// map, then "Other".
fn categorize(app: &str, title: &str, rules: &[CategoryRule]) -> String {
    let app_l = app.to_lowercase();
    let title_l = title.to_lowercase();
    for rule in rules {
        let hay = if rule.field == "title" { &title_l } else { &app_l };
        let needle = rule.pattern.to_lowercase();
        if needle.is_empty() {
            continue;
        }
        let hit = if rule.op == "is" { *hay == needle } else { hay.contains(&needle) };
        if hit {
            return rule.category.clone();
        }
    }
    builtin_category(&app_l).map(str::to_string).unwrap_or_else(|| CATEGORY_OTHER.to_string())
}

// ── OS sampling (Windows) ───────────────────────────────────────────────────

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(windows)]
fn local_day_key() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let st = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02}", st.wYear, st.wMonth, st.wDay)
}

#[cfg(windows)]
fn idle_secs() -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info).as_bool() {
            let now = GetTickCount();
            (now.wrapping_sub(info.dwTime) / 1000) as u64
        } else {
            0
        }
    }
}

/// Foreground `(app, title)` — `app` is the exe base name without `.exe`.
#[cfg(windows)]
fn foreground() -> Option<(String, String)> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0 == 0 {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }

        let mut title = String::new();
        let mut title_buf = [0u16; 512];
        let title_len = GetWindowTextW(hwnd, &mut title_buf);
        if title_len > 0 {
            title = String::from_utf16_lossy(&title_buf[..title_len as usize]);
        }

        let mut app = String::new();
        if let Ok(handle) =
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid)
        {
            let mut name_buf = [0u16; 260];
            let name_len = GetModuleBaseNameW(handle, None, &mut name_buf);
            let _ = CloseHandle(handle);
            if name_len > 0 {
                let raw = String::from_utf16_lossy(&name_buf[..name_len as usize]);
                app = if raw.len() >= 4 && raw.as_bytes()[raw.len() - 4..].eq_ignore_ascii_case(b".exe")
                {
                    raw[..raw.len() - 4].to_string()
                } else {
                    raw
                };
            }
        }
        if app.is_empty() {
            return None;
        }
        Some((app, title.trim().to_string()))
    }
}

#[cfg(not(windows))]
fn local_day_key() -> String {
    "1970-01-01".to_string()
}
#[cfg(not(windows))]
fn idle_secs() -> u64 {
    0
}
#[cfg(not(windows))]
fn foreground() -> Option<(String, String)> {
    None
}

// ── Background capture state + loop ──────────────────────────────────────────

struct TrackerState {
    config: TrackerConfig,
    config_loaded_ms: i64,
    /// Local day the buffer belongs to.
    day: String,
    /// All sessions recorded for `day` so far (rewritten to disk on flush).
    buffer: Vec<ActivityEvent>,
    /// The currently-open session (its `end_ms` advances each tick).
    current: Option<ActivityEvent>,
    last_flush_ms: i64,
}

static STATE: LazyLock<Mutex<TrackerState>> = LazyLock::new(|| {
    Mutex::new(TrackerState {
        config: TrackerConfig::default(),
        config_loaded_ms: 0,
        day: String::new(),
        buffer: Vec::new(),
        current: None,
        last_flush_ms: 0,
    })
});

fn pref_db_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(local_db::database_path_for_dir(&preferences::preferences_dir(app)?))
}

fn load_config(app: &AppHandle) -> TrackerConfig {
    match pref_db_path(app) {
        Ok(path) => {
            local_db::read_json_or_default(&path, CONFIG_KEY, TrackerConfig::default())
                .unwrap_or_default()
        }
        Err(_) => TrackerConfig::default(),
    }
}

fn load_day(app: &AppHandle, day: &str) -> Vec<ActivityEvent> {
    match pref_db_path(app) {
        Ok(path) => local_db::read_json_or_default(&path, &day_key(day), Vec::new())
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Persist `buffer` (+ a snapshot of the open `current`) to the day-blob, and
/// keep the days index up to date so retention/wipe can find every day.
fn flush(app: &AppHandle, state: &mut TrackerState) {
    let Ok(path) = pref_db_path(app) else {
        return;
    };
    let mut events = state.buffer.clone();
    if let Some(mut cur) = state.current.clone() {
        cur.end_ms = now_ms();
        events.push(cur);
    }
    if events.is_empty() {
        return;
    }
    let _ = local_db::write_json(&path, &day_key(&state.day), &events);
    // Maintain the days index (for retention + wipe).
    let mut days: Vec<String> =
        local_db::read_json_or_default(&path, DAYS_INDEX_KEY, Vec::new()).unwrap_or_default();
    if !days.contains(&state.day) {
        days.push(state.day.clone());
        let _ = local_db::write_json(&path, DAYS_INDEX_KEY, &days);
    }
    state.last_flush_ms = now_ms();
}

/// Drop day-blobs older than the retention window (overwrites them empty and
/// prunes the index). `retention_days == 0` means keep forever.
fn enforce_retention(app: &AppHandle, retention_days: u32) {
    if retention_days == 0 {
        return;
    }
    let Ok(path) = pref_db_path(app) else {
        return;
    };
    let days: Vec<String> =
        local_db::read_json_or_default(&path, DAYS_INDEX_KEY, Vec::new()).unwrap_or_default();
    if days.is_empty() {
        return;
    }
    // Day keys are lexicographically sortable (YYYY-MM-DD). Keep the newest N.
    let mut sorted = days.clone();
    sorted.sort();
    if sorted.len() <= retention_days as usize {
        return;
    }
    let cutoff = sorted.len() - retention_days as usize;
    let (drop, keep) = sorted.split_at(cutoff);
    for day in drop {
        let _ = local_db::remove_json(&path, &day_key(day));
    }
    let _ = local_db::write_json(&path, DAYS_INDEX_KEY, &keep.to_vec());
}

/// One sample: figure out the current window/idle/excluded state, extend or roll
/// the open session, and flush on the cadence.
fn tick(app: &AppHandle) {
    let now = now_ms();
    let today = local_day_key();
    let mut state = match STATE.lock() {
        Ok(state) => state,
        Err(_) => return,
    };

    // Refresh config from disk periodically (Settings edits + pause).
    if now - state.config_loaded_ms >= CONFIG_REFRESH_SECS * 1000 || state.config_loaded_ms == 0 {
        state.config = load_config(app);
        state.config_loaded_ms = now;
    }

    // Day rollover (or first run): finalize the old day, load the new one.
    if state.day != today {
        if state.day.is_empty() {
            state.day = today.clone();
            state.buffer = load_day(app, &today);
        } else {
            if let Some(mut cur) = state.current.take() {
                cur.end_ms = now;
                state.buffer.push(cur);
            }
            flush(app, &mut state);
            let retention = state.config.retention_days;
            enforce_retention(app, retention);
            state.day = today.clone();
            state.buffer = load_day(app, &today);
        }
    }

    // Paused / disabled → close any open session, don't record.
    let paused = state.config.paused_until_ms > now;
    if !state.config.enabled || paused {
        if let Some(mut cur) = state.current.take() {
            cur.end_ms = now;
            state.buffer.push(cur);
            flush(app, &mut state);
        }
        return;
    }

    // Decide the target window state for this sample.
    let (app_name, title, category, idle) = if (idle_secs() as u32) >= state.config.idle_threshold_secs
    {
        (String::new(), String::new(), CATEGORY_IDLE.to_string(), true)
    } else {
        match foreground() {
            Some((a, t)) => {
                let excluded = state
                    .config
                    .excluded_apps
                    .iter()
                    .any(|x| x.eq_ignore_ascii_case(&a));
                if excluded {
                    ("(private)".to_string(), String::new(), CATEGORY_PRIVATE.to_string(), false)
                } else {
                    let title = if state.config.capture_titles { t } else { String::new() };
                    let category = categorize(&a, &title, &state.config.rules);
                    (a, title, category, false)
                }
            }
            // No resolvable foreground (lock screen, desktop) — treat as a short
            // gap by holding the open session; don't manufacture a window.
            None => {
                if let Some(cur) = state.current.as_mut() {
                    cur.end_ms = now;
                }
                maybe_flush(app, &mut state, now);
                return;
            }
        }
    };

    // Extend the open session, or close it and open a new one.
    match state.current.as_mut() {
        Some(cur) if cur.same_window(&app_name, &title, idle) => {
            cur.end_ms = now;
        }
        _ => {
            if let Some(mut cur) = state.current.take() {
                cur.end_ms = now;
                state.buffer.push(cur);
            }
            state.current = Some(ActivityEvent {
                start_ms: now,
                end_ms: now,
                app: app_name,
                title,
                category,
                idle,
            });
        }
    }

    maybe_flush(app, &mut state, now);
}

fn maybe_flush(app: &AppHandle, state: &mut TrackerState, now: i64) {
    if now - state.last_flush_ms >= FLUSH_SECS * 1000 {
        flush(app, state);
    }
}

/// Spawn the background capture thread. Idempotent-safe to call once from setup;
/// failures are silent (the tracker just stays empty).
pub fn start(app: AppHandle) {
    std::thread::spawn(move || loop {
        tick(&app);
        std::thread::sleep(Duration::from_secs(POLL_SECS));
    });
}

// ── Aggregation (for the dashboard) ─────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySummary {
    pub name: String,
    pub color: String,
    pub kind: String,
    pub secs: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSummary {
    pub app: String,
    pub category: String,
    pub secs: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaySummary {
    pub date: String,
    pub active_secs: i64,
    pub idle_secs: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalProgress {
    pub category: String,
    pub target_mins: u32,
    pub actual_mins: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RangeSummary {
    pub total_active_secs: i64,
    pub total_idle_secs: i64,
    pub focus_score: u8,
    pub by_category: Vec<CategorySummary>,
    pub by_app: Vec<AppSummary>,
    pub by_day: Vec<DaySummary>,
    pub timeline: Vec<ActivityEvent>,
    pub goals: Vec<GoalProgress>,
    pub categories: Vec<CategoryDef>,
}

fn secs_of(event: &ActivityEvent) -> i64 {
    ((event.end_ms - event.start_ms).max(0)) / 1000
}

/// Today's events come from the live in-memory buffer (so the dashboard is
/// real-time); other days come from disk.
fn events_for(app: &AppHandle, date: &str) -> Vec<ActivityEvent> {
    if let Ok(state) = STATE.lock() {
        if state.day == date {
            let mut events = state.buffer.clone();
            if let Some(mut cur) = state.current.clone() {
                cur.end_ms = now_ms();
                events.push(cur);
            }
            return events;
        }
    }
    load_day(app, date)
}

// ── Tauri commands ──────────────────────────────────────────────────────────

#[tauri::command]
pub fn time_tracker_get_config(app: AppHandle) -> Result<TrackerConfig, String> {
    Ok(load_config(&app))
}

#[tauri::command]
pub fn time_tracker_set_config(app: AppHandle, config: TrackerConfig) -> Result<(), String> {
    let path = pref_db_path(&app)?;
    local_db::write_json(&path, CONFIG_KEY, &config)?;
    // Apply immediately to the running thread (don't wait for the refresh tick).
    if let Ok(mut state) = STATE.lock() {
        state.config = config;
        state.config_loaded_ms = now_ms();
    }
    Ok(())
}

/// Pause capture for `minutes` (0 resumes immediately).
#[tauri::command]
pub fn time_tracker_pause(app: AppHandle, minutes: u32) -> Result<(), String> {
    let mut config = load_config(&app);
    config.paused_until_ms = if minutes == 0 { 0 } else { now_ms() + (minutes as i64) * 60_000 };
    time_tracker_set_config(app, config)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerStatus {
    pub enabled: bool,
    pub paused: bool,
    pub paused_until_ms: i64,
    pub tracking: bool,
    pub current_app: String,
    pub current_category: String,
}

#[tauri::command]
pub fn time_tracker_status() -> Result<TrackerStatus, String> {
    let state = STATE.lock().map_err(|_| "tracker state poisoned".to_string())?;
    let now = now_ms();
    let paused = state.config.paused_until_ms > now;
    let (current_app, current_category) = state
        .current
        .as_ref()
        .filter(|c| !c.idle)
        .map(|c| (c.app.clone(), c.category.clone()))
        .unwrap_or_default();
    Ok(TrackerStatus {
        enabled: state.config.enabled,
        paused,
        paused_until_ms: state.config.paused_until_ms,
        tracking: state.config.enabled && !paused,
        current_app,
        current_category,
    })
}

/// Aggregate the given local dates (the frontend builds the list — today, or the
/// 7 days of a week) into a dashboard summary.
#[tauri::command]
pub fn time_tracker_range(app: AppHandle, dates: Vec<String>) -> Result<RangeSummary, String> {
    let config = load_config(&app);
    use std::collections::HashMap;

    let mut by_category: HashMap<String, i64> = HashMap::new();
    let mut by_app: HashMap<(String, String), i64> = HashMap::new();
    let mut by_day: Vec<DaySummary> = Vec::new();
    let mut timeline: Vec<ActivityEvent> = Vec::new();
    let mut total_active = 0i64;
    let mut total_idle = 0i64;

    for date in &dates {
        let events = events_for(&app, date);
        let mut day_active = 0i64;
        let mut day_idle = 0i64;
        for event in &events {
            let secs = secs_of(event);
            if secs <= 0 {
                continue;
            }
            if event.idle {
                day_idle += secs;
                total_idle += secs;
            } else {
                day_active += secs;
                total_active += secs;
                *by_category.entry(event.category.clone()).or_insert(0) += secs;
                if event.category != CATEGORY_PRIVATE {
                    *by_app
                        .entry((event.app.clone(), event.category.clone()))
                        .or_insert(0) += secs;
                }
            }
        }
        by_day.push(DaySummary {
            date: date.clone(),
            active_secs: day_active,
            idle_secs: day_idle,
        });
        timeline.extend(events);
    }

    // Category summaries in config order (stable colors), then any unknowns.
    let color_of = |name: &str| -> (String, String) {
        config
            .categories
            .iter()
            .find(|c| c.name == name)
            .map(|c| (c.color.clone(), c.kind.clone()))
            .unwrap_or_else(|| ("#94a3b8".to_string(), "neutral".to_string()))
    };
    let mut category_summaries: Vec<CategorySummary> = by_category
        .iter()
        .map(|(name, secs)| {
            let (color, kind) = color_of(name);
            CategorySummary { name: name.clone(), color, kind, secs: *secs }
        })
        .collect();
    category_summaries.sort_by(|a, b| b.secs.cmp(&a.secs));

    let mut app_summaries: Vec<AppSummary> = by_app
        .into_iter()
        .map(|((app, category), secs)| AppSummary { app, category, secs })
        .collect();
    app_summaries.sort_by(|a, b| b.secs.cmp(&a.secs));

    // Focus score = productive active time / total active time.
    let productive: i64 = category_summaries
        .iter()
        .filter(|c| c.kind == "productive")
        .map(|c| c.secs)
        .sum();
    let focus_score = if total_active > 0 {
        ((productive as f64 / total_active as f64) * 100.0).round() as u8
    } else {
        0
    };

    // Goals: target scales by the number of days in the range.
    let day_count = dates.len().max(1) as i64;
    let goals: Vec<GoalProgress> = config
        .goals
        .iter()
        .map(|goal| {
            let actual = by_category.get(&goal.category).copied().unwrap_or(0) / 60;
            GoalProgress {
                category: goal.category.clone(),
                target_mins: (goal.daily_target_mins as i64 * day_count) as u32,
                actual_mins: actual,
            }
        })
        .collect();

    Ok(RangeSummary {
        total_active_secs: total_active,
        total_idle_secs: total_idle,
        focus_score,
        by_category: category_summaries,
        by_app: app_summaries,
        by_day,
        timeline,
        goals,
        categories: config.categories,
    })
}

/// Permanently delete all tracked data (every day-blob + the index) and reset
/// the live buffer. The config (categories/rules/goals) is kept.
#[tauri::command]
pub fn time_tracker_wipe(app: AppHandle) -> Result<(), String> {
    let path = pref_db_path(&app)?;
    let days: Vec<String> =
        local_db::read_json_or_default(&path, DAYS_INDEX_KEY, Vec::new()).unwrap_or_default();
    for day in &days {
        let _ = local_db::remove_json(&path, &day_key(day));
    }
    let _ = local_db::remove_json(&path, DAYS_INDEX_KEY);
    if let Ok(mut state) = STATE.lock() {
        state.buffer.clear();
        state.current = None;
        state.day = String::new();
    }
    Ok(())
}
