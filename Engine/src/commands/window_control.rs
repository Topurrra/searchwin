//! Shared Win32 window-control core: enumerate the user's open windows,
//! focus one, or toggle it (focus ⇄ minimize).
//!
//! Two features sit on top of this module and nothing else:
//!   * the command palette's **Windows** chip (Enter focuses a window), and
//!   * **per-app hotkeys** (Settings → Shortcuts): a global chord that
//!     toggles an app's window, or launches the app when it has none.
//!
//! ## Where the enumeration lives
//! [`enumerate_alt_tab_windows`] is the single implementation of the
//! "alt-tab-eligible top-level window" filter set (visible, non-empty title,
//! un-owned, not a tool window, not our own PID). It was MOVED here verbatim
//! from `processes::enumerate_gui_windows` on 2026-07-29 — that function now
//! delegates to this one and drops the HWNDs, so "a running app" means exactly
//! the same thing in the process list, the launcher's running-dot, the palette's
//! window switcher, and per-app hotkeys. Duplicating the filters would let those
//! four surfaces drift apart.
//!
//! ## Foreground activation
//! A bare `SetForegroundWindow` is routinely refused by Win32's focus-stealing
//! prevention. We reuse `clipboard_history::focus_window_reliably`, which
//! already solved this for clipboard paste-back (AttachThreadInput + confirm
//! via `GetForegroundWindow` rather than trusting the return value).
//!
//! Privacy / privilege: everything here is local OS state read and written
//! through ordinary user-level Win32. No admin, no UAC, no network — the same
//! posture as alt-tabbing by hand.

use serde::Serialize;

/// One alt-tab-eligible top-level window.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    /// Native window handle. `isize` widened to `i64` so it survives the JSON
    /// bridge (real HWNDs are 32-bit values, far inside JS's safe range).
    pub hwnd: i64,
    /// The window's title bar text.
    pub title: String,
    /// PID of the process that owns the window.
    pub pid: u32,
    /// Friendly app name — the exe file stem, e.g. "chrome".
    pub app: String,
    /// Full exe path, lowercased, when resolvable.
    pub exe_path: Option<String>,
    /// Whether this was the foreground window at enumeration time.
    /// Best effort: it is a snapshot, and can be stale by the time the user
    /// acts on it. `toggle_window` re-reads the live foreground rather than
    /// trusting this.
    pub is_foreground: bool,
}

/// A window handle plus its owning PID, straight out of `EnumWindows`.
#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct RawWindow {
    pub hwnd: isize,
    pub pid: u32,
}

/// Enumerate every alt-tab-eligible top-level window (one entry per window,
/// so an app with N windows appears N times), excluding our own process.
///
/// The filter set is the shell's own alt-tab contract: visible, has a title,
/// has no owner (dialogs and tool palettes are not top-level apps), and is not
/// a `WS_EX_TOOLWINDOW`.
#[cfg(windows)]
pub(crate) fn enumerate_alt_tab_windows() -> Vec<RawWindow> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowLongW, GetWindowTextLengthW, GetWindowThreadProcessId,
        IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
    };

    let mut found: Vec<RawWindow> = Vec::new();
    let own_pid = std::process::id();

    // SAFETY: `lparam` is the `&mut Vec<RawWindow>` we pass below, valid for
    // the synchronous duration of EnumWindows. We only read window metadata
    // (visibility, ex-style, owner, title length, owning PID) — no pointers
    // are retained past this call.
    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let found = &mut *(lparam.0 as *mut Vec<RawWindow>);

        // Visible only.
        if !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }
        // Must have a non-empty title.
        if GetWindowTextLengthW(hwnd) == 0 {
            return TRUE;
        }
        // Owned windows (dialogs, tool palettes) aren't top-level apps.
        if GetWindow(hwnd, GW_OWNER).0 != 0 {
            return TRUE;
        }
        // Tool windows are excluded from the alt-tab list by the shell.
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
            return TRUE;
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 {
            found.push(RawWindow { hwnd: hwnd.0, pid });
        }
        TRUE
    }

    // SAFETY: EnumWindows synchronously invokes `enum_cb` for each top-level
    // window, passing the LPARAM through untouched. The pointer targets our
    // stack Vec, which outlives the call.
    unsafe {
        let _ = EnumWindows(
            Some(enum_cb),
            LPARAM(&mut found as *mut Vec<RawWindow> as isize),
        );
    }

    // Drop our own GUI windows — KeepItLocal must not appear in its own
    // switcher, and must never be the target of a per-app hotkey toggle.
    found.retain(|w| w.pid != own_pid);
    found
}

#[cfg(windows)]
fn window_title(hwnd: windows::Win32::Foundation::HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

    // SAFETY: both calls only read the window's title into a buffer we own.
    // The +1 is room for the NUL terminator GetWindowTextW always writes.
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let written = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..written as usize])
    }
}

/// Every open window the user could alt-tab to, newest-first by nothing in
/// particular — the frontend sorts. Foreground window is flagged.
#[cfg(windows)]
#[tauri::command(async)]
pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let raw = enumerate_alt_tab_windows();
    if raw.is_empty() {
        return Ok(Vec::new());
    }

    // Resolve exe paths once per PID (many windows share a PID).
    let pid_list: Vec<sysinfo::Pid> = raw
        .iter()
        .map(|w| sysinfo::Pid::from_u32(w.pid))
        .collect();
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&pid_list), true);

    // SAFETY: GetForegroundWindow takes no arguments and returns a handle.
    let foreground = unsafe { GetForegroundWindow() };

    Ok(raw
        .into_iter()
        .map(|w| {
            let proc = sys.process(sysinfo::Pid::from_u32(w.pid));
            let exe_path = proc
                .and_then(|p| p.exe())
                .map(|p| p.to_string_lossy().to_ascii_lowercase());
            let app = exe_path
                .as_deref()
                .and_then(|p| std::path::Path::new(p).file_stem())
                .map(|s| s.to_string_lossy().to_string())
                .or_else(|| {
                    proc.map(|p| {
                        let raw_name = p.name().to_string_lossy().to_string();
                        std::path::Path::new(&raw_name)
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or(raw_name)
                    })
                })
                .unwrap_or_else(|| format!("PID {}", w.pid));

            WindowInfo {
                hwnd: w.hwnd as i64,
                title: window_title(HWND(w.hwnd)),
                pid: w.pid,
                app,
                exe_path,
                is_foreground: w.hwnd == foreground.0,
            }
        })
        .collect())
}

/// What `toggle_window` should do, given whether the target is already the
/// foreground window. Split out as a pure function so the decision is
/// unit-testable without a desktop session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToggleAction {
    /// Already focused → get it out of the way.
    Minimize,
    /// Not focused (or minimized) → bring it up.
    Focus,
}

pub(crate) fn decide_toggle(is_foreground: bool) -> ToggleAction {
    if is_foreground {
        ToggleAction::Minimize
    } else {
        ToggleAction::Focus
    }
}

/// Restore `hwnd` if minimized, then bring it to the foreground.
///
/// Returns `Err` when the OS refused to hand over focus — the caller decides
/// whether that is worth surfacing. It is never fatal.
#[cfg(windows)]
pub(crate) fn focus_hwnd(hwnd: isize) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindow, ShowWindow, SW_RESTORE};

    let handle = HWND(hwnd);
    // SAFETY: read-only window-state queries on a caller-supplied handle;
    // an invalid handle simply reports false rather than faulting.
    if !unsafe { IsWindow(handle) }.as_bool() {
        return Err("That window is gone".to_string());
    }
    // SAFETY: ShowWindow only changes the window's show-state.
    unsafe {
        if IsIconic(handle).as_bool() {
            let _ = ShowWindow(handle, SW_RESTORE);
        }
    }
    // Reuse the hardened activation path already proven for clipboard
    // paste-back: a bare SetForegroundWindow is frequently refused, so this
    // attaches our input thread to the target's and confirms the result.
    if crate::commands::clipboard_history::focus_window_reliably(handle) {
        Ok(())
    } else {
        Err("Windows refused to switch focus to that window".to_string())
    }
}

#[cfg(windows)]
fn minimize_hwnd(hwnd: isize) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsWindow, ShowWindow, SW_MINIMIZE};

    let handle = HWND(hwnd);
    // SAFETY: liveness check then a show-state change; both are safe on any
    // handle value (an invalid one is a no-op returning false).
    unsafe {
        if !IsWindow(handle).as_bool() {
            return Err("That window is gone".to_string());
        }
        let _ = ShowWindow(handle, SW_MINIMIZE);
    }
    Ok(())
}

/// Restore + focus the given window.
#[cfg(windows)]
#[tauri::command(async)]
pub fn focus_window(hwnd: i64) -> Result<(), String> {
    focus_hwnd(hwnd as isize)
}

/// Focus the window, or minimize it if it is already the foreground window.
#[cfg(windows)]
#[tauri::command(async)]
pub fn toggle_window(hwnd: i64) -> Result<(), String> {
    toggle_hwnd(hwnd as isize)
}

#[cfg(windows)]
pub(crate) fn toggle_hwnd(hwnd: isize) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // Re-read the LIVE foreground rather than trusting a `WindowInfo`
    // snapshot the frontend may have been holding for minutes.
    // SAFETY: no arguments, returns a handle.
    let foreground = unsafe { GetForegroundWindow() }.0;
    match decide_toggle(foreground == hwnd) {
        ToggleAction::Minimize => minimize_hwnd(hwnd),
        ToggleAction::Focus => focus_hwnd(hwnd),
    }
}

/// Find the window belonging to a launcher target (an `.exe` path, a Start-Menu
/// `.lnk`, or a `shell:AppsFolder\<AUMID>` handle).
///
/// Matching rules are the SAME ones `processes::launch_targets_running` uses to
/// decide whether a launcher row is "Running" — reused, not re-derived, so a row
/// that shows the running dot is exactly a row whose hotkey will toggle rather
/// than launch:
///   * packaged (MSIX/Store) apps match on package name + publisher hash inside
///     the running process's `WindowsApps` path (there is no exe to compare),
///   * a `.lnk` is resolved to its target exe first, and skipped when it merely
///     opens a document/URL/folder *through* a host app,
///   * everything else matches on lowercased exe file stem.
///
/// Prefers a non-foreground window when several match, so a hotkey pressed
/// against a multi-window app cycles toward something you can't already see
/// rather than minimizing the one in front of you.
pub(crate) fn find_window_for_target(target: &str, windows: &[WindowInfo]) -> Option<i64> {
    let matches: Vec<&WindowInfo> = if let Some(aumid) = target.strip_prefix("shell:AppsFolder\\") {
        windows
            .iter()
            .filter(|w| {
                w.exe_path.as_deref().is_some_and(|p| {
                    crate::commands::processes::packaged_app_is_running(
                        aumid,
                        std::slice::from_ref(&p.to_ascii_lowercase()),
                    )
                })
            })
            .collect()
    } else {
        let (exe, args) = resolve_launch_target(target);
        let stem = std::path::Path::new(&exe)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())?;
        if !crate::commands::processes::target_is_app_itself(&stem, args.as_deref()) {
            return None;
        }
        windows
            .iter()
            .filter(|w| w.app.to_ascii_lowercase() == stem)
            .collect()
    };

    matches
        .iter()
        .find(|w| !w.is_foreground)
        .or(matches.first())
        .map(|w| w.hwnd)
}

/// Resolve a launcher target to `(exe_path, arguments)`. A `.lnk` is decoded;
/// anything else is already the exe and carries no arguments.
#[cfg(windows)]
fn resolve_launch_target(target: &str) -> (String, Option<String>) {
    crate::commands::launcher_icons::resolve_lnk_full(target)
        .unwrap_or_else(|| (target.to_string(), None))
}

#[cfg(not(windows))]
fn resolve_launch_target(target: &str) -> (String, Option<String>) {
    (target.to_string(), None)
}

/// The action behind a per-app hotkey: toggle the app's window if it has one,
/// otherwise launch it. Called from the global-shortcut dispatcher.
///
/// Launching reuses `search::launch_cached_target`, which enforces the
/// trusted-launcher-cache check — a per-app binding therefore cannot be used to
/// start an arbitrary executable, only something already in the app index.
#[cfg(windows)]
pub(crate) fn toggle_or_launch(app: &tauri::AppHandle, target: String) {
    let windows = list_windows().unwrap_or_default();
    if let Some(hwnd) = find_window_for_target(&target, &windows) {
        if let Err(error) = toggle_hwnd(hwnd as isize) {
            eprintln!("window_control: toggle failed for {target}: {error}");
        }
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) =
            crate::commands::search::launch_cached_target(handle, target.clone()).await
        {
            eprintln!("window_control: launch failed for {target}: {error}");
        }
    });
}

// ─── Non-Windows stubs (keep the crate cross-platform) ────────────────

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn list_windows() -> Result<Vec<WindowInfo>, String> {
    Ok(Vec::new())
}

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn focus_window(_hwnd: i64) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn toggle_window(_hwnd: i64) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn toggle_or_launch(_app: &tauri::AppHandle, _target: String) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(hwnd: i64, app: &str, exe: &str, foreground: bool) -> WindowInfo {
        WindowInfo {
            hwnd,
            title: format!("{app} — window"),
            pid: 100 + hwnd as u32,
            app: app.to_string(),
            exe_path: Some(exe.to_string()),
            is_foreground: foreground,
        }
    }

    #[test]
    fn toggle_minimizes_only_the_foreground_window() {
        assert_eq!(decide_toggle(true), ToggleAction::Minimize);
        assert_eq!(decide_toggle(false), ToggleAction::Focus);
    }

    #[test]
    fn matches_a_plain_exe_target_by_stem() {
        let windows = vec![
            win(1, "chrome", r"c:\program files\google\chrome\chrome.exe", false),
            win(2, "code", r"c:\users\x\code.exe", true),
        ];
        assert_eq!(
            find_window_for_target(r"C:\Program Files\Google\Chrome\chrome.exe", &windows),
            Some(1)
        );
        assert_eq!(find_window_for_target(r"D:\other\code.exe", &windows), Some(2));
        assert_eq!(find_window_for_target(r"C:\nope\slack.exe", &windows), None);
    }

    /// A hotkey against a multi-window app should reach a window you can't
    /// already see, instead of minimizing the one in front of you.
    #[test]
    fn prefers_a_background_window_over_the_foreground_one() {
        let windows = vec![
            win(1, "chrome", r"c:\chrome.exe", true),
            win(2, "chrome", r"c:\chrome.exe", false),
        ];
        assert_eq!(find_window_for_target(r"c:\chrome.exe", &windows), Some(2));
    }

    /// ...but with only one window, that window is the answer even when it is
    /// foreground — that is exactly the press that should minimize it.
    #[test]
    fn falls_back_to_the_foreground_window_when_it_is_the_only_one() {
        let windows = vec![win(7, "chrome", r"c:\chrome.exe", true)];
        assert_eq!(find_window_for_target(r"c:\chrome.exe", &windows), Some(7));
    }

    #[test]
    fn matches_a_packaged_app_by_aumid() {
        let windows = vec![
            win(
                3,
                "chatgpt",
                r"c:\program files\windowsapps\openai.codex_26.707.9981.0_x64__2p2nqsd0c76g0\app\chatgpt.exe",
                false,
            ),
            win(4, "explorer", r"c:\windows\explorer.exe", false),
        ];
        assert_eq!(
            find_window_for_target("shell:AppsFolder\\OpenAI.Codex_2p2nqsd0c76g0!App", &windows),
            Some(3)
        );
        assert_eq!(
            find_window_for_target(
                "shell:AppsFolder\\Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe!App",
                &windows
            ),
            None
        );
    }

    #[test]
    fn no_windows_means_no_match() {
        assert_eq!(find_window_for_target(r"c:\chrome.exe", &[]), None);
    }
}
