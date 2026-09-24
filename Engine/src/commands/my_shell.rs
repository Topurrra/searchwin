//! Shell command runner for user-defined "My Commands" of type
//! `shell`. Wave 4.1 (2026-05-27), expanded Wave 4.1b (2026-05-27) with
//! the three pre-launch caveats:
//!
//!   1. **Subprocess cancel** — Wave 2.5b OCR pattern. We stash the
//!      spawned `Child` in `MY_SHELL_CHILDREN` keyed by `operation_id`
//!      so `cancel_my_shell_command` can `.kill()` it. Long-running
//!      commands (`npm test`, `cargo build`) abort cleanly instead of
//!      blocking the worker thread until the process exits.
//!
//!   2. **Shell kind picker** — `shell_kind` option dispatches to
//!      `powershell.exe` (default, Windows PowerShell 5.1),
//!      `pwsh.exe` (PowerShell 7+), or `cmd.exe`. Most user shell needs
//!      (git, npm, dir, ls, cargo, curl, etc.) work fine in PowerShell;
//!      cmd is offered for `for /f` / native `.bat` quirks and pwsh for
//!      users who've installed Core.
//!
//!   3. **Output streaming** — Wave 3.3 live-grep event pattern.
//!      When `operation_id` is supplied, stdout + stderr are read on
//!      background threads and each line is emitted as a
//!      `my-shell-output-<opId>` Tauri event. The palette renders a
//!      streaming output panel so a long `git log` / `npm test` run is
//!      visible as it happens. The final summary still carries a
//!      ~200-char preview for the legacy toast path.
//!
//! Trust model is unchanged: the user authored the command in
//! Settings → My Commands; running it has the same trust posture as
//! installing any Windows app. CREATE_NO_WINDOW suppresses the console
//! flash on all three shells.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ── Cancel + child registries (Wave 4.1b-1, 2026-05-27) ───────────────
//
// Shape mirrors `ocr.rs`: a HashMap of in-flight Child handles keyed by
// operation_id (for `.kill()`) and a HashSet of cancelled ids so a
// post-kill `wait()` result can be distinguished from a real failure.

static MY_SHELL_CHILDREN: LazyLock<Mutex<HashMap<String, Child>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static CANCELLED_MY_SHELL: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Mark an in-flight shell command for cancellation and best-effort
/// kill the underlying subprocess. Safe to call with an unknown
/// `operation_id` (no-op).
#[tauri::command]
pub fn cancel_my_shell_command(operation_id: String) -> Result<(), String> {
    CANCELLED_MY_SHELL
        .lock()
        .map_err(|_| "Cancel registry lock failed".to_string())?
        .insert(operation_id.clone());
    if let Ok(mut map) = MY_SHELL_CHILDREN.lock() {
        if let Some(mut child) = map.remove(&operation_id) {
            let _ = child.kill();
        }
    }
    Ok(())
}

fn is_cancelled(op_id: &str) -> bool {
    CANCELLED_MY_SHELL
        .lock()
        .ok()
        .map(|s| s.contains(op_id))
        .unwrap_or(false)
}

fn clear_cancel(op_id: &str) {
    if let Ok(mut s) = CANCELLED_MY_SHELL.lock() {
        s.remove(op_id);
    }
    if let Ok(mut m) = MY_SHELL_CHILDREN.lock() {
        m.remove(op_id);
    }
}

// ── Options + result shapes ───────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMyShellOptions {
    /// Shell payload — exactly what the user typed in Settings →
    /// My Commands → Shell command. Interpretation depends on
    /// `shell_kind` (PowerShell `-Command`, `cmd /C`, or `pwsh
    /// -Command`).
    pub command: String,
    /// Optional working directory. Defaults to the worker process's
    /// cwd when omitted. The frontend doesn't currently expose a cwd
    /// picker — reserved for future polish.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Which shell to invoke (Wave 4.1b-2). Recognized values:
    ///   - "powershell" (default) — `powershell.exe`, Windows
    ///     PowerShell 5.1, ships with every modern Windows.
    ///   - "pwsh"                 — `pwsh.exe`, PowerShell 7+,
    ///     only when the user has installed Core.
    ///   - "cmd"                  — `cmd.exe`, the legacy command
    ///     interpreter, for `.bat`-style quirks.
    /// Unknown values fall back to "powershell".
    #[serde(default)]
    pub shell_kind: Option<String>,
    /// Per-invocation id (Wave 4.1b-1 + 4.1b-3). When supplied:
    ///   - stdout/stderr lines are streamed via
    ///     `my-shell-output-<opId>` Tauri events
    ///   - `cancel_my_shell_command(opId)` can kill the subprocess
    /// When omitted, the legacy non-streaming path runs (single
    /// blocking `output()` → 200-char preview, no cancel handle). The
    /// frontend always supplies one in production; the None branch is
    /// kept for headless / non-interactive callers and tests.
    #[serde(default)]
    pub operation_id: Option<String>,
}

/// One streamed output line, emitted as the payload of a
/// `my-shell-output-<opId>` Tauri event.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MyShellLine {
    /// "stdout" or "stderr" — the frontend colors stderr lines red.
    pub stream: String,
    /// One line of output, trailing newline stripped.
    pub line: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMyShellResult {
    /// First ~200 chars of captured stdout (newline-joined), trimmed
    /// of trailing whitespace, suitable for a success toast. `None`
    /// when stdout was empty.
    pub output_preview: Option<String>,
    /// True if stderr produced any output. The frontend uses this to
    /// color the toast / panel banner amber even on exit code 0
    /// (common for tools that log warnings to stderr).
    pub had_stderr: bool,
    /// Process exit code. `None` when the process was killed (cancel)
    /// or its exit status couldn't be retrieved.
    pub exit_code: Option<i32>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// True if `cancel_my_shell_command` was called for this op_id
    /// (the user clicked cancel; the process was killed). Distinct
    /// from `exit_code == None` because a process can also exit with
    /// no code for other reasons (signal on non-Windows).
    pub cancelled: bool,
    /// Total stdout + stderr lines emitted. Drives the panel header
    /// ("12 lines · 230 ms").
    pub line_count: usize,
}

// ── The command ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn run_my_shell_command(
    app: AppHandle,
    options: RunMyShellOptions,
) -> Result<RunMyShellResult, String> {
    let app_for_worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let trimmed = options.command.trim();
        if trimmed.is_empty() {
            return Err("Shell command is empty.".to_string());
        }
        let kind = options
            .shell_kind
            .as_deref()
            .unwrap_or("powershell")
            .trim();

        let mut cmd = build_shell_command(kind, trimmed)?;
        if let Some(cwd) = options.cwd.as_deref().filter(|s| !s.trim().is_empty()) {
            cmd.current_dir(cwd);
        }

        let start = Instant::now();
        match options.operation_id.clone() {
            Some(op_id) => run_streaming(app_for_worker, cmd, op_id, start),
            None => run_blocking(cmd, start),
        }
    })
    .await
    .map_err(|e| format!("Shell-command worker failed: {e}"))?
}

/// Build a `Command` for the requested shell. Centralized so every
/// caller goes through the same set of safe flags + no-window flag.
fn build_shell_command(kind: &str, payload: &str) -> Result<Command, String> {
    #[cfg(windows)]
    {
        let mut cmd = match kind {
            "cmd" => {
                let mut c = Command::new("cmd.exe");
                // /D = skip AutoRun (don't run user's HKCU\…\AutoRun
                //      script, which would change the working env).
                // /S = preserve quotes around the /C argument verbatim
                //      (the documented "complex command line" recipe).
                // /C = run command then exit.
                c.args(["/D", "/S", "/C", payload]);
                c
            }
            "pwsh" => {
                let mut c = Command::new("pwsh.exe");
                c.args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    payload,
                ]);
                c
            }
            // "powershell" | "" | unknown → Windows PowerShell 5.1.
            // Default is the only one guaranteed installed on every
            // modern Windows; the user has to actively install pwsh.
            _ => {
                let mut c = Command::new("powershell.exe");
                c.args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    payload,
                ]);
                c
            }
        };
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // stdin = null so commands that probe stdin (read-host etc.)
            // don't hang the worker waiting for input that never comes.
            .stdin(Stdio::null());
        Ok(cmd)
    }
    #[cfg(not(windows))]
    {
        // POSIX fallback: sh -c. shell_kind is ignored — on non-Windows
        // the My Commands shell type isn't really meant to ship anyway,
        // but we keep this branch so the crate compiles for tests.
        let _ = kind;
        let mut c = Command::new("sh");
        c.arg("-c")
            .arg(payload)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        Ok(c)
    }
}

/// Blocking non-streaming path: `.output()` returns the whole captured
/// stdout/stderr at once. Used when the caller didn't supply an
/// operation_id (no cancel, no streaming events).
fn run_blocking(mut cmd: Command, start: Instant) -> Result<RunMyShellResult, String> {
    let output = cmd
        .output()
        .map_err(|e| format!("Could not run shell command: {e}"))?;
    let duration_ms = start.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        let detail = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else if !stdout.trim().is_empty() {
            stdout.trim().to_string()
        } else {
            format!("exit code {}", output.status.code().unwrap_or(-1))
        };
        return Err(detail);
    }

    let trimmed_out = stdout.trim_end_matches(['\n', '\r']);
    let output_preview = preview_from(trimmed_out);
    let line_count = trimmed_out.lines().count() + stderr.lines().count();

    Ok(RunMyShellResult {
        output_preview,
        had_stderr: !stderr.trim().is_empty(),
        exit_code: output.status.code(),
        duration_ms,
        cancelled: false,
        line_count,
    })
}

/// Streaming path: spawn the child, read stdout + stderr on background
/// threads emitting one Tauri event per line, then wait for the child
/// and assemble the final summary. The Child handle is stashed in
/// `MY_SHELL_CHILDREN` so `cancel_my_shell_command` can race in and
/// `.kill()` it.
fn run_streaming(
    app: AppHandle,
    mut cmd: Command,
    op_id: String,
    start: Instant,
) -> Result<RunMyShellResult, String> {
    // Defensive: clear any stale cancel flag from a previous run with
    // the same op_id (the frontend SHOULD generate fresh ids, but
    // belt-and-braces).
    if let Ok(mut s) = CANCELLED_MY_SHELL.lock() {
        s.remove(&op_id);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Could not run shell command: {e}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Stash the handle so cancel_my_shell_command can kill() it.
    if let Ok(mut m) = MY_SHELL_CHILDREN.lock() {
        m.insert(op_id.clone(), child);
    }

    let event_name = format!("my-shell-output-{}", op_id);
    let line_count = Arc::new(AtomicUsize::new(0));
    let had_stderr = Arc::new(AtomicBool::new(false));
    // Accumulated stdout preview: first ~200 chars (newline-joined)
    // for the toast. We don't keep all output in memory — large dumps
    // would blow up — we just maintain a small running prefix.
    let preview_buf = Arc::new(Mutex::new(String::new()));

    let stdout_handle = stdout.map(|out| {
        let app = app.clone();
        let event = event_name.clone();
        let line_count = line_count.clone();
        let preview_buf = preview_buf.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines().map_while(Result::ok) {
                line_count.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut p) = preview_buf.lock() {
                    if p.chars().count() < 200 {
                        if !p.is_empty() {
                            p.push('\n');
                        }
                        p.push_str(&line);
                    }
                }
                let _ = app.emit(
                    &event,
                    MyShellLine {
                        stream: "stdout".to_string(),
                        line,
                    },
                );
            }
        })
    });

    let stderr_handle = stderr.map(|err| {
        let app = app.clone();
        let event = event_name.clone();
        let line_count = line_count.clone();
        let had_stderr = had_stderr.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(err);
            for line in reader.lines().map_while(Result::ok) {
                line_count.fetch_add(1, Ordering::Relaxed);
                had_stderr.store(true, Ordering::Relaxed);
                let _ = app.emit(
                    &event,
                    MyShellLine {
                        stream: "stderr".to_string(),
                        line,
                    },
                );
            }
        })
    });

    // Wait for the child to exit. If `cancel_my_shell_command` already
    // removed it from the registry (and killed it), the take() returns
    // None and we skip wait — the reader threads will hit EOF on the
    // killed pipes and join cleanly below.
    let wait_status = {
        let taken = MY_SHELL_CHILDREN
            .lock()
            .ok()
            .and_then(|mut m| m.remove(&op_id));
        match taken {
            Some(mut c) => c.wait().ok(),
            None => None,
        }
    };

    // Drain reader threads before we report — otherwise we'd race and
    // miss the last few lines.
    if let Some(h) = stdout_handle {
        let _ = h.join();
    }
    if let Some(h) = stderr_handle {
        let _ = h.join();
    }

    let cancelled = is_cancelled(&op_id);
    clear_cancel(&op_id);

    let duration_ms = start.elapsed().as_millis() as u64;
    let line_count_v = line_count.load(Ordering::Relaxed);
    let had_stderr_v = had_stderr.load(Ordering::Relaxed);
    let preview_raw = preview_buf.lock().ok().map(|p| p.clone()).unwrap_or_default();
    let output_preview = preview_from(&preview_raw);
    let exit_code = wait_status.as_ref().and_then(|s| s.code());

    // Failure surfacing: a non-zero exit (when NOT cancelled) returns
    // Err so the existing toast path stays consistent with Wave 4.1.
    // The frontend's streaming panel already shows every line, so the
    // Err message just needs to be a short summary for the toast.
    if !cancelled {
        let exited_ok = wait_status.as_ref().map(|s| s.success()).unwrap_or(false);
        if !exited_ok {
            let detail = output_preview.clone().unwrap_or_else(|| {
                exit_code
                    .map(|c| format!("exit code {c}"))
                    .unwrap_or_else(|| "Failed".to_string())
            });
            return Err(detail);
        }
    }

    Ok(RunMyShellResult {
        output_preview,
        had_stderr: had_stderr_v,
        exit_code,
        duration_ms,
        cancelled,
        line_count: line_count_v,
    })
}

/// Trim + truncate to a toast-friendly preview. 200 chars is enough
/// for a short `git status -s` or a couple of lines; longer output
/// gets the `…` suffix.
fn preview_from(raw: &str) -> Option<String> {
    let trimmed = raw.trim_end_matches(['\n', '\r', ' ', '\t']);
    if trimmed.is_empty() {
        None
    } else if trimmed.chars().count() > 200 {
        let cut: String = trimmed.chars().take(200).collect();
        Some(format!("{cut}…"))
    } else {
        Some(trimmed.to_string())
    }
}
