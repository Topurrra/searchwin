mod commands;
mod core;
mod media_protocol;

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    Emitter, LogicalSize, Manager, RunEvent, Size, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use commands::{
    activity_log::{clear_activity, list_activity, record_activity},
    archive::{cancel_archive_operation, create_archive, extract_archive, inspect_archive},
    automation::{
        add_automation_activity, clear_automation_activity, get_automation_activity,
        get_automation_recipes, open_automation_data_folder, save_automation_recipes,
    },
    browser_search::{clear_browser_snapshot, search_browser, warm_browser_search},
    camscan::{camscan_detect_corners, camscan_warp},
    cleaner::{analyze_system_cleaner, cancel_cleaner_operation, clean_system_cache},
    clipboard_actions::run_clipboard_action,
    clipboard_history::{
        clear_clipboard_history, copy_clipboard_entry_to_clipboard, delete_clipboard_entries,
        delete_clipboard_entry, get_clipboard_exclusions, get_clipboard_history,
        get_clipboard_image_retention_days, get_clipboard_images_enabled, get_clipboard_paused,
        get_clipboard_retention_days, get_clipboard_text, label_clipboard_entry,
        paste_clipboard_entry, paste_snippet_text, pin_clipboard_entries, pin_clipboard_entry,
        reset_clipboard_exclusions_to_defaults, set_clipboard_exclusions,
        set_clipboard_image_retention_days, set_clipboard_images_enabled, set_clipboard_paused,
        set_clipboard_retention_days, start_clipboard_listener, take_clipboard_recovery_notice,
        type_out_text,
    },
    cron_tasks::{create_cron_task, delete_cron_task, list_cron_tasks, run_cron_task_now},
    crypto_tool::{
        cancel_crypto_operation, crypto_decrypt_file, crypto_decrypt_text, crypto_encrypt_file,
        crypto_encrypt_text, crypto_inspect_file,
    },
    diff_merge::{
        cancel_diff_operation, diff_read_text, file_unified_diff, folder_diff, sync_folders,
        three_way_merge,
    },
    doc_metadata::strip_doc_metadata,
    doc_unlock::remove_docx_password,
    duplicate_preview::preview_duplicate_file,
    encoders::encode_decode,
    error_logs::{clear_log_entries, export_log_text, get_log_folder, list_log_entries, log_event},
    ffmpeg::{ffmpeg_set_path, ffmpeg_status},
    file_manager::{
        fm_backup, fm_cancel, fm_copy, fm_delete_to_recycle, fm_dir_size, fm_list_dir, fm_make_dir,
        fm_move, fm_path_suggestions, fm_rename,
    },
    files::{
        apply_bulk_rename, cancel_duplicate_scan, find_duplicate_files, get_file_recovery_state,
        list_folder_children, move_duplicate_files, preview_bulk_rename, undo_bulk_rename,
        undo_duplicate_move,
    },
    format::convert_format,
    frecency::{get_frecency_boosts, get_recent_items, record_frecency_launch},
    halcyon::{halcyon_probe, halcyon_reset},
    hash::{cancel_hash_operation, compute_hashes},
    image_extra_tools::{cancel_image_extra_operation, generate_favicons, watermark_images},
    image_tools::{
        background_removal_model_status, cancel_image_operation, compress_images, convert_images,
        crop_images, download_background_removal_model, images_to_base64, remove_image_background,
        resize_images, run_image_automation,
    },
    launcher_icons::ensure_launcher_icon,
    live_grep::{cancel_live_grep, live_grep_in_folders},
    local_db,
    media_utility::{cancel_media_operation, media_compress_video, media_extract_audio},
    metadata::strip_metadata,
    my_shell::{cancel_my_shell_command, run_my_shell_command},
    notes::{
        copy_note_asset, create_note, create_note_folder, delete_note, delete_note_folder,
        delete_trashed_note, diff_note_revision, empty_note_trash, get_note_attachment,
        get_notes_dir, list_note_folders, list_note_link_sources, list_note_revisions,
        list_note_templates, list_notes, list_trashed_notes, move_note, open_note_templates_folder,
        open_notes_folder, open_or_create_daily_note, read_note, read_note_revision,
        read_note_template, rename_note_folder, restore_note_revision, restore_trashed_note,
        search_note_bodies, write_note,
    },
    notes_pdf::{
        notes_export_docx, notes_export_html, notes_export_markdown, notes_export_styled_pdf,
    },
    preferences::{
        export_app_data, get_app_storage_paths, get_storage_insights, import_app_data,
        load_app_settings, load_enabled_tool_packs, load_onboarding_state, load_profiles_state,
        reset_all_data, save_app_settings, save_enabled_tool_packs, save_onboarding_state,
        save_profiles_state,
    },
    privacy_audit::{
        audit_browser_extensions, audit_dev_secrets, audit_hosts_file, audit_mic_camera,
        audit_outbound_connections, audit_scheduled_tasks, audit_startup_programs,
        audit_unencrypted_pii, open_privacy_setting, privacy_get_acknowledged,
        privacy_set_acknowledged, reveal_in_explorer,
    },
    processes::{kill_process, launch_targets_running, list_processes},
    qr::{generate_qr, save_qr_png, save_qr_svg},
    quick_actions::{
        check_app_available, evaluate_quick_query, execute_system_command, list_system_commands,
        open_external_url,
    },
    redact::redact_image,
    regex_tools::regex_from_examples,
    reminders::{create_reminder_task, delete_reminder_task, reconcile_reminder_tasks},
    screenrec_cmds::{
        screenrec_close_redact_selector, screenrec_close_region_selector, screenrec_close_toolbar,
        screenrec_export_gif, screenrec_list_windows, screenrec_open_redact_selector,
        screenrec_open_region_selector, screenrec_open_toolbar, screenrec_pause, screenrec_resume,
        screenrec_start, screenrec_status, screenrec_stop,
    },
    search::{
        cancel_file_search_index_build, get_file_search_status, launch_cached_target,
        list_logical_drives, open_search_result_path, prewarm_search_engines, read_file_preview,
        refresh_launch_target_cache, save_file_search_index_options,
        save_file_search_rebuild_schedule, search_file_contents, search_launch_targets,
        search_local_files, start_content_search_index, start_file_search_index,
        start_file_search_scheduler, start_filename_search_index, stop_file_search_index_watcher,
        update_notes_index,
    },
    secure_kv::{secure_kv_get, secure_kv_set},
    sensitive_allowlist::{
        count_dismissed_findings, dismiss_finding, is_finding_dismissed, restore_allowlist,
        undismiss_finding,
    },
    sensitive_scan::{preview_file_findings, scan_text_for_findings},
    shredder::{cancel_operation, shred_files, wipe_free_space},
    snippet_expand::{set_snippet_autoexpand_enabled, sync_snippet_expand_data},
    snippets::{
        create_snippet, delete_snippet, list_snippets, preview_snippet_expansion,
        record_snippet_use, update_snippet,
    },
    spreadsheet::{
        cancel_spreadsheet_operation, clean_csv_file, collect_csv_sources, collect_excel_sources,
        csv_to_excel, csv_to_json, excel_to_csv, inspect_spreadsheet, merge_csv_files,
        preview_csv_cleanup, split_csv_files,
    },
    sql_format::{analyze_sql, format_sql},
    ssh_keys::{
        ssh_delete_key, ssh_dir_path, ssh_generate_key, ssh_import_key, ssh_list_keys,
        ssh_read_config, ssh_read_known_hosts, ssh_remove_known_host, ssh_write_config,
    },
    system_info::system_info,
    time_tracker::{
        time_tracker_get_config, time_tracker_pause, time_tracker_range, time_tracker_set_config,
        time_tracker_status, time_tracker_wipe,
    },
    window_control::{focus_window, list_windows, toggle_window},
    windows_hardening::{
        apply_hardening_tweak, export_hardening_backup, read_hardening_state, revert_all_hardening,
        revert_hardening_tweak,
    },
    word_pdf::{
        cancel_word_markdown_operation, cancel_word_text_operation, collect_word_docx_sources,
        read_docx_markdown, word_to_markdown, word_to_text,
    },
};

// Voice-to-text is Windows-only (uses WinRT Media.SpeechRecognition).
// Imported separately so the cfg gate stays clean. On non-Windows
// targets we expose stubs that return a clear error so the frontend
// always has the same command surface to call into.
#[cfg(windows)]
use commands::voice::{
    voice_cancel_recognize, voice_check_availability, voice_download_model,
    voice_list_downloadable_models, voice_list_installed_models, voice_recognize_once,
    voice_release_models, voice_set_vad_enabled, voice_start_continuous, voice_stop_continuous,
};
#[cfg(windows)]
use commands::voice_input::{
    get_foreground_window_info, voice_get_foreground_app, voice_mouse_click, voice_mouse_move,
    voice_mouse_scroll, voice_mouse_warp, voice_send_keystroke, voice_window_action,
};
#[cfg(windows)]
use commands::voice_ui::{voice_get_ui_elements, voice_list_ui_elements};
// Cross-platform — user-authored voice command files (#21).
use commands::voice_scripts::{
    voice_get_command_overrides, voice_open_user_commands_file, voice_read_user_commands,
    voice_set_command_overrides, voice_user_commands_path, voice_watch_user_commands,
    voice_write_user_commands,
};

#[cfg(not(windows))]
#[tauri::command]
async fn voice_check_availability() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "available": false,
        "hint": "Voice recognition is currently Windows-only.",
        "defaultLanguage": null,
    }))
}

#[cfg(not(windows))]
#[tauri::command]
async fn voice_recognize_once(
    _model_path: Option<String>,
    _source: Option<String>,
) -> Result<serde_json::Value, String> {
    Err("Voice recognition is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_cancel_recognize() {}

#[cfg(not(windows))]
#[tauri::command]
async fn voice_start_continuous(
    _model_path: Option<String>,
    _source: Option<String>,
    _grammar: Option<Vec<String>>,
) -> Result<(), String> {
    Err("Voice recognition is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_stop_continuous() {}

#[cfg(not(windows))]
#[tauri::command]
fn voice_set_vad_enabled(_enabled: bool) {}

#[cfg(not(windows))]
#[tauri::command]
fn voice_release_models() {}

#[cfg(not(windows))]
#[tauri::command]
fn voice_list_downloadable_models() -> Vec<serde_json::Value> {
    Vec::new()
}

#[cfg(not(windows))]
#[tauri::command]
async fn voice_download_model(_model_id: String, _target_dir: String) -> Result<String, String> {
    Err("Voice model downloads are currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_list_installed_models() -> Vec<serde_json::Value> {
    Vec::new()
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_send_keystroke(_key: String, _modifiers: Vec<String>) -> Result<(), String> {
    Err("Voice input emulation is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_mouse_click(_button: String, _double: bool) -> Result<(), String> {
    Err("Voice input emulation is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_mouse_scroll(_direction: String, _notches: i32) -> Result<(), String> {
    Err("Voice input emulation is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_mouse_move(_dx: i32, _dy: i32) -> Result<(), String> {
    Err("Voice input emulation is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_mouse_warp(_fx: f64, _fy: f64) -> Result<(), String> {
    Err("Voice input emulation is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_window_action(_action: String) -> Result<(), String> {
    Err("Voice window control is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_get_foreground_app() -> String {
    String::new()
}

#[cfg(not(windows))]
#[tauri::command]
async fn voice_list_ui_elements() -> Result<usize, String> {
    Err("UI Automation control is currently Windows-only.".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn voice_get_ui_elements() -> Vec<serde_json::Value> {
    Vec::new()
}

const MAIN_WINDOW_LABEL: &str = "main";
/// First-run onboarding window. Born maximized (so the welcome flow
/// fills the screen with no flicker between splash close and welcome
/// paint — the old in-main approach showed a brief 900×700 dark frame
/// because Windows runs maximize-animation when transitioning a
/// hidden window from non-maximized to maximized at show time).
/// Created dynamically by `setup` only when `welcome_completed` is
/// false, so returning users pay zero memory cost for this window.
const WELCOME_WINDOW_LABEL: &str = "welcome";
const OVERLAY_WINDOW_LABEL: &str = "overlay";
const CLIPBOARD_OVERLAY_WINDOW_LABEL: &str = "clipboard-overlay";
const VOICE_OVERLAY_WINDOW_LABEL: &str = "voice-overlay";
const MOUSE_GRID_WINDOW_LABEL: &str = "mouse-grid";
const UI_ELEMENTS_WINDOW_LABEL: &str = "ui-elements";
const TRAY_OPEN_MAIN_ID: &str = "tray-open-main";
const TRAY_OPEN_OVERLAY_ID: &str = "tray-open-overlay";
const TRAY_OPEN_CLIPBOARD_ID: &str = "tray-open-clipboard";
const TRAY_QUIT_ID: &str = "tray-quit";
// Main quick-search overlay dimensions. Sized to wrap tightly around the
// search panel — feels like Raycast/Spotlight rather than a floating dialog.
const OVERLAY_WINDOW_WIDTH: f64 = 720.0;
// Compact height was 112 before the footer landed. The footer is a fixed
// ~32px row of keyboard hints at the bottom of the panel; without bumping
// here the footer overflows the panel in compact mode (no query / no
// recent items). 144px keeps the same visual rhythm while accommodating
// the new strip.
const OVERLAY_COMPACT_HEIGHT: f64 = 144.0;
const OVERLAY_RESULTS_HEIGHT: f64 = 520.0;
const DEFAULT_OVERLAY_SHORTCUT: &str = "CommandOrControl+Alt+S";

// Dedicated clipboard-history overlay shortcut (the width/height constants
// that used to live here were never read after the overlay window itself
// got destroyed in Cleanup Wave 1 — only the keybinding lives on).
const DEFAULT_CLIPBOARD_OVERLAY_SHORTCUT: &str = "CommandOrControl+Shift+V";

// Dedicated voice-dictation overlay shortcut (same story — the W/H
// constants were dead after Cleanup Wave 1 retired the inline overlay).
const DEFAULT_VOICE_OVERLAY_SHORTCUT: &str = "CommandOrControl+Alt+V";

// Push-to-talk hotkey. Unlike every other shortcut here, this one is a
// HOLD: pressing it starts dictation, releasing it stops + pastes. The
// handler treats it specially (both Pressed and Released states fire),
// see `handle_global_hotkey_event`. Default Ctrl+Alt+Space — Space is
// a natural "talk" key, Ctrl+Alt keeps it clear of the bare spacebar.
const DEFAULT_PUSH_TO_TALK_SHORTCUT: &str = "CommandOrControl+Alt+Space";

// Unified command palette (the new front door — eventually replaces the
// three overlays above). Wider than the search overlay because it carries
// a preview pane + action panel. Unlike the other overlays this window is
// PRE-CREATED at startup (warm) so the first hotkey press shows it
// instantly — no wasted-first-press dance. The ~80-100 MB resident cost of
// a warm WebView2 window is the deliberate tradeoff the user accepted for
// instant 1-press summon (the search overlay declined it to stay light;
// the command palette is the case where instant-feel wins).
const COMMAND_WINDOW_LABEL: &str = "command";
// Widened 760 → 900 so the footer keyboard hints (Navigate / Execute / Close /
// Actions / Syntax / Files-Inside + the Offline badge) sit on ONE line during
// an active search instead of wrapping. Keep in sync with the `.cmd-panel`
// max-width in the route's CSS.
const COMMAND_WINDOW_WIDTH: f64 = 900.0;
const COMMAND_WINDOW_HEIGHT: f64 = 560.0;
const DEFAULT_COMMAND_OVERLAY_SHORTCUT: &str = "CommandOrControl+Alt+K";

// Quick-note sticky window. RAM-conscious by design: created on demand and
// DESTROYED on close (deliberately absent from the close-to-hide + hide-on-blur
// handlers), so its WebView2 memory is reclaimed the moment it's closed.
const QUICK_NOTE_WINDOW_LABEL: &str = "quicknote";
const DEFAULT_QUICK_NOTE_SHORTCUT: &str = "CommandOrControl+Alt+N";

static SINGLE_INSTANCE_LOCK: LazyLock<Mutex<Option<File>>> = LazyLock::new(|| Mutex::new(None));

/// The palette tab we last opened via `show_command_window_in_mode`
/// ("clipboard" / "voice", or None for default search). Lets the clipboard and
/// voice hotkeys tell "dismiss the tab I'm already showing" from "switch to my
/// tab" — the palette's live tab isn't readable from Rust (the mode is emitted
/// fire-and-forget). Cleared on hide. See toggle_clipboard_overlay_window.
static LAST_COMMAND_MODE: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));

struct SetupState {
    frontend_ready: bool,
    backend_ready: bool,
}

pub struct CancelFlag(pub Arc<AtomicBool>);
pub struct BusyFlag(pub Arc<AtomicBool>);

/// Collects non-fatal backend initialization issues (hotkey registration
/// failures, clipboard listener errors) so the frontend can surface them
/// as dismissable warning banners after the main window shows.
pub struct BackendInitIssues(pub Arc<Mutex<Vec<String>>>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayHotkeyConfig {
    pub enabled: bool,
    pub mode: String,
    pub shortcut: String,
}

impl Default for OverlayHotkeyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "shortcut".to_string(),
            shortcut: DEFAULT_OVERLAY_SHORTCUT.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct OverlayHotkeyRuntimeState {
    config: OverlayHotkeyConfig,
    active_shortcut: Option<String>,
}

static OVERLAY_HOTKEY_STATE: LazyLock<Mutex<OverlayHotkeyRuntimeState>> =
    LazyLock::new(|| Mutex::new(OverlayHotkeyRuntimeState::default()));

/// Config for the dedicated clipboard-overlay hotkey. Intentionally simpler
/// than `OverlayHotkeyConfig` — no double-space mode (clipboard is too
/// destructive a thing to invoke on accidental double-spaces in prose), just
/// an enable flag and a shortcut string.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardOverlayHotkeyConfig {
    pub enabled: bool,
    pub shortcut: String,
}

impl Default for ClipboardOverlayHotkeyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            shortcut: DEFAULT_CLIPBOARD_OVERLAY_SHORTCUT.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ClipboardOverlayHotkeyRuntimeState {
    config: ClipboardOverlayHotkeyConfig,
    active_shortcut: Option<String>,
}

static CLIPBOARD_OVERLAY_HOTKEY_STATE: LazyLock<Mutex<ClipboardOverlayHotkeyRuntimeState>> =
    LazyLock::new(|| Mutex::new(ClipboardOverlayHotkeyRuntimeState::default()));

/// Config for the unified command-palette hotkey. Same enable + shortcut
/// shape as the clipboard hotkey — the palette is a toggle (press to show,
/// press again to hide), no double-tap mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandOverlayHotkeyConfig {
    pub enabled: bool,
    pub shortcut: String,
}

impl Default for CommandOverlayHotkeyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            shortcut: DEFAULT_COMMAND_OVERLAY_SHORTCUT.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct CommandOverlayHotkeyRuntimeState {
    config: CommandOverlayHotkeyConfig,
    active_shortcut: Option<String>,
}

static COMMAND_OVERLAY_HOTKEY_STATE: LazyLock<Mutex<CommandOverlayHotkeyRuntimeState>> =
    LazyLock::new(|| Mutex::new(CommandOverlayHotkeyRuntimeState::default()));

/// Config for the sticky quick-note hotkey. Same enable + shortcut shape as
/// the command palette, but the action is NOT a toggle: each press summons a
/// note (Windows Sticky Notes style), with an empty-note dedup — if an open
/// note is still empty it is raised instead of spawning a duplicate blank.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteHotkeyConfig {
    pub enabled: bool,
    pub shortcut: String,
}

impl Default for QuickNoteHotkeyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            shortcut: DEFAULT_QUICK_NOTE_SHORTCUT.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct QuickNoteHotkeyRuntimeState {
    config: QuickNoteHotkeyConfig,
    active_shortcut: Option<String>,
}

static QUICK_NOTE_HOTKEY_STATE: LazyLock<Mutex<QuickNoteHotkeyRuntimeState>> =
    LazyLock::new(|| Mutex::new(QuickNoteHotkeyRuntimeState::default()));

/// Config for per-app hotkeys. Unlike every other hotkey in this file, this is
/// a VARIABLE-length list — the user adds one row per app in
/// Settings → Shortcuts. So the runtime state keeps a chord → target MAP
/// instead of a single `active_shortcut`, and the dispatcher does a map lookup
/// where the fixed hotkeys do a string compare.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppHotkeysConfig {
    pub bindings: Vec<commands::preferences::AppHotkeyBinding>,
}

#[derive(Debug, Clone, Default)]
struct AppHotkeysRuntimeState {
    /// Canonical chord string (`Shortcut::to_string()`, matching what the
    /// dispatcher receives) → launcher target of the app to toggle.
    active: std::collections::HashMap<String, String>,
}

static APP_HOTKEYS_STATE: LazyLock<Mutex<AppHotkeysRuntimeState>> =
    LazyLock::new(|| Mutex::new(AppHotkeysRuntimeState::default()));

/// Config for the screen-recording start/stop hotkey. A TOGGLE: press to start,
/// press again to stop. The action is handled in the frontend (via the
/// `screenrec:hotkey-toggle` event) so it reuses the recorder's full start/stop
/// flow — including saving to the remembered folder and stopping from the
/// content-protected toolbar even while the app is minimized. Default Ctrl+Alt+R.
const DEFAULT_SCREEN_RECORDING_SHORTCUT: &str = "CommandOrControl+Alt+R";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenRecordingHotkeyConfig {
    pub enabled: bool,
    pub shortcut: String,
}

impl Default for ScreenRecordingHotkeyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            shortcut: DEFAULT_SCREEN_RECORDING_SHORTCUT.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ScreenRecordingHotkeyRuntimeState {
    config: ScreenRecordingHotkeyConfig,
    active_shortcut: Option<String>,
}

static SCREEN_RECORDING_HOTKEY_STATE: LazyLock<Mutex<ScreenRecordingHotkeyRuntimeState>> =
    LazyLock::new(|| Mutex::new(ScreenRecordingHotkeyRuntimeState::default()));

/// Per-window emptiness registry: quicknote window label → is the note body
/// empty. Emptiness lives only inside each note's webview (`!body.trim()`), so
/// the note reports it here via `set_quick_note_empty` on every edit. The
/// hotkey dedup reads this to decide raise-empty vs spawn-new. Stale entries
/// are harmless (the dedup filters by live windows) and are pruned on close.
static QUICK_NOTE_EMPTY: LazyLock<Mutex<std::collections::HashMap<String, bool>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Config for the voice-overlay hotkey. Same shape as the clipboard
/// hotkey — a simple enable + shortcut pair. The voice overlay has
/// no modes (no double-tap variant) because triggering it accidentally
/// would steal the user's mic and feels worse than missing a press.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceOverlayHotkeyConfig {
    pub enabled: bool,
    pub shortcut: String,
}

impl Default for VoiceOverlayHotkeyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            shortcut: DEFAULT_VOICE_OVERLAY_SHORTCUT.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct VoiceOverlayHotkeyRuntimeState {
    config: VoiceOverlayHotkeyConfig,
    active_shortcut: Option<String>,
}

static VOICE_OVERLAY_HOTKEY_STATE: LazyLock<Mutex<VoiceOverlayHotkeyRuntimeState>> =
    LazyLock::new(|| Mutex::new(VoiceOverlayHotkeyRuntimeState::default()));

/// Config for the push-to-talk hotkey. Same enable + shortcut shape as
/// the overlay hotkeys, but the *behavior* differs: PTT is a press-and-
/// hold dictation trigger, not a toggle. The handler dispatches both
/// `Pressed` and `Released` for this shortcut (every other shortcut is
/// Pressed-only) — see `handle_global_hotkey_event`. Default disabled:
/// PTT is an opt-in alternative to the hands-free voice overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushToTalkHotkeyConfig {
    pub enabled: bool,
    pub shortcut: String,
}

impl Default for PushToTalkHotkeyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            shortcut: DEFAULT_PUSH_TO_TALK_SHORTCUT.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct PushToTalkHotkeyRuntimeState {
    config: PushToTalkHotkeyConfig,
    active_shortcut: Option<String>,
}

static PUSH_TO_TALK_HOTKEY_STATE: LazyLock<Mutex<PushToTalkHotkeyRuntimeState>> =
    LazyLock::new(|| Mutex::new(PushToTalkHotkeyRuntimeState::default()));

pub fn maybe_run_index_worker_from_args() -> bool {
    commands::search::maybe_run_index_worker_from_args()
}

pub fn maybe_run_archive_worker_from_args() -> bool {
    commands::archive::maybe_run_archive_worker_from_args()
}

/// Return and drain the list of non-fatal backend init issues collected
/// during app startup (hotkey registration failures, clipboard listener
/// errors, etc.). Called once by +layout.svelte after the main window
/// appears; the caller shows a dismissable banner for each issue. Returns
/// an empty Vec when startup was clean.
#[tauri::command]
fn get_backend_init_issues(issues: tauri::State<BackendInitIssues>) -> Vec<String> {
    let mut guard = issues.0.lock().unwrap_or_else(|p| p.into_inner());
    std::mem::take(&mut *guard)
}

/// Consume the DB-corruption-recovery flag. Returns `true` once (the first
/// call after a corruption event that caused `quarantine_corrupt_db` to
/// run), then `false` for all subsequent calls until the next event.
/// Called by +layout.svelte so the frontend can show a one-time banner
/// warning the user that their preferences were reset after corruption.
#[tauri::command]
fn take_db_corruption_notice() -> bool {
    local_db::take_db_corruption_recovered()
}

#[tauri::command]
fn set_busy(flag: tauri::State<BusyFlag>, busy: bool) {
    flag.0.store(busy, Ordering::Relaxed);
}

/// True when this process was relaunched by a reminder's scheduled task
/// (`--reminder <id>`): such launches stay in the tray and only fire the
/// reminder toast — no splash, no main window.
fn is_reminder_launch() -> bool {
    std::env::args().any(|arg| arg == "--reminder")
}

#[tauri::command]
fn set_ready(app: tauri::AppHandle, task: String) {
    let state = app.state::<Mutex<SetupState>>();
    // Recover from poisoning so a panic in another setup-related handler
    // doesn't permanently break the readiness signal. The SetupState
    // struct holds two booleans — even if a previous lock-holder
    // panicked mid-write, neither field can be in an inconsistent state.
    let mut s = state.lock().unwrap_or_else(|p| p.into_inner());
    match task.as_str() {
        "frontend" => s.frontend_ready = true,
        "backend" => s.backend_ready = true,
        _ => return,
    }
    if s.frontend_ready && s.backend_ready {
        if let Some(splash) = app.get_webview_window("splashscreen") {
            splash.close().ok();
        }
        // First-run path (Wave 7.9): if the welcome window exists, it
        // owns the post-splash hand-off — main stays hidden until the
        // user finishes the welcome flow and `welcome_finished` swaps
        // the windows. Welcome's own +page calls set_ready when its
        // first paint lands, so this branch fires exactly when welcome
        // is ready to be shown.
        if let Some(welcome) = app.get_webview_window(WELCOME_WINDOW_LABEL) {
            let _ = welcome.show();
            let _ = welcome.set_focus();
            return;
        }
        // Palette-only mode (Settings -> System -> App mode): keep the main
        // window hidden on launch -- the user reaches it via the palette
        // hotkey or the tray. Read straight from the settings struct so the
        // gate survives restarts with no extra handshake. Any read failure
        // falls back to the default "both" behavior (show main).
        let palette_only = load_app_settings(app.clone())
            .map(|cfg| cfg.app_mode == "palette-only")
            .unwrap_or(false);
        // A reminder relaunch stays in the tray — the hidden main webview fires
        // the toast; we reveal nothing.
        if !palette_only && !is_reminder_launch() {
            if let Some(main) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                let _ = main.show();
                let _ = main.set_focus();
            }
        }
    }
}

/// First-run hand-off: closes the welcome window and reveals the main
/// window. Invoked by the welcome flow's "Get started" / final-step
/// action AFTER `completeWelcome` has persisted `welcome_completed = true`.
///
/// At call time the main window has already been mounted in the
/// background (it's defined in `tauri.conf.json` with `visible: false`)
/// and its stores have initialized against the pre-welcome state. We
/// emit `kit-state-refresh` so main re-reads onboarding / packs / settings
/// from disk before becoming visible — that way any pack toggles the
/// user made during welcome are reflected the moment main paints.
#[tauri::command(async)]
async fn welcome_finished(app: tauri::AppHandle) -> Result<(), String> {
    // Tell main to re-init its stores from the freshly-saved backend
    // state. Best-effort — a listen failure shouldn't block the
    // hand-off. Fired BEFORE we show main so the re-fetch can be in
    // flight when main paints.
    let _ = app.emit("kit-state-refresh", ());

    if let Some(main) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = main.show();
        let _ = main.set_focus();
    }
    // Wave 7.9.2 (2026-05-28): destroy() instead of close() so we
    // bypass the CloseRequested event entirely.
    //
    // History: Wave 7.9 used welcome.close(), which fires CloseRequested
    // → my user-close handler (which exits the app, by design for the
    // X-button case) → main vanished right after becoming visible (the
    // "app closing after Finish" bug the user reported).
    //
    // Wave 7.9.1 tried to fix this with a WELCOME_FINISHING AtomicBool
    // flag: set it before close(), reset on Drop. That FAILED because
    // close() queues CloseRequested for the next event-loop tick, but
    // welcome_finished returns immediately — Drop ran before the event
    // was processed, so the close handler still saw flag=false and
    // exited. destroy() solves this cleanly: it force-closes synchronously
    // without firing CloseRequested at all, so the close handler never
    // sees the programmatic close. User-initiated closes (Alt+F4,
    // title-bar X) still go through close() → CloseRequested →
    // app.exit(0), which is what we want for that path.
    if let Some(welcome) = app.get_webview_window(WELCOME_WINDOW_LABEL) {
        let _ = welcome.destroy();
    }
    Ok(())
}

#[tauri::command]
fn apply_overlay_hotkey_config(
    app: tauri::AppHandle,
    config: OverlayHotkeyConfig,
) -> Result<OverlayHotkeyConfig, String> {
    apply_overlay_hotkey_config_internal(&app, config)
}

#[tauri::command]
fn show_main_window_command(app: tauri::AppHandle) -> Result<(), String> {
    show_main_window(&app)
}

#[tauri::command]
fn restart_keepitlocal_command(app: tauri::AppHandle) -> Result<(), String> {
    restart_keepitlocal_impl(&app)
}

/// Internal restart helper. Pulled out of the Tauri command so it can be
/// called from other Rust modules (specifically the palette's system-
/// command executor in `quick_actions.rs`) without round-tripping
/// through the tauri::invoke layer.
pub(crate) fn restart_keepitlocal_impl(app: &tauri::AppHandle) -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Cannot locate KeepItLocal executable: {error}"))?;
    if let Ok(mut guard) = SINGLE_INSTANCE_LOCK.lock() {
        if let Some(lock_file) = guard.take() {
            let _ = lock_file.unlock();
        }
    }
    std::process::Command::new(executable)
        .spawn()
        .map_err(|error| format!("Cannot restart KeepItLocal: {error}"))?;
    app.exit(0);
    Ok(())
}

// Async commands (`#[tauri::command(async)]`) run on the tokio runtime
// rather than blocking the Tauri main event loop. The window-show path
// MUST be async because `WebviewWindowBuilder::build()` internally needs
// the main event loop to create the OS window — if the show command is
// sync, the Tauri main loop is busy dispatching the IPC reply for this
// very command while the command's body is waiting for the main loop to
// create the window → deadlock, main app freezes.
//
// The global-shortcut callback path doesn't hit this because it runs on
// its own thread, not through the IPC machinery. So clicking the Command
// button used to freeze even though pressing Ctrl+Alt+S didn't.
#[tauri::command(async)]
async fn show_overlay_window_command(app: tauri::AppHandle) -> Result<(), String> {
    show_overlay_window(&app)
}

#[tauri::command(async)]
async fn hide_overlay_window_command(app: tauri::AppHandle) -> Result<(), String> {
    hide_overlay_window(&app)
}

#[tauri::command]
fn resize_overlay_window_command(app: tauri::AppHandle, height: f64) -> Result<(), String> {
    let overlay = get_or_create_overlay_window(&app)?;
    let clamped_height = height.clamp(OVERLAY_COMPACT_HEIGHT, OVERLAY_RESULTS_HEIGHT);
    overlay
        .set_size(Size::Logical(LogicalSize::new(
            OVERLAY_WINDOW_WIDTH,
            clamped_height,
        )))
        .map_err(|error| format!("Cannot resize overlay window: {error}"))?;
    Ok(())
}

// Clipboard + voice overlay commands made async for the same deadlock
// reason as the search overlay (see show_overlay_window_command above).
// Even though the user currently summons these via hotkey (which isn't
// affected), any future button that invokes them from main webview JS
// would hit the same IPC↔main-loop deadlock.
#[tauri::command(async)]
async fn show_clipboard_overlay_window_command(app: tauri::AppHandle) -> Result<(), String> {
    show_clipboard_overlay_window(&app)
}

#[tauri::command(async)]
async fn hide_clipboard_overlay_window_command(app: tauri::AppHandle) -> Result<(), String> {
    hide_clipboard_overlay_window(&app)
}

// Command palette show/hide. Async for the same reason as the other
// overlays — a JS-invoked show that needs to build/show the window must
// not block the main event loop while waiting on the IPC reply.
#[tauri::command(async)]
async fn show_command_window_command(app: tauri::AppHandle) -> Result<(), String> {
    show_command_window(&app)
}

#[tauri::command(async)]
async fn hide_command_window_command(app: tauri::AppHandle) -> Result<(), String> {
    hide_command_window(&app)
}

/// Toggle a real Windows DWM acrylic backdrop on the command-palette window
/// so the desktop behind it is blurred (frosted glass). The frontend drives
/// this from the user's `commandAppearance.desktopBlur` preference — applied
/// on summon and whenever the toggle changes. Off by default; reversible.
///
/// Acrylic fills the whole window rect, so on the transparent rounded palette
/// the corners would read as square — we counter that by asking DWM to round
/// the actual OS window corners (Win11+; ignored on older Windows), so the
/// acrylic is clipped to rounded corners instead of leaking square edges. The
/// CSS side (`.has-desktop-blur`) lets the panel fill the window so DWM does
/// all the rounding. No-op (Ok) when the window doesn't exist yet or on
/// non-Windows targets.
#[tauri::command(async)]
async fn set_command_window_blur(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let Some(window) = app.get_webview_window(COMMAND_WINDOW_LABEL) else {
        return Ok(());
    };
    #[cfg(windows)]
    {
        use window_vibrancy::{apply_acrylic, clear_acrylic};
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DEFAULT,
            DWMWCP_DONOTROUND, DWM_WINDOW_CORNER_PREFERENCE,
        };
        if enabled {
            // Slight dark tint so the frosted desktop never washes out the
            // palette's own text in light desktops.
            apply_acrylic(&window, Some((18, 18, 18, 90)))
                .map_err(|error| format!("Could not apply acrylic: {error}"))?;
        } else {
            let _ = clear_acrylic(&window);
        }
        // Flip the OS window corners. The historical attempt was to ROUND
        // both CSS and DWM so they'd match — but Windows acrylic on a
        // rounded window leaks dark square wedges into the rounded
        // corners on most builds, which we could never fix from CSS. So
        // the Palette Appearance phase reversed the policy: when blur
        // is ON, force SQUARE corners (DWMWCP_DONOTROUND) and the CSS
        // side drops its `border-radius` to match. No rounded corners =
        // no wedges to leak into. When blur is OFF, restore the OS
        // default and the CSS gets its full 16-px radius back.
        //
        // HWND is a thin *mut c_void wrapper in both Tauri's `windows`
        // and our 0.54, so rebuild ours from the raw pointer to avoid
        // a cross-version type mismatch.
        if let Ok(raw) = window.hwnd() {
            let hwnd = HWND(raw.0 as isize);
            let pref: DWM_WINDOW_CORNER_PREFERENCE = if enabled {
                DWMWCP_DONOTROUND
            } else {
                DWMWCP_DEFAULT
            };
            unsafe {
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE,
                    &pref as *const DWM_WINDOW_CORNER_PREFERENCE as *const std::ffi::c_void,
                    std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
                );
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (&window, enabled);
    }
    Ok(())
}

/// Whether the OS rounds window corners for the acrylic command palette
/// (Windows 11, build >= 22000). The frontend uses this to hide the
/// experimental "Blur the desktop" toggle on Windows 10, where DWM ignores
/// the corner-preference attribute and the acrylic would show square edges.
#[tauri::command]
fn supports_window_corner_rounding() -> bool {
    #[cfg(windows)]
    {
        use windows::Wdk::System::SystemServices::RtlGetVersion;
        use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
        let mut info = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        // RtlGetVersion (ntdll) reports the REAL build number, unlike the
        // manifest-gated GetVersionEx. STATUS_SUCCESS == 0.
        let status = unsafe { RtlGetVersion(&mut info) };
        status.0 == 0 && info.dwBuildNumber >= 22000
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[tauri::command(async)]
async fn show_voice_overlay_window_command(app: tauri::AppHandle) -> Result<(), String> {
    show_voice_overlay_window(&app)
}

#[tauri::command(async)]
async fn hide_voice_overlay_window_command(app: tauri::AppHandle) -> Result<(), String> {
    hide_voice_overlay_window(&app)
}

#[tauri::command]
fn show_mouse_grid_window_command(app: tauri::AppHandle) -> Result<(), String> {
    show_mouse_grid_window(&app)
}

#[tauri::command]
fn hide_mouse_grid_window_command(app: tauri::AppHandle) -> Result<(), String> {
    hide_mouse_grid_window(&app)
}

#[tauri::command]
fn show_ui_elements_window_command(app: tauri::AppHandle) -> Result<(), String> {
    show_ui_elements_window(&app)
}

#[tauri::command]
fn hide_ui_elements_window_command(app: tauri::AppHandle) -> Result<(), String> {
    hide_ui_elements_window(&app)
}

#[tauri::command]
fn apply_clipboard_overlay_hotkey_config(
    app: tauri::AppHandle,
    config: ClipboardOverlayHotkeyConfig,
) -> Result<ClipboardOverlayHotkeyConfig, String> {
    apply_clipboard_overlay_hotkey_config_internal(&app, config)
}

#[tauri::command]
fn apply_command_overlay_hotkey_config(
    app: tauri::AppHandle,
    config: CommandOverlayHotkeyConfig,
) -> Result<CommandOverlayHotkeyConfig, String> {
    apply_command_overlay_hotkey_config_internal(&app, config)
}

#[tauri::command]
fn apply_quick_note_hotkey_config(
    app: tauri::AppHandle,
    config: QuickNoteHotkeyConfig,
) -> Result<QuickNoteHotkeyConfig, String> {
    apply_quick_note_hotkey_config_internal(&app, config)
}

#[tauri::command]
fn apply_app_hotkeys_config(
    app: tauri::AppHandle,
    config: AppHotkeysConfig,
) -> Result<AppHotkeysConfig, String> {
    apply_app_hotkeys_config_internal(&app, config)
}

#[tauri::command]
fn apply_screen_recording_hotkey_config(
    app: tauri::AppHandle,
    config: ScreenRecordingHotkeyConfig,
) -> Result<ScreenRecordingHotkeyConfig, String> {
    apply_screen_recording_hotkey_config_internal(&app, config)
}

/// The quick-note webview reports whether its body is currently empty
/// (`!body.trim()`), debounced on edit + on mount + on close. This is the
/// only source of truth for note emptiness (it lives in the webview), and it
/// drives the sticky-note hotkey's raise-empty-vs-spawn-new dedup.
#[tauri::command]
fn set_quick_note_empty(label: String, empty: bool) {
    if let Ok(mut map) = QUICK_NOTE_EMPTY.lock() {
        map.insert(label, empty);
    }
}

#[tauri::command]
fn apply_voice_overlay_hotkey_config(
    app: tauri::AppHandle,
    config: VoiceOverlayHotkeyConfig,
) -> Result<VoiceOverlayHotkeyConfig, String> {
    apply_voice_overlay_hotkey_config_internal(&app, config)
}

/// Re-register the push-to-talk hotkey. Unregisters the previously-active
/// PTT shortcut (if any) and registers the new one. Called from the
/// frontend when the PTT enable toggle or shortcut field changes.
#[tauri::command]
fn set_push_to_talk_hotkey(
    app: tauri::AppHandle,
    config: PushToTalkHotkeyConfig,
) -> Result<PushToTalkHotkeyConfig, String> {
    apply_push_to_talk_hotkey_config_internal(&app, config)
}

fn normalize_overlay_hotkey_config(mut config: OverlayHotkeyConfig) -> OverlayHotkeyConfig {
    // Trigger mode is force-pinned to "shortcut". The old "doublespace"
    // mode was removed from the UI because it needs a low-level keyboard
    // hook (WH_KEYBOARD_LL) to detect double-tap without globally
    // intercepting every spacebar press — non-trivial complexity for
    // marginal benefit. Legacy installs that have `mode: "doubleSpace"`
    // get silently coerced here so the daemon doesn't hang trying to
    // register the bare Space key as a global shortcut.
    config.mode = "shortcut".to_string();

    config.shortcut = config.shortcut.trim().to_string();
    if config.shortcut.is_empty() {
        config.shortcut = DEFAULT_OVERLAY_SHORTCUT.to_string();
    }

    config
}

fn resolve_hotkey_shortcut(config: &OverlayHotkeyConfig) -> String {
    // doubleSpace path retired (see normalize_overlay_hotkey_config). Mode
    // is always normalized to "shortcut" before reaching us, so this
    // unconditionally returns the configured chord.
    config.shortcut.clone()
}

fn apply_overlay_hotkey_config_internal(
    app: &tauri::AppHandle,
    config: OverlayHotkeyConfig,
) -> Result<OverlayHotkeyConfig, String> {
    let config = normalize_overlay_hotkey_config(config);

    let mut state = OVERLAY_HOTKEY_STATE
        .lock()
        .map_err(|_| "Overlay hotkey lock failed".to_string())?;

    if let Some(previous_shortcut) = state.active_shortcut.take() {
        let _ = app.global_shortcut().unregister(previous_shortcut.as_str());
    }

    if config.enabled {
        let shortcut = resolve_hotkey_shortcut(&config);
        app.global_shortcut()
            .register(shortcut.as_str())
            .map_err(|error| format!("Cannot register global hotkey {shortcut}: {error}"))?;
        state.active_shortcut = Some(shortcut);
    }

    state.config = config.clone();
    Ok(config)
}

fn handle_global_hotkey_event(
    app: &tauri::AppHandle,
    shortcut: &tauri_plugin_global_shortcut::Shortcut,
    event: tauri_plugin_global_shortcut::ShortcutEvent,
) {
    // Both overlays use the same global-shortcut plugin handler. We dispatch
    // by comparing the matched Shortcut's string form against each overlay's
    // currently-registered shortcut. This is robust to plugin canonicalization
    // (Display impl produces the same string we stored at register time).
    let shortcut_str = shortcut.to_string();

    // Push-to-talk is the ONE shortcut that's a press-and-hold: it must
    // see BOTH the Pressed and Released transitions (Pressed → start a
    // dictation session, Released → stop + paste). Handle it before the
    // Pressed-only gate below so the Released event isn't dropped. Every
    // other shortcut keeps its toggle/Pressed-only semantics unchanged.
    let push_to_talk_match = matches!(
        PUSH_TO_TALK_HOTKEY_STATE.lock(),
        Ok(ref state) if state.active_shortcut.as_deref() == Some(shortcut_str.as_str())
    );
    if push_to_talk_match {
        // The instant the PTT key goes down, capture the foreground window:
        // that is the app the user is dictating into, and the one the
        // released-key paste path must target. A global-hotkey press does
        // NOT steal focus, so `GetForegroundWindow` here is the real target.
        // Without this, the paste lands in whatever window happened to be
        // remembered last (stale, or our own window).
        if event.state == ShortcutState::Pressed {
            commands::clipboard_history::remember_foreground_window();
        }
        // Emit to the main webview — the persistent PTT handler there
        // owns the voice session lifecycle (start continuous, accumulate
        // finals, stop + paste). The backend stays pure orchestration.
        let event_name = match event.state {
            ShortcutState::Pressed => "voice-ptt-start",
            ShortcutState::Released => "voice-ptt-stop",
        };
        if let Some(main) = app.get_webview_window(MAIN_WINDOW_LABEL) {
            let _ = main.emit(event_name, ());
        }
        return;
    }

    // All remaining shortcuts are Pressed-only (toggle-on-press). A
    // Released event for any of them is a no-op.
    if event.state != ShortcutState::Pressed {
        return;
    }

    // Clipboard overlay — toggle. If it's already visible and focused, hide;
    // otherwise show. This is the natural feel for Win+V style hotkeys.
    let clipboard_match = matches!(
        CLIPBOARD_OVERLAY_HOTKEY_STATE.lock(),
        Ok(ref state) if state.active_shortcut.as_deref() == Some(shortcut_str.as_str())
    );
    if clipboard_match {
        toggle_clipboard_overlay_window(app);
        return;
    }

    // Command palette — toggle. The window is pre-created at startup so
    // the first press shows it instantly (no wasted-first-press dance).
    let command_match = matches!(
        COMMAND_OVERLAY_HOTKEY_STATE.lock(),
        Ok(ref state) if state.active_shortcut.as_deref() == Some(shortcut_str.as_str())
    );
    if command_match {
        toggle_command_window(app);
        return;
    }

    // Voice overlay — same toggle pattern.
    let voice_match = matches!(
        VOICE_OVERLAY_HOTKEY_STATE.lock(),
        Ok(ref state) if state.active_shortcut.as_deref() == Some(shortcut_str.as_str())
    );
    if voice_match {
        toggle_voice_overlay_window(app);
        return;
    }

    // Sticky quick-note — NOT a toggle: each press summons a note (Windows
    // Sticky Notes style), unless an open note is still empty, in which case
    // that one is raised instead of spawning a duplicate blank.
    let quick_note_match = matches!(
        QUICK_NOTE_HOTKEY_STATE.lock(),
        Ok(ref state) if state.active_shortcut.as_deref() == Some(shortcut_str.as_str())
    );
    if quick_note_match {
        summon_quick_note(app);
        return;
    }

    // Screen recording — toggle start/stop. The action lives in the frontend (it
    // owns the save-folder/dialog choice + the multi-window teardown), so we just
    // emit and let the recorder page (start, when idle) or the always-alive
    // content-protected toolbar (stop, even while the app is minimized) react.
    // Emitting to ALL webviews covers both surfaces.
    let recording_match = matches!(
        SCREEN_RECORDING_HOTKEY_STATE.lock(),
        Ok(ref state) if state.active_shortcut.as_deref() == Some(shortcut_str.as_str())
    );
    if recording_match {
        let _ = app.emit("screenrec:hotkey-toggle", ());
        return;
    }

    // Per-app hotkeys — a MAP lookup rather than a compare, because this is a
    // user-defined list of chords, not one fixed chord. Toggles the bound app's
    // window (focus if it isn't foreground, minimize if it is), or launches the
    // app when it has no window at all.
    let app_hotkey_target = match APP_HOTKEYS_STATE.lock() {
        Ok(state) => state.active.get(shortcut_str.as_str()).cloned(),
        Err(_) => None,
    };
    if let Some(target) = app_hotkey_target {
        commands::window_control::toggle_or_launch(app, target);
        return;
    }

    // The standalone search overlay (Ctrl+Alt+S) is retired: the command
    // palette (Ctrl+Alt+K) is the single search surface, and Ctrl+Alt+S is no
    // longer registered — so there is no fallthrough action for any other
    // shortcut.
}

/// Toggle the clipboard surface (Ctrl+Shift+V).
///
/// The standalone clipboard-overlay window was retired in Cleanup Wave 1 —
/// `show_clipboard_overlay_window` now redirects to the palette in clipboard
/// mode. But this kept probing `CLIPBOARD_OVERLAY_WINDOW_LABEL`, a window that
/// is never created, so the lookup always missed and EVERY press fell through
/// to "show": the hotkey could open the clipboard but never dismiss it.
///
/// Probing the command window alone isn't enough either: the palette may be
/// open on a different tab, where the right answer is "switch to clipboard",
/// not "hide". Rust can't read the palette's tab — the mode is fire-and-forget
/// via `command-overlay-reset` — so we track the mode we last put it in.
///
/// ponytail: last-mode-we-set, not the palette's live tab. If the user opens
/// clipboard via this hotkey then switches tabs with a scope chip, this still
/// thinks "clipboard" and a press hides instead of switching back — one wasted
/// press, self-correcting. The fix if that ever annoys: mirror the recorder's
/// `screenrec:hotkey-toggle` pattern and let the frontend, which knows its own
/// tab, decide.
fn toggle_clipboard_overlay_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(COMMAND_WINDOW_LABEL) {
        let visible = window.is_visible().unwrap_or(false);
        let focused = window.is_focused().unwrap_or(false);
        let showing_clipboard = LAST_COMMAND_MODE
            .lock()
            .ok()
            .map(|m| m.as_deref() == Some("clipboard"))
            .unwrap_or(false);
        if visible && focused && showing_clipboard {
            let _ = hide_command_window(app);
            return;
        }
    }
    let _ = show_clipboard_overlay_window(app);
}

fn show_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    let main = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .ok_or_else(|| "Main window is unavailable".to_string())?;
    let _ = main.unminimize();
    main.show()
        .map_err(|error| format!("Cannot show main window: {error}"))?;
    main.set_focus()
        .map_err(|error| format!("Cannot focus main window: {error}"))?;
    Ok(())
}

/// Reliable Rust-side heartbeat for the reminder scheduler. Chromium throttles
/// JS `setInterval` to ~once a minute while the main window is hidden in the
/// tray, which made due reminders fire late. This (non-throttled) thread emits a
/// `reminders-tick` the main window listens for, so its due-check runs on time
/// regardless of window state. Firing itself reuses the existing toast/glow path.
fn start_reminder_tick(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(15));
        let _ = app.emit("reminders-tick", ());
    });
}

fn get_or_create_overlay_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    if let Some(overlay) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        return Ok(overlay);
    }

    // The overlay window is transparent (no DWM acrylic / mica) so the
    // rounded panel reads as a clean floating sheet — DWM acrylic paints
    // the square window bounds and leaks dark wedges into the rounded
    // corners. Voice + clipboard overlays follow the same pattern.
    WebviewWindowBuilder::new(
        app,
        OVERLAY_WINDOW_LABEL,
        WebviewUrl::App("/overlay".into()),
    )
    .title("KeepItLocal Quick Search")
    .inner_size(OVERLAY_WINDOW_WIDTH, OVERLAY_RESULTS_HEIGHT)
    .center()
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false)
    .build()
    .map_err(|error| format!("Cannot create overlay window: {error}"))
}

fn show_overlay_window(app: &tauri::AppHandle) -> Result<(), String> {
    // The standalone search overlay is retired — redirect to the unified
    // command palette (default search mode). Kept as a thin wrapper so existing
    // callers (e.g. the File Index "Quick search" button) need no change and the
    // old overlay window is never created.
    show_command_window(app)
}

fn hide_overlay_window(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(overlay) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
        return Ok(());
    };
    overlay
        .hide()
        .map_err(|error| format!("Cannot hide overlay window: {error}"))?;
    Ok(())
}

// ─── Clipboard overlay window + hotkey ──────────────────────────────────
//
// Cleanup Wave 1 retired the standalone clipboard overlay; everything now
// lands in the unified command palette's Clipboard tab. `show_*` redirects
// there, `hide_*` stays as a defensive no-op in case some legacy code path
// still has a label-targeted window reference.

fn show_clipboard_overlay_window(app: &tauri::AppHandle) -> Result<(), String> {
    // Retired the standalone clipboard overlay — redirect to the command
    // palette's Clipboard tab. (remember_foreground_window runs inside the
    // palette show path, so clipboard-mode paste still re-targets the prev app.)
    show_command_window_in_mode(app, Some("clipboard"))
}

fn hide_clipboard_overlay_window(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(CLIPBOARD_OVERLAY_WINDOW_LABEL) else {
        return Ok(());
    };
    window
        .hide()
        .map_err(|error| format!("Cannot hide clipboard overlay: {error}"))?;
    Ok(())
}

// ─── Command palette window + hotkey ─────────────────────────────────
//
// The unified palette. Unlike the three overlays above (lazily created on
// first press), this window is PRE-CREATED at startup — see the call in
// `.setup()` — so the first hotkey press shows it instantly. It otherwise
// follows the same transparent / frameless / always-on-top / hide-on-blur
// pattern. It mirrors the clipboard overlay's `remember_foreground_window`
// call because the palette's clipboard mode can paste into the previously
// focused app.

fn get_or_create_command_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(COMMAND_WINDOW_LABEL) {
        return Ok(window);
    }

    WebviewWindowBuilder::new(
        app,
        COMMAND_WINDOW_LABEL,
        WebviewUrl::App("/command".into()),
    )
    .title("KeepItLocal Command Palette")
    .inner_size(COMMAND_WINDOW_WIDTH, COMMAND_WINDOW_HEIGHT)
    .center()
    // Palette Appearance Wave F (2026-05-27): keep `resizable(true)` so
    // the user-tunable Width preset (Compact / Standard / Wide) can
    // actually resize the window via setSize from the frontend.
    // `.decorations(false)` already removes the OS resize handles, so
    // users can't drag-resize themselves — but programmatic setSize
    // works only when the window is API-resizable.
    .resizable(true)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false)
    .build()
    .map_err(|error| format!("Cannot create command palette window: {error}"))
}

fn show_command_window(app: &tauri::AppHandle) -> Result<(), String> {
    show_command_window_in_mode(app, None)
}

/// Show the command palette, optionally jumping straight to a sub-mode
/// ("clipboard" or "voice"). The standalone search / clipboard / voice overlays
/// are retired: every old summon path now lands HERE on the right tab, so the
/// palette is the single surface and those overlay windows are never created
/// (zero RAM).
fn show_command_window_in_mode(app: &tauri::AppHandle, mode: Option<&str>) -> Result<(), String> {
    // Capture the foreground window BEFORE stealing focus so the palette's
    // clipboard-mode paste can re-target the user's previous app.
    commands::clipboard_history::remember_foreground_window();

    let window = get_or_create_command_window(app)?;
    let _ = window.unminimize();
    // Palette Appearance Wave I (2026-05-27): NO size/center reset on
    // show. The builder already sets initial geometry on first creation
    // (`get_or_create_command_window` — inner_size + center). Forcing
    // these on every show overrode the user's saved Width / Position
    // preferences from commandAppearance.ts — the frontend $effect would
    // apply the user's value, then the next show would reset it back to
    // 640×560 centered. Removing the reset lets the user's choice stick.
    // Edge case: if a user drags the window to a weird spot, it stays
    // there until they pick a Position in the Appearance editor — the
    // intentional pick beats the implicit reset.
    window
        .show()
        .map_err(|error| format!("Cannot show command palette: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Cannot focus command palette: {error}"))?;
    // The window is reused between presses, so onMount fires only once. This
    // event tells the route to reset to default mode, clear the query, refresh
    // recents, and refocus on every summon. The payload carries the target
    // sub-mode ("clipboard"/"voice") or null (default search), so a redirected
    // clipboard/voice hotkey opens straight on that tab.
    let _ = window.emit("command-overlay-reset", mode);
    // Remember which tab we put it on, so the clipboard/voice hotkeys can tell
    // "dismiss the tab I'm on" from "switch to my tab". See
    // toggle_clipboard_overlay_window for the staleness ceiling.
    if let Ok(mut last) = LAST_COMMAND_MODE.lock() {
        *last = mode.map(str::to_string);
    }
    Ok(())
}

fn hide_command_window(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(COMMAND_WINDOW_LABEL) else {
        return Ok(());
    };
    window
        .hide()
        .map_err(|error| format!("Cannot hide command palette: {error}"))?;
    // A hidden palette is on no tab. Without this, dismissing it would leave
    // "clipboard" latched and the next Ctrl+Shift+V would read as "dismiss the
    // tab I'm showing" while nothing is showing.
    if let Ok(mut last) = LAST_COMMAND_MODE.lock() {
        *last = None;
    }
    Ok(())
}

fn toggle_command_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(COMMAND_WINDOW_LABEL) {
        let visible = window.is_visible().unwrap_or(false);
        let focused = window.is_focused().unwrap_or(false);
        if visible && focused {
            let _ = hide_command_window(app);
            return;
        }
    }
    let _ = show_command_window(app);
}

// ─── Quick-note sticky window ────────────────────────────────────────
//
// A deliberately MINIMAL sticky note for jotting something without opening
// the full app. RAM-conscious: the route is a bare <textarea> (no editor
// framework), the window is created ON DEMAND, and — unlike the warm command
// palette — it is DESTROYED on close (it is intentionally NOT in the
// close-to-hide or hide-on-blur handlers), so its WebView2 memory is freed the
// moment it closes. It saves straight to a `.ki` file via the notes commands,
// so quick notes appear in the Notes app + search like any other note.
//
// The old `get_or_create_quick_note_window` "single shared window" helper
// is gone — `show_quick_note` always builds a fresh window from scratch
// (label = `quicknote-<n>` from QUICK_NOTE_COUNTER) so multiple notes
// can be open at once.

/// Monotonic counter so each "Create note" spawns its OWN window
/// (`quicknote-<n>`) — multiple sticky notes can be open at once, Windows
/// Sticky Notes style. Each window is in no close-to-hide handler, so it is
/// DESTROYED on close and its WebView2 memory is freed immediately.
static QUICK_NOTE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Build + show a fresh sticky note window. Synchronous (the builder calls are
/// sync), so both the async command and the global-hotkey handler can drive it
/// — the handler dispatches it onto the async runtime, because a new
/// WebviewWindow must not be built on the global-shortcut handler thread.
fn show_quick_note_impl(app: &tauri::AppHandle) -> Result<(), String> {
    let n = QUICK_NOTE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let label = format!("{QUICK_NOTE_WINDOW_LABEL}-{n}");
    // Eager-seed the emptiness registry so a just-spawned note counts as empty
    // for the hotkey dedup the instant it's built — before its webview's
    // onMount round-trips its empty state back. Shrinks the rapid-double-press
    // window where a second press would otherwise spawn a duplicate blank.
    // Pruned on close (and overwritten by the webview's own reports).
    if let Ok(mut map) = QUICK_NOTE_EMPTY.lock() {
        map.insert(label.clone(), true);
    }
    // Cascade each new note so they don't stack exactly on top of one another.
    let offset = (n % 8) as f64 * 28.0;
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("/quicknote".into()))
        .title("Quick Note")
        .inner_size(340.0, 400.0)
        .min_inner_size(260.0, 220.0)
        .position(180.0 + offset, 140.0 + offset)
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(false)
        .shadow(false)
        .visible(false)
        .build()
        .map_err(|error| format!("Cannot create quick-note window: {error}"))?;
    let _ = window.unminimize();
    window
        .show()
        .map_err(|error| format!("Cannot show quick note: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Cannot focus quick note: {error}"))?;
    Ok(())
}

#[tauri::command(async)]
async fn show_quick_note(app: tauri::AppHandle) -> Result<(), String> {
    show_quick_note_impl(&app)
}

/// Parse the trailing `-<n>` index of a `quicknote-<n>` label (for picking the
/// most-recently-created empty note). Returns 0 if there is no numeric suffix.
fn quick_note_index(label: &str) -> u64 {
    label
        .rsplit('-')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Summon a sticky note for the global hotkey, with empty-note dedup: if any
/// open note is still empty, raise the most-recently-created empty one instead
/// of spawning a duplicate blank; otherwise spawn a fresh note. The actual
/// build is dispatched onto the async runtime (matching how `show_quick_note`
/// runs) because a window must not be built on the shortcut-handler thread.
fn summon_quick_note(app: &tauri::AppHandle) {
    let prefix = format!("{QUICK_NOTE_WINDOW_LABEL}-");
    let empty_label = {
        let map = match QUICK_NOTE_EMPTY.lock() {
            Ok(map) => map,
            Err(_) => return,
        };
        app.webview_windows()
            .into_keys()
            .filter(|label| label.starts_with(&prefix))
            .filter(|label| map.get(label).copied().unwrap_or(false))
            .max_by_key(|label| quick_note_index(label))
    };
    if let Some(label) = empty_label {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            return;
        }
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // The hotkey path used to discard this error, so a failed window build
        // was indistinguishable from "the hotkey never fired" — the palette
        // path toasts its error, this one had no signal at all. Route it into
        // the diagnostic log (Settings → Logs) instead.
        if let Err(error) = show_quick_note_impl(&app) {
            log_backend_error(&app, "Sticky note hotkey", error);
        }
    });
}

// ─── Voice overlay window ────────────────────────────────────────────
//
// Mirrors the clipboard-overlay pattern: pre-created once at startup,
// hidden by default, summoned via global hotkey. The voice overlay is
// the only surface that *auto-arms* the voice session — that's its
// whole reason for being. Other surfaces (search overlay, clipboard
// overlay, pages) require a manual mic-button click.
//
// Cleanup Wave 1 retired the standalone voice overlay too — `show_*`
// redirects to the command palette's Voice tab. `hide_*` remains as a
// defensive no-op in case some path still holds the label.

fn show_voice_overlay_window(app: &tauri::AppHandle) -> Result<(), String> {
    // Retired the standalone voice overlay — redirect to the command palette's
    // Voice tab.
    show_command_window_in_mode(app, Some("voice"))
}

fn hide_voice_overlay_window(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(VOICE_OVERLAY_WINDOW_LABEL) else {
        return Ok(());
    };
    window
        .hide()
        .map_err(|error| format!("Cannot hide voice overlay: {error}"))?;
    Ok(())
}

// ─── Mouse-grid overlay window (#18b) ────────────────────────────────
//
// A transparent, full-screen, always-on-top window for hands-free
// pointer targeting. Created on first use, then reused (hidden, not
// destroyed). The route at /mouse-grid runs its own digit-grammar
// recognizer; on a final selection it hides this window, warps the
// cursor, and clicks the app beneath.

fn get_or_create_mouse_grid_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(MOUSE_GRID_WINDOW_LABEL) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(
        app,
        MOUSE_GRID_WINDOW_LABEL,
        WebviewUrl::App("/mouse-grid".into()),
    )
    .title("KeepItLocal Mouse Grid")
    // Placeholder size — `show_mouse_grid_window` resizes to the full
    // monitor before showing.
    .inner_size(800.0, 600.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false)
    .build()
    .map_err(|error| format!("Cannot create mouse grid window: {error}"))
}

fn show_mouse_grid_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = get_or_create_mouse_grid_window(app)?;
    // Cover the entire primary monitor — including over the taskbar —
    // so the grid's fractional coords map 1:1 to the SetCursorPos
    // screen pixels `voice_mouse_warp` resolves against.
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let _ = window.set_position(*monitor.position());
        let _ = window.set_size(*monitor.size());
    }
    let _ = window.unminimize();
    window
        .show()
        .map_err(|error| format!("Cannot show mouse grid: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Cannot focus mouse grid: {error}"))?;
    // The window is reused between invocations — tell the route to
    // reset its state and re-arm its recognizer on every show.
    let _ = window.emit("mouse-grid-reset", ());
    Ok(())
}

fn hide_mouse_grid_window(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(MOUSE_GRID_WINDOW_LABEL) else {
        return Ok(());
    };
    window
        .hide()
        .map_err(|error| format!("Cannot hide mouse grid: {error}"))?;
    Ok(())
}

// ─── UI-elements overlay window (#20) ────────────────────────────────
//
// A transparent, full-screen, always-on-top window for the voice
// accessibility "show elements" command. `voice_list_ui_elements`
// enumerates the focused window's controls FIRST (while the user's app
// is still foreground); this window then numbers them with badges. The
// route at /ui-elements runs its own recognizer (numbers + labels); on
// a selection it hides this window, warps the cursor, and clicks the
// app beneath. Created on first use, then reused (hidden, not destroyed).

fn get_or_create_ui_elements_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(UI_ELEMENTS_WINDOW_LABEL) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(
        app,
        UI_ELEMENTS_WINDOW_LABEL,
        WebviewUrl::App("/ui-elements".into()),
    )
    .title("KeepItLocal UI Elements")
    // Placeholder size — `show_ui_elements_window` resizes to the full
    // monitor before showing.
    .inner_size(800.0, 600.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false)
    .build()
    .map_err(|error| format!("Cannot create UI elements window: {error}"))
}

fn show_ui_elements_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = get_or_create_ui_elements_window(app)?;
    // Cover the entire primary monitor so the element badges' fractional
    // coords map 1:1 to the SetCursorPos screen pixels `voice_mouse_warp`
    // resolves against.
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let _ = window.set_position(*monitor.position());
        let _ = window.set_size(*monitor.size());
    }
    let _ = window.unminimize();
    window
        .show()
        .map_err(|error| format!("Cannot show UI elements overlay: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Cannot focus UI elements overlay: {error}"))?;
    // The window is reused between invocations — tell the route to
    // re-fetch the freshly enumerated elements and re-arm its recognizer.
    let _ = window.emit("ui-elements-reset", ());
    Ok(())
}

fn hide_ui_elements_window(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(UI_ELEMENTS_WINDOW_LABEL) else {
        return Ok(());
    };
    window
        .hide()
        .map_err(|error| format!("Cannot hide UI elements overlay: {error}"))?;
    Ok(())
}

// ─── Focus break overlay (Pomodoro takeover) ─────────────────────────
//
// A full-screen, OPAQUE, always-on-top window shown when a Pomodoro work
// phase ends and a break begins — far more noticeable than a toast you miss.
// The /focus-break route shows the break length + a live countdown (mirrored
// from an absolute end-time the main window emits) + Skip / Dismiss. Opaque,
// so the binding overlay rules (no DWM acrylic on transparent rounded panels)
// don't apply. Created on first use, then DESTROYED on dismiss to free its RAM —
// it's a big, infrequent full-screen overlay, so the cold re-create on the next
// break is a fine trade (mirrors the quick-note window's destroy-on-close).

const FOCUS_BREAK_WINDOW_LABEL: &str = "focus-break";

fn get_or_create_focus_break_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(FOCUS_BREAK_WINDOW_LABEL) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(
        app,
        FOCUS_BREAK_WINDOW_LABEL,
        WebviewUrl::App("/focus-break".into()),
    )
    .title("KeepItLocal Break")
    .inner_size(800.0, 600.0)
    .resizable(false)
    .decorations(false)
    .transparent(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false)
    .build()
    .map_err(|error| format!("Cannot create break overlay: {error}"))
}

fn show_focus_break_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = get_or_create_focus_break_window(app)?;
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let _ = window.set_position(*monitor.position());
        let _ = window.set_size(*monitor.size());
    }
    let _ = window.unminimize();
    window
        .show()
        .map_err(|error| format!("Cannot show break overlay: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Cannot focus break overlay: {error}"))?;
    Ok(())
}

fn hide_focus_break_window(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(FOCUS_BREAK_WINDOW_LABEL) {
        // DESTROY (not hide) so the full-screen break overlay's WebView2 renderer
        // (~100-150 MB) is freed the moment the break ends. Breaks are infrequent
        // and not latency-sensitive, so a cold re-create on the next break is a
        // fine trade for the reclaimed RAM; `show_focus_break_window` rebuilds it.
        window
            .destroy()
            .map_err(|error| format!("Cannot close break overlay: {error}"))?;
    }
    Ok(())
}

// ─── Focus glow overlay ──────────────────────────────────────────────
//
// A transparent, CLICK-THROUGH, always-on-top full-screen window that flashes
// an orange screen-edge glow when a blocked app is opened during a Focus
// session. It never takes focus or input — the user stays in their app; this
// is a peripheral nudge. (Transparent: obeys the overlay rules — no DWM
// acrylic, content mounts visible so there's no entrance flicker.)

const FOCUS_GLOW_WINDOW_LABEL: &str = "focus-glow";

fn get_or_create_focus_glow_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(FOCUS_GLOW_WINDOW_LABEL) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(
        app,
        FOCUS_GLOW_WINDOW_LABEL,
        WebviewUrl::App("/focus-glow".into()),
    )
    .title("KeepItLocal Focus")
    .inner_size(800.0, 600.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .focused(false)
    .visible(false)
    .build()
    .map_err(|error| format!("Cannot create focus glow: {error}"))
}

fn show_focus_glow_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = get_or_create_focus_glow_window(app)?;
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let _ = window.set_position(*monitor.position());
        let _ = window.set_size(*monitor.size());
    }
    // Click-through: pointer events pass straight to the app underneath.
    let _ = window.set_ignore_cursor_events(true);
    window
        .show()
        .map_err(|error| format!("Cannot show focus glow: {error}"))?;
    // Deliberately NOT set_focus — never steal focus from the user's app.
    Ok(())
}

fn hide_focus_glow_window(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(FOCUS_GLOW_WINDOW_LABEL) {
        window
            .hide()
            .map_err(|error| format!("Cannot hide focus glow: {error}"))?;
    }
    Ok(())
}

// Window-show commands must be `(async)` — `WebviewWindowBuilder::build()`
// needs the main event loop, so the command runs off it (see the note on the
// other show_* commands above).
#[tauri::command(async)]
async fn show_focus_break_window_command(app: tauri::AppHandle) -> Result<(), String> {
    show_focus_break_window(&app)
}
#[tauri::command(async)]
async fn hide_focus_break_window_command(app: tauri::AppHandle) -> Result<(), String> {
    hide_focus_break_window(&app)
}
#[tauri::command(async)]
async fn show_focus_glow_window_command(app: tauri::AppHandle) -> Result<(), String> {
    show_focus_glow_window(&app)
}
#[tauri::command(async)]
async fn hide_focus_glow_window_command(app: tauri::AppHandle) -> Result<(), String> {
    hide_focus_glow_window(&app)
}

// ─── Notification toast overlay ──────────────────────────────────────
//
// A transparent, CLICK-THROUGH, always-on-top full-screen window that renders a
// small toast card (bottom-right) carrying title + body text. This is our OWN
// reliable notification channel: Windows toast notifications silently no-op in
// dev (they need an installed Start-Menu shortcut / AUMID) and can be suppressed
// even when installed — but a due reminder, a Pomodoro phase change, or a Focus
// warning MUST be seen. This window always shows, no matter which app is focused
// or whether KeepItLocal is minimized to the tray. (Same overlay rules as the
// glow: no DWM acrylic, content mounts visible, never steals focus or input.)

const NOTIFY_TOAST_WINDOW_LABEL: &str = "notify-toast";

fn get_or_create_notify_toast_window(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(NOTIFY_TOAST_WINDOW_LABEL) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(
        app,
        NOTIFY_TOAST_WINDOW_LABEL,
        WebviewUrl::App("/notify-toast".into()),
    )
    .title("KeepItLocal")
    .inner_size(800.0, 600.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .focused(false)
    .visible(false)
    .build()
    .map_err(|error| format!("Cannot create notify toast: {error}"))
}

fn show_notify_toast_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = get_or_create_notify_toast_window(app)?;
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let _ = window.set_position(*monitor.position());
        let _ = window.set_size(*monitor.size());
    }
    // Click-through: the toast is informational; pointer events pass straight to
    // whatever the user is doing underneath.
    let _ = window.set_ignore_cursor_events(true);
    window
        .show()
        .map_err(|error| format!("Cannot show notify toast: {error}"))?;
    // Deliberately NOT set_focus — a notification never steals the user's focus.
    Ok(())
}

fn hide_notify_toast_window(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(NOTIFY_TOAST_WINDOW_LABEL) {
        window
            .hide()
            .map_err(|error| format!("Cannot hide notify toast: {error}"))?;
    }
    Ok(())
}

#[tauri::command(async)]
async fn show_notify_toast_window_command(app: tauri::AppHandle) -> Result<(), String> {
    show_notify_toast_window(&app)
}
#[tauri::command(async)]
async fn hide_notify_toast_window_command(app: tauri::AppHandle) -> Result<(), String> {
    hide_notify_toast_window(&app)
}

fn toggle_voice_overlay_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(VOICE_OVERLAY_WINDOW_LABEL) {
        let visible = window.is_visible().unwrap_or(false);
        let focused = window.is_focused().unwrap_or(false);
        if visible && focused {
            let _ = hide_voice_overlay_window(app);
            return;
        }
    }
    let _ = show_voice_overlay_window(app);
}

fn normalize_voice_overlay_hotkey_config(
    mut config: VoiceOverlayHotkeyConfig,
) -> VoiceOverlayHotkeyConfig {
    config.shortcut = config.shortcut.trim().to_string();
    if config.shortcut.is_empty() {
        config.shortcut = DEFAULT_VOICE_OVERLAY_SHORTCUT.to_string();
    }
    config
}

fn apply_voice_overlay_hotkey_config_internal(
    app: &tauri::AppHandle,
    config: VoiceOverlayHotkeyConfig,
) -> Result<VoiceOverlayHotkeyConfig, String> {
    let config = normalize_voice_overlay_hotkey_config(config);

    let mut state = VOICE_OVERLAY_HOTKEY_STATE
        .lock()
        .map_err(|_| "Voice overlay hotkey lock failed".to_string())?;

    if let Some(previous_shortcut) = state.active_shortcut.take() {
        let _ = app.global_shortcut().unregister(previous_shortcut.as_str());
    }

    if config.enabled {
        let shortcut_str = config.shortcut.clone();
        let parsed: tauri_plugin_global_shortcut::Shortcut = shortcut_str
            .parse()
            .map_err(|error| format!("Cannot parse shortcut '{shortcut_str}': {error}"))?;
        // Idempotent: drop any prior registration of this chord first so a
        // double-apply (startup + frontend, multi-window, or capture-resume)
        // can never fail with "HotKey already registered". No-op if unregistered.
        let _ = app.global_shortcut().unregister(parsed);
        app.global_shortcut().register(parsed).map_err(|error| {
            format!("Cannot register voice overlay hotkey {shortcut_str}: {error}")
        })?;
        state.active_shortcut = Some(parsed.to_string());
    }

    state.config = config.clone();
    Ok(config)
}

fn load_startup_voice_overlay_hotkey(app: &tauri::AppHandle) -> Result<(), String> {
    let loaded = load_app_settings(app.clone())?;
    let config = VoiceOverlayHotkeyConfig {
        enabled: loaded.voice_overlay_enabled,
        shortcut: loaded.voice_overlay_shortcut,
    };
    let _ = apply_voice_overlay_hotkey_config_internal(app, config)?;
    Ok(())
}

fn normalize_push_to_talk_hotkey_config(
    mut config: PushToTalkHotkeyConfig,
) -> PushToTalkHotkeyConfig {
    config.shortcut = config.shortcut.trim().to_string();
    if config.shortcut.is_empty() {
        config.shortcut = DEFAULT_PUSH_TO_TALK_SHORTCUT.to_string();
    }
    config
}

fn apply_push_to_talk_hotkey_config_internal(
    app: &tauri::AppHandle,
    config: PushToTalkHotkeyConfig,
) -> Result<PushToTalkHotkeyConfig, String> {
    let config = normalize_push_to_talk_hotkey_config(config);

    let mut state = PUSH_TO_TALK_HOTKEY_STATE
        .lock()
        .map_err(|_| "Push-to-talk hotkey lock failed".to_string())?;

    if let Some(previous_shortcut) = state.active_shortcut.take() {
        let _ = app.global_shortcut().unregister(previous_shortcut.as_str());
    }

    if config.enabled {
        let shortcut_str = config.shortcut.clone();
        // Parse first, then register + store the parsed value's string
        // form — the plugin canonicalizes accelerators, and the
        // Pressed/Released dispatch in `handle_global_hotkey_event`
        // compares against `active_shortcut`. Both sides must go through
        // identical canonicalization or the comparison silently misses.
        let parsed: tauri_plugin_global_shortcut::Shortcut = shortcut_str
            .parse()
            .map_err(|error| format!("Cannot parse shortcut '{shortcut_str}': {error}"))?;
        // Idempotent: drop any prior registration of this chord first so a
        // double-apply (startup + frontend, multi-window, or capture-resume)
        // can never fail with "HotKey already registered". No-op if unregistered.
        let _ = app.global_shortcut().unregister(parsed);
        app.global_shortcut().register(parsed).map_err(|error| {
            format!("Cannot register push-to-talk hotkey {shortcut_str}: {error}")
        })?;
        state.active_shortcut = Some(parsed.to_string());
    }

    state.config = config.clone();
    Ok(config)
}

fn load_startup_push_to_talk_hotkey(app: &tauri::AppHandle) -> Result<(), String> {
    let loaded = load_app_settings(app.clone())?;
    let config = PushToTalkHotkeyConfig {
        enabled: loaded.push_to_talk_enabled,
        shortcut: loaded.push_to_talk_shortcut,
    };
    let _ = apply_push_to_talk_hotkey_config_internal(app, config)?;
    Ok(())
}

fn normalize_clipboard_overlay_hotkey_config(
    mut config: ClipboardOverlayHotkeyConfig,
) -> ClipboardOverlayHotkeyConfig {
    config.shortcut = config.shortcut.trim().to_string();
    if config.shortcut.is_empty() {
        config.shortcut = DEFAULT_CLIPBOARD_OVERLAY_SHORTCUT.to_string();
    }
    config
}

fn apply_clipboard_overlay_hotkey_config_internal(
    app: &tauri::AppHandle,
    config: ClipboardOverlayHotkeyConfig,
) -> Result<ClipboardOverlayHotkeyConfig, String> {
    let config = normalize_clipboard_overlay_hotkey_config(config);

    let mut state = CLIPBOARD_OVERLAY_HOTKEY_STATE
        .lock()
        .map_err(|_| "Clipboard overlay hotkey lock failed".to_string())?;

    if let Some(previous_shortcut) = state.active_shortcut.take() {
        let _ = app.global_shortcut().unregister(previous_shortcut.as_str());
    }

    if config.enabled {
        let shortcut_str = config.shortcut.clone();
        // The plugin canonicalizes "CommandOrControl+Shift+V" → "Ctrl+Shift+V"
        // (or platform equivalent) when it dispatches WM_HOTKEY events back
        // to our handler. We must store the *canonical* form in state so the
        // handler's `shortcut.to_string()` comparison matches; storing the
        // raw user input would silently break dispatch every time. Parsing
        // here and using the parsed value for both register and storage
        // guarantees both sides go through identical canonicalization.
        let parsed: tauri_plugin_global_shortcut::Shortcut = shortcut_str
            .parse()
            .map_err(|error| format!("Cannot parse shortcut '{shortcut_str}': {error}"))?;
        // Idempotent: drop any prior registration of this chord first so a
        // double-apply (startup + frontend, multi-window, or capture-resume)
        // can never fail with "HotKey already registered". No-op if unregistered.
        let _ = app.global_shortcut().unregister(parsed);
        app.global_shortcut()
            .register(parsed)
            .map_err(|error| format!("Cannot register clipboard hotkey {shortcut_str}: {error}"))?;
        state.active_shortcut = Some(parsed.to_string());
    }

    state.config = config.clone();
    Ok(config)
}

fn normalize_command_overlay_hotkey_config(
    mut config: CommandOverlayHotkeyConfig,
) -> CommandOverlayHotkeyConfig {
    config.shortcut = config.shortcut.trim().to_string();
    if config.shortcut.is_empty() {
        config.shortcut = DEFAULT_COMMAND_OVERLAY_SHORTCUT.to_string();
    }
    config
}

fn apply_command_overlay_hotkey_config_internal(
    app: &tauri::AppHandle,
    config: CommandOverlayHotkeyConfig,
) -> Result<CommandOverlayHotkeyConfig, String> {
    let config = normalize_command_overlay_hotkey_config(config);

    let mut state = COMMAND_OVERLAY_HOTKEY_STATE
        .lock()
        .map_err(|_| "Command overlay hotkey lock failed".to_string())?;

    if let Some(previous_shortcut) = state.active_shortcut.take() {
        let _ = app.global_shortcut().unregister(previous_shortcut.as_str());
    }

    if config.enabled {
        let shortcut_str = config.shortcut.clone();
        // Parse-then-store the canonical form so the handler's
        // `shortcut.to_string()` comparison matches (see the clipboard
        // applier above for the full rationale).
        let parsed: tauri_plugin_global_shortcut::Shortcut = shortcut_str
            .parse()
            .map_err(|error| format!("Cannot parse shortcut '{shortcut_str}': {error}"))?;
        // Idempotent: drop any prior registration of this chord first so a
        // double-apply (startup + frontend, multi-window, or capture-resume)
        // can never fail with "HotKey already registered". No-op if unregistered.
        let _ = app.global_shortcut().unregister(parsed);
        app.global_shortcut().register(parsed).map_err(|error| {
            format!("Cannot register command palette hotkey {shortcut_str}: {error}")
        })?;
        state.active_shortcut = Some(parsed.to_string());
    }

    state.config = config.clone();
    Ok(config)
}

fn normalize_quick_note_hotkey_config(mut config: QuickNoteHotkeyConfig) -> QuickNoteHotkeyConfig {
    config.shortcut = config.shortcut.trim().to_string();
    if config.shortcut.is_empty() {
        config.shortcut = DEFAULT_QUICK_NOTE_SHORTCUT.to_string();
    }
    config
}

fn apply_quick_note_hotkey_config_internal(
    app: &tauri::AppHandle,
    config: QuickNoteHotkeyConfig,
) -> Result<QuickNoteHotkeyConfig, String> {
    let config = normalize_quick_note_hotkey_config(config);

    let mut state = QUICK_NOTE_HOTKEY_STATE
        .lock()
        .map_err(|_| "Quick note hotkey lock failed".to_string())?;

    if let Some(previous_shortcut) = state.active_shortcut.take() {
        let _ = app.global_shortcut().unregister(previous_shortcut.as_str());
    }

    if config.enabled {
        let shortcut_str = config.shortcut.clone();
        // Parse-then-store the canonical form so the handler's
        // `shortcut.to_string()` comparison matches.
        let parsed: tauri_plugin_global_shortcut::Shortcut = shortcut_str
            .parse()
            .map_err(|error| format!("Cannot parse shortcut '{shortcut_str}': {error}"))?;
        // Idempotent: drop any prior registration of this chord first so a
        // double-apply (startup + frontend, or capture-resume) never fails
        // with "HotKey already registered". No-op if unregistered.
        let _ = app.global_shortcut().unregister(parsed);
        app.global_shortcut().register(parsed).map_err(|error| {
            format!("Cannot register sticky note hotkey {shortcut_str}: {error}")
        })?;
        state.active_shortcut = Some(parsed.to_string());
    }

    state.config = config.clone();
    Ok(config)
}

/// Register the user's per-app hotkeys, replacing whatever was registered
/// before. Mirrors `apply_quick_note_hotkey_config_internal` (unregister the
/// previous chords, register the current ones, remember the canonical forms),
/// but over a list.
///
/// A single bad row must not take the others down with it: an unparseable or
/// already-taken chord is skipped and reported in the returned error string,
/// while every other binding still registers.
fn apply_app_hotkeys_config_internal(
    app: &tauri::AppHandle,
    config: AppHotkeysConfig,
) -> Result<AppHotkeysConfig, String> {
    let mut state = APP_HOTKEYS_STATE
        .lock()
        .map_err(|_| "App hotkeys lock failed".to_string())?;

    for previous in state.active.keys() {
        let _ = app.global_shortcut().unregister(previous.as_str());
    }
    state.active.clear();

    let mut failures: Vec<String> = Vec::new();
    for binding in &config.bindings {
        let shortcut_str = binding.shortcut.trim();
        let target = binding.target.trim();
        if !binding.enabled || shortcut_str.is_empty() || target.is_empty() {
            continue;
        }
        // Parse-then-store the canonical form so the dispatcher's
        // `shortcut.to_string()` lookup matches.
        let parsed: tauri_plugin_global_shortcut::Shortcut = match shortcut_str.parse() {
            Ok(parsed) => parsed,
            Err(error) => {
                failures.push(format!("{shortcut_str}: {error}"));
                continue;
            }
        };
        let canonical = parsed.to_string();
        // Two rows bound to the same chord: first one wins, second is
        // reported rather than silently shadowing it.
        if state.active.contains_key(&canonical) {
            failures.push(format!(
                "{shortcut_str}: already used by another app hotkey"
            ));
            continue;
        }
        // We removed the previous per-app registrations above. If the chord is
        // still registered, it belongs to one of KeepItLocal's fixed shortcuts;
        // never unregister it here or the fixed dispatcher branch would win
        // while this row misleadingly appears active.
        if app.global_shortcut().is_registered(canonical.as_str()) {
            failures.push(format!(
                "{shortcut_str}: already used by another KeepItLocal shortcut"
            ));
            continue;
        }
        if let Err(error) = app.global_shortcut().register(parsed) {
            failures.push(format!("{shortcut_str}: {error}"));
            continue;
        }
        state.active.insert(canonical, target.to_string());
    }

    if failures.is_empty() {
        Ok(config)
    } else {
        Err(format!(
            "Some app hotkeys could not be registered — {}",
            failures.join("; ")
        ))
    }
}

fn normalize_screen_recording_hotkey_config(
    mut config: ScreenRecordingHotkeyConfig,
) -> ScreenRecordingHotkeyConfig {
    config.shortcut = config.shortcut.trim().to_string();
    if config.shortcut.is_empty() {
        config.shortcut = DEFAULT_SCREEN_RECORDING_SHORTCUT.to_string();
    }
    config
}

fn apply_screen_recording_hotkey_config_internal(
    app: &tauri::AppHandle,
    config: ScreenRecordingHotkeyConfig,
) -> Result<ScreenRecordingHotkeyConfig, String> {
    let config = normalize_screen_recording_hotkey_config(config);

    let mut state = SCREEN_RECORDING_HOTKEY_STATE
        .lock()
        .map_err(|_| "Screen recording hotkey lock failed".to_string())?;

    if let Some(previous_shortcut) = state.active_shortcut.take() {
        let _ = app.global_shortcut().unregister(previous_shortcut.as_str());
    }

    if config.enabled {
        let shortcut_str = config.shortcut.clone();
        let parsed: tauri_plugin_global_shortcut::Shortcut = shortcut_str
            .parse()
            .map_err(|error| format!("Cannot parse shortcut '{shortcut_str}': {error}"))?;
        let _ = app.global_shortcut().unregister(parsed);
        app.global_shortcut().register(parsed).map_err(|error| {
            format!("Cannot register screen recording hotkey {shortcut_str}: {error}")
        })?;
        state.active_shortcut = Some(parsed.to_string());
    }

    state.config = config.clone();
    Ok(config)
}

/// Temporarily unregister all KeepItLocal global shortcuts so the Settings
/// shortcut picker can capture key combos in the webview without the OS
/// intercepting them at the global-shortcut layer first.
///
/// Without this, pressing `Ctrl+Shift+V` to remap the clipboard overlay
/// would actually OPEN the clipboard overlay (because the OS routes the
/// chord to the registered global handler before our webview's keydown
/// listener ever sees it). The picker would only ever see modifiers.
///
/// `active_shortcut` fields are LEFT INTACT in the runtime state — the
/// resume command relies on them to know what to re-register. Callers
/// must always pair `suspend_global_shortcuts_for_capture` with either a
/// fresh apply-config call (when committing a new shortcut) OR a
/// `resume_global_shortcuts_after_capture` (when canceling).
#[tauri::command]
fn suspend_global_shortcuts_for_capture(app: tauri::AppHandle) -> Result<(), String> {
    // Unregister the search-overlay shortcut, if any.
    if let Ok(state) = OVERLAY_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().unregister(shortcut);
        }
    }
    // Same for the clipboard-overlay shortcut.
    if let Ok(state) = CLIPBOARD_OVERLAY_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().unregister(shortcut);
        }
    }
    // Same for the command-palette shortcut.
    if let Ok(state) = COMMAND_OVERLAY_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().unregister(shortcut);
        }
    }
    // Same for the voice-overlay shortcut.
    if let Ok(state) = VOICE_OVERLAY_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().unregister(shortcut);
        }
    }
    // Same for the push-to-talk shortcut.
    if let Ok(state) = PUSH_TO_TALK_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().unregister(shortcut);
        }
    }
    // Same for the sticky-note shortcut.
    if let Ok(state) = QUICK_NOTE_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().unregister(shortcut);
        }
    }
    if let Ok(state) = SCREEN_RECORDING_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().unregister(shortcut);
        }
    }
    // Per-app hotkeys — a list, so we release every registered chord. Without
    // this, recording a new chord in Settings → Shortcuts while an existing
    // per-app binding uses that same combination would fire the app toggle
    // instead of reaching the picker's keydown handler.
    if let Ok(state) = APP_HOTKEYS_STATE.lock() {
        for shortcut in state.active.keys() {
            let _ = app.global_shortcut().unregister(shortcut.as_str());
        }
    }
    Ok(())
}

/// Pair with `suspend_global_shortcuts_for_capture`. Re-registers the
/// previously-active shortcuts using their cached active_shortcut
/// strings. Errors are swallowed (best-effort restore) — if a shortcut
/// fails to re-register the user will notice the missing hotkey and
/// can fix it in Settings.
#[tauri::command]
fn resume_global_shortcuts_after_capture(app: tauri::AppHandle) -> Result<(), String> {
    if let Ok(state) = OVERLAY_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().register(shortcut);
        }
    }
    if let Ok(state) = CLIPBOARD_OVERLAY_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().register(shortcut);
        }
    }
    if let Ok(state) = COMMAND_OVERLAY_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().register(shortcut);
        }
    }
    if let Ok(state) = VOICE_OVERLAY_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().register(shortcut);
        }
    }
    if let Ok(state) = PUSH_TO_TALK_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().register(shortcut);
        }
    }
    if let Ok(state) = QUICK_NOTE_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().register(shortcut);
        }
    }
    if let Ok(state) = SCREEN_RECORDING_HOTKEY_STATE.lock() {
        if let Some(shortcut) = state.active_shortcut.as_deref() {
            let _ = app.global_shortcut().register(shortcut);
        }
    }
    // Per-app hotkeys. Matters most on CANCEL: a committed capture is
    // followed by a full re-apply from the settings store, but an Esc'd one
    // is not — without this the user's app chords would stay dead until the
    // next settings change.
    if let Ok(state) = APP_HOTKEYS_STATE.lock() {
        for shortcut in state.active.keys() {
            let _ = app.global_shortcut().register(shortcut.as_str());
        }
    }
    Ok(())
}

fn load_startup_clipboard_overlay_hotkey(app: &tauri::AppHandle) -> Result<(), String> {
    let loaded = load_app_settings(app.clone())?;
    let config = ClipboardOverlayHotkeyConfig {
        enabled: loaded.clipboard_overlay_enabled,
        shortcut: loaded.clipboard_overlay_shortcut,
    };
    let _ = apply_clipboard_overlay_hotkey_config_internal(app, config)?;
    Ok(())
}

fn load_startup_command_overlay_hotkey(app: &tauri::AppHandle) -> Result<(), String> {
    let loaded = load_app_settings(app.clone())?;
    let config = CommandOverlayHotkeyConfig {
        enabled: loaded.command_overlay_enabled,
        shortcut: loaded.command_overlay_shortcut,
    };
    let _ = apply_command_overlay_hotkey_config_internal(app, config)?;
    Ok(())
}

fn load_startup_quick_note_hotkey(app: &tauri::AppHandle) -> Result<(), String> {
    let loaded = load_app_settings(app.clone())?;
    let config = QuickNoteHotkeyConfig {
        enabled: loaded.quick_note_hotkey_enabled,
        shortcut: loaded.quick_note_hotkey_shortcut,
    };
    let _ = apply_quick_note_hotkey_config_internal(app, config)?;
    Ok(())
}

fn load_startup_app_hotkeys(app: &tauri::AppHandle) -> Result<(), String> {
    let loaded = load_app_settings(app.clone())?;
    let config = AppHotkeysConfig {
        bindings: loaded.app_hotkeys,
    };
    let _ = apply_app_hotkeys_config_internal(app, config)?;
    Ok(())
}

/// Deliberately unused until Screen Recorder exposes a supported global start/stop
/// hotkey. Its only caller in `setup` stays commented out, so the hotkey can be
/// restored in one line. Kept compiling so it can't silently rot in the meantime.
#[allow(dead_code)]
fn load_startup_screen_recording_hotkey(app: &tauri::AppHandle) -> Result<(), String> {
    let loaded = load_app_settings(app.clone())?;
    let config = ScreenRecordingHotkeyConfig {
        enabled: loaded.screen_recording_hotkey_enabled,
        shortcut: loaded.screen_recording_hotkey_shortcut,
    };
    let _ = apply_screen_recording_hotkey_config_internal(app, config)?;
    Ok(())
}

/// Route a backend error into the diagnostic log (Settings → Logs) so users
/// can see it — bare stderr is invisible in a production Windows app.
/// Best-effort: a logging failure is itself swallowed.
fn log_backend_error(app: &tauri::AppHandle, source: &str, message: String) {
    let _ = log_event(
        app.clone(),
        commands::error_logs::LogInput {
            level: "error".to_string(),
            source: source.to_string(),
            message,
            details: None,
        },
    );
}

/// Tell Windows where to find the bundled libvosk.dll (and its 3
/// MinGW runtime DLLs) for production builds.
///
/// Tauri places resources under `<install-dir>/resources/`, but the
/// default DLL search order doesn't recurse into subdirs — we need to
/// add it explicitly with `SetDllDirectoryW`. This is called once at
/// startup; the first vosk function call later in the program triggers
/// the actual `LoadLibrary("libvosk.dll")` which then walks the
/// extended search path and finds the file.
///
/// Best-effort: errors are logged but don't block startup. If this
/// fails the user gets a clear runtime error from vosk later instead
/// of a silent crash.
#[cfg(all(feature = "vosk", windows))]
fn register_vosk_dll_dir(app: &tauri::AppHandle) {
    use std::os::windows::ffi::OsStrExt;
    use tauri::Manager;
    use windows::core::PCWSTR;
    use windows::Win32::System::LibraryLoader::SetDllDirectoryW;

    let resource_dir = match app.path().resource_dir() {
        Ok(p) => p,
        Err(error) => {
            log_backend_error(
                app,
                "rust:vosk",
                format!("Could not resolve resource_dir for Vosk DLLs: {error}"),
            );
            return;
        }
    };

    // Convert to a wide-string for the Win32 API. Pre-allocate +1 for
    // the null terminator that PCWSTR semantics require.
    let mut wide: Vec<u16> = resource_dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: SetDllDirectoryW is a process-global call. The pointer
    // we hand it must outlive the call but not beyond it (Windows
    // copies the path internally). The Vec stays alive through the
    // unsafe block, satisfying that.
    let result = unsafe { SetDllDirectoryW(PCWSTR(wide.as_mut_ptr())) };
    if let Err(error) = result {
        log_backend_error(
            app,
            "rust:vosk",
            format!(
                "SetDllDirectoryW({:?}) failed: {error}. \
                 libvosk.dll may fail to load at first Vosk call.",
                resource_dir.display()
            ),
        );
    }
}

/// Two pre-built tray icons stored in app state — the normal idle
/// icon and a copy with a red "live mic" dot painted in the bottom-
/// right corner. We pre-build both at startup so swapping is a
/// constant-time clone, never a re-render.
///
/// Held inside an Arc so the typed state can be cloned cheaply (the
/// underlying buffers are a few KB each).
struct TrayIconSet {
    normal: tauri::image::Image<'static>,
    listening: tauri::image::Image<'static>,
}

/// Toggle the tray icon between normal and listening states.
///
/// Best-effort — failures are routed to the diagnostic log. The
/// tray icon swap is a UX nicety, not a correctness requirement; we
/// don't want a tray API error to prevent voice recognition from
/// running.
pub fn set_tray_listening(app: &tauri::AppHandle, listening: bool) {
    use tauri::Manager;

    let icons = match app.try_state::<TrayIconSet>() {
        Some(s) => s,
        None => return, // Setup hasn't run yet, or icon construction failed.
    };
    let icon = if listening {
        &icons.listening
    } else {
        &icons.normal
    };
    if let Some(tray) = app.tray_by_id("keepitlocal-tray") {
        if let Err(error) = tray.set_icon(Some(icon.clone())) {
            log_backend_error(
                app,
                "rust:tray",
                format!("Could not set tray icon: {error}"),
            );
        }
    }
}

/// Build a copy of the supplied icon with a red microphone-active
/// indicator painted into its bottom-right corner.
///
/// The indicator is a filled circle plus a thin lighter halo, so the
/// "mic is hot" cue reads clearly even at the 16-32px sizes Windows
/// renders the tray icon at. We use raw RGBA pixel manipulation
/// instead of a heavier image-processing pass because the icons are
/// tiny and we only do this once at startup.
fn build_listening_icon(base: &tauri::image::Image<'static>) -> tauri::image::Image<'static> {
    let width = base.width();
    let height = base.height();
    let mut buf: Vec<u8> = base.rgba().to_vec();

    // Indicator is a small circle in the bottom-right. ~18% of the
    // icon dimension feels right at 16-32px tray scale — visible at
    // a glance without dominating the icon. Earlier 32% covered most
    // of the bottom-right quadrant, which read as "the icon broke".
    let radius = ((width.min(height) as f32) * 0.18).max(3.0) as i32;
    // Inset by a small margin so the dot doesn't kiss the icon edge
    // (looks tidier and survives scaling to other tray sizes).
    let inset = (radius / 3).max(1);
    let cx = width as i32 - radius - inset;
    let cy = height as i32 - radius - inset;
    // Halo is just a hairline outside the core — enough to lift the
    // dot off dark icon backgrounds without making it look bloated.
    let halo = radius + 1;

    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let dx = x - cx;
            let dy = y - cy;
            let dist_sq = dx * dx + dy * dy;
            let i = ((y as usize) * width as usize + x as usize) * 4;
            if i + 3 >= buf.len() {
                continue;
            }
            if dist_sq <= radius * radius {
                // Solid bright red core.
                buf[i] = 232;
                buf[i + 1] = 60;
                buf[i + 2] = 60;
                buf[i + 3] = 255;
            } else if dist_sq <= halo * halo {
                // Soft halo to lift the dot off dark icon backgrounds.
                buf[i] = 232;
                buf[i + 1] = 60;
                buf[i + 2] = 60;
                buf[i + 3] = 200;
            }
        }
    }

    tauri::image::Image::new_owned(buf, width, height)
}

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let open_main = MenuItem::with_id(
        app,
        TRAY_OPEN_MAIN_ID,
        "Open KeepItLocal",
        true,
        None::<&str>,
    )?;
    let open_overlay = MenuItem::with_id(
        app,
        TRAY_OPEN_OVERLAY_ID,
        "Quick Search",
        true,
        None::<&str>,
    )?;
    let open_clipboard = MenuItem::with_id(
        app,
        TRAY_OPEN_CLIPBOARD_ID,
        "Clipboard History",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, TRAY_QUIT_ID, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_main, &open_overlay, &open_clipboard, &quit])?;

    let mut builder = TrayIconBuilder::with_id("keepitlocal-tray")
        .tooltip("KeepItLocal")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } => {
                let _ = show_main_window(tray.app_handle());
            }
            _ => {}
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            TRAY_OPEN_MAIN_ID => {
                let _ = show_main_window(app.app_handle());
            }
            TRAY_OPEN_OVERLAY_ID => {
                let _ = show_overlay_window(app.app_handle());
            }
            TRAY_OPEN_CLIPBOARD_ID => {
                let _ = show_clipboard_overlay_window(app.app_handle());
            }
            TRAY_QUIT_ID => {
                app.exit(0);
            }
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());

        // Pre-build the listening-state icon (red dot overlay) and
        // store both icons in app state so set_tray_listening() is a
        // cheap clone+swap. The original icon's `Image` is borrowed
        // from the bundled resources; we convert to the owned form so
        // we can hand out 'static clones without lifetime gymnastics.
        let normal_owned =
            tauri::image::Image::new_owned(icon.rgba().to_vec(), icon.width(), icon.height());
        let listening = build_listening_icon(&normal_owned);
        app.manage(TrayIconSet {
            normal: normal_owned,
            listening,
        });
    }

    let _tray = builder.build(app)?;
    Ok(())
}

fn load_startup_overlay_hotkey(_app: &tauri::AppHandle) -> Result<(), String> {
    // The standalone search overlay is retired — its Ctrl+Alt+S hotkey is no
    // longer registered. The command palette (Ctrl+Alt+K) is the single search
    // surface. Kept as a no-op so the startup call site stays unchanged and the
    // overlay code remains available if we ever re-enable it.
    Ok(())
}

fn ensure_single_instance(app: &tauri::App) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Cannot resolve app data directory: {error}"))?;
    fs::create_dir_all(&data_dir)
        .map_err(|error| format!("Cannot create app data directory: {error}"))?;

    let lock_path = data_dir.join("keepitlocal.lock");
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| format!("Cannot open single-instance lock: {error}"))?;

    match lock_file.try_lock_exclusive() {
        Ok(()) => {
            let mut guard = SINGLE_INSTANCE_LOCK
                .lock()
                .map_err(|_| "Single-instance lock storage failed".to_string())?;
            *guard = Some(lock_file);
            Ok(())
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::WouldBlock
                || error.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            // A reminder's scheduled task relaunched us while an instance is
            // already running. That instance fires the reminder on its own
            // heartbeat, so this relaunch must exit SILENTLY — no "already
            // running" dialog at every reminder time.
            if std::env::args().any(|arg| arg == "--reminder") {
                std::process::exit(0);
            }
            show_system_message(
                "KeepItLocal is already running",
                "KeepItLocal is already running in the background. Use the tray icon to reopen it.",
            );
            std::process::exit(0);
        }
        Err(error) => Err(format!("Cannot lock single-instance file: {error}")),
    }
}

#[cfg(windows)]
fn show_system_message(title: &str, message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    const MB_OK: u32 = 0x00000000;
    const MB_ICONINFORMATION: u32 = 0x00000040;

    unsafe extern "system" {
        fn MessageBoxW(
            hwnd: *mut std::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            kind: u32,
        ) -> i32;
    }

    let text: Vec<u16> = OsStr::new(message).encode_wide().chain(Some(0)).collect();
    let caption: Vec<u16> = OsStr::new(title).encode_wide().chain(Some(0)).collect();

    unsafe {
        let _ = MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

#[cfg(not(windows))]
fn show_system_message(title: &str, message: &str) {
    eprintln!("{title}: {message}");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    std::env::set_var(
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "--disable-background-networking --disable-component-update --process-per-site",
    );
    //--enable-low-end-device-mode

    let busy_flag = Arc::new(AtomicBool::new(false));

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    handle_global_hotkey_event(app, shortcut, event);
                })
                .build(),
        )
        .manage(CancelFlag(Arc::new(AtomicBool::new(false))))
        .manage(BusyFlag(busy_flag))
        .manage(Mutex::new(SetupState {
            frontend_ready: false,
            backend_ready: false,
        }))
        .manage(BackendInitIssues(Arc::new(Mutex::new(Vec::new()))))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--silent"]),
        ))
        // Off-main-thread media streaming for the palette's audio/video preview.
        // The built-in `asset:` protocol reads files on the UI thread (whole
        // file for a range-less GET), so huge movies froze every window. This
        // serves the same bytes from a worker thread, bounded per request.
        .register_asynchronous_uri_scheme_protocol(media_protocol::SCHEME, media_protocol::handle)
        .invoke_handler(tauri::generate_handler![
            commands::ocr::ocr_available,
            commands::ocr::ocr_languages,
            commands::ocr::ocr_image,
            commands::ocr::cancel_ocr_operation,
            compute_hashes,
            cancel_hash_operation,
            encode_decode,
            load_app_settings,
            save_app_settings,
            load_enabled_tool_packs,
            save_enabled_tool_packs,
            load_onboarding_state,
            save_onboarding_state,
            load_profiles_state,
            save_profiles_state,
            get_app_storage_paths,
            get_storage_insights,
            reset_all_data,
            export_app_data,
            import_app_data,
            get_backend_init_issues,
            take_db_corruption_notice,
            generate_qr,
            save_qr_png,
            save_qr_svg,
            convert_format,
            strip_metadata,
            shred_files,
            wipe_free_space,
            cancel_operation,
            run_clipboard_action,
            record_activity,
            list_activity,
            clear_activity,
            time_tracker_get_config,
            time_tracker_set_config,
            time_tracker_pause,
            time_tracker_status,
            time_tracker_range,
            time_tracker_wipe,
            ssh_list_keys,
            ssh_generate_key,
            ssh_import_key,
            ssh_delete_key,
            ssh_read_config,
            ssh_write_config,
            ssh_read_known_hosts,
            ssh_remove_known_host,
            ssh_dir_path,
            crypto_encrypt_file,
            crypto_decrypt_file,
            crypto_encrypt_text,
            crypto_decrypt_text,
            crypto_inspect_file,
            cancel_crypto_operation,
            folder_diff,
            file_unified_diff,
            three_way_merge,
            diff_read_text,
            cancel_diff_operation,
            sync_folders,
            log_event,
            list_log_entries,
            clear_log_entries,
            get_log_folder,
            export_log_text,
            list_snippets,
            create_snippet,
            update_snippet,
            delete_snippet,
            record_snippet_use,
            preview_snippet_expansion,
            paste_snippet_text,
            set_snippet_autoexpand_enabled,
            sync_snippet_expand_data,
            secure_kv_get,
            secure_kv_set,
            // Notes — local .ki note storage (Documents/KeepItLocal Notes).
            get_notes_dir,
            copy_note_asset,
            get_note_attachment,
            open_notes_folder,
            list_notes,
            search_note_bodies,
            list_note_link_sources,
            search_browser,
            warm_browser_search,
            clear_browser_snapshot,
            list_note_templates,
            read_note_template,
            open_note_templates_folder,
            list_trashed_notes,
            restore_trashed_note,
            delete_trashed_note,
            empty_note_trash,
            read_note,
            write_note,
            list_note_revisions,
            read_note_revision,
            restore_note_revision,
            diff_note_revision,
            create_note,
            open_or_create_daily_note,
            delete_note,
            list_note_folders,
            create_note_folder,
            rename_note_folder,
            delete_note_folder,
            move_note,
            notes_export_styled_pdf,
            notes_export_markdown,
            notes_export_html,
            notes_export_docx,
            run_my_shell_command,
            cancel_my_shell_command,
            type_out_text,
            camscan_detect_corners,
            camscan_warp,
            screenrec_start,
            screenrec_stop,
            screenrec_status,
            screenrec_pause,
            screenrec_resume,
            screenrec_export_gif,
            screenrec_open_region_selector,
            screenrec_close_region_selector,
            screenrec_open_redact_selector,
            screenrec_close_redact_selector,
            screenrec_list_windows,
            screenrec_open_toolbar,
            screenrec_close_toolbar,
            ffmpeg_status,
            ffmpeg_set_path,
            media_extract_audio,
            media_compress_video,
            cancel_media_operation,
            redact_image,
            regex_from_examples,
            scan_text_for_findings,
            preview_file_findings,
            dismiss_finding,
            undismiss_finding,
            restore_allowlist,
            count_dismissed_findings,
            is_finding_dismissed,
            set_busy,
            start_file_search_index,
            start_content_search_index,
            start_filename_search_index,
            update_notes_index,
            read_file_preview,
            list_folder_children,
            read_docx_markdown,
            list_logical_drives,
            // Dual-pane File Manager.
            fm_list_dir,
            fm_dir_size,
            fm_path_suggestions,
            fm_copy,
            fm_move,
            fm_delete_to_recycle,
            fm_rename,
            fm_make_dir,
            fm_backup,
            fm_cancel,
            cancel_file_search_index_build,
            stop_file_search_index_watcher,
            get_file_search_status,
            save_file_search_index_options,
            save_file_search_rebuild_schedule,
            search_local_files,
            search_file_contents,
            search_launch_targets,
            refresh_launch_target_cache,
            launch_cached_target,
            ensure_launcher_icon,
            live_grep_in_folders,
            cancel_live_grep,
            evaluate_quick_query,
            execute_system_command,
            list_system_commands,
            list_processes,
            kill_process,
            launch_targets_running,
            list_windows,
            focus_window,
            toggle_window,
            apply_app_hotkeys_config,
            system_info,
            check_app_available,
            open_external_url,
            record_frecency_launch,
            get_frecency_boosts,
            get_recent_items,
            open_search_result_path,
            create_archive,
            extract_archive,
            inspect_archive,
            strip_doc_metadata,
            remove_docx_password,
            audit_mic_camera,
            audit_unencrypted_pii,
            audit_browser_extensions,
            audit_startup_programs,
            audit_scheduled_tasks,
            audit_outbound_connections,
            audit_hosts_file,
            audit_dev_secrets,
            privacy_get_acknowledged,
            privacy_set_acknowledged,
            open_privacy_setting,
            reveal_in_explorer,
            read_hardening_state,
            apply_hardening_tweak,
            revert_hardening_tweak,
            revert_all_hardening,
            export_hardening_backup,
            inspect_spreadsheet,
            preview_csv_cleanup,
            collect_csv_sources,
            collect_excel_sources,
            clean_csv_file,
            merge_csv_files,
            excel_to_csv,
            csv_to_excel,
            csv_to_json,
            split_csv_files,
            cancel_word_markdown_operation,
            word_to_markdown,
            word_to_text,
            cancel_word_text_operation,
            collect_word_docx_sources,
            format_sql,
            analyze_sql,
            create_reminder_task,
            delete_reminder_task,
            reconcile_reminder_tasks,
            create_cron_task,
            delete_cron_task,
            run_cron_task_now,
            list_cron_tasks,
            voice_check_availability,
            voice_recognize_once,
            voice_cancel_recognize,
            voice_start_continuous,
            voice_stop_continuous,
            voice_set_vad_enabled,
            voice_release_models,
            voice_list_downloadable_models,
            voice_send_keystroke,
            voice_mouse_click,
            voice_mouse_scroll,
            voice_mouse_move,
            voice_mouse_warp,
            voice_window_action,
            voice_get_foreground_app,
            get_foreground_window_info,
            voice_list_ui_elements,
            voice_get_ui_elements,
            voice_user_commands_path,
            voice_read_user_commands,
            voice_write_user_commands,
            voice_open_user_commands_file,
            voice_watch_user_commands,
            voice_get_command_overrides,
            voice_set_command_overrides,
            voice_download_model,
            voice_list_installed_models,
            convert_images,
            compress_images,
            resize_images,
            crop_images,
            remove_image_background,
            background_removal_model_status,
            download_background_removal_model,
            images_to_base64,
            generate_favicons,
            watermark_images,
            find_duplicate_files,
            preview_duplicate_file,
            move_duplicate_files,
            analyze_system_cleaner,
            cancel_cleaner_operation,
            clean_system_cache,
            preview_bulk_rename,
            apply_bulk_rename,
            get_file_recovery_state,
            undo_bulk_rename,
            undo_duplicate_move,
            cancel_duplicate_scan,
            cancel_image_operation,
            cancel_image_extra_operation,
            cancel_spreadsheet_operation,
            cancel_archive_operation,
            run_image_automation,
            get_automation_recipes,
            save_automation_recipes,
            get_automation_activity,
            add_automation_activity,
            clear_automation_activity,
            open_automation_data_folder,
            halcyon_reset,
            halcyon_probe,
            apply_overlay_hotkey_config,
            apply_clipboard_overlay_hotkey_config,
            apply_command_overlay_hotkey_config,
            apply_quick_note_hotkey_config,
            apply_screen_recording_hotkey_config,
            apply_voice_overlay_hotkey_config,
            set_push_to_talk_hotkey,
            suspend_global_shortcuts_for_capture,
            resume_global_shortcuts_after_capture,
            show_main_window_command,
            restart_keepitlocal_command,
            show_overlay_window_command,
            hide_overlay_window_command,
            resize_overlay_window_command,
            show_clipboard_overlay_window_command,
            hide_clipboard_overlay_window_command,
            show_command_window_command,
            hide_command_window_command,
            set_command_window_blur,
            show_quick_note,
            set_quick_note_empty,
            supports_window_corner_rounding,
            show_voice_overlay_window_command,
            hide_voice_overlay_window_command,
            show_mouse_grid_window_command,
            hide_mouse_grid_window_command,
            show_ui_elements_window_command,
            hide_ui_elements_window_command,
            show_focus_break_window_command,
            hide_focus_break_window_command,
            show_focus_glow_window_command,
            hide_focus_glow_window_command,
            show_notify_toast_window_command,
            hide_notify_toast_window_command,
            set_ready,
            welcome_finished,
            start_clipboard_listener,
            get_clipboard_history,
            take_clipboard_recovery_notice,
            clear_clipboard_history,
            pin_clipboard_entry,
            pin_clipboard_entries,
            label_clipboard_entry,
            delete_clipboard_entry,
            delete_clipboard_entries,
            copy_clipboard_entry_to_clipboard,
            paste_clipboard_entry,
            get_clipboard_exclusions,
            set_clipboard_exclusions,
            reset_clipboard_exclusions_to_defaults,
            get_clipboard_paused,
            get_clipboard_text,
            set_clipboard_paused,
            get_clipboard_retention_days,
            set_clipboard_retention_days,
            get_clipboard_images_enabled,
            set_clipboard_images_enabled,
            get_clipboard_image_retention_days,
            set_clipboard_image_retention_days
        ])
        .setup(|app| {
            ensure_single_instance(app).map_err(|error| std::io::Error::other(error))?;

            // User-selected FFmpeg lives in the encrypted local store. Restore
            // it before any recorder or media command can run.
            commands::ffmpeg::hydrate_override(app.handle());

            // Semantic search (beta): point the embedder at its bundled MiniLM
            // model dir (<resources>/embedding-runtime/, same resource path as the
            // Vosk/Tesseract runtimes). No-op in builds without the `semantic`
            // feature; a resolve failure just leaves semantic search unavailable,
            // never fails startup.
            if let Ok(resource_dir) = app.path().resource_dir() {
                commands::embedding::set_model_dir(resource_dir.join("embedding-runtime"));
            }

            // Browser search keeps short-lived COPIES of browser history DBs in
            // temp. A crash mid-session would otherwise leave a complete copy of
            // the user's browsing history sitting there indefinitely — this sweep
            // is the only mitigation for that, so it is NOT optional.
            commands::browser_search::sweep_orphan_snapshots();

            // The splash starts hidden (visible:false in tauri.conf.json) so a
            // reminder relaunch never even flashes it. On a normal launch we reveal
            // it here; a reminder relaunch stays invisible — the hidden main webview
            // still boots and fires the toast (set_ready + the safety fallback below
            // keep main hidden), so only the toast + glow appear.
            if is_reminder_launch() {
                if let Some(splash) = app.get_webview_window("splashscreen") {
                    let _ = splash.close();
                }
            } else if let Some(splash) = app.get_webview_window("splashscreen") {
                let _ = splash.show();
            }

            // Vosk runtime DLL discovery (production builds only).
            //
            // libvosk.dll + its 3 MinGW runtime DLLs ship as Tauri
            // resources, so they end up under <install>/resources/ next
            // to the exe. Windows' default DLL search order checks the
            // exe directory first but NOT subdirectories — we need to
            // append the resources path to the search list before any
            // vosk function gets called (the first call lazy-loads the
            // DLL). In dev mode the DLLs sit alongside keepitlocal_lib.dll
            // in target/debug, which is already on the search path, so
            // this whole block is a no-op there.
            #[cfg(all(feature = "vosk", windows))]
            register_vosk_dll_dir(app.handle());

            setup_tray(app)?;

            // Capture hotkey registration failures into the init-issues
            // list rather than silently discarding them. The frontend polls
            // this list on first mount and shows a dismissable banner for
            // each issue so the user learns why a shortcut doesn't work
            // rather than wondering in silence.
            {
                let issues = app.state::<BackendInitIssues>();
                macro_rules! try_hotkey {
                    ($result:expr, $label:literal) => {
                        if let Err(error) = $result {
                            if let Ok(mut guard) = issues.0.lock() {
                                guard.push(format!("{}: {}", $label, error));
                            }
                        }
                    };
                }
                try_hotkey!(
                    load_startup_overlay_hotkey(app.handle()),
                    "Search overlay hotkey"
                );
                try_hotkey!(
                    load_startup_clipboard_overlay_hotkey(app.handle()),
                    "Clipboard overlay hotkey"
                );
                try_hotkey!(
                    load_startup_command_overlay_hotkey(app.handle()),
                    "Command palette hotkey"
                );
                try_hotkey!(
                    load_startup_quick_note_hotkey(app.handle()),
                    "Sticky note hotkey"
                );
                // Screen-recording hotkey is not registered yet. The Media workspace
                // is visible, but Ctrl+Alt+R must not start a recording from
                // anywhere until that global start/stop flow is explicitly
                // supported. The loader, config, dispatcher branch and Settings UI
                // stay, so restoring it is this one call.
                // try_hotkey!(
                //     load_startup_screen_recording_hotkey(app.handle()),
                //     "Screen recording hotkey"
                // );
                try_hotkey!(
                    load_startup_voice_overlay_hotkey(app.handle()),
                    "Voice overlay hotkey"
                );
                try_hotkey!(
                    load_startup_push_to_talk_hotkey(app.handle()),
                    "Push-to-talk hotkey"
                );
                try_hotkey!(load_startup_app_hotkeys(app.handle()), "Per-app hotkeys");
            }
            start_file_search_scheduler(app.handle().clone());
            // Warm the search index readers off-thread so the global overlay's
            // first query does not pay the cold-open cost (Search #14).
            prewarm_search_engines(app.handle().clone());

            // Overlay windows (search, clipboard, voice) are created
            // LAZILY on first hotkey press, not pre-created at startup.
            //
            // The expected UX is "first press creates the window
            // (sometimes flashes invisibly), second press shows the
            // populated overlay" — the same wasted-first-press dance
            // every overlay does. That's an acceptable Tauri quirk.
            //
            // The "first press hangs the main app" failure is NOT
            // intrinsic to lazy creation — it's gated by how many
            // concurrent invokes the overlay's route fires inside its
            // own onMount during WebView2 cold init. Voice + Clipboard
            // routes fire ~2 invokes each and survive cold init fine.
            // The search overlay's mount must stay under the same
            // budget; the lift is done in `/overlay/+page.svelte` by
            // deferring non-critical prefetches a small amount past
            // mount. See the comment in that file's onMount.
            //
            // History: we DID briefly pre-create the search overlay
            // at startup to mask the cold-init deadlock, but pre-create
            // costs ~100 MB of resident RAM for a renderer process that
            // most users don't need at boot. The lighter fix (defer the
            // prefetches) keeps idle RAM at ~134 MB.
            //
            // EXCEPTION — the unified command palette IS pre-created here.
            // The user explicitly wants a 1-press summon (no wasted-first-
            // press dance), which requires a warm window. We accept the
            // ~80-100 MB resident cost for this one window as the stated
            // tradeoff: it's the new front door, expected to feel instant.
            // Built hidden; first hotkey press just shows the warm window.
            // Its route keeps a lean onMount (no heavy prefetch burst) so
            // building it during startup doesn't trip the cold-init budget.
            if let Err(error) = get_or_create_command_window(app.handle()) {
                log_backend_error(
                    app.handle(),
                    "rust:command-palette",
                    format!("Could not pre-create command palette window: {error}"),
                );
            }

            // Start the reliable Rust-side reminder heartbeat (replaces the JS
            // setInterval that Chromium throttled while minimized to the tray).
            start_reminder_tick(app.handle().clone());

            // Start the Time Tracker's background sampler (foreground window +
            // idle). Gated internally by its own enabled/pause config; all data
            // is local + DPAPI-encrypted. Non-fatal if it can't start.
            commands::time_tracker::start(app.handle().clone());

            // Wave 7.9 (2026-05-28): first-run welcome window.
            //
            // Welcome lives in its own Tauri window — born `maximized: true`
            // + `visible: false` — so the post-splash hand-off goes:
            //
            //   splash (420×280) → welcome (maximized, hidden until ready)
            //
            // without ever showing the small dark main-window frame the
            // user saw under the old in-main approach (Windows runs a
            // visible maximize-animation when you transition a hidden
            // window from non-maximized to maximized at show time, which
            // is why the JS-side maximize-before-set_ready fix in Wave
            // 7.8.4 didn't help). The welcome window is conditionally
            // created here so returning users (welcome_completed = true)
            // pay zero memory cost.
            {
                let onboarding_incomplete = load_onboarding_state(app.handle().clone())
                    .map(|state| !state.welcome_completed)
                    .unwrap_or(true);
                if onboarding_incomplete {
                    // Wave 7.9.1 (2026-05-28): set background_color at
                    // the OS-window level. Even though we wait for the
                    // /welcome route's first paint before calling show(),
                    // the underlying Win32 window is created earlier and
                    // its native background is what the user briefly sees
                    // in the split second between the maximized window
                    // appearing on screen and the webview committing its
                    // CSS-driven background. Without this, that frame is
                    // bright white — pure user-eye assault between the
                    // dark splash and the dark welcome. #0c0c0e matches
                    // splash.html exactly so the transition is invisible.
                    let welcome_builder = WebviewWindowBuilder::new(
                        app.handle(),
                        WELCOME_WINDOW_LABEL,
                        WebviewUrl::App("/welcome".into()),
                    )
                    .title("Welcome to KeepItLocal")
                    .maximized(true)
                    .visible(false)
                    .decorations(false)
                    .background_color(tauri::webview::Color(0x0c, 0x0c, 0x0e, 0xff));
                    if let Err(error) = welcome_builder.build() {
                        log_backend_error(
                            app.handle(),
                            "rust:welcome-window",
                            format!("Could not create welcome window: {error}"),
                        );
                    }
                }
            }

            // Static-splash → main handoff — SAFETY FALLBACK ONLY.
            // The splash window loads a plain static HTML page (instant paint,
            // no SvelteKit bundle to boot), so it has no JS to signal readiness.
            // The normal close is driven by the MAIN window itself: its +page
            // calls `set_ready` once its first meaningful content is painted, so
            // main is revealed exactly when it's ready (machine-independent — no
            // guessed delay that could flash a half-loaded main). This timer is
            // the backstop: if main never signals (e.g. a first-screen load
            // error), close the splash + show main anyway after a generous hold,
            // so the app can never get stuck on the splash. In the normal path
            // the splash is already closed and main already shown by the time
            // this fires, making both calls harmless no-ops.
            {
                let splash_app = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(2000));
                    if let Some(splash) = splash_app.get_webview_window("splashscreen") {
                        let _ = splash.close();
                    }
                    // Reminder relaunch: stay in the tray — reveal nothing.
                    if is_reminder_launch() {
                        return;
                    }
                    // Palette-only mode: reveal nothing here either (2026-07-23).
                    // THIS IS WHY "App mode" APPEARED TO DO NOTHING. The comment
                    // above assumes main "is already shown by the time this fires,
                    // making both calls harmless no-ops" — true in the default
                    // mode, and exactly inverted in palette-only, where `set_ready`
                    // deliberately never shows main. So 2s after every launch this
                    // backstop revealed the window the setting had just suppressed.
                    // `is_reminder_launch()` above was already guarded for the same
                    // reason; palette-only simply got missed when it was added.
                    let palette_only = load_app_settings(splash_app.clone())
                        .map(|cfg| cfg.app_mode == "palette-only")
                        .unwrap_or(false);
                    // Wave 7.9: on first-run welcome owns the post-splash
                    // window, so the safety fallback must reveal it (not
                    // main, which is hidden until welcome_finished swaps
                    // the windows). If welcome doesn't exist (returning
                    // user), fall back to the normal main-show path.
                    if let Some(welcome) = splash_app.get_webview_window(WELCOME_WINDOW_LABEL) {
                        // Welcome still shows in palette-only: a first-run user
                        // with no configured hotkey would otherwise face a blank
                        // desktop with no way in.
                        let _ = welcome.show();
                        let _ = welcome.set_focus();
                    } else if !palette_only {
                        if let Some(main) = splash_app.get_webview_window(MAIN_WINDOW_LABEL) {
                            let _ = main.show();
                            let _ = main.set_focus();
                        }
                    }
                });
            }

            // Start clipboard history capture in the background. Idempotent
            // — safe to call from setup. Failure here is non-fatal: clipboard
            // history will just be empty / unpopulated, but the rest of the
            // app still works.
            if let Err(error) = start_clipboard_listener(app.handle().clone()) {
                log_backend_error(
                    app.handle(),
                    "rust:clipboard",
                    format!("Could not start clipboard listener: {error}"),
                );
                // Also surface to the frontend init-issues banner so the
                // user knows clipboard history is non-functional.
                if let Ok(mut guard) = app.state::<BackendInitIssues>().0.lock() {
                    guard.push(format!("Clipboard listener: {error}"));
                }
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |app, event| {
            // On true app exit (tray Quit / app.exit()), terminate any running
            // index / extractor worker process trees so quitting never leaves
            // them orphaned and indexing in the background.
            if matches!(event, RunEvent::Exit) {
                commands::search::stop_all_index_workers();
                commands::archive::stop_all_archive_workers();
            }
            if let RunEvent::WindowEvent {
                label,
                event: WindowEvent::CloseRequested { api, .. },
                ..
            } = event
            {
                if label == MAIN_WINDOW_LABEL {
                    api.prevent_close();
                    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                        let _ = window.hide();
                    }
                    return;
                }

                // Wave 7.9 / 7.9.2: welcome owns the entire first-run
                // experience. Closing it via Alt+F4 / title-bar X before
                // the user clicks "Get started" exits the app — main
                // isn't visible yet, and we don't want to silently
                // strand the user with the app running in the tray with
                // no UI. The programmatic close inside welcome_finished
                // uses destroy() (NOT close()), so it bypasses this
                // CloseRequested handler entirely — only user-initiated
                // closes reach this branch.
                if label == WELCOME_WINDOW_LABEL {
                    app.exit(0);
                    return;
                }

                if label == OVERLAY_WINDOW_LABEL {
                    api.prevent_close();
                    let _ = hide_overlay_window(app.app_handle());
                }

                if label == CLIPBOARD_OVERLAY_WINDOW_LABEL {
                    api.prevent_close();
                    let _ = hide_clipboard_overlay_window(app.app_handle());
                }

                // Command palette: close-to-hide so the warm window
                // survives (we never want to destroy + re-create it,
                // that would reintroduce the cold first-press).
                if label == COMMAND_WINDOW_LABEL {
                    api.prevent_close();
                    let _ = hide_command_window(app.app_handle());
                }

                if label == VOICE_OVERLAY_WINDOW_LABEL {
                    api.prevent_close();
                    let _ = hide_voice_overlay_window(app.app_handle());
                }

                // Sticky notes are destroy-on-close; drop their emptiness-
                // registry entry so it doesn't linger after the window is gone.
                if label.starts_with(QUICK_NOTE_WINDOW_LABEL) {
                    if let Ok(mut map) = QUICK_NOTE_EMPTY.lock() {
                        map.remove(&label);
                    }
                }
            } else if let RunEvent::WindowEvent {
                label,
                event: WindowEvent::Focused(false),
                ..
            } = event
            {
                if label == OVERLAY_WINDOW_LABEL {
                    let _ = hide_overlay_window(app.app_handle());
                }
                if label == CLIPBOARD_OVERLAY_WINDOW_LABEL {
                    let _ = hide_clipboard_overlay_window(app.app_handle());
                }
                // Command palette hides on blur — clicking away dismisses
                // it, same as the other overlays (Spotlight behaviour).
                if label == COMMAND_WINDOW_LABEL {
                    let _ = hide_command_window(app.app_handle());
                }
                if label == VOICE_OVERLAY_WINDOW_LABEL {
                    let _ = hide_voice_overlay_window(app.app_handle());
                }
            }
        });
}
