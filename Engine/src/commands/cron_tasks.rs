//! Cron tasks — register recurring jobs on Windows Task Scheduler from a cron
//! expression, so they fire even when KeepItLocal is fully closed and across
//! reboots, with zero background process of our own.
//!
//! The frontend translates a cron expression into a small set of native trigger
//! specs (see stores/cronEngine.ts `planSchedule`) — Windows Task Scheduler is
//! not cron, so only the cleanly-representable shapes reach here. This module
//! turns those specs into PowerShell `*-ScheduledTask` cmdlets, mirroring the
//! proven approach in `reminders.rs` (current-user context, no admin, locale-safe
//! `[datetime]` triggers). Tasks live under the `\KeepItLocal\Cron\` folder.

use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Task Scheduler folder that groups all of our cron tasks (kept separate from
/// the reminders folder so the two never collide on reconcile).
const TASK_PATH: &str = "\\KeepItLocal\\Cron\\";

/// One native Windows trigger, as produced by the frontend translator.
///
/// `rename_all` on an internally-tagged enum only renames the VARIANT names —
/// the per-variant `rename_all` is what maps the camelCase fields the frontend
/// sends (`everyMinutes`, `daysOfWeek`, `everyHours`) onto these snake_case
/// fields. Without it, deserialization fails with "missing field every_minutes".
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TriggerSpec {
    Startup,
    #[serde(rename_all = "camelCase")]
    Daily { hour: u32, minute: u32 },
    #[serde(rename_all = "camelCase")]
    Weekly { days_of_week: Vec<u32>, hour: u32, minute: u32 },
    #[serde(rename_all = "camelCase")]
    Minutely { every_minutes: u32 },
    #[serde(rename_all = "camelCase")]
    Hourly { every_hours: u32, minute: u32 },
}

/// A registered cron task, surfaced back to the UI list.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronTaskInfo {
    id: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    next_run: String,
    #[serde(default)]
    last_run: String,
    #[serde(default)]
    last_result: i64,
}

/// Task ids are app-generated (`cron-<base36>`). Validate before interpolating
/// into a PowerShell command — defense in depth.
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

fn dow_name(d: u32) -> Result<&'static str, String> {
    Ok(match d {
        0 => "Sunday",
        1 => "Monday",
        2 => "Tuesday",
        3 => "Wednesday",
        4 => "Thursday",
        5 => "Friday",
        6 => "Saturday",
        _ => return Err("Invalid day-of-week".to_string()),
    })
}

/// Build the PowerShell expression that evaluates to one trigger object.
fn ps_trigger(t: &TriggerSpec) -> Result<String, String> {
    Ok(match t {
        TriggerSpec::Startup => "New-ScheduledTaskTrigger -AtStartup".to_string(),
        TriggerSpec::Daily { hour, minute } => {
            if *hour > 23 || *minute > 59 {
                return Err("Invalid daily time".to_string());
            }
            format!("New-ScheduledTaskTrigger -Daily -At ([datetime]::Today.AddHours({hour}).AddMinutes({minute}))")
        }
        TriggerSpec::Weekly { days_of_week, hour, minute } => {
            if *hour > 23 || *minute > 59 {
                return Err("Invalid weekly time".to_string());
            }
            if days_of_week.is_empty() {
                return Err("Weekly trigger needs at least one day".to_string());
            }
            let mut names = Vec::new();
            for d in days_of_week {
                names.push(dow_name(*d)?);
            }
            let days = names.join(",");
            format!("New-ScheduledTaskTrigger -Weekly -DaysOfWeek {days} -At ([datetime]::Today.AddHours({hour}).AddMinutes({minute}))")
        }
        TriggerSpec::Minutely { every_minutes } => {
            if *every_minutes == 0 || *every_minutes > 1440 {
                return Err("Invalid minute interval".to_string());
            }
            format!(
                "New-ScheduledTaskTrigger -Once -At ([datetime]::Today) -RepetitionInterval (New-TimeSpan -Minutes {every_minutes}) -RepetitionDuration (New-TimeSpan -Days 3650)"
            )
        }
        TriggerSpec::Hourly { every_hours, minute } => {
            if *every_hours == 0 || *every_hours > 24 || *minute > 59 {
                return Err("Invalid hour interval".to_string());
            }
            format!(
                "New-ScheduledTaskTrigger -Once -At ([datetime]::Today.AddMinutes({minute})) -RepetitionInterval (New-TimeSpan -Hours {every_hours}) -RepetitionDuration (New-TimeSpan -Days 3650)"
            )
        }
    })
}

#[cfg(windows)]
fn run_powershell(script: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;

    // Feed the script over stdin (`-Command -`) instead of as a `-Command
    // "<script>"` argument. Our scripts legitimately contain double quotes
    // (e.g. an action like `cmd /c start "" "<path>"` for "open file/app"),
    // and Windows command-line arg quoting mangles those before PowerShell
    // ever parses them. stdin sidesteps all of it. With `-Command -`,
    // PowerShell reads the whole script to EOF, then executes — so closing
    // stdin before reading stdout can't deadlock.
    let mut child = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Failed to run PowerShell: {e}"))?;

    child
        .stdin
        .take()
        .ok_or_else(|| "Failed to open PowerShell stdin".to_string())?
        .write_all(script.as_bytes())
        .map_err(|e| format!("Failed to send script to PowerShell: {e}"))?;

    let output = child
        .wait_with_output()
        .map_err(|e| format!("PowerShell did not complete: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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
fn run_powershell(_script: &str) -> Result<String, String> {
    Err("Scheduled cron tasks are only supported on Windows".to_string())
}

/// Register (or replace) a cron task that runs `program` (with `args`) on the
/// given translated triggers.
#[tauri::command]
pub fn create_cron_task(
    id: String,
    program: String,
    args: String,
    working_dir: String,
    triggers: Vec<TriggerSpec>,
) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Invalid task id".to_string());
    }
    if program.trim().is_empty() {
        return Err("Choose a program or command to run".to_string());
    }
    if triggers.is_empty() {
        return Err("This schedule has no triggers".to_string());
    }

    let id_q = ps_quote(&id);
    let prog_q = ps_quote(program.trim());
    let workdir_clause = if working_dir.trim().is_empty() {
        String::new()
    } else {
        format!(" -WorkingDirectory '{}'", ps_quote(working_dir.trim()))
    };
    let action = if args.trim().is_empty() {
        format!("New-ScheduledTaskAction -Execute '{prog_q}'{workdir_clause}")
    } else {
        format!(
            "New-ScheduledTaskAction -Execute '{prog_q}' -Argument '{}'{workdir_clause}",
            ps_quote(args.trim())
        )
    };

    let mut trig_exprs = Vec::new();
    for t in &triggers {
        trig_exprs.push(format!("({})", ps_trigger(t)?));
    }
    let trig_array = trig_exprs.join(",");

    let script = format!(
        "$ErrorActionPreference='Stop';\
         $a={action};\
         $t=@({trig_array});\
         $s=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable;\
         Register-ScheduledTask -TaskName '{id_q}' -TaskPath '{TASK_PATH}' -Action $a -Trigger $t -Settings $s -Force | Out-Null"
    );
    run_powershell(&script).map(|_| ())
}

/// Remove a cron task. Idempotent.
#[tauri::command]
pub fn delete_cron_task(id: String) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Invalid task id".to_string());
    }
    let id_q = ps_quote(&id);
    let script = format!(
        "Unregister-ScheduledTask -TaskName '{id_q}' -TaskPath '{TASK_PATH}' -Confirm:$false -ErrorAction SilentlyContinue"
    );
    run_powershell(&script).map(|_| ())
}

/// Trigger a cron task immediately (the "Run now" button).
#[tauri::command]
pub fn run_cron_task_now(id: String) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Invalid task id".to_string());
    }
    let id_q = ps_quote(&id);
    let script = format!("Start-ScheduledTask -TaskName '{id_q}' -TaskPath '{TASK_PATH}'");
    run_powershell(&script).map(|_| ())
}

/// List every cron task we registered, with next/last run and state.
#[tauri::command]
pub fn list_cron_tasks() -> Result<Vec<CronTaskInfo>, String> {
    let script = format!(
        "$ErrorActionPreference='SilentlyContinue';\
         $list=@(Get-ScheduledTask -TaskPath '{TASK_PATH}*' | ForEach-Object {{ \
           $i=$_ | Get-ScheduledTaskInfo; \
           [pscustomobject]@{{ \
             id=$_.TaskName; \
             state=[string]$_.State; \
             action=(($_.Actions | ForEach-Object {{ ($_.Execute + ' ' + $_.Arguments).Trim() }}) -join '; '); \
             nextRun=if($i.NextRunTime){{$i.NextRunTime.ToString('o')}}else{{''}}; \
             lastRun=if($i.LastRunTime){{$i.LastRunTime.ToString('o')}}else{{''}}; \
             lastResult=[int]$i.LastTaskResult \
           }} }});\
         if($list.Count -eq 0){{ '[]' }} else {{ $list | ConvertTo-Json -Compress -Depth 4 }}"
    );
    let out = run_powershell(&script)?;
    let trimmed = out.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(Vec::new());
    }
    let value: serde_json::Value =
        serde_json::from_str(trimmed).map_err(|e| format!("Failed to read task list: {e}"))?;
    let items = match value {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };
    let mut tasks = Vec::new();
    for item in items {
        if let Ok(t) = serde_json::from_value::<CronTaskInfo>(item) {
            tasks.push(t);
        }
    }
    Ok(tasks)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the frontend↔backend contract: the exact JSON shapes that
    /// cronEngine.ts `planSchedule` emits must deserialize into TriggerSpec.
    /// (A camelCase/snake_case mismatch here once broke "Create schedule" with
    /// `missing field every_minutes`.)
    #[test]
    fn deserializes_frontend_trigger_payloads() {
        let json = r#"[
            {"kind":"minutely","everyMinutes":1},
            {"kind":"hourly","everyHours":2,"minute":0},
            {"kind":"daily","hour":9,"minute":30},
            {"kind":"weekly","daysOfWeek":[1,2,3,4,5],"hour":9,"minute":0},
            {"kind":"startup"}
        ]"#;
        let parsed: Vec<TriggerSpec> =
            serde_json::from_str(json).expect("frontend trigger payload must deserialize");
        assert_eq!(parsed.len(), 5);
        for t in &parsed {
            assert!(
                ps_trigger(t).is_ok(),
                "every translated trigger should build a PowerShell expression"
            );
        }
    }
}
