//! Reminders are unavailable until Search can activate and display them.
//! Existing tasks owned by this engine world can still be deleted or reconciled.
//!
//! We drive Task Scheduler through PowerShell's `*-ScheduledTask` cmdlets rather
//! than `schtasks.exe`: PowerShell takes a real `[datetime]` trigger (locale
//! robust — no MM/DD/YYYY vs DD/MM/YYYY guessing) and converts the epoch from
//! the frontend with `[datetimeoffset]::FromUnixTimeMilliseconds`. Tasks run in
//! the current user's context (no admin, no stored password).

use std::process::Command;
use tauri::AppHandle;

use super::scheduler_scope::{SchedulerScope, TaskKind};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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

/// Search has no reminder activation path yet, so scheduling one would silently
/// launch an engine process that cannot display the reminder.
#[tauri::command]
pub fn create_reminder_task(app: AppHandle, id: String, due_ms: i64) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Invalid reminder id".to_string());
    }
    if due_ms <= 0 {
        return Err("Invalid due time".to_string());
    }
    let _scope = SchedulerScope::from_app(&app)?;
    Err(
        "Reminders are unavailable until Search can open and display scheduled reminders"
            .to_string(),
    )
}

/// Remove the scheduled task for a reminder (on fire or delete). Idempotent.
#[tauri::command]
pub fn delete_reminder_task(app: AppHandle, id: String) -> Result<(), String> {
    delete_reminder_task_using(app, id, run_powershell)
}

fn delete_reminder_task_using(
    app: AppHandle,
    id: String,
    run: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Invalid reminder id".to_string());
    }
    let id_q = ps_quote(&id);
    let task_path = ps_quote(&SchedulerScope::from_app(&app)?.task_path(TaskKind::Reminders));
    let script = format!(
        "Unregister-ScheduledTask -TaskName '{id_q}' -TaskPath '{task_path}' -Confirm:$false -ErrorAction SilentlyContinue"
    );
    run(&script)
}

/// Prune any scheduled tasks in our folder that aren't in the active set —
/// cleans up reminders deleted or fired while the app was closed.
#[tauri::command]
pub fn reconcile_reminder_tasks(app: AppHandle, active_ids: Vec<String>) -> Result<(), String> {
    reconcile_reminder_tasks_using(app, active_ids, run_powershell)
}

fn reconcile_reminder_tasks_using(
    app: AppHandle,
    active_ids: Vec<String>,
    run: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    if active_ids.iter().any(|id| !valid_id(id)) {
        return Err("Invalid reminder id".to_string());
    }
    let task_path = ps_quote(&SchedulerScope::from_app(&app)?.task_path(TaskKind::Reminders));
    let keep = active_ids
        .iter()
        .map(|id| format!("'{}'", ps_quote(id)))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "$keep=@({keep});\
         Get-ScheduledTask -TaskPath '{task_path}' -ErrorAction SilentlyContinue | \
         Where-Object {{ $_.TaskPath -eq '{task_path}' -and $keep -notcontains $_.TaskName }} | \
         Unregister-ScheduledTask -Confirm:$false -ErrorAction SilentlyContinue"
    );
    run(&script)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn reminder_creation_is_honestly_unavailable() {
        let fixture = std::env::temp_dir().join(format!(
            "search-reminder-unavailable-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&fixture).unwrap();
        let app = AppHandle::new(fixture.clone(), fixture.clone());
        let error = create_reminder_task(app, "rem-123".into(), 1_800_000_000_000).unwrap_err();
        assert!(error.contains("unavailable"));
        std::fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn missing_scope_prevents_reminder_deletion() {
        let app = AppHandle::new(std::path::PathBuf::new(), std::path::PathBuf::new());
        let error =
            delete_reminder_task_using(app, "rem-123".into(), |_| panic!("scheduler reached"))
                .unwrap_err();
        assert!(error.contains("scheduler scope"));
    }

    #[test]
    fn reconcile_removes_only_stale_tasks_in_its_exact_world_folder() {
        let fixture =
            std::env::temp_dir().join(format!("search-reminder-scope-{}", std::process::id()));
        let world = fixture.join("world");
        std::fs::create_dir_all(&world).unwrap();
        let app = AppHandle::new(world, fixture.clone());
        let path = SchedulerScope::from_app(&app)
            .unwrap()
            .task_path(TaskKind::Reminders);
        let other_path = format!("{}other\\", path);
        let fixture_script = r#"
$script:tasks = @(
    [pscustomobject]@{TaskName='keep';TaskPath='@OWNER@'},
    [pscustomobject]@{TaskName='stale';TaskPath='@OWNER@'},
    [pscustomobject]@{TaskName='child';TaskPath='@CHILD@'},
    [pscustomobject]@{TaskName='legacy';TaskPath='\KeepItLocal\'}
)
function Get-ScheduledTask { [CmdletBinding()] param([string]$TaskPath) $script:tasks }
function Unregister-ScheduledTask {
    [CmdletBinding(SupportsShouldProcess=$true)]
    param([Parameter(ValueFromPipeline=$true)]$InputObject,[string]$TaskName,[string]$TaskPath)
    process {
        $name = if ($InputObject) { $InputObject.TaskName } else { $TaskName }
        $path = if ($InputObject) { $InputObject.TaskPath } else { $TaskPath }
        $script:tasks = @($script:tasks | Where-Object { $_.TaskName -ne $name -or $_.TaskPath -ne $path })
    }
}
@SCRIPT@
@($script:tasks | ForEach-Object { $_.TaskName }) | ConvertTo-Json -Compress
"#;
        reconcile_reminder_tasks_using(app, vec!["keep".into()], |script| {
            let source = fixture_script
                .replace("@OWNER@", &path)
                .replace("@CHILD@", &other_path)
                .replace("@SCRIPT@", script);
            let script_path = fixture.join("fake-scheduler.ps1");
            std::fs::write(&script_path, &source).unwrap();
            let output = Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-File"])
                .arg(script_path)
                .output()
                .map_err(|e| e.to_string())?;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let remaining: Vec<String> =
                serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
                    panic!(
                        "{e}; stdout={}; stderr={}; source={source}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                });
            assert_eq!(remaining, ["keep", "child", "legacy"]);
            Ok(())
        })
        .unwrap();
        std::fs::remove_dir_all(fixture).unwrap();
    }
}
