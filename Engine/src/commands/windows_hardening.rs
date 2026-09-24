//! Privacy Hardening — the WRITE counterpart to the read-only Privacy Audit.
//!
//! A curated, REVERSIBLE catalog of Windows privacy tweaks. Two tiers:
//!   • `apply`  — user-scope (HKCU) DWORD flips this tool applies + reverts
//!     directly. NO admin, ever (the app installs per-user and never elevates).
//!     Before any write we snapshot the prior value (or "was absent") to redb,
//!     so Revert restores the EXACT original state — never a guessed default.
//!   • `guide`  — machine-wide tweaks that genuinely need admin (e.g. the
//!     DiagTrack telemetry service). We never run these; we show a step-by-step
//!     guide + a copyable elevated command so the user does it themselves.
//!
//! Security posture (mirrors `privacy_audit::open_privacy_setting`): the
//! frontend may only pass a catalog `id`. Every writable {key,value} pair is
//! hard-coded in the static `CATALOG` below — an arbitrary registry path can
//! never reach `reg.exe`. Scope is strictly cosmetic/telemetry/ads user
//! preferences (no boot, no security, no service-disable in the apply tier),
//! so the worst case of any toggle is a convenience feature turning off, fully
//! restorable via Revert.
//!
//! Mechanism: we shell out to `reg.exe` with CREATE_NO_WINDOW (no console
//! flash), matching `quick_actions::toggle_explorer_advanced`. `winreg` is kept
//! read-only by convention in this repo, so writes live here in their own
//! module rather than tainting the audit's read-only guarantee.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::commands::{local_db, preferences};

/// One HKCU DWORD a tweak flips. `hardened` is the value that means "applied".
struct RegValue {
    key: &'static str,
    name: &'static str,
    hardened: u32,
}

/// An apply-tier (HKCU, no-admin) tweak. May touch several values at once.
struct Tweak {
    id: &'static str,
    title: &'static str,
    detail: &'static str,
    category: &'static str,
    risk: &'static str, // "low" | "medium" | "high"
    values: &'static [RegValue],
}

/// A guide-tier tweak — needs admin, so we never run it; we show the steps +
/// a copyable elevated command and let the user do it with eyes open.
struct GuideTweak {
    id: &'static str,
    title: &'static str,
    detail: &'static str,
    category: &'static str,
    risk: &'static str,
    command: &'static str,
    steps: &'static [&'static str],
}

const CDM: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";

const CAT_ADS: &str = "Advertising & tracking";
const CAT_TELEMETRY: &str = "Telemetry & diagnostics";
const CAT_SEARCH: &str = "Cortana & search";
const CAT_SUGGESTED: &str = "Suggested content & ads";
const CAT_LOCKSCREEN: &str = "Lock screen & tips";
const CAT_APPS: &str = "App behavior";

/// The apply-tier catalog. CLEAN-ROOM — re-authored from well-known Windows
/// registry conventions, NOT lifted from privacy.sexy's AGPL data.
static CATALOG: &[Tweak] = &[
    Tweak {
        id: "advertising-id",
        title: "Disable the advertising ID",
        detail: "Stops apps from using a per-user advertising ID to build a cross-app interest profile for targeted ads.",
        category: CAT_ADS,
        risk: "low",
        values: &[RegValue {
            key: r"HKCU\Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
            name: "Enabled",
            hardened: 0,
        }],
    },
    Tweak {
        id: "app-launch-tracking",
        title: "Turn off app-launch tracking",
        detail: "Stops Windows from tracking which apps you open to personalise Start and search ordering.",
        category: CAT_ADS,
        risk: "low",
        values: &[RegValue {
            key: r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced",
            name: "Start_TrackProgs",
            hardened: 0,
        }],
    },
    Tweak {
        id: "language-list-optout",
        title: "Stop sharing your language list with websites",
        detail: "Prevents apps and sites from reading your full Windows language list, a common fingerprinting signal.",
        category: CAT_ADS,
        risk: "low",
        values: &[RegValue {
            key: r"HKCU\Control Panel\International\User Profile",
            name: "HttpAcceptLanguageOptOut",
            hardened: 1,
        }],
    },
    Tweak {
        id: "tailored-experiences",
        title: "Disable tailored experiences from diagnostic data",
        detail: "Stops Windows from using your diagnostic data to show tailored tips, ads and recommendations.",
        category: CAT_TELEMETRY,
        risk: "low",
        values: &[RegValue {
            key: r"HKCU\Software\Microsoft\Windows\CurrentVersion\Privacy",
            name: "TailoredExperiencesWithDiagnosticDataEnabled",
            hardened: 0,
        }],
    },
    Tweak {
        id: "feedback-frequency",
        title: "Stop feedback request prompts",
        detail: "Sets Windows to never ask for feedback, removing the periodic 'how happy are you' prompts. (User-scope only; the machine telemetry level needs admin — see the guide below.)",
        category: CAT_TELEMETRY,
        risk: "low",
        values: &[RegValue {
            key: r"HKCU\Software\Microsoft\Siuf\Rules",
            name: "NumberOfSIUFInPeriod",
            hardened: 0,
        }],
    },
    Tweak {
        id: "bing-web-search",
        title: "Disable web/Bing results in Start search",
        detail: "Keeps Start-menu search local — it no longer sends your typed query to Bing or shows web results.",
        category: CAT_SEARCH,
        risk: "medium",
        values: &[
            RegValue {
                key: r"HKCU\Software\Microsoft\Windows\CurrentVersion\Search",
                name: "BingSearchEnabled",
                hardened: 0,
            },
            RegValue {
                key: r"HKCU\Software\Microsoft\Windows\CurrentVersion\Search",
                name: "CortanaConsent",
                hardened: 0,
            },
        ],
    },
    Tweak {
        id: "search-history",
        title: "Don't keep device search history",
        detail: "Stops Windows from saving your search-box history on this device.",
        category: CAT_SEARCH,
        risk: "low",
        values: &[RegValue {
            key: r"HKCU\Software\Microsoft\Windows\CurrentVersion\SearchSettings",
            name: "IsDeviceSearchHistoryEnabled",
            hardened: 0,
        }],
    },
    Tweak {
        id: "suggested-content-settings",
        title: "Hide suggested content in Settings",
        detail: "Removes the promotional 'suggested content' tiles that appear inside the Settings app.",
        category: CAT_SUGGESTED,
        risk: "low",
        values: &[
            RegValue { key: CDM, name: "SubscribedContent-338393Enabled", hardened: 0 },
            RegValue { key: CDM, name: "SubscribedContent-353694Enabled", hardened: 0 },
            RegValue { key: CDM, name: "SubscribedContent-353696Enabled", hardened: 0 },
        ],
    },
    Tweak {
        id: "start-app-suggestions",
        title: "Hide Start menu app suggestions",
        detail: "Stops Windows from inserting suggested/promoted apps into the Start menu.",
        category: CAT_SUGGESTED,
        risk: "low",
        values: &[RegValue { key: CDM, name: "SystemPaneSuggestionsEnabled", hardened: 0 }],
    },
    Tweak {
        id: "tips-notifications",
        title: "Stop tips & suggestion notifications",
        detail: "Disables the 'get even more out of Windows' tip and suggestion notifications.",
        category: CAT_SUGGESTED,
        risk: "low",
        values: &[
            RegValue { key: CDM, name: "SoftLandingEnabled", hardened: 0 },
            RegValue { key: CDM, name: "SubscribedContent-338389Enabled", hardened: 0 },
        ],
    },
    Tweak {
        id: "welcome-experience",
        title: "Skip the post-update 'finish setting up' screen",
        detail: "Stops the full-screen 'Get even more out of Windows' / 'Let's finish setting up your device' welcome page shown after some updates and sign-ins.",
        category: CAT_SUGGESTED,
        risk: "low",
        values: &[RegValue { key: CDM, name: "SubscribedContent-310093Enabled", hardened: 0 }],
    },
    Tweak {
        id: "lockscreen-spotlight",
        title: "Disable lock-screen tips & 'fun facts'",
        detail: "Stops the lock screen from showing rotating Windows Spotlight tips, ads and fun facts.",
        category: CAT_LOCKSCREEN,
        risk: "low",
        values: &[
            RegValue { key: CDM, name: "RotatingLockScreenOverlayEnabled", hardened: 0 },
            RegValue { key: CDM, name: "SubscribedContent-338387Enabled", hardened: 0 },
        ],
    },
    Tweak {
        id: "sponsored-apps",
        title: "Stop auto-installing sponsored apps",
        detail: "Stops Windows from silently installing promoted/sponsored apps (the Candy-Crush-style ones) into your account.",
        category: CAT_APPS,
        risk: "low",
        values: &[
            RegValue { key: CDM, name: "SilentInstalledAppsEnabled", hardened: 0 },
            RegValue { key: CDM, name: "OemPreInstalledAppsEnabled", hardened: 0 },
        ],
    },
];

/// Guide-tier (admin-needed) tweaks. Shown with steps + a copyable elevated
/// command; this app NEVER runs them.
static GUIDE_CATALOG: &[GuideTweak] = &[
    GuideTweak {
        id: "guide-diagtrack",
        title: "Disable the DiagTrack telemetry service",
        detail: "Stops and disables 'Connected User Experiences and Telemetry' (DiagTrack), the service that collects and uploads diagnostic data. This is machine-wide, so it needs admin.",
        category: CAT_TELEMETRY,
        risk: "medium",
        command: "Stop-Service DiagTrack -ErrorAction SilentlyContinue; Set-Service DiagTrack -StartupType Disabled",
        steps: &[
            "Press Win, type \"PowerShell\", right-click it and choose \"Run as administrator\".",
            "Paste the command below and press Enter.",
            "Close the window. To undo later, run: Set-Service DiagTrack -StartupType Automatic",
        ],
    },
    GuideTweak {
        id: "guide-telemetry-level",
        title: "Set diagnostic data to the minimum level",
        detail: "Sets the machine-wide diagnostic-data level to the lowest your Windows edition allows (0 = Security on Enterprise/Education; Home and Pro floor at Basic). Machine policy, so it needs admin.",
        category: CAT_TELEMETRY,
        risk: "medium",
        command: "reg add \"HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection\" /v AllowTelemetry /t REG_DWORD /d 0 /f",
        steps: &[
            "Press Win, type \"PowerShell\", right-click it and choose \"Run as administrator\".",
            "Paste the command below and press Enter.",
            "To undo later, run: reg delete \"HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection\" /v AllowTelemetry /f",
        ],
    },
    GuideTweak {
        id: "guide-activity-feed",
        title: "Turn off activity history / Timeline (machine)",
        detail: "Disables the machine-wide activity history feed used by Timeline. This is a machine policy, so it needs admin.",
        category: CAT_TELEMETRY,
        risk: "low",
        command: "reg add \"HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\System\" /v EnableActivityFeed /t REG_DWORD /d 0 /f",
        steps: &[
            "Press Win, type \"PowerShell\", right-click it and choose \"Run as administrator\".",
            "Paste the command below and press Enter.",
            "To undo later, run: reg delete \"HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\System\" /v EnableActivityFeed /f",
        ],
    },
];

/// One tweak as the frontend sees it. `applied` reflects the LIVE registry
/// (so settings the user already changed in Windows show as on); the redb
/// snapshot is consulted only for *how* to revert, never for this flag.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HardeningTweak {
    id: String,
    title: String,
    detail: String,
    category: String,
    risk: String,
    /// "apply" (a no-admin toggle) or "guide" (admin — shown with steps).
    tier: String,
    applied: bool,
    /// Guide tier only: the copyable elevated command.
    command: Option<String>,
    /// Guide tier only: step-by-step instructions.
    steps: Vec<String>,
}

/// The prior state of one registry value, captured before the first apply so
/// revert can restore it exactly. `prior: None` means "the value was absent" →
/// revert deletes it rather than guessing a default.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PriorValue {
    key: String,
    name: String,
    prior: Option<u32>,
}

const SNAPSHOT_KEY: &str = "windows-hardening/snapshots";

/// Serialises the snapshot map's read-modify-write so two concurrent
/// apply/revert calls can't drop each other's first-apply snapshot (the
/// exact-prior-value guarantee). Held across each apply_one / revert_one.
static SNAPSHOT_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn load_snapshots(app: &tauri::AppHandle) -> Result<HashMap<String, Vec<PriorValue>>, String> {
    let path = local_db::database_path_for_dir(&preferences::preferences_dir(app)?);
    local_db::read_json_or_default(&path, SNAPSHOT_KEY, HashMap::new())
}

fn save_snapshots(
    app: &tauri::AppHandle,
    snaps: &HashMap<String, Vec<PriorValue>>,
) -> Result<(), String> {
    let path = local_db::database_path_for_dir(&preferences::preferences_dir(app)?);
    local_db::write_json(&path, SNAPSHOT_KEY, snaps)
}

/// Is every value of this tweak currently at its hardened state? (Live read.)
fn is_applied(t: &Tweak) -> bool {
    #[cfg(windows)]
    {
        t.values
            .iter()
            .all(|v| reg_read_dword(v.key, v.name) == Some(v.hardened))
    }
    #[cfg(not(windows))]
    {
        let _ = t;
        false
    }
}

#[cfg(windows)]
fn reg_read_dword(key: &str, name: &str) -> Option<u32> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = Command::new("reg")
        .args(["query", key, "/v", name])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_reg_dword_hex(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(windows)]
fn reg_write_dword(key: &str, name: &str, value: u32) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = Command::new("reg")
        .args([
            "add",
            key,
            "/v",
            name,
            "/t",
            "REG_DWORD",
            "/d",
            &value.to_string(),
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("reg add failed: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "reg add failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

/// Best-effort delete — a missing value is already in the desired (absent)
/// state, so any error here is benign for our purposes.
#[cfg(windows)]
fn reg_delete_value(key: &str, name: &str) {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = Command::new("reg")
        .args(["delete", key, "/v", name, "/f"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

/// Parse the hex DWORD off `reg query` output (mirrors
/// `quick_actions::parse_reg_dword_hex`; kept local so this module stays
/// self-contained). None when no `0x…` token is present (key/value missing).
#[cfg(windows)]
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

/// Tell the shell associations changed so open Explorer/Start surfaces pick up
/// the new values without a restart (same broadcast as the Explorer toggles).
#[cfg(windows)]
fn broadcast_shell_change() {
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    // SAFETY: pure broadcast — event id + flags, both PIDL pointers None.
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

#[cfg(windows)]
fn apply_one(app: &tauri::AppHandle, t: &Tweak) -> Result<(), String> {
    let _guard = SNAPSHOT_LOCK
        .lock()
        .map_err(|_| "snapshot lock poisoned".to_string())?;
    // Snapshot the prior state ONCE — never overwrite the original if the user
    // toggles the same tweak more than once. Each value carries its own key so
    // revert is self-contained even if the catalog later changes.
    let mut snaps = load_snapshots(app)?;
    if !snaps.contains_key(t.id) {
        let prior: Vec<PriorValue> = t
            .values
            .iter()
            .map(|v| PriorValue {
                key: v.key.to_string(),
                name: v.name.to_string(),
                prior: reg_read_dword(v.key, v.name),
            })
            .collect();
        snaps.insert(t.id.to_string(), prior);
        save_snapshots(app, &snaps)?;
    }
    // A mid-loop write failure leaves the tweak half-applied but FULLY
    // snapshotted — Revert restores every value from the snapshot, so the state
    // is always recoverable.
    for v in t.values {
        reg_write_dword(v.key, v.name, v.hardened)?;
    }
    Ok(())
}

#[cfg(windows)]
fn revert_one(app: &tauri::AppHandle, t: &Tweak) -> Result<(), String> {
    let _guard = SNAPSHOT_LOCK
        .lock()
        .map_err(|_| "snapshot lock poisoned".to_string())?;
    let mut snaps = load_snapshots(app)?;
    if let Some(prior) = snaps.get(t.id).cloned() {
        // Restore the EXACT captured prior state (the key travels with the
        // snapshot, so this is independent of the live catalog).
        for pv in &prior {
            match pv.prior {
                Some(val) => reg_write_dword(&pv.key, &pv.name, val)?,
                None => reg_delete_value(&pv.key, &pv.name),
            }
        }
        snaps.remove(t.id);
        save_snapshots(app, &snaps)?;
    } else {
        // No snapshot (e.g. applied outside this tool) → restore Windows
        // default by deleting each value.
        for v in t.values {
            reg_delete_value(v.key, v.name);
        }
    }
    Ok(())
}

/// Read every catalog tweak's live state (apply tier) plus the guide tier.
#[tauri::command(async)]
pub fn read_hardening_state() -> Result<Vec<HardeningTweak>, String> {
    let mut out = Vec::with_capacity(CATALOG.len() + GUIDE_CATALOG.len());
    for t in CATALOG {
        out.push(HardeningTweak {
            id: t.id.into(),
            title: t.title.into(),
            detail: t.detail.into(),
            category: t.category.into(),
            risk: t.risk.into(),
            tier: "apply".into(),
            applied: is_applied(t),
            command: None,
            steps: Vec::new(),
        });
    }
    for g in GUIDE_CATALOG {
        out.push(HardeningTweak {
            id: g.id.into(),
            title: g.title.into(),
            detail: g.detail.into(),
            category: g.category.into(),
            risk: g.risk.into(),
            tier: "guide".into(),
            applied: false,
            command: Some(g.command.into()),
            steps: g.steps.iter().map(|s| (*s).to_string()).collect(),
        });
    }
    Ok(out)
}

#[tauri::command(async)]
pub fn apply_hardening_tweak(app: tauri::AppHandle, id: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        let t = CATALOG
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| format!("Unknown tweak: {id}"))?;
        apply_one(&app, t)?;
        broadcast_shell_change();
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (app, id);
        Err("Windows hardening is Windows-only".to_string())
    }
}

#[tauri::command(async)]
pub fn revert_hardening_tweak(app: tauri::AppHandle, id: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        let t = CATALOG
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| format!("Unknown tweak: {id}"))?;
        revert_one(&app, t)?;
        broadcast_shell_change();
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (app, id);
        Err("Windows hardening is Windows-only".to_string())
    }
}

#[tauri::command(async)]
pub fn revert_all_hardening(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        let snaps = load_snapshots(&app)?;
        for t in CATALOG {
            if snaps.contains_key(t.id) || is_applied(t) {
                revert_one(&app, t)?;
            }
        }
        broadcast_shell_change();
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("Windows hardening is Windows-only".to_string())
    }
}

/// Write a JSON snapshot of every apply-tier registry value's CURRENT value to
/// `path` — a portable safety net the user can keep outside the app.
#[tauri::command(async)]
pub fn export_hardening_backup(path: String) -> Result<(), String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct BackupEntry {
        key: String,
        name: String,
        value: Option<u32>,
    }
    let mut entries: Vec<BackupEntry> = Vec::new();
    #[cfg(windows)]
    {
        for t in CATALOG {
            for v in t.values {
                entries.push(BackupEntry {
                    key: v.key.to_string(),
                    name: v.name.to_string(),
                    value: reg_read_dword(v.key, v.name),
                });
            }
        }
    }
    let json = serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Cannot write backup: {e}"))?;
    Ok(())
}
