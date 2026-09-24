//! What the engine asks the browser to do.
//!
//! In Workspace, these commands drove the app's own windows, tray and global
//! hotkeys. In Search the browser owns all of those, so each one here keeps its
//! name and arguments — the tool pages call them unchanged — and turns into a
//! `shell:request` event the browser acts on (or ignores). The few that were
//! about the engine itself (busy flag, init issues, restart) still do that.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub struct CancelFlag(pub Arc<AtomicBool>);
pub struct BusyFlag(pub Arc<AtomicBool>);

/// Non-fatal problems met while starting (a listener that couldn't start…),
/// kept until the browser asks.
pub struct BackendInitIssues(pub Arc<Mutex<Vec<String>>>);

/// Tell the browser to do something only it can.
pub fn ask(app: &AppHandle, request: &str, detail: Value) {
    let _ = app.emit("shell:request", json!({ "request": request, "detail": detail }));
}

/// Voice shows "listening" in Workspace's tray icon; in Search the browser
/// shows it wherever it likes.
pub fn set_tray_listening(app: &AppHandle, listening: bool) {
    ask(app, "listening", json!(listening));
}

/// The engine ends; the browser starts it again when it next needs it.
pub(crate) fn restart_keepitlocal_impl(app: &AppHandle) -> Result<(), String> {
    app.restart();
    Ok(())
}

// ─── About the engine itself ────────────────────────────────────────────

#[tauri::command]
pub fn get_backend_init_issues(issues: tauri::State<BackendInitIssues>) -> Vec<String> {
    let mut guard = issues.0.lock().unwrap_or_else(|p| p.into_inner());
    std::mem::take(&mut *guard)
}

#[tauri::command]
pub fn take_db_corruption_notice() -> bool {
    crate::commands::local_db::take_db_corruption_recovered()
}

#[tauri::command]
pub fn set_busy(flag: tauri::State<BusyFlag>, busy: bool) {
    flag.0.store(busy, Ordering::Relaxed);
}

#[tauri::command]
pub fn restart_keepitlocal_command(app: AppHandle) -> Result<(), String> {
    restart_keepitlocal_impl(&app)
}

/// Workspace's splash waited for this; the browser has no splash.
#[tauri::command]
pub fn set_ready(_task: String) {}

#[tauri::command(async)]
pub async fn welcome_finished(app: AppHandle) -> Result<(), String> {
    let _ = app.emit("kit-state-refresh", ());
    Ok(())
}

/// The acrylic palette is Workspace's; Search's field replaces it.
#[tauri::command]
pub fn supports_window_corner_rounding() -> bool {
    false
}

#[tauri::command(async)]
pub async fn set_command_window_blur(_enabled: bool) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub fn set_quick_note_empty(_label: String, _empty: bool) {}

// ─── Windows: the browser's ─────────────────────────────────────────────

macro_rules! window_requests {
    ($($name:ident => $request:literal),* $(,)?) => {
        $(
            #[tauri::command(async)]
            pub async fn $name(app: AppHandle) -> Result<(), String> {
                ask(&app, $request, Value::Null);
                Ok(())
            }
        )*
    };
}

window_requests!(
    show_main_window_command => "show-browser",
    show_overlay_window_command => "show-field",
    hide_overlay_window_command => "hide-field",
    show_clipboard_overlay_window_command => "show-clipboard",
    hide_clipboard_overlay_window_command => "hide-clipboard",
    show_command_window_command => "show-field",
    hide_command_window_command => "hide-field",
    show_voice_overlay_window_command => "show-voice",
    hide_voice_overlay_window_command => "hide-voice",
    show_mouse_grid_window_command => "show-mouse-grid",
    hide_mouse_grid_window_command => "hide-mouse-grid",
    show_ui_elements_window_command => "show-ui-elements",
    hide_ui_elements_window_command => "hide-ui-elements",
    show_quick_note => "show-quick-note",
    show_focus_break_window_command => "show-focus-break",
    hide_focus_break_window_command => "hide-focus-break",
    show_focus_glow_window_command => "show-focus-glow",
    hide_focus_glow_window_command => "hide-focus-glow",
    show_notify_toast_window_command => "show-toast",
    hide_notify_toast_window_command => "hide-toast",
);

#[tauri::command]
pub fn resize_overlay_window_command(app: AppHandle, height: f64) -> Result<(), String> {
    ask(&app, "resize-field", json!({ "height": height }));
    Ok(())
}

// ─── Global hotkeys: the browser's ──────────────────────────────────────

/// Screen capture pauses the hotkeys so they can't fire mid-take.
#[tauri::command]
pub fn suspend_global_shortcuts_for_capture(app: AppHandle) -> Result<(), String> {
    ask(&app, "suspend-hotkeys", Value::Null);
    Ok(())
}

#[tauri::command]
pub fn resume_global_shortcuts_after_capture(app: AppHandle) -> Result<(), String> {
    ask(&app, "resume-hotkeys", Value::Null);
    Ok(())
}

macro_rules! hotkey_configs {
    ($($config:ident { $($field:ident: $ty:ty),* } via $command:ident => $request:literal),* $(,)?) => {
        $(
            #[derive(Debug, Clone, Serialize, Deserialize)]
            #[serde(rename_all = "camelCase")]
            pub struct $config {
                $(pub $field: $ty),*
            }

            /// Registering the key is the browser's; the settings come back
            /// as given, the way the page expects.
            #[tauri::command]
            pub fn $command(app: AppHandle, config: $config) -> Result<$config, String> {
                ask(&app, $request, serde_json::to_value(&config).unwrap_or(Value::Null));
                Ok(config)
            }
        )*
    };
}

hotkey_configs!(
    OverlayHotkeyConfig { enabled: bool, mode: String, shortcut: String }
        via apply_overlay_hotkey_config => "hotkey:field",
    ClipboardOverlayHotkeyConfig { enabled: bool, shortcut: String }
        via apply_clipboard_overlay_hotkey_config => "hotkey:clipboard",
    CommandOverlayHotkeyConfig { enabled: bool, shortcut: String }
        via apply_command_overlay_hotkey_config => "hotkey:command",
    QuickNoteHotkeyConfig { enabled: bool, shortcut: String }
        via apply_quick_note_hotkey_config => "hotkey:quick-note",
    ScreenRecordingHotkeyConfig { enabled: bool, shortcut: String }
        via apply_screen_recording_hotkey_config => "hotkey:screen-recording",
    VoiceOverlayHotkeyConfig { enabled: bool, shortcut: String }
        via apply_voice_overlay_hotkey_config => "hotkey:voice",
    PushToTalkHotkeyConfig { enabled: bool, shortcut: String }
        via set_push_to_talk_hotkey => "hotkey:push-to-talk",
    AppHotkeysConfig { bindings: Vec<crate::commands::preferences::AppHotkeyBinding> }
        via apply_app_hotkeys_config => "hotkey:apps",
);
