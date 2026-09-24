//! Running-app enumeration + termination for the command-palette
//! "Commands" chip.
//!
//! [`list_processes`] returns one [`ProcessGroup`] per GUI application the
//! user can alt-tab to — not every PID in the system. We enumerate
//! alt-tab-eligible top-level windows (visible, titled, un-owned, not a
//! tool-window), map each to its owning PID, drop our own process, then
//! group the PIDs by app (exe path) so a multi-window app like a browser
//! shows up once with a summed memory figure and a window count.
//!
//! [`kill_process`] terminates the given PIDs. It is **destructive** and
//! mirrors the confirmed-gate in `quick_actions::execute_system_command`:
//! it refuses to do anything unless `confirmed == Some(true)`.
//!
//! Privacy: everything is local OS state read through Win32 + sysinfo —
//! no network, no telemetry. Same posture as opening Task Manager.

use serde::Serialize;

/// One running GUI application, aggregated across all its top-level windows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessGroup {
    /// Friendly app name (exe file stem, e.g. "chrome").
    pub name: String,
    /// Full path to the app's executable, when resolvable.
    pub exe_path: Option<String>,
    /// Every GUI PID belonging to this app.
    pub pids: Vec<u32>,
    /// Summed working-set memory across this app's PIDs, in bytes.
    pub memory_bytes: u64,
    /// Number of this app's alt-tab-eligible top-level windows.
    pub window_count: u32,
}

#[cfg(windows)]
#[tauri::command(async)]
pub fn list_processes() -> Result<Vec<ProcessGroup>, String> {
    use std::collections::HashMap;

    // 1. Collect (pid, hwnd) for every alt-tab-eligible top-level window,
    //    excluding our own process.
    let windows = enumerate_gui_windows();
    if windows.is_empty() {
        return Ok(Vec::new());
    }

    // 2. Per app, tally window count + the set of distinct PIDs. Keyed by
    //    PID first so we can resolve exe/memory once per PID via sysinfo.
    let mut window_count_by_pid: HashMap<u32, u32> = HashMap::new();
    for &pid in &windows {
        *window_count_by_pid.entry(pid).or_insert(0) += 1;
    }

    // 3. Refresh just the PIDs we care about (cheaper than new_all()).
    let pid_list: Vec<sysinfo::Pid> =
        window_count_by_pid.keys().map(|&p| sysinfo::Pid::from_u32(p)).collect();
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&pid_list), true);

    // 4. Resolve each PID's exe path + memory, then group by app.
    //    Grouping key = lowercased exe path, falling back to lowercased name.
    struct Acc {
        name: String,
        exe_path: Option<String>,
        pids: Vec<u32>,
        memory_bytes: u64,
        window_count: u32,
    }
    let mut groups: HashMap<String, Acc> = HashMap::new();

    for (&pid, &win_count) in &window_count_by_pid {
        let proc = sys.process(sysinfo::Pid::from_u32(pid));
        let exe_path: Option<String> = proc
            .and_then(|p| p.exe())
            .map(|p| p.to_string_lossy().to_string());
        let memory = proc.map(|p| p.memory()).unwrap_or(0);

        // Friendly name = the exe file stem when we have a path, else the
        // process name (also stem-ified), else a fallback.
        let name = exe_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .or_else(|| {
                proc.map(|p| {
                    let raw = p.name().to_string_lossy().to_string();
                    std::path::Path::new(&raw)
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or(raw)
                })
            })
            .unwrap_or_else(|| format!("PID {pid}"));

        let key = exe_path
            .as_deref()
            .map(|p| p.to_lowercase())
            .unwrap_or_else(|| name.to_lowercase());

        let entry = groups.entry(key).or_insert_with(|| Acc {
            name: name.clone(),
            exe_path: exe_path.clone(),
            pids: Vec::new(),
            memory_bytes: 0,
            window_count: 0,
        });
        if !entry.pids.contains(&pid) {
            entry.pids.push(pid);
        }
        entry.memory_bytes += memory;
        entry.window_count += win_count;
    }

    let mut out: Vec<ProcessGroup> = groups
        .into_values()
        .map(|a| ProcessGroup {
            name: a.name,
            exe_path: a.exe_path,
            pids: a.pids,
            memory_bytes: a.memory_bytes,
            window_count: a.window_count,
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

/// Enumerate the PIDs of every alt-tab-eligible top-level window — one entry
/// per qualifying window (so an app with N windows appears N times). Excludes
/// our own process.
///
/// 2026-07-29: the `EnumWindows` body that used to live here was MOVED VERBATIM
/// (not rewritten, not reimplemented) into
/// `window_control::enumerate_alt_tab_windows`, which returns the HWNDs too.
/// The palette's window switcher and per-app hotkeys need those handles; having
/// them re-declare the same visible / titled / un-owned / not-a-tool-window
/// filter set would let "a running app" mean different things in different
/// parts of the app. This function is now the PID-only projection of that one
/// shared enumeration, so every caller here behaves exactly as before.
#[cfg(windows)]
fn enumerate_gui_windows() -> Vec<u32> {
    super::window_control::enumerate_alt_tab_windows()
        .into_iter()
        .map(|w| w.pid)
        .collect()
}

/// Lowercased exe *file stems* of every app with an alt-tab-eligible window
/// right now (e.g. {"chrome", "code", "slack"}). The identity we can actually
/// match a launch target against — a running process is keyed by its exe, and
/// a stem (not a full path) survives the launcher storing a `.lnk` whose
/// resolved target sits in a different folder than where the process reports
/// its exe. Reuses `enumerate_gui_windows` so "running" means the same thing
/// here as in the process list (a real top-level window, not a background svc).
#[cfg(windows)]
fn running_gui_exe_paths() -> Vec<String> {
    let pids = enumerate_gui_windows();
    if pids.is_empty() {
        return Vec::new();
    }
    let pid_list: Vec<sysinfo::Pid> = pids.iter().map(|&p| sysinfo::Pid::from_u32(p)).collect();
    let mut sys = sysinfo::System::new();
    // No memory refresh needed — we only read exe paths (cheaper than list_processes).
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&pid_list), true);

    pids.iter()
        .filter_map(|&pid| {
            let proc = sys.process(sysinfo::Pid::from_u32(pid))?;
            let raw = proc
                .exe()
                .map(|p| p.to_path_buf())
                // Fall back to the reported process name when the exe path is
                // unreadable (some protected processes hide it).
                .or_else(|| Some(std::path::PathBuf::from(proc.name())))?;
            Some(raw.to_string_lossy().to_ascii_lowercase())
        })
        .collect()
}

#[cfg(windows)]
fn stems_of(paths: &[String]) -> std::collections::HashSet<String> {
    paths
        .iter()
        .filter_map(|p| {
            std::path::Path::new(p)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
        })
        .collect()
}

/// Split a package family name out of an AUMID into `(package_name, publisher_hash)`.
///
/// `OpenAI.Codex_2p2nqsd0c76g0!App` → `("openai.codex", "2p2nqsd0c76g0")`.
/// The family is everything before `!`; the publisher hash is the trailing
/// `_`-delimited segment.
fn parse_package_family(aumid: &str) -> Option<(String, String)> {
    let family = aumid.split('!').next()?;
    let (name, hash) = family.rsplit_once('_')?;
    if name.is_empty() || hash.is_empty() {
        return None;
    }
    Some((name.to_ascii_lowercase(), hash.to_ascii_lowercase()))
}

/// Whether a packaged app is running, given its AUMID and the running exe paths.
///
/// Packaged apps can't be matched by exe stem — the launcher knows them only as
/// an AUMID, and the running process reports a path under `WindowsApps` whose
/// folder is `<Name>_<Version>_<Arch>__<PublisherHash>` (e.g. ChatGPT runs as
/// `...\WindowsApps\OpenAI.Codex_26.707.9981.0_x64__2p2nqsd0c76g0\app\ChatGPT.exe`).
/// Both the package name and the publisher hash must appear, so two apps sharing
/// a name from different publishers can't be confused for one another.
pub(crate) fn packaged_app_is_running(aumid: &str, running_paths: &[String]) -> bool {
    let Some((name, hash)) = parse_package_family(aumid) else {
        return false;
    };
    let name_marker = format!("{name}_");
    let hash_marker = format!("__{hash}");
    running_paths.iter().any(|p| {
        p.contains("\\windowsapps\\") && p.contains(&name_marker) && p.contains(&hash_marker)
    })
}

/// Generic "host" executables that other things are launched *through*: file
/// managers, browsers, and script hosts. A shortcut whose target is one of
/// these AND that carries arguments is opening a folder / URL / script — it is
/// not the host app itself.
fn is_generic_host_exe(stem: &str) -> bool {
    matches!(
        stem,
        "explorer"
            | "chrome"
            | "msedge"
            | "firefox"
            | "brave"
            | "opera"
            | "vivaldi"
            | "chromium"
            | "iexplore"
            | "cmd"
            | "powershell"
            | "pwsh"
            | "rundll32"
            | "mshta"
            | "wscript"
            | "cscript"
    )
}

/// Whether a resolved launch target represents *the app itself* (so its running
/// state is meaningful), rather than a document/URL/folder opened through a host.
///
/// Observed false positives this kills, both from a real Start Menu:
///   * `Mono Radio` → `firefox.exe -taskbar-tab <guid>` — a web-app shortcut that
///     would claim "Running" whenever Firefox is open.
///   * `Windows Software Development Kit` → `explorer.exe "C:\...\Windows Kits\10\"`
///     — a folder shortcut that would claim "Running" *always*, since Explorer is.
///
/// Arguments alone are NOT the signal: `Task Manager` ships as `taskmgr.exe /7`
/// and is a legitimate match. It takes both a generic host AND arguments.
pub(crate) fn target_is_app_itself(stem: &str, args: Option<&str>) -> bool {
    match args {
        Some(a) if !a.trim().is_empty() => !is_generic_host_exe(stem),
        _ => true,
    }
}

/// Given launcher result paths (exe paths OR Start-Menu `.lnk` shortcuts),
/// return the subset that maps to a currently-running windowed app.
///
/// The launcher indexes `.lnk` shortcuts by display name (`Google Chrome.lnk`)
/// while processes are keyed by exe (`chrome.exe`), so a string compare can't
/// bridge them — we resolve each `.lnk` to its target exe first (reusing the
/// pure-Rust `resolve_lnk_full`), then match by lowercased file stem, skipping
/// shortcuts that merely open something through a host app.
///
/// Bounded by design: the palette calls this ONLY for the handful of app rows
/// it's about to show, so the per-path `.lnk` decode never runs over the whole
/// Start-Menu catalog.
#[cfg(windows)]
#[tauri::command(async)]
pub fn launch_targets_running(paths: Vec<String>) -> Result<Vec<String>, String> {
    let running_paths = running_gui_exe_paths();
    if running_paths.is_empty() {
        return Ok(Vec::new());
    }
    let running_stems = stems_of(&running_paths);
    let out = paths
        .into_iter()
        .filter(|path| {
            // Packaged (MSIX / Store) apps have no exe to match — they are known
            // only by AUMID, so they match on the package identity embedded in
            // the running process's WindowsApps path instead.
            if let Some(aumid) = path.strip_prefix("shell:AppsFolder\\") {
                return packaged_app_is_running(aumid, &running_paths);
            }
            // A `.lnk` resolves to its target exe (+ args); a real exe path is
            // used as-is and carries no arguments.
            let (target, args) = crate::commands::launcher_icons::resolve_lnk_full(path)
                .unwrap_or_else(|| (path.clone(), None));
            let Some(stem) = std::path::Path::new(&target)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
            else {
                return false;
            };
            target_is_app_itself(&stem, args.as_deref()) && running_stems.contains(&stem)
        })
        .collect();
    Ok(out)
}

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn launch_targets_running(_paths: Vec<String>) -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

#[cfg(windows)]
#[tauri::command(async)]
pub fn kill_process(pids: Vec<u32>, confirmed: Option<bool>) -> Result<Option<String>, String> {
    // Destructive — mirror the confirmed-gate in
    // quick_actions::execute_system_command. Without explicit confirmation a
    // compromised frontend could silently terminate the user's apps.
    if confirmed != Some(true) {
        return Err("Confirmation required".into());
    }

    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    for pid in pids {
        // Best-effort: a PID that's already gone (or that we can't open) is
        // skipped, never fatal. We never panic.
        // SAFETY: OpenProcess returns an owned handle we close below;
        // TerminateProcess takes that handle + an exit code and touches no
        // memory of ours. A failed open/terminate is just skipped.
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(handle, 1);
                let _ = CloseHandle(handle);
            }
        }
    }
    Ok(None)
}

// ─── Non-Windows stubs (keep the crate cross-platform) ────────────────

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn list_processes() -> Result<Vec<ProcessGroup>, String> {
    Ok(Vec::new())
}

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn kill_process(_pids: Vec<u32>, _confirmed: Option<bool>) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two false positives found by simulating the matcher against a real
    /// Start Menu (206 shortcuts) before this guard existed. Both resolve to a
    /// running host exe, so without the guard they claimed "Running" — the
    /// Explorer one on *every* launch, since Explorer is always up.
    #[test]
    fn host_exe_with_args_is_not_the_app_itself() {
        // "Mono Radio" -> firefox.exe -taskbar-tab <guid>
        assert!(!target_is_app_itself(
            "firefox",
            Some("\"-taskbar-tab\" \"17d61bf1-0d09-4379-86ca-8f5b08\"")
        ));
        // "Windows Software Development Kit" -> explorer.exe "C:\...\Windows Kits\10\"
        assert!(!target_is_app_itself(
            "explorer",
            Some("\"C:\\Program Files (x86)\\Windows Kits\\10\\\"")
        ));
    }

    /// Arguments alone must NOT disqualify a shortcut — Task Manager really does
    /// ship as `taskmgr.exe /7`, and it is a legitimate running match.
    #[test]
    fn non_host_exe_with_args_is_still_the_app() {
        assert!(target_is_app_itself("taskmgr", Some("/7")));
        assert!(target_is_app_itself("code", Some("--new-window")));
    }

    /// A host exe with NO arguments is the host app itself: a plain
    /// "File Explorer" or "Firefox" shortcut should still light up.
    #[test]
    fn host_exe_without_args_is_the_app() {
        assert!(target_is_app_itself("explorer", None));
        assert!(target_is_app_itself("firefox", None));
        assert!(target_is_app_itself("firefox", Some("   "))); // whitespace-only
    }

    /// Plain exe launch targets (no `.lnk`, so never any args) are unaffected.
    #[test]
    fn plain_exe_targets_pass_through() {
        assert!(target_is_app_itself("notepad++", None));
        assert!(target_is_app_itself("keepitlocal", None));
    }

    #[test]
    fn parses_package_family_from_aumid() {
        assert_eq!(
            parse_package_family("OpenAI.Codex_2p2nqsd0c76g0!App"),
            Some(("openai.codex".into(), "2p2nqsd0c76g0".into()))
        );
        assert_eq!(
            parse_package_family("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"),
            Some(("microsoft.windowsterminal".into(), "8wekyb3d8bbwe".into()))
        );
        // Not an AUMID / malformed.
        assert_eq!(parse_package_family("notepad.exe"), None);
        assert_eq!(parse_package_family("_abc!App"), None);
    }

    /// Real data: ChatGPT's AUMID vs the exe path its process actually reports.
    /// The version and architecture differ between the two, which is exactly why
    /// the match keys on package name + publisher hash rather than the folder name.
    #[test]
    fn packaged_app_running_matches_real_windowsapps_path() {
        let running = vec![
            r"c:\program files\windowsapps\openai.codex_26.707.9981.0_x64__2p2nqsd0c76g0\app\chatgpt.exe"
                .to_string(),
            r"c:\windows\explorer.exe".to_string(),
        ];
        assert!(packaged_app_is_running("OpenAI.Codex_2p2nqsd0c76g0!App", &running));
        // A different Store app that is NOT running.
        assert!(!packaged_app_is_running(
            "Microsoft.MicrosoftStickyNotes_8wekyb3d8bbwe!App",
            &running
        ));
    }

    /// Same package name, different publisher — must not be confused. This is
    /// why the publisher hash is part of the match and not just the name.
    #[test]
    fn packaged_match_requires_matching_publisher_hash() {
        let running = vec![
            r"c:\program files\windowsapps\contoso.app_1.0.0.0_x64__aaaaaaaaaaaaa\app\contoso.exe"
                .to_string(),
        ];
        assert!(packaged_app_is_running("Contoso.App_aaaaaaaaaaaaa!App", &running));
        assert!(!packaged_app_is_running("Contoso.App_bbbbbbbbbbbbb!App", &running));
    }

    /// A non-packaged process whose path merely resembles the name must not match.
    #[test]
    fn packaged_match_requires_windowsapps_location() {
        let running = vec![r"c:\tools\openai.codex_2p2nqsd0c76g0\fake.exe".to_string()];
        assert!(!packaged_app_is_running("OpenAI.Codex_2p2nqsd0c76g0!App", &running));
    }
}
