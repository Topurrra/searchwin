use super::local_db;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const PREFERENCES_DIR: &str = "preferences";
const SETTINGS_FILE: &str = "settings.json";
const PROFILES_FILE: &str = "profiles.json";
const ENABLED_PACKS_FILE: &str = "enabled-packs.json";
const ONBOARDING_FILE: &str = "onboarding.json";
const PROFILE_SCHEMA_VERSION: u32 = 1;
const TOOL_PACK_SCHEMA_VERSION: u32 = 2;
const ONBOARDING_SCHEMA_VERSION: u32 = 1;
const APP_SETTINGS_SCHEMA_VERSION: u32 = 1;
const AUTOMATION_DIR: &str = "automation";
const AUTOMATION_ACTIVITY_DB: &str = "automation_activity.redb";
const FILE_SEARCH_INDEX_DIR: &str = "file-search-index";

/// One per-app hotkey binding: press `shortcut` anywhere → toggle (or launch)
/// the app at `target`.
///
/// `target` is a launcher path in exactly the form the app index stores it —
/// an `.exe`, a Start-Menu `.lnk`, or a `shell:AppsFolder\<AUMID>` handle — so
/// window matching and launching both reuse the existing launcher plumbing
/// rather than inventing a second notion of "an app".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppHotkeyBinding {
    /// Stable client-generated row id. Only used to key the Settings list.
    pub id: String,
    /// Tauri-style chord, e.g. `CommandOrControl+Alt+1`.
    pub shortcut: String,
    /// Launcher path of the bound app.
    pub target: String,
    /// Display name for the Settings row.
    #[serde(default)]
    pub name: String,
    /// Per-row switch, so a binding can be parked without deleting it.
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsPayload {
    /// Settings schema version. Missing on pre-versioning files (deserializes
    /// as 0); `migrate_app_settings` upgrades and stamps the current value.
    #[serde(default)]
    pub schema_version: u32,
    /// How KeepItLocal presents itself on launch. "both" (default) shows the
    /// main window + the palette. "palette-only" runs tray-resident
    /// Raycast-style — the main window stays hidden until the user opens it
    /// from the tray or palette. Read in `set_ready` to gate the splash →
    /// main transition. Takes effect on the next launch.
    #[serde(default = "default_app_mode")]
    pub app_mode: String,
    #[serde(default)]
    pub theme: Theme,
    pub sidebar_collapsed: bool,
    #[serde(default = "default_overlay_hotkey_shortcut")]
    pub overlay_hotkey_shortcut: String,
    /// When true, "bang" prefixes like `g foo`, `? foo`, `gh foo` show a
    /// web-search row in the overlay that opens the corresponding site's
    /// search URL in the user's default browser. **Default off** — KeepItLocal
    /// is local-first; web search is an explicit opt-in.
    #[serde(default)]
    pub web_search_enabled: bool,
    /// Master switch for browser bookmark + history search in the palette.
    /// **Default off** — reading browser profiles is surprising, and this is
    /// the only "connector" in v1. Enabling it surfaces BOOKMARKS only.
    /// Nothing is stored by us and nothing leaves the machine.
    #[serde(default)]
    pub browser_search_enabled: bool,
    /// Additionally include browsing HISTORY. **Default off**, separately from
    /// the master switch — history is far more sensitive than bookmarks, so it
    /// takes a second, deliberate opt-in.
    #[serde(default)]
    pub browser_history_enabled: bool,
    /// Whether the dedicated clipboard-history overlay's global hotkey is
    /// active. Default on — most users want clipboard recall reachable from
    /// anywhere via Ctrl+Shift+V.
    #[serde(default = "default_clipboard_overlay_enabled")]
    pub clipboard_overlay_enabled: bool,
    /// The global shortcut for the clipboard-history overlay. Tauri-style
    /// modifier syntax (`CommandOrControl+Shift+V`, etc.).
    #[serde(default = "default_clipboard_overlay_shortcut")]
    pub clipboard_overlay_shortcut: String,
    /// Whether the unified command palette's global hotkey is registered at
    /// startup. Default on — the palette is the new front door.
    #[serde(default = "default_command_overlay_enabled")]
    pub command_overlay_enabled: bool,
    /// The global shortcut for the command palette. Tauri-style modifier
    /// syntax. Default `CommandOrControl+Alt+K`.
    #[serde(default = "default_command_overlay_shortcut")]
    pub command_overlay_shortcut: String,
    /// Whether the sticky quick-note global hotkey is registered at startup.
    /// Default on — summoning a sticky note from anywhere is low-risk.
    #[serde(default = "default_quick_note_hotkey_enabled")]
    pub quick_note_hotkey_enabled: bool,
    /// The global shortcut that summons a sticky quick-note. Tauri-style
    /// modifier syntax. Default `CommandOrControl+Alt+N`.
    #[serde(default = "default_quick_note_hotkey_shortcut")]
    pub quick_note_hotkey_shortcut: String,
    /// Per-app hotkeys: a user-defined, VARIABLE-length list of chord → app
    /// bindings (Settings → Shortcuts → "Per-app hotkeys"). Pressing a chord
    /// anywhere toggles that app's window, or launches it when it has none.
    /// Defaults to empty — every binding here is one the user added by hand.
    #[serde(default)]
    pub app_hotkeys: Vec<AppHotkeyBinding>,
    /// Whether the screen-recording start/stop global hotkey is registered at
    /// startup. Default on.
    #[serde(default = "default_screen_recording_hotkey_enabled")]
    pub screen_recording_hotkey_enabled: bool,
    /// The global shortcut that starts/stops a screen recording. Tauri-style
    /// modifier syntax. Default `CommandOrControl+Alt+R`.
    #[serde(default = "default_screen_recording_hotkey_shortcut")]
    pub screen_recording_hotkey_shortcut: String,
    /// When true, pressing Enter on a clipboard history entry automatically
    /// pastes into the previous app via SendInput. Default on. Frontend-only
    /// — backend doesn't read this; it's threaded through every paste call.
    #[serde(default = "default_clipboard_auto_paste")]
    pub clipboard_auto_paste: bool,
    /// First-run banner state: once the user has dismissed the hotkey hint,
    /// this flips to true and the banner never shows again.
    #[serde(default)]
    pub onboarding_hotkeys_shown: bool,
    /// First-run product tour state: once the user has completed (or
    /// permanently dismissed) the 5-step welcome tour, this flips to true
    /// and the tour modal never auto-shows again. They can still replay it
    /// from Settings → Onboarding & Tips.
    #[serde(default)]
    pub tour_completed: bool,
    /// Capture images / GIFs alongside text in the clipboard history.
    /// **Default ON** (changed 2026-05-26 per user verdict). Image
    /// retention is bounded by `clipboard_image_retention_days` (default
    /// 2 days) so the disk footprint stays reasonable. When false, image
    /// copies are silently ignored at capture time (the original on your
    /// clipboard is untouched). The authoritative runtime value lives in
    /// `clipboard_history.rs::State::images_enabled` (which has its own
    /// one-time migration on the persisted history file); this
    /// settings-side default exists for UI hydration.
    #[serde(default = "default_clipboard_images_enabled")]
    pub clipboard_images_enabled: bool,
    /// Days before non-pinned image entries get auto-purged. Defaults to a
    /// much shorter window than text (2 days vs 14) because each image is
    /// many MB on disk. Pinned images survive regardless.
    #[serde(default = "default_clipboard_image_retention_days")]
    pub clipboard_image_retention_days: u32,
    /// Absolute path to the Vosk model directory. Empty string when
    /// the user hasn't downloaded a model yet.
    #[serde(default)]
    pub vosk_model_path: String,
    /// Whether the dedicated voice-dictation overlay's global hotkey
    /// is registered at startup.
    #[serde(default = "default_voice_overlay_enabled")]
    pub voice_overlay_enabled: bool,
    /// Tauri-style accelerator string for the voice overlay.
    #[serde(default = "default_voice_overlay_shortcut")]
    pub voice_overlay_shortcut: String,
    /// Whether the push-to-talk hotkey is registered at startup.
    /// Default OFF — PTT is an opt-in alternative to the hands-free
    /// voice overlay (hold a key to dictate, release to paste).
    #[serde(default)]
    pub push_to_talk_enabled: bool,
    /// Tauri-style accelerator string for the push-to-talk hold key.
    #[serde(default = "default_push_to_talk_shortcut")]
    pub push_to_talk_shortcut: String,
    /// The user's display name — powers the Home greeting and the {{name}}
    /// snippet placeholder. Empty until set in Settings → System. Local-only.
    /// `#[serde(default)]` → "" so older settings files load fine.
    #[serde(default)]
    pub user_name: String,
    /// Opt-in cadence for the automatic Privacy Audit. One of
    /// `"off" | "launch" | "daily" | "weekly"`. **Default `"off"`** — the
    /// Privacy Audit is otherwise user-initiated only; this is the single
    /// owner-approved relaxation, gated behind explicit opt-in. The actual
    /// scheduling lives in the frontend (`privacyAuditScheduler.ts`); this
    /// field only persists the user's choice. `#[serde(default = …)]` →
    /// `"off"` so settings files predating this field still deserialize.
    #[serde(default = "default_privacy_audit_schedule")]
    pub privacy_audit_schedule: String,
}

/// UI theme. Serialized as a lowercase string (`"dark"`, `"light"`, ...); an
/// unrecognized value deserializes to `Dark` rather than failing the whole
/// settings load (which would reset every other setting too).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    Dracula,
    Nord,
}

impl Theme {
    fn from_token(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" => Theme::Light,
            "dracula" => Theme::Dracula,
            "nord" => Theme::Nord,
            _ => Theme::Dark,
        }
    }
}

impl<'de> Deserialize<'de> for Theme {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(Theme::from_token(&raw))
    }
}

fn default_overlay_hotkey_shortcut() -> String {
    "CommandOrControl+Alt+S".into()
}

fn default_app_mode() -> String {
    "both".into()
}

fn default_clipboard_overlay_enabled() -> bool {
    true
}

fn default_voice_overlay_enabled() -> bool {
    true
}

fn default_voice_overlay_shortcut() -> String {
    "CommandOrControl+Alt+V".into()
}

fn default_push_to_talk_shortcut() -> String {
    "CommandOrControl+Alt+Space".into()
}

fn default_clipboard_overlay_shortcut() -> String {
    "CommandOrControl+Shift+V".into()
}

fn default_command_overlay_enabled() -> bool {
    true
}

fn default_command_overlay_shortcut() -> String {
    "CommandOrControl+Alt+K".into()
}

fn default_quick_note_hotkey_enabled() -> bool {
    true
}

fn default_quick_note_hotkey_shortcut() -> String {
    "CommandOrControl+Alt+N".into()
}

fn default_screen_recording_hotkey_enabled() -> bool {
    true
}

fn default_screen_recording_hotkey_shortcut() -> String {
    "CommandOrControl+Alt+R".into()
}

fn default_clipboard_auto_paste() -> bool {
    true
}

fn default_clipboard_image_retention_days() -> u32 {
    2
}

/// Default for `clipboard_images_enabled` — `true` since 2026-05-26
/// (was `false`). The authoritative runtime default lives in the
/// clipboard-history state; this exists so the preferences payload
/// hydrates correctly when the persisted JSON predates the flip.
fn default_clipboard_images_enabled() -> bool {
    true
}

/// Default cadence for the automatic Privacy Audit — `"off"`. The audit only
/// runs automatically once the user explicitly opts in (Settings → System).
fn default_privacy_audit_schedule() -> String {
    "off".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePayload {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tool_ids: Vec<String>,
    pub built_in: bool,
    pub schema_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesStatePayload {
    pub profiles: Vec<ProfilePayload>,
    pub active_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnabledToolPacksPayload {
    pub schema_version: u32,
    pub enabled_pack_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingStatePayload {
    pub schema_version: u32,
    pub welcome_completed: bool,
    pub completed_at_ms: Option<u128>,
    /// True once the first-run disk index has been auto-seeded. Lives here (not
    /// in localStorage) so it survives normal restarts but resets on a data
    /// wipe (this file is removed by `reset_all_data`), exactly like
    /// `welcome_completed`. `#[serde(default)]` keeps older onboarding.json
    /// files (without this field) loading cleanly as `false`.
    #[serde(default)]
    pub first_disk_index_seeded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStoragePathsPayload {
    pub app_data_dir: String,
    pub preferences_database: String,
    pub automation_dir: String,
    pub automation_database: String,
    pub automation_activity_db: String,
    pub file_search_index_dir: String,
    pub file_search_database: String,
    /// Folder under app data where clipboard image captures are stored as
    /// `<id>.<ext>` files. Surfaced in Settings → Local Storage so users
    /// can see where their clipboard image copies live (and clean them up
    /// manually if they want to).
    pub clipboard_images_dir: String,
    /// The JSON file that backs the clipboard history ring (text entries +
    /// image entry metadata referencing the files in `clipboard_images_dir`).
    pub clipboard_history_json: String,
}

pub fn preferences_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot resolve app data directory: {e}"))?
        .join(PREFERENCES_DIR);

    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create preferences directory: {e}"))?;
    Ok(dir)
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(preferences_dir(app)?.join(SETTINGS_FILE))
}

fn profiles_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(preferences_dir(app)?.join(PROFILES_FILE))
}

fn enabled_packs_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(preferences_dir(app)?.join(ENABLED_PACKS_FILE))
}

fn onboarding_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(preferences_dir(app)?.join(ONBOARDING_FILE))
}

fn local_database_for_json_path(path: &Path) -> Result<(PathBuf, String), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Cannot resolve local database directory".to_string())?;
    let key = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Cannot resolve local database key".to_string())?
        .to_string();
    Ok((local_db::database_path_for_dir(parent), key))
}

fn automation_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot resolve app data directory: {e}"))?
        .join(AUTOMATION_DIR);

    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create automation directory: {e}"))?;
    Ok(dir)
}

fn atomic_write_json(path: PathBuf, bytes: &[u8]) -> Result<(), String> {
    let (db_path, key) = local_database_for_json_path(&path)?;
    local_db::write_json_bytes(&db_path, &key, bytes)
}

fn default_settings() -> AppSettingsPayload {
    AppSettingsPayload {
        schema_version: APP_SETTINGS_SCHEMA_VERSION,
        app_mode: default_app_mode(),
        theme: Theme::default(),
        sidebar_collapsed: false,
        overlay_hotkey_shortcut: default_overlay_hotkey_shortcut(),
        web_search_enabled: false,
        browser_search_enabled: false,
        browser_history_enabled: false,
        clipboard_overlay_enabled: default_clipboard_overlay_enabled(),
        clipboard_overlay_shortcut: default_clipboard_overlay_shortcut(),
        command_overlay_enabled: default_command_overlay_enabled(),
        command_overlay_shortcut: default_command_overlay_shortcut(),
        quick_note_hotkey_enabled: default_quick_note_hotkey_enabled(),
        quick_note_hotkey_shortcut: default_quick_note_hotkey_shortcut(),
        app_hotkeys: Vec::new(),
        screen_recording_hotkey_enabled: default_screen_recording_hotkey_enabled(),
        screen_recording_hotkey_shortcut: default_screen_recording_hotkey_shortcut(),
        clipboard_auto_paste: default_clipboard_auto_paste(),
        onboarding_hotkeys_shown: false,
        tour_completed: false,
        clipboard_images_enabled: default_clipboard_images_enabled(),
        clipboard_image_retention_days: default_clipboard_image_retention_days(),
        vosk_model_path: String::new(),
        voice_overlay_enabled: default_voice_overlay_enabled(),
        voice_overlay_shortcut: default_voice_overlay_shortcut(),
        push_to_talk_enabled: false,
        push_to_talk_shortcut: default_push_to_talk_shortcut(),
        user_name: String::new(),
        privacy_audit_schedule: default_privacy_audit_schedule(),
    }
}

fn default_profiles_state() -> ProfilesStatePayload {
    ProfilesStatePayload {
        profiles: Vec::new(),
        active_id: "all".into(),
    }
}

fn default_enabled_packs() -> EnabledToolPacksPayload {
    EnabledToolPacksPayload {
        schema_version: TOOL_PACK_SCHEMA_VERSION,
        enabled_pack_ids: vec!["core".into(), "media".into()],
    }
}

fn default_onboarding_state() -> OnboardingStatePayload {
    OnboardingStatePayload {
        schema_version: ONBOARDING_SCHEMA_VERSION,
        welcome_completed: false,
        completed_at_ms: None,
        first_disk_index_seeded: false,
    }
}

fn read_json_or_default<T>(path: PathBuf, fallback: T) -> Result<T, String>
where
    T: Clone + for<'de> Deserialize<'de> + Serialize,
{
    let (db_path, key) = local_database_for_json_path(&path)?;
    local_db::read_json_or_default(&db_path, &key, fallback)
}

/// Upgrade an older settings schema to the current version. Runs after
/// deserialization, before the settings reach the rest of the app. Today the
/// only step is 0 → 1 (stamp the version on pre-versioning files); future
/// field renames or removals add their handling here, keyed on the incoming
/// `schema_version`.
fn migrate_app_settings(mut settings: AppSettingsPayload) -> AppSettingsPayload {
    if settings.schema_version < APP_SETTINGS_SCHEMA_VERSION {
        settings.schema_version = APP_SETTINGS_SCHEMA_VERSION;
    }
    settings
}

#[tauri::command]
pub fn load_app_settings(app: AppHandle) -> Result<AppSettingsPayload, String> {
    let loaded = read_json_or_default(settings_path(&app)?, default_settings())?;
    Ok(migrate_app_settings(loaded))
}

#[tauri::command]
pub fn save_app_settings(
    app: AppHandle,
    settings: AppSettingsPayload,
) -> Result<AppSettingsPayload, String> {
    let mut settings = settings;
    settings.schema_version = APP_SETTINGS_SCHEMA_VERSION;
    let bytes = serde_json::to_vec_pretty(&settings)
        .map_err(|e| format!("Cannot serialize settings: {e}"))?;
    atomic_write_json(settings_path(&app)?, &bytes)?;
    Ok(settings)
}

#[tauri::command]
pub fn load_profiles_state(app: AppHandle) -> Result<ProfilesStatePayload, String> {
    let mut state = read_json_or_default(profiles_path(&app)?, default_profiles_state())?;
    for profile in &mut state.profiles {
        if profile.schema_version == 0 {
            profile.schema_version = PROFILE_SCHEMA_VERSION;
        }
    }
    Ok(state)
}

#[tauri::command]
pub fn save_profiles_state(
    app: AppHandle,
    state: ProfilesStatePayload,
) -> Result<ProfilesStatePayload, String> {
    let mut cleaned = state;
    for profile in &mut cleaned.profiles {
        if profile.schema_version == 0 {
            profile.schema_version = PROFILE_SCHEMA_VERSION;
        }
    }
    let bytes = serde_json::to_vec_pretty(&cleaned)
        .map_err(|e| format!("Cannot serialize profiles: {e}"))?;
    atomic_write_json(profiles_path(&app)?, &bytes)?;
    Ok(cleaned)
}

fn migrate_enabled_tool_packs(mut state: EnabledToolPacksPayload) -> EnabledToolPacksPayload {
    if state.schema_version < TOOL_PACK_SCHEMA_VERSION {
        if !state.enabled_pack_ids.iter().any(|id| id == "media") {
            state.enabled_pack_ids.push("media".into());
        }
        state.schema_version = TOOL_PACK_SCHEMA_VERSION;
    }
    if !state.enabled_pack_ids.iter().any(|id| id == "core") {
        state.enabled_pack_ids.insert(0, "core".into());
    }
    state
}

#[tauri::command]
pub fn load_enabled_tool_packs(app: AppHandle) -> Result<EnabledToolPacksPayload, String> {
    let state = read_json_or_default(enabled_packs_path(&app)?, default_enabled_packs())?;
    Ok(migrate_enabled_tool_packs(state))
}

#[tauri::command]
pub fn save_enabled_tool_packs(
    app: AppHandle,
    state: EnabledToolPacksPayload,
) -> Result<EnabledToolPacksPayload, String> {
    let mut cleaned = state;
    cleaned.schema_version = TOOL_PACK_SCHEMA_VERSION;
    if !cleaned.enabled_pack_ids.iter().any(|id| id == "core") {
        cleaned.enabled_pack_ids.insert(0, "core".into());
    }
    cleaned.enabled_pack_ids.sort();
    cleaned.enabled_pack_ids.dedup();

    let bytes = serde_json::to_vec_pretty(&cleaned)
        .map_err(|e| format!("Cannot serialize enabled tool packs: {e}"))?;
    atomic_write_json(enabled_packs_path(&app)?, &bytes)?;
    Ok(cleaned)
}

#[tauri::command]
pub fn load_onboarding_state(app: AppHandle) -> Result<OnboardingStatePayload, String> {
    let mut state = read_json_or_default(onboarding_path(&app)?, default_onboarding_state())?;
    if state.schema_version == 0 {
        state.schema_version = ONBOARDING_SCHEMA_VERSION;
    }
    Ok(state)
}

#[tauri::command]
pub fn save_onboarding_state(
    app: AppHandle,
    state: OnboardingStatePayload,
) -> Result<OnboardingStatePayload, String> {
    let mut cleaned = state;
    cleaned.schema_version = ONBOARDING_SCHEMA_VERSION;
    let bytes = serde_json::to_vec_pretty(&cleaned)
        .map_err(|e| format!("Cannot serialize onboarding state: {e}"))?;
    atomic_write_json(onboarding_path(&app)?, &bytes)?;
    Ok(cleaned)
}

#[tauri::command]
pub fn get_app_storage_paths(app: AppHandle) -> Result<AppStoragePathsPayload, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot resolve app data directory: {e}"))?;
    // Legacy *.json paths (settings/profiles/enabled-packs/onboarding/
    // automation-rules/search-config) used to be reported here for the
    // Settings → Local Storage panel. They're now hidden because all of
    // them have either been migrated into redb (preferences) or were never
    // read by current code (automation, search). The local_db migrator
    // deletes them on the next read after a successful redb seed; fresh
    // installs never create them in the first place.
    let preferences_database = local_db::database_path_for_dir(&preferences_dir(&app)?);
    let automation = automation_dir(&app)?;
    let automation_database = local_db::database_path_for_dir(&automation);
    let automation_activity = automation.join(AUTOMATION_ACTIVITY_DB);
    let file_search_index = app_data_dir.join(FILE_SEARCH_INDEX_DIR);
    let file_search_database = local_db::database_path_for_dir(&file_search_index);
    // Clipboard storage paths. The directory is created lazily by
    // clipboard_history when the first image lands; reporting the would-be
    // path here is fine even before it exists. The JSON file lives under
    // preferences/ alongside other settings files.
    let clipboard_images_dir = app_data_dir.join("clipboard-images");
    let clipboard_history_json = preferences_dir(&app)?.join("clipboard_history.json");

    Ok(AppStoragePathsPayload {
        app_data_dir: app_data_dir.to_string_lossy().to_string(),
        preferences_database: preferences_database.to_string_lossy().to_string(),
        automation_dir: automation.to_string_lossy().to_string(),
        automation_database: automation_database.to_string_lossy().to_string(),
        automation_activity_db: automation_activity.to_string_lossy().to_string(),
        file_search_index_dir: file_search_index.to_string_lossy().to_string(),
        file_search_database: file_search_database.to_string_lossy().to_string(),
        clipboard_images_dir: clipboard_images_dir.to_string_lossy().to_string(),
        clipboard_history_json: clipboard_history_json.to_string_lossy().to_string(),
    })
}

/// Snapshot of every byte KeepItLocal has on disk plus every counter that
/// shows up in the "Storage & Privacy" dashboard. Frontend renders this
/// into the transparency panel so users can see exactly what we've
/// collected and where it lives.
///
/// All values are best-effort: missing files / unreadable dirs report
/// as 0 rather than erroring out. The dashboard's whole purpose is to
/// reassure, not to fail-loud.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInsightsPayload {
    /// Size of `keepitlocal.redb` (preferences + clipboard history + settings).
    pub preferences_db_bytes: u64,
    /// Sum of every `.png` file in the clipboard images cache directory.
    pub clipboard_images_bytes: u64,
    /// Count of files in the clipboard images directory.
    pub clipboard_images_count: u64,
    /// Total bytes used by the file-search Tantivy index (recursive).
    pub file_search_index_bytes: u64,
    /// Total bytes used by the automation directory (recursive).
    pub automation_bytes: u64,
    /// Grand total — sum of all the byte counters above, for the
    /// "total disk footprint" headline number on the dashboard.
    pub total_bytes: u64,
    /// Number of `*.corrupt-*.redb` quarantine files left over from
    /// previous corruption recoveries. Surfacing this lets users see
    /// (and clean up) artifacts they probably forgot exist.
    pub quarantine_count: u64,
    /// Combined bytes of all quarantine files. Counted separately
    /// from total_bytes so the user can tell what's "live data" vs
    /// "recoverable from a past crash".
    pub quarantine_bytes: u64,
}

#[tauri::command]
pub fn get_storage_insights(app: AppHandle) -> Result<StorageInsightsPayload, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot resolve app data directory: {e}"))?;

    // Per-area accumulators. file_size() returns 0 for missing files —
    // we treat that as "absent" rather than an error, since "we haven't
    // captured anything yet" is a normal state.
    let preferences_db_bytes =
        file_size(&local_db::database_path_for_dir(&preferences_dir(&app)?));
    let clipboard_images = sum_dir_files(&app_data_dir.join("clipboard-images"), Some("png"));
    let file_search_index_bytes = sum_dir_recursive(&app_data_dir.join(FILE_SEARCH_INDEX_DIR));
    let automation_bytes = sum_dir_recursive(&automation_dir(&app)?);

    // Quarantine files: anything matching `*.corrupt-*.redb` directly under
    // the preferences dir. Cheap walk, since the dir is flat.
    let (quarantine_count, quarantine_bytes) = sum_quarantine_files(&preferences_dir(&app)?);

    let total_bytes = preferences_db_bytes
        .saturating_add(clipboard_images.0)
        .saturating_add(file_search_index_bytes)
        .saturating_add(automation_bytes);

    Ok(StorageInsightsPayload {
        preferences_db_bytes,
        clipboard_images_bytes: clipboard_images.0,
        clipboard_images_count: clipboard_images.1,
        file_search_index_bytes,
        automation_bytes,
        total_bytes,
        quarantine_count,
        quarantine_bytes,
    })
}

/// Return the size of `path` if it points at a real file, else 0.
fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Sum file sizes in a flat directory (no recursion), optionally filtered
/// by extension. Returns `(total_bytes, file_count)`. Errors → (0, 0).
fn sum_dir_files(dir: &Path, only_extension: Option<&str>) -> (u64, u64) {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut bytes: u64 = 0;
    let mut count: u64 = 0;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if let Some(ext_filter) = only_extension {
            if path.extension().and_then(|e| e.to_str()) != Some(ext_filter) {
                continue;
            }
        }
        if let Ok(metadata) = entry.metadata() {
            if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
                count += 1;
            }
        }
    }
    (bytes, count)
}

/// Recursively sum every file under `dir`. Used for indexes and workspaces
/// that have nested structure. Errors / unreadable subdirs are skipped.
fn sum_dir_recursive(dir: &Path) -> u64 {
    let mut total: u64 = 0;
    let walker = walkdir::WalkDir::new(dir).follow_links(false);
    for entry in walker.into_iter().flatten() {
        if entry.file_type().is_file() {
            if let Ok(metadata) = entry.metadata() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

/// Count + sum of `*.corrupt-*.redb` files directly under `dir`. These
/// are the artifacts our `open_db` quarantine path drops when it
/// recovers from a corrupt database.
fn sum_quarantine_files(dir: &Path) -> (u64, u64) {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut count: u64 = 0;
    let mut bytes: u64 = 0;
    for entry in read_dir.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // The quarantine path uses `.with_extension("corrupt-<ms>.redb")`,
        // which produces names like `keepitlocal.corrupt-1733091823412.redb`.
        if name.contains(".corrupt-") && name.ends_with(".redb") {
            if let Ok(metadata) = entry.metadata() {
                bytes = bytes.saturating_add(metadata.len());
                count += 1;
            }
        }
    }
    (count, bytes)
}

/// Wipe every byte of user-collected state KeepItLocal has written to disk.
/// Resets clipboard history (text + images), file-search index, PDF
/// workspace, automation history, all preferences (settings, profiles,
/// tool packs, onboarding state), and any leftover quarantine files.
///
/// **Does NOT touch:**
///   - The user's actual documents/photos/etc. — only KeepItLocal's own
///     state under its app data dir.
///   - The running daemon — caller is responsible for restarting the app
///     after this returns so state initializes from a clean slate.
///
/// The destructive intent should already have been confirmed by the user
/// upstream — this command itself does no further confirmation.
#[tauri::command]
pub fn reset_all_data(app: AppHandle) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot resolve app data directory: {e}"))?;

    // Every directory we own under app data. We delete each one whole.
    // Listing them explicitly (rather than `rm -rf app_data_dir`) keeps us
    // from blowing away anything Tauri puts there that ISN'T ours —
    // e.g. tauri-plugin-autostart might cache state under the same root.
    let targets: &[PathBuf] = &[
        preferences_dir(&app)?,
        automation_dir(&app)?,
        app_data_dir.join(FILE_SEARCH_INDEX_DIR),
        app_data_dir.join("clipboard-images"),
    ];

    for target in targets {
        if !target.exists() {
            continue;
        }
        if let Err(error) = fs::remove_dir_all(target) {
            // Best-effort: log and keep going. Partial wipe is better than
            // nothing — and many "permission denied" errors come from a
            // file locked by an external process (AV scanner, etc.) that
            // releases on next launch.
            eprintln!(
                "reset_all_data: could not remove {:?}: {error}",
                target
            );
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Data export / import
// ─────────────────────────────────────────────────────────────────────────────

/// Preference DB keys covered by export/import. `snippets_v1` lives in
/// the same preferences database as the settings — we read it here as a
/// raw JSON value so we don't need to import the snippets module's types.
const EXPORT_KEYS: &[(&str, &str)] = &[
    ("settings", SETTINGS_FILE),
    ("profiles", PROFILES_FILE),
    ("enabledPacks", ENABLED_PACKS_FILE),
    ("onboarding", ONBOARDING_FILE),
    ("snippets", "snippets_v1"),
];

/// Current bundle schema version. Bump if the bundle format changes in a
/// way that requires a migration on import.
const EXPORT_VERSION: u32 = 1;

/// Export all user preference data as a plain-JSON bundle.
///
/// The bundle is returned as a string; the caller (Settings.svelte) writes
/// it to a user-chosen file via `@tauri-apps/plugin-fs`. Using a string
/// return keeps this command pure-data with no filesystem side-effects on
/// the Rust side.
///
/// DPAPI encryption that protects data at rest is machine-specific and
/// cannot travel across machines, so we decrypt (via the normal `read_json`
/// path) and export plain JSON. Re-encryption happens on import.
///
/// Bundle format:
/// ```json
/// {
///   "exportVersion": 1,
///   "exportedAtMs": 1748000000000,
///   "settings":     { ... } | null,
///   "profiles":     { ... } | null,
///   "enabledPacks": { ... } | null,
///   "onboarding":   { ... } | null,
///   "snippets":     { ... } | null
/// }
/// ```
#[tauri::command]
pub fn export_app_data(app: AppHandle) -> Result<String, String> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let db_path = local_db::database_path_for_dir(&preferences_dir(&app)?);
    let exported_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let mut bundle = serde_json::json!({
        "exportVersion": EXPORT_VERSION,
        "exportedAtMs": exported_at_ms,
    });

    for (bundle_key, db_key) in EXPORT_KEYS {
        // read_json returns Ok(None) when the key is absent — that's fine,
        // we write `null` into the bundle and skip it on import.
        let value: Option<serde_json::Value> = local_db::read_json(&db_path, db_key)
            .unwrap_or(None);
        bundle[bundle_key] = value.unwrap_or(serde_json::Value::Null);
    }

    serde_json::to_string_pretty(&bundle)
        .map_err(|e| format!("Cannot serialise export bundle: {e}"))
}

/// Import a JSON bundle previously produced by `export_app_data`.
///
/// Validates `exportVersion`, then writes each non-null section back to
/// the preferences DB (re-encrypting via DPAPI on the way in).
///
/// Returns a short human-readable summary of what was restored, e.g.
/// `"Restored: settings, profiles, snippets"`.
#[tauri::command]
pub fn import_app_data(app: AppHandle, bundle_json: String) -> Result<String, String> {
    let bundle: serde_json::Value = serde_json::from_str(&bundle_json)
        .map_err(|e| format!("Cannot parse import bundle: {e}"))?;

    // Version check — future-proofs against breaking bundle format changes.
    let version = bundle
        .get("exportVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    if version == 0 {
        return Err("Import bundle is missing exportVersion — not a valid KeepItLocal backup file.".to_string());
    }
    if version > EXPORT_VERSION {
        return Err(format!(
            "Import bundle was created with a newer version of KeepItLocal (v{version}). \
             Please update the app before importing."
        ));
    }

    let db_path = local_db::database_path_for_dir(&preferences_dir(&app)?);
    let mut restored: Vec<&str> = Vec::new();

    for (bundle_key, db_key) in EXPORT_KEYS {
        let value = match bundle.get(*bundle_key) {
            Some(v) if !v.is_null() => v,
            _ => continue, // absent or null — skip silently
        };
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|e| format!("Cannot serialise '{bundle_key}': {e}"))?;
        local_db::write_json_bytes(&db_path, db_key, &bytes)
            .map_err(|e| format!("Cannot write '{bundle_key}': {e}"))?;
        restored.push(bundle_key);
    }

    if restored.is_empty() {
        Ok("No data sections found in the bundle — nothing was changed.".to_string())
    } else {
        Ok(format!("Restored: {}", restored.join(", ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_round_trip_through_json() {
        let original = default_settings();
        let json = serde_json::to_string(&original).expect("serialize settings");
        let parsed: AppSettingsPayload =
            serde_json::from_str(&json).expect("deserialize settings");
        assert_eq!(
            serde_json::to_value(&original).unwrap(),
            serde_json::to_value(&parsed).unwrap(),
        );
    }

    #[test]
    fn settings_serialize_with_camel_case_keys() {
        let json = serde_json::to_string(&default_settings()).unwrap();
        assert!(json.contains("\"sidebarCollapsed\""));
        assert!(json.contains("\"overlayHotkeyShortcut\""));
        assert!(json.contains("\"voskModelPath\""));
        assert!(!json.contains("\"sidebar_collapsed\""));
    }

    #[test]
    fn settings_load_fills_missing_optional_fields_with_defaults() {
        // `theme` and `sidebarCollapsed` are the only required fields; every
        // other field carries a #[serde(default)] so older settings files
        // keep loading after new fields are introduced.
        let minimal = r#"{"theme":"dark","sidebarCollapsed":false}"#;
        let parsed: AppSettingsPayload =
            serde_json::from_str(minimal).expect("deserialize minimal settings");
        assert_eq!(
            parsed.overlay_hotkey_shortcut,
            default_overlay_hotkey_shortcut()
        );
        assert_eq!(parsed.vosk_model_path, String::new());
        assert_eq!(
            parsed.clipboard_overlay_shortcut,
            default_clipboard_overlay_shortcut()
        );
        assert!(!parsed.web_search_enabled);
    }

    #[test]
    fn settings_load_ignores_unknown_fields() {
        let with_unknown = r#"{"theme":"dark","sidebarCollapsed":true,"someFutureField":42}"#;
        let parsed: AppSettingsPayload =
            serde_json::from_str(with_unknown).expect("unknown fields tolerated");
        assert!(parsed.sidebar_collapsed);
    }

    #[test]
    fn enabled_tool_packs_round_trip_through_json() {
        let original = default_enabled_packs();
        assert_eq!(original.schema_version, 2);
        assert_eq!(original.enabled_pack_ids, vec!["core", "media"]);
        let json = serde_json::to_string(&original).expect("serialize packs");
        let parsed: EnabledToolPacksPayload =
            serde_json::from_str(&json).expect("deserialize packs");
        assert_eq!(parsed.schema_version, original.schema_version);
        assert_eq!(parsed.enabled_pack_ids, original.enabled_pack_ids);
    }

    #[test]
    fn v1_enabled_tool_packs_enable_media() {
        let migrated = migrate_enabled_tool_packs(EnabledToolPacksPayload {
            schema_version: 1,
            enabled_pack_ids: vec!["core".into()],
        });

        assert_eq!(migrated.schema_version, 2);
        assert!(migrated.enabled_pack_ids.iter().any(|id| id == "media"));
    }

    #[test]
    fn v2_enabled_tool_packs_preserve_media_opt_out() {
        let migrated = migrate_enabled_tool_packs(EnabledToolPacksPayload {
            schema_version: 2,
            enabled_pack_ids: vec!["core".into()],
        });

        assert_eq!(migrated.schema_version, 2);
        assert_eq!(migrated.enabled_pack_ids, vec!["core"]);
    }

    #[test]
    fn onboarding_state_round_trip_through_json() {
        let original = default_onboarding_state();
        let json = serde_json::to_string(&original).expect("serialize onboarding");
        let parsed: OnboardingStatePayload =
            serde_json::from_str(&json).expect("deserialize onboarding");
        assert_eq!(parsed.schema_version, original.schema_version);
        assert_eq!(parsed.welcome_completed, original.welcome_completed);
    }
}
