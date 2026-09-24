//! Reminders — Windows Task Scheduler integration so reminders fire even when
//! KeepItLocal is fully closed.
//!
//! Each future reminder registers a one-time scheduled task under the
//! `\KeepItLocal\` task folder that relaunches the app (`--reminder <id>`) at
//! its due time. On launch the app's existing due-check shows the same toast +
//! glow notification — so the experience is identical whether the app was open
//! or closed. Tasks are removed when the reminder fires or is deleted, and a
//! reconcile pass on startup prunes any orphans.
//!
//! We drive Task Scheduler through PowerShell's `*-ScheduledTask` cmdlets rather
//! than `schtasks.exe`: PowerShell takes a real `[datetime]` trigger (locale
//! robust — no MM/DD/YYYY vs DD/MM/YYYY guessing) and converts the epoch from
//! the frontend with `[datetimeoffset]::FromUnixTimeMilliseconds`. Tasks run in
//! the current user's context (no admin, no stored password).

use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Task Scheduler folder that groups all of our reminder tasks.
const TASK_PATH: &str = "\\KeepItLocal\\";

/// Reminder ids are app-generated (`rem-<base36>-<base36>`). Validate before
/// interpolating into a PowerShell command — defense in depth, even though the
/// reminder text itself is never passed to the task.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Escape a string for a PowerShell single-quoted literal (only `'` is special).
fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(windows)]
fn run_powershell(script: &str) -> Result<(), String> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to run PowerShell: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            "Scheduled-task command failed".to_string()
        } else {
            err
        })
    }
}

#[cfg(not(windows))]
fn run_powershell(_script: &str) -> Result<(), String> {
    Err("Scheduled reminders are only supported on Windows".to_string())
}

/// Register (or replace) a one-time task that fires the reminder at `due_ms`.
#[tauri::command]
pub fn create_reminder_task(id: String, due_ms: i64) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Invalid reminder id".to_string());
    }
    if due_ms <= 0 {
        return Err("Invalid due time".to_string());
    }
    let exe = std::env::current_exe()
        .map_err(|e| format!("Cannot resolve executable path: {e}"))?
        .to_string_lossy()
        .to_string();
    let exe_q = ps_quote(&exe);
    let id_q = ps_quote(&id);
    let script = format!(
        "$ErrorActionPreference='Stop';\
         $at=[datetimeoffset]::FromUnixTimeMilliseconds({due_ms}).LocalDateTime;\
         $a=New-ScheduledTaskAction -Execute '{exe_q}' -Argument '--reminder {id_q}';\
         $t=New-ScheduledTaskTrigger -Once -At $at;\
         $s=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable;\
         Register-ScheduledTask -TaskName '{id_q}' -TaskPath '{TASK_PATH}' -Action $a -Trigger $t -Settings $s -Force | Out-Null"
    );
    run_powershell(&script)
}

/// Remove the scheduled task for a reminder (on fire or delete). Idempotent.
#[tauri::command]
pub fn delete_reminder_task(id: String) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Invalid reminder id".to_string());
    }
    let id_q = ps_quote(&id);
    let script = format!(
        "Unregister-ScheduledTask -TaskName '{id_q}' -TaskPath '{TASK_PATH}' -Confirm:$false -ErrorAction SilentlyContinue"
    );
    run_powershell(&script)
}

/// Prune any scheduled tasks in our folder that aren't in the active set —
/// cleans up reminders deleted or fired while the app was closed.
#[tauri::command]
pub fn reconcile_reminder_tasks(active_ids: Vec<String>) -> Result<(), String> {
    let keep = active_ids
        .iter()
        .filter(|id| valid_id(id))
        .map(|id| format!("'{}'", ps_quote(id)))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "$keep=@({keep});\
         Get-ScheduledTask -TaskPath '{TASK_PATH}*' -ErrorAction SilentlyContinue | \
         Where-Object {{ $keep -notcontains $_.TaskName }} | \
         Unregister-ScheduledTask -Confirm:$false -ErrorAction SilentlyContinue"
    );
    run_powershell(&script)
}
