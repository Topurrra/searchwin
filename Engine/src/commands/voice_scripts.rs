//! voice_scripts -- user-authored voice command files (#21).
//!
//! Voice upgrade, #21. Power users can define their own spoken
//! commands in a plain-text file -- the KeepItLocal equivalent of a
//! Talon command file. This module owns ONLY the file side: resolving
//! its path, reading it, creating a documented starter, and watching it
//! for changes so the frontend can hot-reload. The format itself is
//! parsed in TypeScript (`userVoiceCommands.ts`), where the command
//! registry lives -- the parser turns the text into extra
//! `VoiceCommandDefinition`s.
//!
//! Cross-platform: this is plain file IO + a `notify` watcher, so it is
//! NOT gated to Windows like the rest of the voice backend. Voice only
//! works on Windows today, but a command file resolving / reading on
//! any OS is harmless and keeps the command surface uniform.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use notify::{recommended_watcher, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, Manager};

/// The user command file's name -- a plain `.txt` so any text editor
/// opens it; the format is line-based plain text.
const USER_COMMANDS_FILENAME: &str = "voice-commands.txt";

/// Emitted when the user command file changes on disk -- the frontend
/// re-reads + re-parses it, and command mode restarts to pick up the
/// new grammar (hot-reload, #21).
const CHANGED_EVENT: &str = "voice-user-commands-changed";

/// The documented starter file, written on the first "open" so the
/// format is self-explanatory in whatever editor opens it.
const STARTER: &str = r##"# KeepItLocal voice commands
#
# Define your own spoken commands here. One command per line:
#
#     what you say = what it does
#
# Two kinds of action are available:
#
#     key <shortcut>    press a keyboard shortcut, e.g. key ctrl+s
#     url <address>     open a web page,           e.g. url https://example.com
#
# Examples -- remove the leading "# " to switch one on:
#
# save my work = key ctrl+s
# check my email = url https://mail.google.com
# run the build = key ctrl+shift+b
#
# Scope commands to one app with a section header. Commands below a
# header only work while that app's window is in front. [app: *] (or
# no header) makes the commands below it work everywhere.
#
# [app: chrome]
# new tab = key ctrl+t
# close tab = key ctrl+w
#
# [app: *]
# lock the screen = key win+l
#
# Notes:
#   - Lines starting with # are comments. Blank lines are ignored.
#   - Spoken phrases should be plain words -- no digits or punctuation.
#   - These commands join Command Mode and Push-to-Talk. They reload
#     automatically a moment after you save this file.
"##;

/// The `notify` watcher, kept alive for the process lifetime -- dropping
/// it stops the watch. `None` until `voice_watch_user_commands` starts it.
static WATCHER: Mutex<Option<RecommendedWatcher>> = Mutex::new(None);

/// Resolve `<app config dir>/voice-commands.txt`, creating the config
/// directory if needed so callers can watch / write the file.
fn user_commands_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Could not resolve the config directory: {e}"))?;
    fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create the config directory: {e}"))?;
    Ok(dir.join(USER_COMMANDS_FILENAME))
}

/// The absolute path of the user command file -- shown in Settings so
/// the user can find it even outside KeepItLocal.
#[tauri::command]
pub fn voice_user_commands_path(app: AppHandle) -> Result<String, String> {
    Ok(user_commands_path(&app)?.to_string_lossy().into_owned())
}

/// Read the user command file. An absent file is not an error -- the
/// user simply has no custom commands yet, so this returns "".
#[tauri::command]
pub fn voice_read_user_commands(app: AppHandle) -> Result<String, String> {
    let path = user_commands_path(&app)?;
    match fs::read_to_string(&path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("Could not read the voice command file: {e}")),
    }
}

/// Overwrite the user command file with `contents`. Used by the in-app
/// command editor (Settings → Voice). Creates the config directory if
/// needed. The on-disk watcher then fires `voice-user-commands-changed`,
/// so the command registry hot-reloads exactly as it does for an external
/// edit — no special-casing needed.
#[tauri::command]
pub fn voice_write_user_commands(app: AppHandle, contents: String) -> Result<(), String> {
    let path = user_commands_path(&app)?;
    fs::write(&path, contents).map_err(|e| format!("Could not save the voice command file: {e}"))
}

/// Open the user command file in the system's default editor, creating
/// it from the documented starter template first if it does not exist.
/// Returns the file path.
#[tauri::command]
pub fn voice_open_user_commands_file(app: AppHandle) -> Result<String, String> {
    let path = user_commands_path(&app)?;
    if !path.exists() {
        fs::write(&path, STARTER)
            .map_err(|e| format!("Could not create the voice command file: {e}"))?;
    }
    open::that(&path)
        .map_err(|e| format!("Could not open the voice command file: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// Start watching the user command file for changes. Idempotent -- the
/// watcher is process-global and started once. On a modify / create /
/// remove of the file it emits `voice-user-commands-changed`, which the
/// frontend turns into a hot-reload.
#[tauri::command]
pub fn voice_watch_user_commands(app: AppHandle) -> Result<(), String> {
    let mut slot = WATCHER
        .lock()
        .map_err(|_| "command-file watcher lock poisoned".to_string())?;
    if slot.is_some() {
        return Ok(());
    }
    let path = user_commands_path(&app)?;
    // notify cannot watch a file that does not exist yet -- watch the
    // parent directory and filter events down to our filename.
    let dir = path
        .parent()
        .ok_or_else(|| "config directory has no parent".to_string())?
        .to_path_buf();
    let emitter = app.clone();
    let mut watcher = recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if !matches!(
            event.kind,
            EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
        ) {
            return;
        }
        let touches_file = event.paths.iter().any(|p| {
            p.file_name()
                .map(|n| n == std::ffi::OsStr::new(USER_COMMANDS_FILENAME))
                .unwrap_or(false)
        });
        if touches_file {
            let _ = emitter.emit(CHANGED_EVENT, ());
        }
    })
    .map_err(|e| format!("Could not start the command-file watcher: {e}"))?;
    watcher
        .watch(&dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Could not watch the config directory: {e}"))?;
    *slot = Some(watcher);
    Ok(())
}

/// redb key holding the user's per-command phrase overrides for the
/// built-in voice commands — a JSON map `commandId -> { en: [...], ka: [...] }`.
const COMMAND_OVERRIDES_KEY: &str = "voice/command-overrides";

/// Read the built-in voice-command phrase overrides. Returns `{}` when the
/// user has never customized a built-in command. Stored as an opaque JSON
/// blob (the shape lives in TypeScript, like the user command file format).
#[tauri::command]
pub fn voice_get_command_overrides(app: AppHandle) -> Result<serde_json::Value, String> {
    let path = crate::commands::local_db::database_path_for_dir(
        &crate::commands::preferences::preferences_dir(&app)?,
    );
    crate::commands::local_db::read_json_or_default(
        &path,
        COMMAND_OVERRIDES_KEY,
        serde_json::json!({}),
    )
}

/// Persist the built-in voice-command phrase overrides (the in-app editor's
/// "edit / reset built-in commands"). DPAPI-encrypted at rest like every
/// other redb value, so it survives restarts.
#[tauri::command]
pub fn voice_set_command_overrides(
    app: AppHandle,
    overrides: serde_json::Value,
) -> Result<(), String> {
    let path = crate::commands::local_db::database_path_for_dir(
        &crate::commands::preferences::preferences_dir(&app)?,
    );
    crate::commands::local_db::write_json(&path, COMMAND_OVERRIDES_KEY, &overrides)
}
