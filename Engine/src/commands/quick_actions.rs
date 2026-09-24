//! Inline calculator, unit conversion, and system commands for the overlay
//! quick-search. Recognizes patterns like:
//!
//!   "23 * 47"     → Calculator result "1081"
//!   "5km to mi"   → Unit conversion "3.1069 mi"
//!   "32f to c"    → Temperature conversion "0 °C"
//!   "lock"        → Lock workstation
//!   "shutdown"    → Power off (with frontend confirmation)
//!   "calc"        → Open Windows Calculator
//!
//! Privacy: everything here is local OS state — calculator runs in-process,
//! unit conversion uses hardcoded factors, system commands call Win32 APIs
//! directly (no network, no telemetry, same as clicking the Start menu's
//! power button).

use serde::Serialize;
use tauri::AppHandle;

/// A result the overlay should render above normal search results.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum QuickAction {
    /// Pure math expression — show the answer, click-to-copy.
    Calculator { expression: String, result: String },
    /// Unit conversion — show the converted value, click-to-copy.
    UnitConversion { original: String, result: String },
    /// System command — show as actionable row, click-to-execute.
    /// `requires_confirmation` flags destructive commands the frontend should
    /// prompt before invoking (shutdown, restart, sign out).
    SystemCommand {
        id: String,
        name: String,
        description: String,
        // `rename_all = "camelCase"` on the enum only renames VARIANT names
        // (so the `type` tag is "systemCommand"), NOT the fields inside a
        // struct variant. Without this explicit rename the field ships as
        // snake_case `requires_confirmation`, the frontend reads
        // `action.requiresConfirmation` as `undefined`, and destructive
        // commands then skip the confirm prompt AND send `confirmed: false`
        // — which the safety gate in `execute_system_command` rejects.
        #[serde(rename = "requiresConfirmation")]
        requires_confirmation: bool,
    },
    /// Detected URL — offer to open it in the user's default browser. No
    /// network call happens until the user explicitly activates this row;
    /// activation just hands the URL to the OS shell via `open`.
    OpenUrl { url: String, display: String },
    /// Explicit web-search request via a "bang" shortcut like `g foo`, `gh
    /// tantivy`, `? meaning of life`. Only fires when web search is enabled
    /// in settings AND the user typed a recognized bang prefix. The URL is
    /// pre-built (query already URL-encoded) so frontend just opens it.
    WebSearch {
        provider: String,
        url: String,
        query: String,
    },
}

/// Single entry point called by the overlay on every query. Returns the first
/// matching action (system command > URL > web-search bang > unit conversion >
/// math) or None if the query is just normal search text.
///
/// `web_search_enabled` is forwarded from the user's settings — when false,
/// bang prefixes like `g foo` are ignored entirely so they fall through to
/// normal local search.
#[tauri::command]
pub fn evaluate_quick_query(
    query: String,
    web_search_enabled: Option<bool>,
) -> Option<QuickAction> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }
    let web_search = web_search_enabled.unwrap_or(false);
    // Order matters: system commands match exact keywords like "lock" or "calc"
    // that we don't want a URL detector or bang to hijack. URL detection rejects
    // anything with whitespace, so it can't conflict with bang phrases like
    // "g hello world".
    match_system_command(trimmed)
        .or_else(|| try_url_detection(trimmed))
        .or_else(|| try_bang_search(trimmed, web_search))
        .or_else(|| try_unit_conversion(trimmed))
        .or_else(|| try_math(trimmed))
}

/// Returns true for command IDs that must be explicitly confirmed by the
/// caller before execution. The backend refuses to run them without
/// `confirmed: Some(true)` so a compromised frontend cannot trigger an
/// unattended shutdown/restart/signout.
///
/// `regedit` is included because it opens a tool that can permanently
/// damage the user's Windows install if misused — same posture as the
/// proposal: "Tier 3, confirm" for Registry Editor.
///
/// `empty-recyclebin` is included because it permanently deletes all
/// items in the Recycle Bin — same destructive posture as Empty Trash
/// in any other shell.
fn is_destructive_system_command(id: &str) -> bool {
    matches!(
        id,
        "shutdown" | "restart" | "signout" | "regedit" | "empty-recyclebin"
    )
}

/// Execute a system command by id. Called when the user activates the
/// quick-action row. Confirmation (for destructive commands) is handled by
/// the frontend before this is invoked.
///
/// `confirmed` must be `Some(true)` for destructive commands (shutdown,
/// restart, signout). Any other value — including `None` or `Some(false)` —
/// causes the command to return an error without executing. This backend gate
/// exists so a compromised frontend (XSS in a loaded web view, etc.) cannot
/// trigger an unattended shutdown even if it can call Tauri commands.
#[tauri::command]
pub fn execute_system_command(
    app: AppHandle,
    id: String,
    confirmed: Option<bool>,
) -> Result<Option<String>, String> {
    if is_destructive_system_command(&id) && !confirmed.unwrap_or(false) {
        return Err(format!(
            "Command '{id}' requires explicit confirmation (confirmed: true)"
        ));
    }
    // KeepItLocal self-actions are handled at this layer because they
    // need the AppHandle (for exit / restart). Other commands fall
    // through to the OS-state impl which doesn't need it.
    match id.as_str() {
        "kil-restart" => return crate::restart_keepitlocal_impl(&app).map(|_| None),
        "kil-quit" => {
            // Tauri's app.exit posts a quit on the event loop and
            // returns; the actual process termination happens once the
            // current command returns and the loop drains.
            app.exit(0);
            return Ok(None);
        }
        "open-privacy-audit" => return open_internal_screen(&app, "privacy-audit", "Privacy Audit"),
        _ => {}
    }
    execute_system_command_impl(&id)
}

/// Surface a KeepItLocal *internal* screen (Privacy Audit, Settings,
/// Notes …) from the palette. Shows the main window if hidden (covers
/// palette-only app mode) and emits a `navigate-tool` event the main
/// window's router listens for — the exact pattern `openMainAtTool`
/// uses for tool clicks in the palette. Wave 2.4 entry-point.
fn open_internal_screen(
    app: &AppHandle,
    tool_id: &str,
    label: &str,
) -> Result<Option<String>, String> {
    use tauri::{Emitter, Manager};
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.unminimize();
        let _ = main.set_focus();
    }
    let _ = app.emit_to("main", "navigate-tool", serde_json::json!({ "toolId": tool_id }));
    Ok(Some(format!("Opening {label}…")))
}

// ─── System commands ──────────────────────────────────────────────────────

/// (matched keyword, displayed name, description, command_id, destructive?)
///
/// Wave 2.1 (2026-05-26) added: hibernate, refresh-desktop, KeepItLocal
/// self-actions (kil-restart / kil-quit), management consoles
/// (devmgmt / services / eventvwr), settings deep-links (Windows
/// Security / Update / Sound / Mouse / Keyboard / About), and
/// regedit (Tier 3 — destructive).
const SYSTEM_COMMANDS: &[(&str, &str, &str, &str, bool)] = &[
    // Power & session
    ("lock", "Lock Workstation", "Lock the screen", "lock", false),
    ("lock screen", "Lock Workstation", "Lock the screen", "lock", false),
    ("sleep", "Sleep", "Put computer to sleep", "sleep", false),
    ("suspend", "Sleep", "Put computer to sleep", "sleep", false),
    ("hibernate", "Hibernate", "Hibernate (save state to disk)", "hibernate", false),
    ("shutdown", "Shut Down", "Turn off the computer", "shutdown", true),
    ("shut down", "Shut Down", "Turn off the computer", "shutdown", true),
    ("power off", "Shut Down", "Turn off the computer", "shutdown", true),
    ("restart", "Restart", "Restart the computer", "restart", true),
    ("reboot", "Restart", "Restart the computer", "restart", true),
    ("sign out", "Sign Out", "Sign out of Windows", "signout", true),
    ("log out", "Sign Out", "Sign out of Windows", "signout", true),
    ("logoff", "Sign Out", "Sign out of Windows", "signout", true),
    // KeepItLocal self-actions
    ("restart app", "Restart KeepItLocal", "Restart the KeepItLocal app", "kil-restart", false),
    ("restart keepitlocal", "Restart KeepItLocal", "Restart the KeepItLocal app", "kil-restart", false),
    ("quit", "Quit KeepItLocal", "Close KeepItLocal", "kil-quit", false),
    ("quit keepitlocal", "Quit KeepItLocal", "Close KeepItLocal", "kil-quit", false),
    ("exit", "Quit KeepItLocal", "Close KeepItLocal", "kil-quit", false),
    // App launches
    ("calc", "Calculator", "Open Windows Calculator", "calc", false),
    ("calculator", "Calculator", "Open Windows Calculator", "calc", false),
    ("cmd", "Command Prompt", "Open command prompt", "cmd", false),
    ("terminal", "Terminal", "Open Windows Terminal", "terminal", false),
    ("powershell", "PowerShell", "Open PowerShell", "powershell", false),
    ("task manager", "Task Manager", "Open Task Manager", "taskmgr", false),
    ("taskmgr", "Task Manager", "Open Task Manager", "taskmgr", false),
    ("screenshot", "Screenshot", "Take a screenshot", "snip", false),
    ("snip", "Snipping Tool", "Take a screenshot", "snip", false),
    ("notepad", "Notepad", "Open Notepad", "notepad", false),
    ("explorer", "File Explorer", "Open File Explorer", "explorer", false),
    // Management consoles
    ("device manager", "Device Manager", "Open Device Manager", "devmgmt", false),
    ("devmgmt", "Device Manager", "Open Device Manager", "devmgmt", false),
    ("services", "Services", "Open the Services console", "services", false),
    ("event viewer", "Event Viewer", "Open Event Viewer", "eventvwr", false),
    ("eventvwr", "Event Viewer", "Open Event Viewer", "eventvwr", false),
    ("registry editor", "Registry Editor", "Open Registry Editor (advanced — caution)", "regedit", true),
    ("regedit", "Registry Editor", "Open Registry Editor (advanced — caution)", "regedit", true),
    // Windows Settings deep-links
    ("settings", "Settings", "Open Windows Settings", "settings", false),
    ("control panel", "Control Panel", "Open Control Panel", "control", false),
    ("windows security", "Windows Security", "Open Windows Security (Defender)", "windows-security", false),
    ("defender", "Windows Security", "Open Windows Security (Defender)", "windows-security", false),
    ("windows update", "Windows Update", "Open Windows Update settings", "windows-update", false),
    ("update", "Windows Update", "Open Windows Update settings", "windows-update", false),
    ("sound settings", "Sound Settings", "Open Sound settings", "sound-settings", false),
    ("mouse settings", "Mouse Settings", "Open Mouse / Touchpad settings", "mouse-settings", false),
    ("keyboard settings", "Keyboard Settings", "Open Keyboard settings", "keyboard-settings", false),
    ("about pc", "About This PC", "Open About this PC", "about-pc", false),
    ("about this pc", "About This PC", "Open About this PC", "about-pc", false),
    ("system info", "About This PC", "Open About this PC", "about-pc", false),
    // Shell refresh (per the 2026-05-26 user clarification on #13 "refresh")
    ("refresh desktop", "Refresh Desktop", "Refresh the Windows desktop icon cache", "refresh-desktop", false),
    // Network (Wave 2.2 — radio toggles via WinRT, no admin needed)
    ("wifi", "Toggle Wi-Fi", "Turn Wi-Fi on or off", "wifi", false),
    ("wi-fi", "Toggle Wi-Fi", "Turn Wi-Fi on or off", "wifi", false),
    ("toggle wifi", "Toggle Wi-Fi", "Turn Wi-Fi on or off", "wifi", false),
    ("bluetooth", "Toggle Bluetooth", "Turn Bluetooth on or off", "bluetooth", false),
    ("toggle bluetooth", "Toggle Bluetooth", "Turn Bluetooth on or off", "bluetooth", false),
    ("airplane", "Toggle Airplane Mode", "Turn all radios off (or back on)", "airplane", false),
    ("airplane mode", "Toggle Airplane Mode", "Turn all radios off (or back on)", "airplane", false),
    ("flush dns", "Flush DNS Cache", "Clear the Windows DNS resolver cache", "flush-dns", false),
    ("clear dns", "Flush DNS Cache", "Clear the Windows DNS resolver cache", "flush-dns", false),
    ("dns", "Flush DNS Cache", "Clear the Windows DNS resolver cache", "flush-dns", false),
    ("ip", "Show Local IP", "Show this machine's LAN IPv4 address", "show-ip", false),
    ("show ip", "Show Local IP", "Show this machine's LAN IPv4 address", "show-ip", false),
    ("my ip", "Show Local IP", "Show this machine's LAN IPv4 address", "show-ip", false),
    ("ipaddress", "Show Local IP", "Show this machine's LAN IPv4 address", "show-ip", false),
    // ─── Wave 2.3 — audio + window management ────────────────────────
    // All audio / window actions synthesize the corresponding Windows
    // keyboard shortcut via SendInput. Simpler + more compatible than
    // Core Audio MMDeviceEnumerator (volume) and IVirtualDesktopManagerInternal
    // (virtual desktops) — those are pinned to OS-version-specific
    // interfaces; SendInput hits Explorer's published shortcuts which
    // are stable across Win10/Win11. Trade-off: we can't set volume to
    // a specific % (only step up/down), and we can't read current state
    // (mute on or off?) — that's a Core Audio follow-up if needed.
    // Audio
    ("mute", "Mute / Unmute", "Toggle system audio mute", "audio-mute", false),
    ("unmute", "Mute / Unmute", "Toggle system audio mute", "audio-mute", false),
    ("volume up", "Volume Up", "Raise system volume one step", "volume-up", false),
    ("vol up", "Volume Up", "Raise system volume one step", "volume-up", false),
    ("volume down", "Volume Down", "Lower system volume one step", "volume-down", false),
    ("vol down", "Volume Down", "Lower system volume one step", "volume-down", false),
    // Window management
    ("show desktop", "Show Desktop", "Minimize all windows to show the desktop", "show-desktop", false),
    ("desktop", "Show Desktop", "Minimize all windows to show the desktop", "show-desktop", false),
    ("minimize all", "Minimize All Windows", "Minimize every visible window", "minimize-all", false),
    ("restore all", "Restore All Windows", "Undo Minimize All (Win+Shift+M)", "restore-all", false),
    ("maximize", "Maximize Active Window", "Maximize the focused window", "win-maximize", false),
    ("minimize", "Minimize Active Window", "Minimize the focused window", "win-minimize", false),
    ("snap left", "Snap Window Left", "Snap the active window to the left half", "win-snap-left", false),
    ("snap right", "Snap Window Right", "Snap the active window to the right half", "win-snap-right", false),
    // Virtual desktops
    ("next desktop", "Next Virtual Desktop", "Switch to the next virtual desktop", "vd-next", false),
    ("previous desktop", "Previous Virtual Desktop", "Switch to the previous virtual desktop", "vd-prev", false),
    ("new desktop", "New Virtual Desktop", "Create a new virtual desktop", "vd-new", false),
    ("close desktop", "Close Virtual Desktop", "Close the current virtual desktop", "vd-close", false),
    ("task view", "Task View", "Open Windows Task View (Win+Tab)", "task-view", false),
    // ─── Wave 2.4 — file system + privacy launchers ──────────────────
    // Recycle Bin
    ("recycle bin", "Open Recycle Bin", "Show the Recycle Bin in Explorer", "open-recyclebin", false),
    ("trash", "Open Recycle Bin", "Show the Recycle Bin in Explorer", "open-recyclebin", false),
    ("empty recycle bin", "Empty Recycle Bin", "Permanently delete all items in the Recycle Bin", "empty-recyclebin", true),
    ("empty trash", "Empty Recycle Bin", "Permanently delete all items in the Recycle Bin", "empty-recyclebin", true),
    // Explorer toggles
    ("toggle hidden files", "Toggle Hidden Files", "Show or hide hidden files in Explorer", "toggle-hidden", false),
    ("show hidden files", "Toggle Hidden Files", "Show or hide hidden files in Explorer", "toggle-hidden", false),
    ("hidden files", "Toggle Hidden Files", "Show or hide hidden files in Explorer", "toggle-hidden", false),
    ("toggle extensions", "Toggle File Extensions", "Show or hide file extensions in Explorer", "toggle-extensions", false),
    ("show extensions", "Toggle File Extensions", "Show or hide file extensions in Explorer", "toggle-extensions", false),
    ("file extensions", "Toggle File Extensions", "Show or hide file extensions in Explorer", "toggle-extensions", false),
    // Known folders (shell: URIs — no SHGetKnownFolderPath needed)
    ("open downloads", "Open Downloads", "Open the Downloads folder", "folder-downloads", false),
    ("downloads", "Open Downloads", "Open the Downloads folder", "folder-downloads", false),
    ("open documents", "Open Documents", "Open the Documents folder", "folder-documents", false),
    ("documents", "Open Documents", "Open the Documents folder", "folder-documents", false),
    ("open desktop", "Open Desktop Folder", "Open the Desktop folder", "folder-desktop", false),
    ("open pictures", "Open Pictures", "Open the Pictures folder", "folder-pictures", false),
    ("pictures", "Open Pictures", "Open the Pictures folder", "folder-pictures", false),
    ("open videos", "Open Videos", "Open the Videos folder", "folder-videos", false),
    ("videos", "Open Videos", "Open the Videos folder", "folder-videos", false),
    ("open music", "Open Music", "Open the Music folder", "folder-music", false),
    ("music", "Open Music", "Open the Music folder", "folder-music", false),
    ("open appdata", "Open AppData", "Open the Roaming AppData folder", "folder-appdata", false),
    ("appdata", "Open AppData", "Open the Roaming AppData folder", "folder-appdata", false),
    ("open localappdata", "Open Local AppData", "Open the Local AppData folder", "folder-localappdata", false),
    ("localappdata", "Open Local AppData", "Open the Local AppData folder", "folder-localappdata", false),
    // Privacy launchers (ms-settings deep-links + KIL internal)
    ("camera privacy", "Camera Privacy Settings", "Open Settings → Privacy → Camera", "camera-privacy", false),
    ("webcam privacy", "Camera Privacy Settings", "Open Settings → Privacy → Camera", "camera-privacy", false),
    ("location privacy", "Location Privacy Settings", "Open Settings → Privacy → Location", "location-privacy", false),
    ("privacy audit", "Open Privacy Audit", "Run KeepItLocal's local privacy scan", "open-privacy-audit", false),
    ("audit", "Open Privacy Audit", "Run KeepItLocal's local privacy scan", "open-privacy-audit", false),
    // Control Panel applets — classic Win32 consoles (.msc / .cpl / .exe),
    // all launched through ShellExecute (which `open::that` wraps). Mirror
    // the multi-alias style above so typed shortcuts resolve too.
    ("disk management", "Disk Management", "Open Disk Management", "diskmgmt", false),
    ("diskmgmt", "Disk Management", "Open Disk Management", "diskmgmt", false),
    ("partitions", "Disk Management", "Open Disk Management", "diskmgmt", false),
    ("computer management", "Computer Management", "Open Computer Management", "compmgmt", false),
    ("compmgmt", "Computer Management", "Open Computer Management", "compmgmt", false),
    ("task scheduler", "Task Scheduler", "Open Task Scheduler", "taskschd", false),
    ("scheduled tasks", "Task Scheduler", "Open Task Scheduler", "taskschd", false),
    ("taskschd", "Task Scheduler", "Open Task Scheduler", "taskschd", false),
    ("performance monitor", "Performance Monitor", "Open Performance Monitor", "perfmon", false),
    ("perfmon", "Performance Monitor", "Open Performance Monitor", "perfmon", false),
    ("programs and features", "Programs and Features", "Uninstall or change a program", "appwiz", false),
    ("uninstall programs", "Programs and Features", "Uninstall or change a program", "appwiz", false),
    ("add remove programs", "Programs and Features", "Uninstall or change a program", "appwiz", false),
    ("appwiz", "Programs and Features", "Uninstall or change a program", "appwiz", false),
    ("installed programs", "Programs and Features", "Uninstall or change a program", "appwiz", false),
    ("network connections", "Network Connections", "Open network adapter connections", "ncpa", false),
    ("network adapters", "Network Connections", "Open network adapter connections", "ncpa", false),
    ("ncpa", "Network Connections", "Open network adapter connections", "ncpa", false),
    ("system properties", "System Properties", "Open the classic System Properties (env vars, etc.)", "sysdm", false),
    ("environment variables", "System Properties", "Open the classic System Properties (env vars, etc.)", "sysdm", false),
    ("sysdm", "System Properties", "Open the classic System Properties (env vars, etc.)", "sysdm", false),
    ("advanced system settings", "System Properties", "Open the classic System Properties (env vars, etc.)", "sysdm", false),
    ("sound control panel", "Sound (Control Panel)", "Open the classic Sound control panel", "mmsys", false),
    ("playback devices", "Sound (Control Panel)", "Open the classic Sound control panel", "mmsys", false),
    ("recording devices", "Sound (Control Panel)", "Open the classic Sound control panel", "mmsys", false),
    ("mmsys", "Sound (Control Panel)", "Open the classic Sound control panel", "mmsys", false),
    ("power options", "Power Options", "Open Power Options", "powercfg", false),
    ("power plan", "Power Options", "Open Power Options", "powercfg", false),
    ("powercfg", "Power Options", "Open Power Options", "powercfg", false),
    ("firewall", "Windows Firewall", "Open Windows Defender Firewall", "firewall", false),
    ("windows firewall", "Windows Firewall", "Open Windows Defender Firewall", "firewall", false),
    ("windows features", "Windows Features", "Turn Windows features on or off", "optionalfeatures", false),
    ("optional features", "Windows Features", "Turn Windows features on or off", "optionalfeatures", false),
    ("optionalfeatures", "Windows Features", "Turn Windows features on or off", "optionalfeatures", false),
    ("group policy", "Group Policy Editor", "Open Local Group Policy Editor (Pro/Enterprise only)", "gpedit", false),
    ("gpedit", "Group Policy Editor", "Open Local Group Policy Editor (Pro/Enterprise only)", "gpedit", false),
    ("local group policy", "Group Policy Editor", "Open Local Group Policy Editor (Pro/Enterprise only)", "gpedit", false),
];

/// A browsable system-command row for the Commands chip. Shape mirrors the
/// `QuickAction::SystemCommand` fields so the frontend can dispatch each one
/// through the existing `execute_system_command` (id + confirmed gate).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemCommandItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub requires_confirmation: bool,
    /// Which browse group this row belongs to: "action" (power/network/
    /// recycle/terminal) or "control-panel" (classic Win32 consoles). The
    /// frontend renders + navigates in the order these are returned.
    pub group: String,
}

/// Curated subset of `SYSTEM_COMMANDS` for browsing in the Commands chip:
/// power/session, network, recycle bin, and terminal — the high-signal
/// actions. App launches and ms-settings deep-links are deliberately
/// excluded (too noisy for a browse list). Reuses the SYSTEM_COMMANDS table
/// (one row per id) so the strings stay single-sourced; confirmation comes
/// from `is_destructive_system_command`.
#[tauri::command]
pub fn list_system_commands() -> Vec<SystemCommandItem> {
    // The single id per curated row (the SYSTEM_COMMANDS table has several
    // keyword aliases per id; we keep one row each, in display order).
    //
    // "action" group: high-signal power/network/recycle/terminal actions.
    const ACTION_IDS: &[&str] = &[
        // Power & session
        "lock",
        "sleep",
        "shutdown",
        "restart",
        "signout",
        // Network
        "wifi",
        "flush-dns",
        "show-ip",
        // Recycle Bin
        "open-recyclebin",
        "empty-recyclebin",
        // Terminal
        "terminal",
    ];

    // "control-panel" group: classic Win32 consoles/applets (.msc/.cpl/.exe).
    const CONTROL_PANEL_IDS: &[&str] = &[
        "taskmgr",
        "devmgmt",
        "diskmgmt",
        "services",
        "compmgmt",
        "taskschd",
        "eventvwr",
        "perfmon",
        "appwiz",
        "ncpa",
        "sysdm",
        "mmsys",
        "powercfg",
        "firewall",
        "optionalfeatures",
        "control",
        "regedit",
        "gpedit",
    ];

    // Build one group's rows by looking each id up in SYSTEM_COMMANDS (so the
    // display strings stay single-sourced) and tagging it with the group name.
    let collect_group = |ids: &[&str], group: &str| -> Vec<SystemCommandItem> {
        ids.iter()
            .filter_map(|wanted| {
                SYSTEM_COMMANDS
                    .iter()
                    .find(|&&(_, _, _, id, _)| id == *wanted)
                    .map(|&(_, name, desc, id, _)| SystemCommandItem {
                        id: id.to_string(),
                        name: name.to_string(),
                        description: desc.to_string(),
                        requires_confirmation: is_destructive_system_command(id),
                        group: group.to_string(),
                    })
            })
            .collect()
    };

    // Order matters: actions first, then control-panel — the frontend renders
    // and navigates in this order, so its two sub-sections stay in sync.
    let mut rows = collect_group(ACTION_IDS, "action");
    rows.extend(collect_group(CONTROL_PANEL_IDS, "control-panel"));
    rows
}

fn match_system_command(query: &str) -> Option<QuickAction> {
    let q = query.to_lowercase();
    for &(pattern, name, desc, id, destructive) in SYSTEM_COMMANDS {
        // Exact match only — avoid hijacking searches like "calculator manual.pdf".
        if q == pattern {
            return Some(QuickAction::SystemCommand {
                id: id.to_string(),
                name: name.to_string(),
                description: desc.to_string(),
                requires_confirmation: destructive,
            });
        }
    }
    None
}

/// Enable a Win32 privilege on the current process token. Required before
/// shutdown / restart / signout — the privileges are GRANTED to standard
/// users by default but Windows still requires them to be explicitly
/// enabled in the token before APIs that check for them will accept the
/// call. Without this, ExitWindowsEx returns ERROR_PRIVILEGE_NOT_HELD
/// (0x80070522), which is what we were seeing from the search overlay.
///
/// Caller passes the privilege name as a wide-string-ready Rust &str
/// (e.g. "SeShutdownPrivilege"). Returns Ok on enable, Err with a
/// human-readable message on any step's failure.
#[cfg(windows)]
fn enable_token_privilege(privilege_name: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // Convert the privilege name to a NUL-terminated wide string.
    let wide: Vec<u16> = privilege_name.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .map_err(|e| format!("OpenProcessToken failed: {e}"))?;

        // Look up the LUID for the named privilege. LUIDs are stable
        // for the life of a boot but can vary across reboots, so we
        // can't hardcode them.
        let mut luid = LUID::default();
        let lookup_result =
            LookupPrivilegeValueW(PCWSTR::null(), PCWSTR(wide.as_ptr()), &mut luid)
                .map_err(|e| format!("LookupPrivilegeValueW failed: {e}"));
        if let Err(error) = lookup_result {
            let _ = CloseHandle(token);
            return Err(error);
        }

        // Build a TOKEN_PRIVILEGES payload requesting one privilege be
        // enabled. The struct has a flexible-array tail; we use the
        // zero-init + manual fill pattern since Rust doesn't ergonomically
        // construct flexible-array structs.
        let mut privileges = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };

        let adjust_result = AdjustTokenPrivileges(
            token,
            false,
            Some(&mut privileges),
            0,
            None,
            None,
        )
        .map_err(|e| format!("AdjustTokenPrivileges failed: {e}"));

        let _ = CloseHandle(token);
        adjust_result?;

        // AdjustTokenPrivileges returns success even when not all
        // privileges were assigned — caller has to check GetLastError
        // for ERROR_NOT_ALL_ASSIGNED. We translate that into a clearer
        // error than "succeeded but actually didn't".
        let last = windows::Win32::Foundation::GetLastError();
        if last.0 == 1300 {
            // ERROR_NOT_ALL_ASSIGNED — the user account doesn't actually
            // have the right granted (rare on standard Windows installs;
            // can happen on locked-down enterprise images).
            return Err(format!(
                "Privilege {privilege_name} is not granted to your account by Group Policy"
            ));
        }
    }

    Ok(())
}

#[cfg(windows)]
fn execute_system_command_impl(id: &str) -> Result<Option<String>, String> {
    use windows::Win32::System::Power::SetSuspendState;
    use windows::Win32::System::Shutdown::{
        ExitWindowsEx, LockWorkStation, EWX_LOGOFF, EWX_POWEROFF, EWX_REBOOT,
        SHUTDOWN_REASON,
    };

    // Wave 2.2 (2026-05-26): the radio + network IDs route through their
    // own helpers (each returns an Option<String> status message so the
    // frontend can toast "Wi-Fi turned ON" instead of a silent success).
    // Handled BEFORE the unsafe Win32 block so the borrow checker
    // doesn't flag the early returns through `?` inside `unsafe`.
    match id {
        "wifi" | "wifi-toggle" => return toggle_wifi(),
        "bluetooth" | "bluetooth-toggle" => return toggle_bluetooth(),
        "airplane" | "airplane-toggle" => return toggle_airplane_mode(),
        "flush-dns" => return flush_dns(),
        "show-ip" => return show_current_ip(),
        // Wave 2.3 — audio (synthesized via VK_VOLUME_* keys).
        "audio-mute" => return audio_action("audio-mute"),
        "volume-up" => return audio_action("volume-up"),
        "volume-down" => return audio_action("volume-down"),
        // Wave 2.3 — window management (synthesized via Win+key combos).
        "show-desktop"
        | "minimize-all"
        | "restore-all"
        | "win-maximize"
        | "win-minimize"
        | "win-snap-left"
        | "win-snap-right"
        | "vd-next"
        | "vd-prev"
        | "vd-new"
        | "vd-close"
        | "task-view" => return window_action(id),
        // Wave 2.4 — recycle bin + explorer toggles + known folders.
        "open-recyclebin" => return open_known_path("shell:RecycleBinFolder"),
        "empty-recyclebin" => return empty_recycle_bin(),
        "toggle-hidden" => return toggle_explorer_advanced("Hidden", 1, 2, "Hidden files"),
        "toggle-extensions" => return toggle_explorer_advanced("HideFileExt", 0, 1, "File extensions"),
        "folder-downloads" => return open_known_path("shell:Downloads"),
        "folder-documents" => return open_known_path("shell:Personal"),
        "folder-desktop" => return open_known_path("shell:Desktop"),
        "folder-pictures" => return open_known_path("shell:My Pictures"),
        "folder-videos" => return open_known_path("shell:My Video"),
        "folder-music" => return open_known_path("shell:My Music"),
        "folder-appdata" => return open_known_path("shell:AppData"),
        "folder-localappdata" => return open_known_path("shell:Local AppData"),
        "camera-privacy" => return open_ms_settings("ms-settings:privacy-webcam"),
        "location-privacy" => return open_ms_settings("ms-settings:privacy-location"),
        _ => {}
    }

    // SAFETY: every Win32 call here is a direct OS state-change API. They take
    // no pointers from us and have no aliasing concerns.
    unsafe {
        match id {
            "lock" => LockWorkStation()
                .map_err(|e| format!("Lock failed: {e}"))?,
            "sleep" => {
                // SetSuspendState(hibernate=false, force=false, disable_wake=false)
                let ok = SetSuspendState(false, false, false);
                if !ok.as_bool() {
                    return Err("Sleep request was rejected by the system".to_string());
                }
            }
            "hibernate" => {
                // SetSuspendState(hibernate=true, ...). Hibernation may
                // be disabled on the machine (most modern laptops ship
                // with `powercfg /h off` by default); when that's the
                // case the kernel falls back to sleep and reports
                // success, OR refuses outright. We surface a clean
                // error and let the user enable it via powercfg /h on.
                let ok = SetSuspendState(true, false, false);
                if !ok.as_bool() {
                    return Err(
                        "Hibernate failed — it may be disabled on this PC. \
                         Try `powercfg /h on` in an elevated terminal."
                            .to_string(),
                    );
                }
            }
            "shutdown" => {
                // SE_SHUTDOWN_NAME must be enabled in the process token
                // before ExitWindowsEx will accept a power-off. Without
                // this the call fails with ERROR_PRIVILEGE_NOT_HELD even
                // for users who legitimately have the right.
                enable_token_privilege("SeShutdownPrivilege")?;
                ExitWindowsEx(EWX_POWEROFF, SHUTDOWN_REASON(0))
                    .map_err(|e| format!("Shutdown failed: {e}"))?
            }
            "restart" => {
                enable_token_privilege("SeShutdownPrivilege")?;
                ExitWindowsEx(EWX_REBOOT, SHUTDOWN_REASON(0))
                    .map_err(|e| format!("Restart failed: {e}"))?
            }
            "signout" => ExitWindowsEx(EWX_LOGOFF, SHUTDOWN_REASON(0))
                .map_err(|e| format!("Sign out failed: {e}"))?,
            // App / settings launches use the `open` crate (which uses
            // ShellExecute internally) — handles both .exe paths and ms-* URIs.
            "calc" => open::that("calc.exe").map_err(|e| e.to_string())?,
            "cmd" => open::that("cmd.exe").map_err(|e| e.to_string())?,
            "terminal" => open::that("wt.exe")
                .or_else(|_| open::that("powershell.exe"))
                .map_err(|e| e.to_string())?,
            "powershell" => open::that("powershell.exe").map_err(|e| e.to_string())?,
            "taskmgr" => open::that("taskmgr.exe").map_err(|e| e.to_string())?,
            "snip" => open::that("ms-screenclip:")
                .or_else(|_| open::that("snippingtool.exe"))
                .map_err(|e| e.to_string())?,
            "settings" => open::that("ms-settings:").map_err(|e| e.to_string())?,
            // Deep-link to Windows Speech settings (Time & Language → Speech).
            // Used by the Voice-to-Text tool's "Open Settings" guidance
            // when speech recognition isn't installed yet.
            "speech-settings" => open::that("ms-settings:speech")
                .or_else(|_| open::that("ms-settings:"))
                .map_err(|e| e.to_string())?,
            // Deep-link to Privacy → Microphone. Used by the Vosk engine
            // path when cpal returns 0x80070005 (access denied) — Vosk
            // captures via WASAPI directly, which respects per-app
            // privacy toggles unlike Windows' shared speech subsystem.
            "microphone-privacy" => open::that("ms-settings:privacy-microphone")
                .or_else(|_| open::that("ms-settings:"))
                .map_err(|e| e.to_string())?,
            "control" => open::that("control.exe").map_err(|e| e.to_string())?,
            "notepad" => open::that("notepad.exe").map_err(|e| e.to_string())?,
            "explorer" => open::that("explorer.exe").map_err(|e| e.to_string())?,
            // Browser launches — used by the voice command system
            // ("open chrome"). Each name is resolved by the OS via the
            // App Paths registry or PATH; missing apps surface as a
            // clean error rather than crashing the recognizer flow.
            // (calc / cmd / terminal / powershell already covered
            // above; we don't duplicate them.)
            "chrome" => open::that("chrome").map_err(|e| e.to_string())?,
            "firefox" => open::that("firefox").map_err(|e| e.to_string())?,
            "msedge" => open::that("msedge").map_err(|e| e.to_string())?,
            // Management consoles — .msc files are dispatched by mmc.exe
            // through ShellExecute, which `open::that` wraps.
            "devmgmt" => open::that("devmgmt.msc").map_err(|e| e.to_string())?,
            "services" => open::that("services.msc").map_err(|e| e.to_string())?,
            "eventvwr" => open::that("eventvwr.msc").map_err(|e| e.to_string())?,
            "regedit" => open::that("regedit.exe").map_err(|e| e.to_string())?,
            // Control Panel applets — .msc consoles, .cpl applets, and a
            // couple of .exe launchers, all resolved via ShellExecute.
            "diskmgmt" => open::that("diskmgmt.msc").map_err(|e| e.to_string())?,
            "compmgmt" => open::that("compmgmt.msc").map_err(|e| e.to_string())?,
            "taskschd" => open::that("taskschd.msc").map_err(|e| e.to_string())?,
            "perfmon" => open::that("perfmon.msc").map_err(|e| e.to_string())?,
            "appwiz" => open::that("appwiz.cpl").map_err(|e| e.to_string())?,
            "ncpa" => open::that("ncpa.cpl").map_err(|e| e.to_string())?,
            "sysdm" => open::that("sysdm.cpl").map_err(|e| e.to_string())?,
            "mmsys" => open::that("mmsys.cpl").map_err(|e| e.to_string())?,
            "powercfg" => open::that("powercfg.cpl").map_err(|e| e.to_string())?,
            "firewall" => open::that("firewall.cpl").map_err(|e| e.to_string())?,
            "optionalfeatures" => open::that("optionalfeatures.exe").map_err(|e| e.to_string())?,
            "gpedit" => open::that("gpedit.msc").map_err(|e| e.to_string())?,
            // Settings deep-links via the ms-settings: URI scheme. Each
            // resolves through ShellExecute on Windows 10/11. The
            // fallback `ms-settings:` re-opens the root Settings app on
            // unusual installs where the specific page isn't registered.
            "windows-security" => open::that("ms-settings:windowsdefender")
                .or_else(|_| open::that("windowsdefender:"))
                .or_else(|_| open::that("ms-settings:"))
                .map_err(|e| e.to_string())?,
            "windows-update" => open::that("ms-settings:windowsupdate")
                .or_else(|_| open::that("ms-settings:"))
                .map_err(|e| e.to_string())?,
            "sound-settings" => open::that("ms-settings:sound")
                .or_else(|_| open::that("ms-settings:"))
                .map_err(|e| e.to_string())?,
            "mouse-settings" => open::that("ms-settings:mousetouchpad")
                .or_else(|_| open::that("ms-settings:"))
                .map_err(|e| e.to_string())?,
            "keyboard-settings" => open::that("ms-settings:keyboard")
                .or_else(|_| open::that("ms-settings:"))
                .map_err(|e| e.to_string())?,
            "about-pc" => open::that("ms-settings:about")
                .or_else(|_| open::that("ms-settings:"))
                .map_err(|e| e.to_string())?,
            // Refresh the Windows shell's icon cache. ie4uinit ships
            // with Windows 10/11; `-show` triggers the icon cache
            // rebuild. Visible desktop-icon refresh (the F5 equivalent)
            // is harder — that's a WM_COMMAND to the desktop ListView —
            // so this gives the user the most common interpretation
            // (rebuild stale icons). Document this nuance with a clear
            // toast on the frontend if needed.
            "refresh-desktop" => {
                use std::os::windows::process::CommandExt;
                use std::process::Command;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                Command::new("ie4uinit.exe")
                    .arg("-show")
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
                    .map_err(|e| format!("Refresh failed: {e}"))?;
            }
            other => return Err(format!("Unknown system command: {other}")),
        }
    }
    Ok(None)
}

#[cfg(not(windows))]
fn execute_system_command_impl(_id: &str) -> Result<Option<String>, String> {
    Err("System commands are Windows-only for now".to_string())
}

/// Check whether a given app/system command can actually be launched
/// on this machine.
///
/// Used by the voice command system to gate-keep "open chrome",
/// "open firefox", etc. — without this we'd happily call
/// ShellExecuteEx with a name that doesn't resolve, and Windows pops
/// up its "Find a program" dialog. That's a terrible UX surface for a
/// voice command, so the frontend pre-checks and either falls back to
/// a web equivalent or shows a clean "not installed" toast.
///
/// Implementation uses `where.exe` because it consults both PATH and
/// the App Paths registry (HKLM\SOFTWARE\Microsoft\Windows\
/// CurrentVersion\App Paths\<exe>.exe), which is where Chrome/Firefox/
/// Edge register themselves. The `CREATE_NO_WINDOW` flag suppresses
/// the brief console flash that would otherwise appear.
///
/// `id` is the same string the frontend passes to `execute_system_command`.
/// We map it back to the candidate exe name(s) here.
#[tauri::command]
pub fn check_app_available(id: String) -> bool {
    #[cfg(windows)]
    {
        let candidates: &[&str] = match id.as_str() {
            "chrome" => &["chrome.exe", "chrome"],
            "firefox" => &["firefox.exe", "firefox"],
            "msedge" => &["msedge.exe", "msedge"],
            "notepad" => &["notepad.exe"],
            "calc" => &["calc.exe"],
            "cmd" => &["cmd.exe"],
            "powershell" => &["powershell.exe"],
            "taskmgr" => &["taskmgr.exe"],
            "control" => &["control.exe"],
            "explorer" => &["explorer.exe"],
            "terminal" => &["wt.exe", "powershell.exe"],
            // ms-settings:// and similar URIs are always available on
            // Windows 10+. We return true unconditionally for them so
            // the frontend doesn't gate-keep system panels.
            "settings"
            | "speech-settings"
            | "microphone-privacy"
            | "snip" => return true,
            _ => return false,
        };
        candidates.iter().any(|name| where_resolves(name))
    }
    #[cfg(not(windows))]
    {
        let _ = id;
        false
    }
}

// ─── Wave 2.2 — radio toggles + network info ──────────────────────────
//
// Wi-Fi / Bluetooth / Airplane all route through WinRT's
// `Windows.Devices.Radios.Radio` API. The big win versus `netsh
// interface set ...` is that the Radio API does NOT require admin
// elevation — the user grants per-app radio access ONCE via the system
// consent dialog on first call. From then on the toggles are
// instantaneous. The trade-off is the API is asynchronous and uses
// WinRT's IAsyncOperation pattern; we block on each .get() inside
// what's already a Tauri command runtime task, so no concurrency
// surprise.
//
// Flush-DNS shells out to `ipconfig /flushdns` (the canonical way; the
// IpHelper equivalent — `DnsFlushResolverCache` — is undocumented and
// only callable from elevated processes). Show-IP uses the UDP-connect
// trick — bind a UDP socket and `connect()` it to a routable address;
// the kernel picks the outbound interface and `local_addr()` reports
// its IP, WITHOUT sending a packet. Pure local, no DNS lookup, no
// outbound traffic — exactly the privacy-first behavior we want.

/// Toggle the Wi-Fi radio on ↔ off. Returns a human-readable status
/// message ("Wi-Fi turned ON" / "Wi-Fi turned OFF") for the frontend
/// to toast.
#[cfg(windows)]
fn toggle_wifi() -> Result<Option<String>, String> {
    use windows::Devices::Radios::RadioKind;
    toggle_radio(RadioKind::WiFi, "Wi-Fi")
}

#[cfg(not(windows))]
fn toggle_wifi() -> Result<Option<String>, String> {
    Err("Wi-Fi toggle is only wired on Windows".to_string())
}

/// Toggle the Bluetooth radio on ↔ off.
#[cfg(windows)]
fn toggle_bluetooth() -> Result<Option<String>, String> {
    use windows::Devices::Radios::RadioKind;
    toggle_radio(RadioKind::Bluetooth, "Bluetooth")
}

#[cfg(not(windows))]
fn toggle_bluetooth() -> Result<Option<String>, String> {
    Err("Bluetooth toggle is only wired on Windows".to_string())
}

/// Toggle airplane mode by flipping ALL radios at once. If any radio is
/// currently on, turn everything off (entering airplane mode). If
/// everything is already off, turn the Wi-Fi + Bluetooth radios back on
/// (leaving airplane mode — we don't touch any non-WiFi/BT radios on
/// the way back because most users only have those two and we
/// shouldn't surprise-enable obscure ones).
#[cfg(windows)]
fn toggle_airplane_mode() -> Result<Option<String>, String> {
    use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};

    // See toggle_radio() for why this is required on Tauri command threads.
    ensure_winrt_apartment();

    let access = Radio::RequestAccessAsync()
        .map_err(|e| format!("Radio access request failed: {e}"))?
        .get()
        .map_err(|e| format!("Radio access wait failed: {e}"))?;
    if access != RadioAccessStatus::Allowed {
        return Err(radio_denied_message(access));
    }
    let radios = Radio::GetRadiosAsync()
        .map_err(|e| format!("GetRadiosAsync failed: {e}"))?
        .get()
        .map_err(|e| format!("GetRadiosAsync wait failed: {e}"))?;
    let count = radios.Size().map_err(|e| e.to_string())?;

    // First pass: collect each radio's kind + state so we can decide
    // the direction (turn-all-off vs turn-wifi-bt-on) before mutating.
    let mut radio_info: Vec<(windows::Devices::Radios::Radio, RadioKind, RadioState)> = Vec::new();
    for i in 0..count {
        let r = radios.GetAt(i).map_err(|e| e.to_string())?;
        let kind = r.Kind().map_err(|e| e.to_string())?;
        let state = r.State().map_err(|e| e.to_string())?;
        radio_info.push((r, kind, state));
    }

    // "Any radio currently on" → we're entering airplane mode.
    let any_on = radio_info
        .iter()
        .any(|(_, _, state)| *state == RadioState::On);
    let target_state = if any_on {
        RadioState::Off
    } else {
        RadioState::On
    };

    let mut touched = 0usize;
    for (radio, kind, state) in &radio_info {
        // Going OFF: touch every radio. Going ON: only WiFi + Bluetooth
        // (see fn doc — avoid surprise-enabling obscure radios).
        // `kind` is `&RadioKind` here (we're iterating by reference);
        // dereference inside the matches! so the patterns match by value.
        let should_touch = if target_state == RadioState::Off {
            true
        } else {
            matches!(*kind, RadioKind::WiFi | RadioKind::Bluetooth)
        };
        if !should_touch || *state == target_state {
            continue;
        }
        // Errors on a single radio shouldn't abort the whole batch —
        // a missing or permission-denied radio still leaves the others
        // togglable. We just skip and continue.
        let _ = radio
            .SetStateAsync(target_state)
            .and_then(|op| op.get());
        touched += 1;
    }

    let label = if target_state == RadioState::Off {
        "Airplane mode ON"
    } else {
        "Airplane mode OFF"
    };
    if touched == 0 {
        Ok(Some(format!("{label} — no radios changed.")))
    } else {
        Ok(Some(format!("{label} ({touched} radio{} toggled).", if touched == 1 { "" } else { "s" })))
    }
}

#[cfg(not(windows))]
fn toggle_airplane_mode() -> Result<Option<String>, String> {
    Err("Airplane mode is only wired on Windows".to_string())
}

/// Inner helper that toggles a single radio of the given kind. Used by
/// `toggle_wifi` and `toggle_bluetooth`. `label` is the human name used
/// in error / status messages.
#[cfg(windows)]
fn toggle_radio(
    target_kind: windows::Devices::Radios::RadioKind,
    label: &str,
) -> Result<Option<String>, String> {
    use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioState};

    // Tauri command threads aren't COM-initialized for WinRT by default;
    // without this, Radio::RequestAccessAsync() returns an HRESULT error
    // (or hangs) and the user sees nothing. Returns S_FALSE if the thread
    // was already initialized — safe to call defensively. We don't pair
    // with RoUninitialize because the thread is reused by Tauri for
    // subsequent commands; un-initializing would break those.
    ensure_winrt_apartment();

    let access = Radio::RequestAccessAsync()
        .map_err(|e| format!("Radio access request failed: {e}"))?
        .get()
        .map_err(|e| format!("Radio access wait failed: {e}"))?;
    if access != RadioAccessStatus::Allowed {
        return Err(radio_denied_message(access));
    }
    let radios = Radio::GetRadiosAsync()
        .map_err(|e| format!("GetRadiosAsync failed: {e}"))?
        .get()
        .map_err(|e| format!("GetRadiosAsync wait failed: {e}"))?;
    let count = radios.Size().map_err(|e| e.to_string())?;

    let mut target_radio: Option<Radio> = None;
    for i in 0..count {
        let r = radios.GetAt(i).map_err(|e| e.to_string())?;
        let kind = r.Kind().map_err(|e| e.to_string())?;
        if kind == target_kind {
            target_radio = Some(r);
            break;
        }
    }
    let radio = target_radio
        .ok_or_else(|| format!("No {label} radio found on this machine."))?;

    let current = radio.State().map_err(|e| e.to_string())?;
    let new_state = if current == RadioState::On {
        RadioState::Off
    } else {
        RadioState::On
    };
    let result = radio
        .SetStateAsync(new_state)
        .map_err(|e| format!("SetStateAsync failed: {e}"))?
        .get()
        .map_err(|e| format!("SetStateAsync wait failed: {e}"))?;
    if result != RadioAccessStatus::Allowed {
        return Err(format!("{label} state change was not allowed."));
    }

    let on = new_state == RadioState::On;
    Ok(Some(format!(
        "{label} turned {}",
        if on { "ON" } else { "OFF" }
    )))
}

/// Initialize the WinRT apartment on this thread if not already done.
/// `RoInitialize(MULTITHREADED)` returns S_FALSE on subsequent calls
/// from a thread already initialized to a compatible apartment, and
/// RPC_E_CHANGED_MODE if a different apartment was previously set —
/// both are non-fatal for our purposes: as long as the thread has a
/// COM apartment of some kind, the WinRT APIs we use here work. We
/// deliberately do NOT call RoUninitialize because Tauri reuses
/// command threads and unpairing would break later commands.
#[cfg(windows)]
fn ensure_winrt_apartment() {
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    // SAFETY: RoInitialize is documented as safe to call multiple times
    // from the same thread; the HRESULT result variants are all
    // handle-able and none represent unsafe behavior.
    let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
}

/// Map a non-Allowed RadioAccessStatus to a user-facing message.
#[cfg(windows)]
fn radio_denied_message(status: windows::Devices::Radios::RadioAccessStatus) -> String {
    use windows::Devices::Radios::RadioAccessStatus;
    if status == RadioAccessStatus::DeniedByUser {
        "Radio access was denied. Re-enable in Windows Settings → Privacy → Radios.".to_string()
    } else if status == RadioAccessStatus::DeniedBySystem {
        "Radio access is blocked by system policy.".to_string()
    } else {
        format!("Radio access not available (status code {}).", status.0)
    }
}

/// Flush the Windows DNS resolver cache via `ipconfig /flushdns`.
#[cfg(windows)]
fn flush_dns() -> Result<Option<String>, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new("ipconfig")
        .arg("/flushdns")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Could not run ipconfig: {e}"))?;
    if output.status.success() {
        Ok(Some("DNS resolver cache flushed.".to_string()))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("ipconfig /flushdns failed: {stderr}"))
    }
}

#[cfg(not(windows))]
fn flush_dns() -> Result<Option<String>, String> {
    Err("Flush DNS is only wired on Windows".to_string())
}

/// Return the LAN IPv4 address this machine would use for outbound
/// traffic. Uses the UDP-connect trick: bind a UDP socket, "connect"
/// it to a routable address (no packet is actually sent — UDP connect
/// is purely a kernel routing-table query), and read back the local
/// address. The destination IP (8.8.8.8) is a placeholder; we pick a
/// well-known address that any default route covers. Zero network
/// traffic, zero DNS, fully local.
fn show_current_ip() -> Result<Option<String>, String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("Could not bind UDP socket: {e}"))?;
    socket
        .connect("8.8.8.8:80")
        .map_err(|e| format!("Could not resolve outbound interface: {e}"))?;
    let addr = socket
        .local_addr()
        .map_err(|e| format!("Could not read local address: {e}"))?;
    Ok(Some(format!("Local IP: {}", addr.ip())))
}

// ─── Wave 2.3 — audio + window management (keystroke synthesis) ───────
//
// All audio / window actions synthesize the OS-published shortcut via
// SendInput. Far simpler than Core Audio (which needs IMMDeviceEnumerator
// + per-endpoint IAudioEndpointVolume) and IVirtualDesktopManagerInternal
// (which is undocumented and OS-version-pinned). The trade-off: we can
// only step volume in OS-defined increments (typically 2%), can't read
// current state, and can't set a specific volume %. For "raise/lower
// volume", "mute", "snap window", "switch virtual desktop", and "show
// desktop" — synthesis is the right tool.

/// Synthesize one or more virtual-key presses via SendInput. Each entry
/// in `keys` is pressed in order, then released in reverse — so passing
/// `[Win, Left]` produces Win-down, Left-down, Left-up, Win-up — the
/// canonical "Win+Left" shortcut Explorer understands. Returns an error
/// if SendInput rejects the batch (rare; usually only at lock screen).
#[cfg(windows)]
fn send_key_combo(keys: &[u16]) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    if keys.is_empty() {
        return Ok(());
    }
    let count = keys.len();
    let mut events: Vec<INPUT> = Vec::with_capacity(count * 2);
    // Press order: in-order.
    for &vk in keys {
        events.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }
    // Release order: reverse — so modifiers (Win, Ctrl, Shift) release
    // AFTER the key they modify, exactly like a human pressing combos.
    for &vk in keys.iter().rev() {
        events.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }
    // SAFETY: SendInput takes a pointer + count; the INPUT structs are
    // fully initialized above. The function copies the data internally
    // before returning, so the local Vec can drop normally afterward.
    let sent = unsafe {
        SendInput(&events, std::mem::size_of::<INPUT>() as i32)
    };
    if sent != events.len() as u32 {
        return Err(format!(
            "SendInput accepted {sent} of {} events (blocked by another window?)",
            events.len()
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn audio_action(id: &str) -> Result<Option<String>, String> {
    // Windows virtual-key codes for media keys — defined in
    // WinUser.h, exposed via VIRTUAL_KEY wrappers but the numeric
    // codes are stable since Win2000:
    //   VK_VOLUME_MUTE = 0xAD
    //   VK_VOLUME_DOWN = 0xAE
    //   VK_VOLUME_UP   = 0xAF
    let (vk, label): (u16, &str) = match id {
        "audio-mute" => (0xAD, "Toggled mute"),
        "volume-down" => (0xAE, "Volume down"),
        "volume-up" => (0xAF, "Volume up"),
        _ => return Err(format!("Unknown audio action: {id}")),
    };
    send_key_combo(&[vk])?;
    Ok(Some(label.to_string()))
}

#[cfg(not(windows))]
fn audio_action(_id: &str) -> Result<Option<String>, String> {
    Err("Audio commands are Windows-only for now".to_string())
}

#[cfg(windows)]
fn window_action(id: &str) -> Result<Option<String>, String> {
    // Virtual-key codes:
    //   VK_LWIN   = 0x5B
    //   VK_LCONTROL = 0xA2
    //   VK_LSHIFT = 0xA0
    //   VK_TAB    = 0x09
    //   VK_LEFT   = 0x25
    //   VK_UP     = 0x26
    //   VK_RIGHT  = 0x27
    //   VK_DOWN   = 0x28
    //   VK_F4     = 0x73
    //   D = 0x44, M = 0x4D
    const VK_LWIN: u16 = 0x5B;
    const VK_LCONTROL: u16 = 0xA2;
    const VK_LSHIFT: u16 = 0xA0;
    const VK_TAB: u16 = 0x09;
    const VK_LEFT: u16 = 0x25;
    const VK_UP: u16 = 0x26;
    const VK_RIGHT: u16 = 0x27;
    const VK_DOWN: u16 = 0x28;
    const VK_F4: u16 = 0x73;
    const VK_D: u16 = 0x44;
    const VK_M: u16 = 0x4D;

    let (combo, label): (&[u16], &str) = match id {
        "show-desktop" => (&[VK_LWIN, VK_D], "Showing desktop"),
        "minimize-all" => (&[VK_LWIN, VK_M], "Minimized all windows"),
        "restore-all" => (&[VK_LWIN, VK_LSHIFT, VK_M], "Restored windows"),
        "win-maximize" => (&[VK_LWIN, VK_UP], "Maximized active window"),
        "win-minimize" => (&[VK_LWIN, VK_DOWN], "Minimized active window"),
        "win-snap-left" => (&[VK_LWIN, VK_LEFT], "Snapped window left"),
        "win-snap-right" => (&[VK_LWIN, VK_RIGHT], "Snapped window right"),
        "vd-next" => (&[VK_LWIN, VK_LCONTROL, VK_RIGHT], "Switched to next desktop"),
        "vd-prev" => (&[VK_LWIN, VK_LCONTROL, VK_LEFT], "Switched to previous desktop"),
        "vd-new" => (&[VK_LWIN, VK_LCONTROL, VK_D], "Created new virtual desktop"),
        "vd-close" => (&[VK_LWIN, VK_LCONTROL, VK_F4], "Closed virtual desktop"),
        "task-view" => (&[VK_LWIN, VK_TAB], "Opened Task View"),
        _ => return Err(format!("Unknown window action: {id}")),
    };
    send_key_combo(combo)?;
    Ok(Some(label.to_string()))
}

#[cfg(not(windows))]
fn window_action(_id: &str) -> Result<Option<String>, String> {
    Err("Window-management commands are Windows-only for now".to_string())
}

// ─── Wave 2.4 — file system + privacy launchers ───────────────────────

/// Open a Windows shell URI (shell:Downloads, shell:RecycleBinFolder, etc.)
/// via the default file-explorer association. Works for every known-
/// folder shortcut without needing SHGetKnownFolderPath plumbing.
fn open_known_path(target: &str) -> Result<Option<String>, String> {
    open::that(target).map_err(|e| format!("Could not open '{target}': {e}"))?;
    // Cosmetic label — strip the "shell:" prefix so the toast reads as a
    // human folder name. For the Recycle Bin variant we hand-roll the
    // friendly name since "RecycleBinFolder" is ugly.
    let label = if target.eq_ignore_ascii_case("shell:RecycleBinFolder") {
        "Recycle Bin".to_string()
    } else {
        target.strip_prefix("shell:").unwrap_or(target).to_string()
    };
    Ok(Some(format!("Opened {label}")))
}

/// Open an `ms-settings:` deep-link with a fallback to the root Settings
/// app if the specific page isn't registered. Returns a clean status
/// label rather than the URI string.
fn open_ms_settings(uri: &str) -> Result<Option<String>, String> {
    open::that(uri)
        .or_else(|_| open::that("ms-settings:"))
        .map_err(|e| format!("Could not open '{uri}': {e}"))?;
    let label = match uri {
        "ms-settings:privacy-webcam" => "Camera privacy settings",
        "ms-settings:privacy-location" => "Location privacy settings",
        _ => "Settings",
    };
    Ok(Some(format!("Opened {label}")))
}

/// Empty the Recycle Bin entirely (all drives). Queries the size first
/// so the success toast tells the user how much space they reclaimed.
/// Destructive — gated by `is_destructive_system_command` so the
/// frontend confirm modal fires before this runs.
#[cfg(windows)]
fn empty_recycle_bin() -> Result<Option<String>, String> {
    use windows::Win32::UI::Shell::{
        SHEmptyRecycleBinW, SHQueryRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI,
        SHERB_NOSOUND, SHQUERYRBINFO,
    };
    // Size + count for the post-empty toast. None means "all drives".
    let mut info = SHQUERYRBINFO {
        cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32,
        i64Size: 0,
        i64NumItems: 0,
    };
    // SAFETY: SHQueryRecycleBinW takes an optional path and an out-pointer
    // to a fully-initialized SHQUERYRBINFO; the i64 fields are populated
    // by the call. A failure HRESULT just means we lose the size hint —
    // not fatal.
    let bytes_reclaimed = unsafe { SHQueryRecycleBinW(None, &mut info) }
        .ok()
        .map(|_| (info.i64Size as u64, info.i64NumItems as u64));

    // Empty the bin. SHERB_NOCONFIRMATION suppresses the Windows shell's
    // own confirmation dialog — we've already shown ours upstream, and
    // showing two in a row is bad UX. NOPROGRESSUI / NOSOUND keep the
    // emptying silent (the toast is the feedback).
    let flags = SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND;
    // SAFETY: SHEmptyRecycleBinW takes an HWND (optional), a path
    // (optional — None means all drives), and a flags bitmask. No
    // pointers are aliased and the function copies what it needs.
    let hr = unsafe { SHEmptyRecycleBinW(None, None, flags) };
    if hr.is_err() {
        return Err(format!("Empty Recycle Bin failed: {hr:?}"));
    }
    let label = match bytes_reclaimed {
        Some((0, 0)) => "Recycle Bin was already empty".to_string(),
        Some((bytes, items)) => format!(
            "Emptied Recycle Bin ({} item{}, ~{})",
            items,
            if items == 1 { "" } else { "s" },
            format_bytes(bytes),
        ),
        None => "Emptied Recycle Bin".to_string(),
    };
    Ok(Some(label))
}

#[cfg(not(windows))]
fn empty_recycle_bin() -> Result<Option<String>, String> {
    Err("Recycle Bin is only wired on Windows".to_string())
}

/// Toggle a DWORD value under HKCU\…\Explorer\Advanced and broadcast the
/// shell-association-changed notification so already-open Explorer
/// windows pick up the change. Used for `Hidden` (show=1 / hide=2) and
/// `HideFileExt` (show=0 / hide=1) — the two semantically-opposite
/// values are explicit args so callers don't have to encode the
/// convention.
///
/// We shell out to `reg.exe` (with CREATE_NO_WINDOW so no console
/// flash) rather than using the windows crate's `Win32_System_Registry`
/// feature — it's one fewer feature flag on the windows dep, the perf
/// cost is ~30 ms which is irrelevant for a one-shot user command, and
/// the registry shape stays trivially auditable.
#[cfg(windows)]
fn toggle_explorer_advanced(
    value_name: &str,
    show_value: u32,
    hide_value: u32,
    label_subject: &str,
) -> Result<Option<String>, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const REG_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

    // Read current value — output looks like:
    //   HKEY_CURRENT_USER\…\Advanced
    //       Hidden    REG_DWORD    0x1
    let read = Command::new("reg")
        .args(["query", REG_KEY, "/v", value_name])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("reg query failed: {e}"))?;
    let stdout = String::from_utf8_lossy(&read.stdout);
    let current = parse_reg_dword_hex(&stdout).unwrap_or(hide_value);

    let next = if current == show_value {
        hide_value
    } else {
        show_value
    };

    // Write the new value. /f = force overwrite without prompting.
    let write = Command::new("reg")
        .args([
            "add",
            REG_KEY,
            "/v",
            value_name,
            "/t",
            "REG_DWORD",
            "/d",
            &next.to_string(),
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("reg add failed: {e}"))?;
    if !write.status.success() {
        return Err(format!(
            "reg add failed: {}",
            String::from_utf8_lossy(&write.stderr)
        ));
    }

    // Tell the shell associations changed. Open Explorer windows
    // re-check ShellState and apply the new value without restart.
    // SAFETY: SHChangeNotify takes an event id, flags, and two
    // optional PIDL pointers. We pass None for both so the call is
    // a pure broadcast with no allocations of our own.
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }

    let showing = next == show_value;
    Ok(Some(format!(
        "{label_subject}: {}",
        if showing { "showing" } else { "hidden" }
    )))
}

#[cfg(not(windows))]
fn toggle_explorer_advanced(
    _value_name: &str,
    _show_value: u32,
    _hide_value: u32,
    _label_subject: &str,
) -> Result<Option<String>, String> {
    Err("Explorer toggles are Windows-only for now".to_string())
}

/// Parse the hex DWORD value off a `reg query` output. Returns None
/// when the output doesn't contain a recognized "0x…" token (key
/// missing, malformed output, etc.) so the caller can fall back to a
/// safe default.
fn parse_reg_dword_hex(text: &str) -> Option<u32> {
    for line in text.lines() {
        if let Some(idx) = line.find("0x") {
            let rest = &line[idx + 2..];
            let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
            if !hex.is_empty() {
                return u32::from_str_radix(&hex, 16).ok();
            }
        }
    }
    None
}

/// Pretty-print a byte count for user-facing toasts. Same conventions
/// as the file-search status string: KB / MB / GB binary multiples,
/// one decimal place, no trailing zeros.
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// True iff `where.exe <name>` exits 0 — i.e., Windows can locate the
/// executable through PATH or the App Paths registry.
#[cfg(windows)]
fn where_resolves(name: &str) -> bool {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    // CREATE_NO_WINDOW (0x08000000) prevents `where`'s console window
    // from flashing on screen.
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    Command::new("where")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Open an external URL in the user's default browser. Only `http`/`https`/
/// `file`/`ftp` schemes are allowed — explicitly *no* `javascript:` or other
/// app-scheme bridging that could be exploited.
///
/// This is the only place KeepItLocal hands a query string to the OS shell for
/// browser navigation, and it only runs when the user explicitly activated a
/// detected URL row (never on raw keystrokes).
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    let lower = url.trim().to_lowercase();
    const ALLOWED: &[&str] = &["http://", "https://", "file://", "ftp://", "ftps://"];
    if !ALLOWED.iter().any(|scheme| lower.starts_with(scheme)) {
        return Err(format!("URL scheme not allowed: {url}"));
    }
    open::that(url.trim()).map_err(|e| format!("Cannot open URL: {e}"))?;
    Ok(())
}

// ─── URL detection ────────────────────────────────────────────────────────

/// Detect whether the query looks like a URL. Three accepted shapes:
///   1. Explicit scheme: `https://github.com/foo`, `file:///C:/path`
///   2. Localhost / IPv4 with optional port + path
///   3. Bare domain (`github.com`) or domain+path (`github.com/foo`) where the
///      TLD is in our known list OR a path/query is present
///
/// Returns `OpenUrl` with the normalized URL on match. Conservative on edge
/// cases — we'd rather miss a URL than wrongly trigger on a filename like
/// `foo.txt`. The known-TLD requirement keeps random "X.txt" / "doc.pdf"
/// from looking like domains.
fn try_url_detection(query: &str) -> Option<QuickAction> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    // URLs don't contain whitespace. Stop early before any other parsing.
    if q.chars().any(char::is_whitespace) {
        return None;
    }

    // Case 1: Explicit scheme — hand it back exactly as typed.
    let lower = q.to_lowercase();
    for scheme in &["http://", "https://", "ftp://", "ftps://", "file://"] {
        if lower.starts_with(scheme) {
            return Some(QuickAction::OpenUrl {
                url: q.to_string(),
                display: q.to_string(),
            });
        }
    }

    // Case 2: localhost or IPv4 — prefix http:// since dev tools usually
    // serve over plain HTTP locally and HTTPS often fails on self-signed certs.
    if looks_like_localhost(q) || looks_like_ipv4(q) {
        return Some(QuickAction::OpenUrl {
            url: format!("http://{q}"),
            display: q.to_string(),
        });
    }

    // Case 3: Bare domain or domain+path — default to https://.
    if looks_like_domain(q) {
        return Some(QuickAction::OpenUrl {
            url: format!("https://{q}"),
            display: q.to_string(),
        });
    }

    None
}

fn looks_like_localhost(s: &str) -> bool {
    let lower = s.to_lowercase();
    if !lower.starts_with("localhost") {
        return false;
    }
    // Must be exactly "localhost" or followed by `:` (port) / `/` (path).
    let after = &lower["localhost".len()..];
    after.is_empty() || after.starts_with(':') || after.starts_with('/')
}

fn looks_like_ipv4(s: &str) -> bool {
    // Slice off any path/query/fragment, then optional port.
    let main = s.split(['/', '?', '#']).next().unwrap_or(s);
    let host = main.split(':').next().unwrap_or(main);
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && part.len() <= 3
            && part.chars().all(|c| c.is_ascii_digit())
            && part.parse::<u8>().is_ok()
    })
}

fn looks_like_domain(s: &str) -> bool {
    // Pull out just the host portion (no port, no path).
    let host_with_port = s.split(['/', '?', '#']).next().unwrap_or(s);
    let host = host_with_port.split(':').next().unwrap_or(host_with_port);

    if !host.contains('.') {
        return false;
    }
    // Reject anything ending with `.` (e.g. "foo.txt." would slip through otherwise)
    if host.starts_with('.') || host.ends_with('.') {
        return false;
    }

    // Each label between dots must be a valid DNS label.
    for label in host.split('.') {
        if label.is_empty() || label.len() > 63 {
            return false;
        }
        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return false;
        }
    }

    let tld = host.rsplit('.').next().unwrap_or("");
    // TLDs are always alphabetic. Length cap of 24 covers everything real
    // (longest legitimate TLD is ~13 chars).
    if tld.len() < 2 || tld.len() > 24 || !tld.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }

    // If the user typed a path or query, treat it as URL regardless of TLD —
    // "myserver.local/admin" is clearly a URL even with an unknown TLD.
    if s.contains('/') || s.contains('?') {
        return true;
    }

    // For bare hosts (no path), require a known TLD to avoid false positives
    // on things like "report.docx" or "manual.pdf".
    is_known_tld(&tld.to_lowercase())
}

#[cfg(test)]
mod url_tests {
    use super::*;

    fn detected(query: &str) -> Option<String> {
        match try_url_detection(query) {
            Some(QuickAction::OpenUrl { url, .. }) => Some(url),
            _ => None,
        }
    }

    #[test]
    fn detects_https_url() {
        assert_eq!(
            detected("https://github.com/foo/bar"),
            Some("https://github.com/foo/bar".to_string())
        );
    }

    #[test]
    fn detects_bare_domain() {
        assert_eq!(detected("github.com"), Some("https://github.com".to_string()));
    }

    #[test]
    fn detects_domain_with_path() {
        assert_eq!(
            detected("github.com/foo/bar"),
            Some("https://github.com/foo/bar".to_string())
        );
    }

    #[test]
    fn detects_localhost_with_port() {
        assert_eq!(
            detected("localhost:3000"),
            Some("http://localhost:3000".to_string())
        );
    }

    #[test]
    fn detects_localhost_with_path() {
        assert_eq!(
            detected("localhost:3000/api/foo"),
            Some("http://localhost:3000/api/foo".to_string())
        );
    }

    #[test]
    fn detects_ipv4_with_port() {
        assert_eq!(
            detected("192.168.1.1:8080"),
            Some("http://192.168.1.1:8080".to_string())
        );
    }

    #[test]
    fn rejects_filename_with_extension() {
        // "report.docx" has a dot but `docx` isn't a known TLD and there's
        // no path → NOT a URL. Same for .pdf, .txt etc.
        assert!(detected("report.docx").is_none());
        assert!(detected("manual.pdf").is_none());
        assert!(detected("notes.txt").is_none());
    }

    #[test]
    fn rejects_query_with_spaces() {
        assert!(detected("hello world").is_none());
        assert!(detected("github.com is cool").is_none());
    }

    #[test]
    fn rejects_bare_word() {
        assert!(detected("github").is_none());
        assert!(detected("chrome").is_none());
    }

    #[test]
    fn accepts_unknown_tld_when_path_present() {
        // .local isn't in the known-TLD list, but with /admin path it's
        // clearly a URL (internal/dev servers).
        assert_eq!(
            detected("myserver.local/admin"),
            Some("https://myserver.local/admin".to_string())
        );
    }
}

/// Top ~100 TLDs by usage — covers >99% of real domains people type. Bare-
/// domain detection (`github.com` with no path) requires this list so we
/// don't false-positive on `notes.txt` / `report.pdf` / etc.
fn is_known_tld(tld: &str) -> bool {
    const TLDS: &[&str] = &[
        // Generic top-level
        "com", "org", "net", "io", "dev", "app", "co", "edu", "gov", "info",
        "biz", "name", "pro", "mobi", "xxx",
        // Modern brand-friendly
        "ai", "me", "ly", "tv", "fm", "cc", "to", "so", "is",
        "cloud", "site", "online", "store", "shop", "tech", "blog", "news",
        "xyz", "wiki", "team", "build", "deals", "ninja", "studio", "design",
        "agency", "academy", "engineer", "media", "events", "global", "world",
        // Country (top 60-ish)
        "us", "uk", "de", "fr", "jp", "ca", "au", "nl", "ru", "br",
        "in", "cn", "kr", "it", "es", "pl", "se", "no", "fi", "dk",
        "ch", "at", "be", "ie", "pt", "gr", "cz", "tr", "mx", "ar",
        "cl", "il", "za", "sg", "hk", "tw", "th", "id", "ph", "vn",
        "my", "nz", "ng", "ke", "eg", "ae", "sa", "ge", "ua", "by",
        "kz", "uz", "rs", "hr", "si", "sk", "ro", "bg", "lv", "lt",
        "ee", "is", "lu", "mt",
    ];
    TLDS.contains(&tld)
}

// ─── Web search bangs ─────────────────────────────────────────────────────

/// A single bang definition. The user types one of `aliases` followed by a
/// space and a query (or `?query` / `r/foo` for the special-cased prefixes).
/// The `url_template` has a single `{}` placeholder that we replace with the
/// URL-encoded query.
struct BangDef {
    aliases: &'static [&'static str],
    name: &'static str,
    url_template: &'static str,
}

/// Curated bang shortcuts. Order matters only for display; matching is by
/// exact alias. To add a new shortcut, just append a row — no other code
/// changes needed.
const BANG_DEFS: &[BangDef] = &[
    // ─── General-purpose search engines ───
    BangDef { aliases: &["g", "google"], name: "Google", url_template: "https://www.google.com/search?q={}" },
    BangDef { aliases: &["ddg", "?"], name: "DuckDuckGo", url_template: "https://duckduckgo.com/?q={}" },
    BangDef { aliases: &["bing"], name: "Bing", url_template: "https://www.bing.com/search?q={}" },
    BangDef { aliases: &["brave"], name: "Brave Search", url_template: "https://search.brave.com/search?q={}" },
    BangDef { aliases: &["kagi"], name: "Kagi", url_template: "https://kagi.com/search?q={}" },
    BangDef { aliases: &["sp", "startpage"], name: "Startpage", url_template: "https://www.startpage.com/sp/search?q={}" },

    // ─── Reference & general knowledge ───
    BangDef { aliases: &["w", "wiki"], name: "Wikipedia", url_template: "https://en.wikipedia.org/w/index.php?search={}" },
    BangDef { aliases: &["tr", "translate"], name: "Google Translate", url_template: "https://translate.google.com/?text={}" },
    BangDef { aliases: &["maps"], name: "Google Maps", url_template: "https://www.google.com/maps/search/{}" },
    BangDef { aliases: &["img", "images"], name: "Google Images", url_template: "https://www.google.com/search?tbm=isch&q={}" },

    // ─── Developer ───
    BangDef { aliases: &["gh", "github"], name: "GitHub", url_template: "https://github.com/search?q={}" },
    BangDef { aliases: &["so"], name: "Stack Overflow", url_template: "https://stackoverflow.com/search?q={}" },
    BangDef { aliases: &["mdn"], name: "MDN", url_template: "https://developer.mozilla.org/en-US/search?q={}" },
    BangDef { aliases: &["crates"], name: "crates.io", url_template: "https://crates.io/search?q={}" },
    BangDef { aliases: &["docs", "docsrs"], name: "docs.rs", url_template: "https://docs.rs/?q={}" },
    BangDef { aliases: &["npm"], name: "npm", url_template: "https://www.npmjs.com/search?q={}" },
    BangDef { aliases: &["pypi"], name: "PyPI", url_template: "https://pypi.org/search/?q={}" },

    // ─── Media / shopping ───
    BangDef { aliases: &["yt", "youtube"], name: "YouTube", url_template: "https://www.youtube.com/results?search_query={}" },
    BangDef { aliases: &["amz", "amazon"], name: "Amazon", url_template: "https://www.amazon.com/s?k={}" },

    // ─── Reddit subreddit (special — uses path not query string) ───
    BangDef { aliases: &["r/"], name: "Reddit", url_template: "https://www.reddit.com/r/{}" },
];

/// Detect whether the query begins with one of the registered bang prefixes.
///
/// Two parsing styles:
///   1. Space-separated: `<bang> <query>` (e.g. `g chrome download`)
///   2. Prefix-attached for single-char bangs `?` and `r/`:
///      - `?meaning of life` → DuckDuckGo
///      - `r/rust`           → Reddit subreddit
///
/// Returns None when web search is disabled, the bang isn't recognized, or
/// the query portion is empty.
fn try_bang_search(query: &str, web_search_enabled: bool) -> Option<QuickAction> {
    if !web_search_enabled {
        return None;
    }
    let (bang, rest) = split_bang_and_query(query)?;
    let def = BANG_DEFS
        .iter()
        .find(|d| d.aliases.iter().any(|a| *a == bang.as_str()))?;
    let encoded = urlencoding::encode(&rest).to_string();
    let url = def.url_template.replace("{}", &encoded);
    Some(QuickAction::WebSearch {
        provider: def.name.to_string(),
        url,
        query: rest,
    })
}

/// Split a query into (bang, rest). Handles the two prefix styles described
/// above. Returns None if no recognized shape applies or if the query portion
/// is empty.
fn split_bang_and_query(input: &str) -> Option<(String, String)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Prefix style 1: `?query` (DuckDuckGo) — single-char bang glued to query.
    if let Some(rest) = trimmed.strip_prefix('?') {
        let q = rest.trim();
        if q.is_empty() {
            return None;
        }
        return Some(("?".to_string(), q.to_string()));
    }

    // Prefix style 2: `r/subreddit` — Reddit subreddit shortcut.
    if let Some(rest) = trimmed.strip_prefix("r/") {
        let q = rest.trim();
        if q.is_empty() {
            return None;
        }
        return Some(("r/".to_string(), q.to_string()));
    }

    // General style: `<bang> <rest>` with whitespace separator.
    let (bang, rest) = trimmed.split_once(char::is_whitespace)?;
    let rest_trimmed = rest.trim();
    if rest_trimmed.is_empty() {
        return None;
    }
    Some((bang.to_lowercase(), rest_trimmed.to_string()))
}

#[cfg(test)]
mod bang_tests {
    use super::*;

    fn run(query: &str, enabled: bool) -> Option<(String, String, String)> {
        match try_bang_search(query, enabled) {
            Some(QuickAction::WebSearch { provider, url, query }) => Some((provider, url, query)),
            _ => None,
        }
    }

    #[test]
    fn disabled_returns_none() {
        assert!(run("g chrome", false).is_none());
        assert!(run("? hello", false).is_none());
    }

    #[test]
    fn google_bang_with_space() {
        let result = run("g chrome download", true).expect("should match");
        assert_eq!(result.0, "Google");
        assert!(result.1.contains("q=chrome%20download") || result.1.contains("q=chrome+download"));
        assert_eq!(result.2, "chrome download");
    }

    #[test]
    fn ddg_question_mark_no_space() {
        let result = run("?meaning of life", true).expect("should match");
        assert_eq!(result.0, "DuckDuckGo");
        assert_eq!(result.2, "meaning of life");
    }

    #[test]
    fn reddit_slash_subreddit() {
        let result = run("r/rust", true).expect("should match");
        assert_eq!(result.0, "Reddit");
        assert!(result.1.ends_with("/r/rust"));
        assert_eq!(result.2, "rust");
    }

    #[test]
    fn github_bang_uppercase_alias() {
        // Aliases are case-insensitive (bang is lowercased in parser).
        let result = run("GH tantivy", true).expect("should match");
        assert_eq!(result.0, "GitHub");
    }

    #[test]
    fn unknown_bang_returns_none() {
        assert!(run("xyz hello", true).is_none());
    }

    #[test]
    fn empty_query_after_bang_returns_none() {
        assert!(run("g ", true).is_none());
        assert!(run("?", true).is_none());
    }

    #[test]
    fn no_space_after_bang_returns_none() {
        // "ghello" should NOT match "g" bang — bangs need word separation.
        assert!(run("ghello", true).is_none());
    }
}

// ─── Math evaluation ──────────────────────────────────────────────────────

fn try_math(query: &str) -> Option<QuickAction> {
    // Run the natural-language preprocessor first — converts patterns like
    // "15% of 240" → "(240 * 15 / 100)" so evalexpr can handle them. Returns
    // the original query unchanged if no pattern matched.
    let rewritten = rewrite_percent_expression(query);

    // Require either an operator or that the preprocessor rewrote something —
    // otherwise bare numbers ("2024") and single words would all be calculator
    // hits, hijacking normal searches.
    let has_operator = rewritten
        .chars()
        .any(|c| matches!(c, '+' | '-' | '*' | '/' | '%' | '^'));
    let was_rewritten = rewritten != query;
    if !has_operator && !was_rewritten {
        return None;
    }
    // Must also contain at least one digit (filters out things like "a/b/c").
    if !rewritten.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }

    let cleaned = rewritten.replace('×', "*").replace('÷', "/");
    let value = evalexpr::eval(&cleaned).ok()?;
    let display = match value {
        evalexpr::Value::Int(i) => i.to_string(),
        evalexpr::Value::Float(f) => {
            // Strip trailing zeros and the decimal point if integer-valued.
            if f.is_nan() || f.is_infinite() {
                return None;
            }
            format_float(f)
        }
        _ => return None,
    };

    Some(QuickAction::Calculator {
        expression: query.to_string(),
        result: display,
    })
}

/// Natural-language percentage preprocessor. Recognized patterns:
///
///   "15% of 240"           → "(240 * 15 / 100)"        = 36
///   "15 percent of 240"    → same
///   "240 + 15%"            → "(240 * (1 + 15 / 100))"  = 276   (tax / markup)
///   "240 - 15%"            → "(240 * (1 - 15 / 100))"  = 204   (discount)
///   "240 plus 15%"         → same as "+"
///   "240 minus 15%"        → same as "-"
///
/// Returns the input string unchanged when no pattern matches, so downstream
/// math evaluation still gets a shot at plain expressions like "2 + 3 * 4".
///
/// Only operates on a single recognized pattern per query — chaining like
/// "15% of 240 + 10" is intentionally not supported to keep parsing simple
/// and predictable. Users wanting complex math can write it the math way.
fn rewrite_percent_expression(query: &str) -> String {
    let q = query.trim();
    let lower = q.to_lowercase();

    // Pattern 1: "<pct>% of <num>"  or  "<pct> percent of <num>"
    for sep in [" % of ", "% of ", " percent of "] {
        if let Some(idx) = lower.find(sep) {
            let left = lower[..idx].trim();
            let right = lower[idx + sep.len()..].trim();
            if let (Some(pct), Some(base)) = (parse_simple_number(left), parse_simple_number(right))
            {
                return format!("({base} * {pct} / 100)");
            }
        }
    }

    // Pattern 2: "<base> +/- <pct>%"  or  "<base> plus/minus <pct>%"
    // Walk through possible separators, look for trailing "%". Spaced variants
    // come first so "240 - 15%" is caught before the compact "-" rule, which
    // matters when the user mixes spaces and signs ("240 - 15%" wouldn't want
    // to be misread by a leading-`-` search inside the compact branch).
    let plus_variants = [" + ", " plus ", "+"];
    let minus_variants = [" - ", " minus ", "-"];

    for sep in plus_variants {
        if let Some(idx) = lower.find(sep) {
            let left = lower[..idx].trim();
            let right = lower[idx + sep.len()..].trim();
            if let Some((pct, true)) = parse_trailing_percent(right) {
                if let Some(base) = parse_simple_number(left) {
                    return format!("({base} * (1 + {pct} / 100))");
                }
            }
        }
    }
    for sep in minus_variants {
        if let Some(idx) = lower.find(sep) {
            let left = lower[..idx].trim();
            let right = lower[idx + sep.len()..].trim();
            if let Some((pct, true)) = parse_trailing_percent(right) {
                if let Some(base) = parse_simple_number(left) {
                    return format!("({base} * (1 - {pct} / 100))");
                }
            }
        }
    }

    q.to_string()
}

/// Parse a plain decimal number (no operators, no units). Returns None if the
/// string contains anything other than digits, decimal point, or a leading minus.
fn parse_simple_number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // Reject anything with non-numeric characters (other than . and leading -)
    // so we don't accidentally parse "240 + 15" as a number.
    let valid = s.chars().enumerate().all(|(i, c)| {
        c.is_ascii_digit() || c == '.' || (i == 0 && c == '-')
    });
    if !valid {
        return None;
    }
    s.parse::<f64>().ok()
}

/// Parse a string ending in `%` into (value, true). Returns None if no `%` or
/// the leading part isn't a clean number.
fn parse_trailing_percent(s: &str) -> Option<(f64, bool)> {
    let s = s.trim();
    let stripped = s.strip_suffix('%')?.trim();
    parse_simple_number(stripped).map(|v| (v, true))
}

/// Render a float compactly: drops trailing zeros, no scientific notation for
/// human-readable ranges.
fn format_float(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e16 {
        return format!("{}", f as i64);
    }
    // Up to 6 fractional digits, then trim trailing zeros.
    let s = format!("{:.6}", f);
    let trimmed = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed
    }
}

// ─── Unit conversion ──────────────────────────────────────────────────────

#[derive(Clone, Copy)]
enum UnitCategory {
    Length,
    Mass,
    DataBinary,
    Time,
    Temperature,
}

struct Unit {
    aliases: &'static [&'static str],
    category: UnitCategory,
    /// For non-temperature units: how many base units one of these equals.
    /// Base units: Length=meter, Mass=gram, DataBinary=byte, Time=second.
    to_base: f64,
    /// Display name when formatting the result.
    display: &'static str,
}

const UNITS: &[Unit] = &[
    // Length (base = meter)
    Unit { aliases: &["m", "meter", "meters", "metre", "metres"], category: UnitCategory::Length, to_base: 1.0, display: "m" },
    Unit { aliases: &["km", "kilometer", "kilometers", "kilometre", "kilometres"], category: UnitCategory::Length, to_base: 1000.0, display: "km" },
    Unit { aliases: &["cm", "centimeter", "centimeters", "centimetre", "centimetres"], category: UnitCategory::Length, to_base: 0.01, display: "cm" },
    Unit { aliases: &["mm", "millimeter", "millimeters", "millimetre", "millimetres"], category: UnitCategory::Length, to_base: 0.001, display: "mm" },
    Unit { aliases: &["in", "inch", "inches"], category: UnitCategory::Length, to_base: 0.0254, display: "in" },
    Unit { aliases: &["ft", "foot", "feet"], category: UnitCategory::Length, to_base: 0.3048, display: "ft" },
    Unit { aliases: &["yd", "yard", "yards"], category: UnitCategory::Length, to_base: 0.9144, display: "yd" },
    Unit { aliases: &["mi", "mile", "miles"], category: UnitCategory::Length, to_base: 1609.344, display: "mi" },

    // Mass (base = gram)
    Unit { aliases: &["g", "gram", "grams"], category: UnitCategory::Mass, to_base: 1.0, display: "g" },
    Unit { aliases: &["kg", "kilogram", "kilograms"], category: UnitCategory::Mass, to_base: 1000.0, display: "kg" },
    Unit { aliases: &["mg", "milligram", "milligrams"], category: UnitCategory::Mass, to_base: 0.001, display: "mg" },
    Unit { aliases: &["t", "ton", "tons", "tonne", "tonnes"], category: UnitCategory::Mass, to_base: 1_000_000.0, display: "t" },
    Unit { aliases: &["oz", "ounce", "ounces"], category: UnitCategory::Mass, to_base: 28.349523125, display: "oz" },
    Unit { aliases: &["lb", "lbs", "pound", "pounds"], category: UnitCategory::Mass, to_base: 453.59237, display: "lb" },

    // Data binary (base = byte, 1024-based)
    Unit { aliases: &["b", "byte", "bytes"], category: UnitCategory::DataBinary, to_base: 1.0, display: "B" },
    Unit { aliases: &["kb", "kib"], category: UnitCategory::DataBinary, to_base: 1024.0, display: "KB" },
    Unit { aliases: &["mb", "mib"], category: UnitCategory::DataBinary, to_base: 1024.0 * 1024.0, display: "MB" },
    Unit { aliases: &["gb", "gib"], category: UnitCategory::DataBinary, to_base: 1024.0 * 1024.0 * 1024.0, display: "GB" },
    Unit { aliases: &["tb", "tib"], category: UnitCategory::DataBinary, to_base: 1024.0 * 1024.0 * 1024.0 * 1024.0, display: "TB" },

    // Time (base = second)
    Unit { aliases: &["s", "sec", "secs", "second", "seconds"], category: UnitCategory::Time, to_base: 1.0, display: "s" },
    Unit { aliases: &["ms", "millisecond", "milliseconds"], category: UnitCategory::Time, to_base: 0.001, display: "ms" },
    Unit { aliases: &["min", "mins", "minute", "minutes"], category: UnitCategory::Time, to_base: 60.0, display: "min" },
    Unit { aliases: &["h", "hr", "hrs", "hour", "hours"], category: UnitCategory::Time, to_base: 3600.0, display: "h" },
    Unit { aliases: &["d", "day", "days"], category: UnitCategory::Time, to_base: 86400.0, display: "d" },
    Unit { aliases: &["w", "wk", "week", "weeks"], category: UnitCategory::Time, to_base: 604800.0, display: "w" },

    // Temperature — handled specially in convert(), to_base is unused.
    Unit { aliases: &["c", "celsius", "°c"], category: UnitCategory::Temperature, to_base: 0.0, display: "°C" },
    Unit { aliases: &["f", "fahrenheit", "°f"], category: UnitCategory::Temperature, to_base: 0.0, display: "°F" },
    Unit { aliases: &["k", "kelvin"], category: UnitCategory::Temperature, to_base: 0.0, display: "K" },
];

fn find_unit(token: &str) -> Option<&'static Unit> {
    let lower = token.to_lowercase();
    UNITS.iter().find(|u| u.aliases.iter().any(|alias| *alias == lower.as_str()))
}

fn try_unit_conversion(query: &str) -> Option<QuickAction> {
    // Patterns we accept:
    //   "5km to mi", "5 km in mi", "5km as mi"
    //   "32f to c", "32 °F to °C"
    //   "5gb to mb"
    let lower = query.to_lowercase();
    // Find separator " to ", " in ", or " as ".
    let (left_part, right_part) = split_on_separator(&lower)?;

    let (value, from_unit_token) = parse_value_and_unit(left_part.trim())?;
    let from_unit = find_unit(&from_unit_token)?;
    let to_unit = find_unit(right_part.trim())?;

    let converted = convert(value, from_unit, to_unit)?;
    let result = format!("{} {}", format_float(converted), to_unit.display);

    Some(QuickAction::UnitConversion {
        original: format!("{} {}", format_float(value), from_unit.display),
        result,
    })
}

/// Split "5km to mi" into ("5km", "mi"). Returns None if no separator found.
fn split_on_separator(input: &str) -> Option<(String, String)> {
    for sep in [" to ", " in ", " as ", "->"] {
        if let Some(idx) = input.find(sep) {
            let (a, b) = input.split_at(idx);
            return Some((a.to_string(), b[sep.len()..].to_string()));
        }
    }
    None
}

/// Parse "5km", "5 km", "5.5 kg" into (5.0, "km").
fn parse_value_and_unit(input: &str) -> Option<(f64, String)> {
    let input = input.trim();
    // Split at the first non-digit/non-dot/non-minus character.
    let split_at = input
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit() && *c != '.' && *c != '-' && *c != ' ')
        .map(|(i, _)| i)?;
    let (num_part, unit_part) = input.split_at(split_at);
    let value: f64 = num_part.trim().parse().ok()?;
    let unit = unit_part.trim().to_string();
    if unit.is_empty() {
        return None;
    }
    Some((value, unit))
}

fn convert(value: f64, from: &Unit, to: &Unit) -> Option<f64> {
    // Temperatures use affine transforms, not pure scaling.
    match (from.category, to.category) {
        (UnitCategory::Temperature, UnitCategory::Temperature) => {
            // Convert from→Kelvin→to.
            let kelvin = match from.aliases[0] {
                "c" => value + 273.15,
                "f" => (value - 32.0) * 5.0 / 9.0 + 273.15,
                "k" => value,
                _ => return None,
            };
            Some(match to.aliases[0] {
                "c" => kelvin - 273.15,
                "f" => (kelvin - 273.15) * 9.0 / 5.0 + 32.0,
                "k" => kelvin,
                _ => return None,
            })
        }
        (a, b) if std::mem::discriminant(&a) == std::mem::discriminant(&b) => {
            // Same category, both linear: value * from.to_base / to.to_base.
            Some(value * from.to_base / to.to_base)
        }
        _ => None, // categories don't match
    }
}
