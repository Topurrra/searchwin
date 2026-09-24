//! Privacy Audit — local, user-initiated privacy scans.
//!
//! KeepItLocal's flagship: report what on this machine can see/hear you or
//! holds your secrets, and tell the user exactly what to do about it.
//!
//! Two non-negotiable rules, enforced by construction:
//!   1. **User-initiated, or a user-CONFIGURED opt-in schedule.** By default
//!      (schedule `off`) nothing here runs in the background — a command fires
//!      only when the user clicks "Scan". The single, owner-approved relaxation
//!      is an explicit opt-in: the user may choose a cadence (on launch / daily
//!      / weekly) in Settings, and `run_scheduled_privacy_audit` then runs the
//!      same scans locally and notifies ONLY when un-acknowledged issues exist.
//!      Nothing ever runs unless the user turns it on; a privacy auditor that
//!      silently watched you without that consent would betray its own premise.
//!   2. **Zero network.** Every scan reads LOCAL system state only. No
//!      telemetry, no lookups, no phoning home. Ever.
//!
//! Phase 0: microphone & camera access. Windows records which apps may use
//! the mic/camera (and when they last did) in the per-user
//! CapabilityAccessManager ConsentStore in the registry. We READ it only —
//! we never change a permission; the "fix" is a deep-link into the Windows
//! Settings page so the user stays in control.

use serde::Serialize;

use crate::commands::{local_db, preferences};

/// redb key for the user's acknowledged privacy findings — persisted across
/// restarts (replaces the old localStorage list that didn't survive relaunch).
const PRIVACY_ACK_KEY: &str = "privacy-audit/acknowledged";

/// Load the acknowledged-finding ids (stable ids like "microphone:Zoom.exe").
#[tauri::command]
pub fn privacy_get_acknowledged(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let path = local_db::database_path_for_dir(&preferences::preferences_dir(&app)?);
    local_db::read_json_or_default(&path, PRIVACY_ACK_KEY, Vec::<String>::new())
}

/// Persist the acknowledged-finding ids.
#[tauri::command]
pub fn privacy_set_acknowledged(app: tauri::AppHandle, ids: Vec<String>) -> Result<(), String> {
    let path = local_db::database_path_for_dir(&preferences::preferences_dir(&app)?);
    local_db::write_json(&path, PRIVACY_ACK_KEY, &ids)
}

/// Severity of a finding, low → high. Drives sorting + the privacy score.
/// (The frontend also styles `low`/`ok`; no scan emits those yet, so they're
/// omitted here — add them back when a scan needs that granularity.)
#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Medium,
    High,
}

/// An optional one-click remediation the frontend can perform for a finding.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingFix {
    /// Button label, e.g. "Open setting", "Open folder".
    pub label: String,
    /// Dispatch kind the frontend switches on:
    ///   "open-setting" → invoke('open_privacy_setting', { target })
    ///   "reveal-path"  → invoke('open_search_result_path', { path: target })
    pub action: String,
    /// The target: an `ms-settings:` URI, or a filesystem path.
    pub target: String,
}

/// One privacy finding — uniform across every scan so the frontend renders
/// them all through a single card component.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// Stable id for keying + dedupe (e.g. "microphone:Zoom.exe").
    pub id: String,
    /// Which scan produced it ("microphone" | "camera" | "secrets" | …).
    pub category: String,
    pub severity: Severity,
    /// Headline — usually the app / subject / file name.
    pub title: String,
    /// One-line plain-English description of what was found.
    pub detail: String,
    /// What the user can do about it.
    pub recommendation: String,
    /// Optional one-click remediation. None when there's no safe auto-fix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<FindingFix>,
    /// Phase 6.5-5 (2026-05-27): kind labels for finding-typed sensitive
    /// content (mirrors `sensitive_scan::Finding::kind`). Populated by
    /// the unencrypted-secrets audit so the UI can render per-finding
    /// chips ("AWS access key", "GitHub token", …) instead of stuffing
    /// the kinds into the detail string. None for findings that don't
    /// have categorized content (e.g. browser extensions, startup
    /// programs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<String>>,
    /// Phase 6.5-5: when the finding is anchored to a file path, this
    /// is the absolute path. Used by the inline Preview panel to fetch
    /// `preview_file_findings`. Mirrors `fix.target` for `reveal-file`
    /// findings but more explicit (a `reveal-path` finding has a
    /// folder target, not a file). None when the finding isn't
    /// file-based.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_path: Option<String>,
}

/// Severity ordering for sorting + scoring (low → high). Shared by every scan.
fn severity_rank(s: Severity) -> u8 {
    match s {
        Severity::Info => 0,
        Severity::Medium => 1,
        Severity::High => 2,
    }
}

/// The result of one scan: its findings + a short human summary.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    /// Scan id (e.g. "mic-camera").
    pub scan: String,
    pub findings: Vec<Finding>,
    /// e.g. "6 apps can use your microphone · 2 your camera".
    pub summary: String,
    /// True when the scan couldn't read its source (e.g. not on Windows,
    /// or the permission store is missing) — the UI shows a soft notice
    /// rather than a misleading "all clear".
    pub unavailable: bool,
}

/// Microphone & camera access audit. Synchronous registry reads, but tagged
/// `(async)` so Tauri runs it on a worker thread and the UI never blocks.
#[tauri::command(async)]
pub fn audit_mic_camera() -> Result<ScanResult, String> {
    #[cfg(windows)]
    {
        win::audit()
    }
    #[cfg(not(windows))]
    {
        Ok(ScanResult {
            scan: "mic-camera".into(),
            findings: Vec::new(),
            summary: "Microphone & camera audit is only available on Windows.".into(),
            unavailable: true,
        })
    }
}

/// Open a Windows privacy Settings page. Tightly allowlisted — the frontend
/// can only ever request the known privacy pages, never an arbitrary URI
/// (the generic `open_external_url` deliberately refuses `ms-settings:`).
#[tauri::command]
pub fn open_privacy_setting(target: String) -> Result<(), String> {
    const ALLOWED: &[&str] = &[
        "ms-settings:privacy-microphone",
        "ms-settings:privacy-webcam",
    ];
    if !ALLOWED.contains(&target.as_str()) {
        return Err(format!("Not an allowed privacy setting: {target}"));
    }
    open::that(&target).map_err(|e| format!("Cannot open setting: {e}"))?;
    Ok(())
}

/// Reveal a file in Windows Explorer with it selected (read-only navigation —
/// opens a folder window highlighting the file; never modifies anything). Used
/// by the unencrypted-secrets findings so the user can locate the exact file.
#[tauri::command]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        // `/select` highlights the item. raw_arg preserves Explorer's finicky
        // comma syntax + the quoting needed for paths containing spaces.
        Command::new("explorer")
            .raw_arg(format!("/select,\"{path}\""))
            .spawn()
            .map_err(|e| format!("Cannot open Explorer: {e}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("Reveal in Explorer is only available on Windows.".into())
    }
}

/// Browser-extension audit. Reads installed Chromium-family extension
/// manifests (Chrome, Edge, Brave, Vivaldi) and flags ones holding broad
/// permissions — read/modify all sites, your cookies, history, native
/// messaging, etc. Read-only, local: we parse manifest.json files, nothing
/// more. Tagged `(async)` so it never blocks the UI.
#[tauri::command(async)]
pub fn audit_browser_extensions() -> Result<ScanResult, String> {
    #[cfg(windows)]
    {
        ext::audit()
    }
    #[cfg(not(windows))]
    {
        Ok(ScanResult {
            scan: "browser-extensions".into(),
            findings: Vec::new(),
            summary: "Browser-extension audit is only available on Windows.".into(),
            unavailable: true,
        })
    }
}

/// Startup-programs audit. Reuses the cleaner's startup inventory (Startup
/// folders + HKCU/HKLM Run keys) and flags entries that launch from unusual
/// locations (Temp, Downloads, the Recycle Bin) or are loose scripts —
/// classic spots for malware / unwanted programs to persist. Normal entries
/// (Program Files, Windows) are counted in the summary but not flagged, to
/// keep the signal high for a general audience. Local read only.
#[tauri::command(async)]
pub fn audit_startup_programs() -> Result<ScanResult, String> {
    let items = super::cleaner::startup_items();
    let total = items.len();
    let mut findings: Vec<Finding> = Vec::new();

    for item in &items {
        let exe = extract_exe_path(&item.command_or_path);
        let Some(reason) = suspicious_startup_reason(&exe.to_lowercase()) else {
            continue;
        };
        let fix = if !exe.is_empty() && std::path::Path::new(&exe).exists() {
            Some(FindingFix {
                label: "Reveal file".into(),
                action: "reveal-file".into(),
                target: exe.clone(),
            })
        } else {
            None
        };
        findings.push(Finding {
            id: format!("startup:{}:{}", item.source, item.name),
            category: "startup".into(),
            severity: Severity::High,
            title: item.name.clone(),
            detail: format!("Runs at startup {reason} · {}", item.command_or_path),
            recommendation:
                "If you don't recognise this, disable it in Windows Settings → Apps → Startup."
                    .into(),
            fix,
            kinds: None,
            preview_path: None,
        });
    }

    findings.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

    let flagged = findings.len();
    let summary = if total == 0 {
        "No startup programs found.".to_string()
    } else if flagged == 0 {
        format!(
            "{total} program{} run at startup — none from unusual locations.",
            if total == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "{flagged} of {total} startup program{} run from unusual locations.",
            if total == 1 { "" } else { "s" }
        )
    };

    Ok(ScanResult {
        scan: "startup-programs".into(),
        findings,
        summary,
        unavailable: false,
    })
}

/// Extract the executable path from a startup command, stripping surrounding
/// quotes and trailing arguments (`"C:\App\app.exe" --min` → `C:\App\app.exe`).
fn extract_exe_path(command: &str) -> String {
    let c = command.trim();
    if let Some(rest) = c.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }
    c.split_whitespace().next().unwrap_or(c).to_string()
}

/// Why a startup entry is worth flagging (or None for normal locations). The
/// heuristic targets where malware / unwanted programs persist — loose scripts
/// and write-anywhere folders — without nagging about every legit app in
/// Program Files.
fn suspicious_startup_reason(exe_lower: &str) -> Option<&'static str> {
    const SCRIPT_EXTS: &[&str] = &[
        ".bat", ".cmd", ".vbs", ".vbe", ".js", ".jse", ".wsf", ".wsh", ".ps1", ".scr", ".hta",
    ];
    if SCRIPT_EXTS.iter().any(|ext| exe_lower.ends_with(ext)) {
        return Some("via a script");
    }
    const SUSPICIOUS_DIRS: &[&str] = &[
        "\\temp\\",
        "\\tmp\\",
        "\\downloads\\",
        "\\public\\",
        "\\$recycle.bin\\",
        "\\appdata\\local\\temp\\",
    ];
    if SUSPICIOUS_DIRS.iter().any(|dir| exe_lower.contains(dir)) {
        return Some("from an unusual location");
    }
    None
}

/// Outbound-connection snapshot — the "phoning home" check. Lists programs
/// with a live outbound connection to the internet right now. This is a local
/// OBSERVATION (a `netstat` snapshot correlated to process names) — nothing is
/// captured, logged, or sent anywhere; the data never leaves the machine, so
/// it doesn't touch the zero-telemetry promise. Informational by nature
/// (browsers, updaters, and cloud apps all connect out), so findings are Info,
/// not alarms — acknowledge the ones you expect and notice anything you don't.
#[tauri::command(async)]
pub fn audit_outbound_connections() -> Result<ScanResult, String> {
    #[cfg(windows)]
    {
        net::audit()
    }
    #[cfg(not(windows))]
    {
        Ok(ScanResult {
            scan: "outbound-connections".into(),
            findings: Vec::new(),
            summary: "Outbound-connection check is only available on Windows.".into(),
            unavailable: true,
        })
    }
}

/// Hosts-file monitor. Reads the Windows hosts file and flags entries that
/// redirect a hostname to a routable PUBLIC IP (classic traffic hijack) or that
/// blackhole (0.0.0.0 / 127.0.0.1) a security / OS-update / banking domain — a
/// trick malware uses to cut off antivirus and Windows Update. Pure local read.
/// Ad-blocking hosts lists (huge 0.0.0.0 blocks of ad domains) are summarized,
/// never flagged, so the signal stays high. Tagged `(async)`.
#[tauri::command(async)]
pub fn audit_hosts_file() -> Result<ScanResult, String> {
    #[cfg(windows)]
    {
        hosts::audit()
    }
    #[cfg(not(windows))]
    {
        Ok(ScanResult {
            scan: "hosts-file".into(),
            findings: Vec::new(),
            summary: "Hosts-file monitor is only available on Windows.".into(),
            unavailable: true,
        })
    }
}

/// Developer-secrets audit. Walks common dev folders (Desktop, Documents,
/// Downloads, ~/source|repos|projects|dev|code) to a bounded depth and flags
/// exposed `.env` files and private-key files on disk — and, when such a file
/// lives in a git working tree that doesn't `.gitignore` it, warns that it is
/// likely committed to version control. Skips node_modules / target / vendor /
/// etc. Local read only. Tagged `(async)`.
#[tauri::command(async)]
pub fn audit_dev_secrets() -> Result<ScanResult, String> {
    #[cfg(windows)]
    {
        dev::audit()
    }
    #[cfg(not(windows))]
    {
        Ok(ScanResult {
            scan: "dev-secrets".into(),
            findings: Vec::new(),
            summary: "Developer-secrets audit is only available on Windows.".into(),
            unavailable: true,
        })
    }
}

/// Task Scheduler persistence audit. Enumerates scheduled tasks (`schtasks`)
/// and flags ones whose action runs from a write-anywhere location (Temp,
/// Downloads, AppData\Local\Temp, Public, Recycle Bin) or is a loose script —
/// the spot where malware re-creates itself to survive reboots. Local read
/// only. Tagged `(async)`.
#[tauri::command(async)]
pub fn audit_scheduled_tasks() -> Result<ScanResult, String> {
    #[cfg(windows)]
    {
        sched::audit()
    }
    #[cfg(not(windows))]
    {
        Ok(ScanResult {
            scan: "scheduled-tasks".into(),
            findings: Vec::new(),
            summary: "Task Scheduler audit is only available on Windows.".into(),
            unavailable: true,
        })
    }
}

/// Unencrypted-secrets audit. Reuses the search index's sensitive-content tags
/// (written at index time by `sensitive_scan`) to report files holding API
/// keys, private keys, passwords, or PII in readable form on disk. NO new
/// content scan — a local index read. Tagged `(async)` so it never blocks UI.
#[tauri::command(async)]
pub fn audit_unencrypted_pii(app: tauri::AppHandle) -> Result<ScanResult, String> {
    let report = super::search::collect_sensitive_files(&app, 500)?;
    if report.index_unavailable {
        return Ok(ScanResult {
            scan: "unencrypted-pii".into(),
            findings: Vec::new(),
            summary: "Build your File Search index first — this scan reads what it already found."
                .into(),
            unavailable: true,
        });
    }

    let mut findings: Vec<Finding> = Vec::with_capacity(report.files.len());
    for file in &report.files {
        let high = file.kinds.iter().any(|k| is_high_risk_kind(k));
        let kinds_h = humanize_kinds(&file.kinds);
        let name = if file.file_name.is_empty() {
            file.path.clone()
        } else {
            file.file_name.clone()
        };
        findings.push(Finding {
            id: format!("pii:{}", file.path),
            category: "secrets".into(),
            severity: if high { Severity::High } else { Severity::Medium },
            title: name,
            detail: format!("Contains {kinds_h} · {}", file.path),
            recommendation:
                "Move the secret into a password manager or an untracked .env, or delete the file if it shouldn't be on disk."
                    .into(),
            fix: Some(FindingFix {
                label: "Reveal file".into(),
                action: "reveal-file".into(),
                target: file.path.clone(),
            }),
            kinds: Some(file.kinds.clone()),
            preview_path: Some(file.path.clone()),
        });
    }

    // High-risk files first.
    findings.sort_by(|a, b| severity_rank(b.severity).cmp(&severity_rank(a.severity)));

    let summary = if findings.is_empty() {
        "No files with detectable secrets in your index.".into()
    } else if report.total > findings.len() {
        format!(
            "{} files contain readable secrets (showing the first {}).",
            report.total,
            findings.len()
        )
    } else {
        format!(
            "{} file{} contain readable secrets.",
            findings.len(),
            if findings.len() == 1 { "" } else { "s" }
        )
    };

    Ok(ScanResult {
        scan: "unencrypted-pii".into(),
        findings,
        summary,
        unavailable: false,
    })
}

/// Kinds that warrant HIGH severity — live credentials / private keys whose
/// leak is immediately exploitable. Everything else (PII, generic passwords)
/// is Medium.
fn is_high_risk_kind(kind: &str) -> bool {
    matches!(
        kind,
        "private_key_pem"
            | "pgp_private_key"
            | "ethereum_private_key"
            | "bitcoin_wif_private_key"
            | "aws_access_key"
            | "stripe_secret_key"
            | "google_api_key"
            | "openai_api_key"
            | "sendgrid_api_key"
            | "twilio_account_sid"
            | "slack_token"
            | "slack_webhook"
            | "database_url_with_password"
            | "github_token"
            | "npm_token"
            | "anthropic_api_key"
    )
}

/// Map a raw detector label to a short human noun phrase.
fn humanize_kind(kind: &str) -> &'static str {
    match kind {
        "aws_access_key" => "an AWS access key",
        "github_token" => "a GitHub token",
        "slack_token" => "a Slack token",
        "slack_webhook" => "a Slack webhook",
        "stripe_secret_key" => "a Stripe secret key",
        "google_api_key" => "a Google API key",
        "twilio_account_sid" => "a Twilio SID",
        "sendgrid_api_key" => "a SendGrid API key",
        "jwt_token" => "a JWT",
        "private_key_pem" => "a private key",
        "pgp_private_key" => "a PGP private key",
        "database_url_with_password" => "a database password URL",
        "ethereum_private_key" => "an Ethereum private key",
        "bitcoin_wif_private_key" => "a Bitcoin private key",
        "anthropic_api_key" => "an Anthropic API key",
        "openai_api_key" => "an OpenAI API key",
        "npm_token" => "an npm token",
        "bitcoin_address" | "bitcoin_bech32" | "ethereum_address" | "solana_address" => {
            "a crypto wallet address"
        }
        "generic_credential_assignment" => "a password or API key",
        "bearer_token" => "a bearer token",
        "us_ssn" => "a US Social Security number",
        "credit_card" => "a credit-card number",
        "iban" => "an IBAN",
        "italian_cf" => "an Italian fiscal code",
        "uk_nino" => "a UK National Insurance number",
        "france_nir" => "a French social security number",
        "us_itin" => "a US ITIN",
        "us_ein" => "a US EIN",
        "vin" => "a vehicle VIN",
        "spain_dni" => "a Spanish DNI/NIE",
        "dutch_bsn" => "a Dutch BSN",
        "india_aadhaar" => "an Aadhaar number",
        "georgian_id" => "a Georgian ID number",
        "mac_address" => "a MAC address",
        "passport" => "a passport number",
        "drivers_license" => "a driver's license number",
        "bank_account" => "a bank account number",
        "medical_record_number" => "a medical record number",
        "dob" => "a date of birth",
        "routing_number" => "a bank routing number",
        "date" => "a date",
        _ => "a secret",
    }
}

/// Human summary of a file's finding kinds, e.g. "an AWS access key and a
/// private key" or "a JWT, a password or API key, and 2 more".
fn humanize_kinds(kinds: &[String]) -> String {
    let mut parts: Vec<&str> = kinds.iter().map(|k| humanize_kind(k)).collect();
    parts.sort_unstable();
    parts.dedup();
    match parts.len() {
        0 => "a secret".to_string(),
        1 => parts[0].to_string(),
        2 => format!("{} and {}", parts[0], parts[1]),
        _ => format!("{}, and {} more", parts[..2].join(", "), parts.len() - 2),
    }
}

#[cfg(windows)]
mod win {
    use super::{Finding, FindingFix, ScanResult, Severity};
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const BASE: &str =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";

    pub fn audit() -> Result<ScanResult, String> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let mut findings: Vec<Finding> = Vec::new();
        let mut mic_count = 0usize;
        let mut cam_count = 0usize;
        let mut active_count = 0usize;
        let mut any_store = false;

        // (registry key, human label, category id, fix deep-link).
        let caps: [(&str, &str, &str, &str); 2] = [
            (
                "microphone",
                "microphone",
                "microphone",
                "ms-settings:privacy-microphone",
            ),
            ("webcam", "camera", "camera", "ms-settings:privacy-webcam"),
        ];

        for (cap_key, cap_label, category, fix_uri) in caps {
            let path = format!(r"{BASE}\{cap_key}");
            let Ok(cap_root) = hkcu.open_subkey(&path) else {
                continue; // store missing for this capability — skip it
            };
            any_store = true;

            // Packaged (Store) apps are direct subkeys (PackageFamilyName);
            // desktop apps live one level deeper under "NonPackaged". Collect
            // (app key, raw subkey name) for both.
            let mut apps: Vec<(RegKey, String)> = Vec::new();
            for sub in cap_root.enum_keys().flatten() {
                if sub.eq_ignore_ascii_case("NonPackaged") {
                    if let Ok(np) = cap_root.open_subkey("NonPackaged") {
                        for app in np.enum_keys().flatten() {
                            if let Ok(k) = np.open_subkey(&app) {
                                apps.push((k, app));
                            }
                        }
                    }
                } else if let Ok(k) = cap_root.open_subkey(&sub) {
                    apps.push((k, sub));
                }
            }

            for (app_key, raw_name) in apps {
                // Only report apps actually ALLOWED — "Deny"/"Prompt" aren't
                // a privacy exposure.
                let value: String = app_key.get_value("Value").unwrap_or_default();
                if !value.eq_ignore_ascii_case("Allow") {
                    continue;
                }
                // FILETIMEs: a non-zero Start with a zero Stop means the app is
                // holding the device open RIGHT NOW.
                let start: u64 = app_key.get_value("LastUsedTimeStart").unwrap_or(0);
                let stop: u64 = app_key.get_value("LastUsedTimeStop").unwrap_or(0);
                let in_use = start != 0 && stop == 0;

                if category == "microphone" {
                    mic_count += 1;
                } else {
                    cam_count += 1;
                }
                if in_use {
                    active_count += 1;
                }

                let location = if raw_name.contains('#') {
                    // NonPackaged desktop apps encode the exe path with '#'.
                    raw_name.replace('#', "\\")
                } else {
                    "Microsoft Store app".to_string()
                };
                let detail = if in_use {
                    format!("Using your {cap_label} right now · {location}")
                } else {
                    format!("Can use your {cap_label} · {location}")
                };

                findings.push(Finding {
                    id: format!("{category}:{raw_name}"),
                    category: category.to_string(),
                    severity: if in_use { Severity::High } else { Severity::Info },
                    title: friendly_name(&raw_name),
                    detail,
                    recommendation: format!(
                        "If you don't recognize this or don't want it accessing your {cap_label}, revoke it in Windows {cap_label} settings."
                    ),
                    fix: Some(FindingFix {
                        label: "Open setting".to_string(),
                        action: "open-setting".to_string(),
                        target: fix_uri.to_string(),
                    }),
                    kinds: None,
                    preview_path: None,
                });
            }
        }

        // Most severe first (active-now floats to the top), then by name.
        findings.sort_by(|a, b| {
            super::severity_rank(b.severity)
                .cmp(&super::severity_rank(a.severity))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });

        let summary = if !any_store {
            "Couldn't read the Windows permission store.".to_string()
        } else if findings.is_empty() {
            "No apps currently have microphone or camera access.".to_string()
        } else {
            let active = if active_count > 0 {
                format!(" · {active_count} active now")
            } else {
                String::new()
            };
            format!(
                "{mic_count} app{} can use your microphone · {cam_count} your camera{active}",
                plural(mic_count)
            )
        };

        Ok(ScanResult {
            scan: "mic-camera".into(),
            findings,
            summary,
            unavailable: !any_store,
        })
    }

    /// Human-readable name from a ConsentStore subkey: the exe filename for
    /// desktop apps, the de-hashed PackageFamilyName for Store apps.
    fn friendly_name(raw: &str) -> String {
        if raw.contains('#') {
            let path = raw.replace('#', "\\");
            path.rsplit('\\').next().unwrap_or(&path).to_string()
        } else {
            // "Microsoft.WindowsCamera_8wekyb3d8bbwe" → "Microsoft.WindowsCamera"
            raw.split('_').next().unwrap_or(raw).to_string()
        }
    }

    fn plural(n: usize) -> &'static str {
        if n == 1 {
            ""
        } else {
            "s"
        }
    }
}

#[cfg(windows)]
mod ext {
    //! Chromium-family browser-extension audit. Walks each browser's profile
    //! Extensions folders, reads each extension's manifest.json, and flags the
    //! ones requesting broad permissions. Pure local filesystem reads.

    use super::{Finding, ScanResult, Severity};
    use serde_json::Value;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// (display name, path under %LOCALAPPDATA%, the browser's extensions-page
    /// URL scheme for the recommendation text).
    const BROWSERS: &[(&str, &str, &str)] = &[
        ("Chrome", r"Google\Chrome\User Data", "chrome"),
        ("Edge", r"Microsoft\Edge\User Data", "edge"),
        ("Brave", r"BraveSoftware\Brave-Browser\User Data", "brave"),
        ("Vivaldi", r"Vivaldi\User Data", "vivaldi"),
    ];

    pub fn audit() -> Result<ScanResult, String> {
        let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
        if local.is_empty() {
            return Ok(unavailable());
        }
        let base = PathBuf::from(&local);

        let mut findings: Vec<Finding> = Vec::new();
        let mut scanned_any = false;
        let mut total = 0usize;

        for (browser, rel, scheme) in BROWSERS {
            let user_data = base.join(rel);
            if !user_data.is_dir() {
                continue;
            }
            scanned_any = true;
            for profile in profile_dirs(&user_data) {
                let ext_root = profile.join("Extensions");
                if !ext_root.is_dir() {
                    continue;
                }
                let profile_name = file_name_of(&profile);
                for ext_id_dir in subdirs(&ext_root) {
                    let Some((ver_dir, manifest)) = manifest_in(&ext_id_dir) else {
                        continue;
                    };
                    total += 1;

                    // Collect the concerning permissions for this extension.
                    let mut phrases: Vec<&'static str> = Vec::new();
                    let mut max_sev = Severity::Medium;
                    let mut any = false;
                    for perm in collect_permissions(&manifest) {
                        if let Some((sev, phrase)) = classify_permission(&perm) {
                            any = true;
                            if !phrases.contains(&phrase) {
                                phrases.push(phrase);
                            }
                            if super::severity_rank(sev) > super::severity_rank(max_sev) {
                                max_sev = sev;
                            }
                        }
                    }
                    if !any {
                        continue;
                    }

                    let shown = if phrases.len() > 3 {
                        format!("{}, and more", phrases[..3].join(", "))
                    } else {
                        phrases.join(", ")
                    };
                    let name = resolve_name(&ver_dir, &manifest);
                    let ext_id = file_name_of(&ext_id_dir);

                    findings.push(Finding {
                        id: format!("ext:{browser}:{profile_name}:{ext_id}"),
                        category: "browser-extension".into(),
                        severity: max_sev,
                        title: format!("{name} ({browser})"),
                        detail: format!("Can {shown}"),
                        recommendation: format!(
                            "Open {scheme}://extensions and remove it if you don't recognise or use it."
                        ),
                        fix: None,
                        kinds: None,
                        preview_path: None,
                    });
                }
            }
        }

        // Most permissive first, then by name.
        findings.sort_by(|a, b| {
            super::severity_rank(b.severity)
                .cmp(&super::severity_rank(a.severity))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });

        let summary = if !scanned_any {
            "No supported browsers found.".to_string()
        } else if findings.is_empty() {
            format!("Checked {total} extensions — none request broad permissions.")
        } else {
            format!(
                "{} of {total} extensions request broad permissions.",
                findings.len()
            )
        };

        Ok(ScanResult {
            scan: "browser-extensions".into(),
            findings,
            summary,
            unavailable: !scanned_any,
        })
    }

    fn unavailable() -> ScanResult {
        ScanResult {
            scan: "browser-extensions".into(),
            findings: Vec::new(),
            summary: "Couldn't locate browser data.".to_string(),
            unavailable: true,
        }
    }

    /// "Default" + every "Profile N" directory under a browser's User Data.
    fn profile_dirs(user_data: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(entries) = fs::read_dir(user_data) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name == "Default" || name.starts_with("Profile ") {
                    out.push(path);
                }
            }
        }
        out
    }

    fn subdirs(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    out.push(path);
                }
            }
        }
        out
    }

    /// First version subdirectory of an extension that has a readable
    /// manifest.json. (An extension folder can hold multiple versions; any
    /// one's permission set is representative enough for an audit.)
    fn manifest_in(ext_id_dir: &Path) -> Option<(PathBuf, Value)> {
        for ver in subdirs(ext_id_dir) {
            let manifest_path = ver.join("manifest.json");
            if let Ok(text) = fs::read_to_string(&manifest_path) {
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    return Some((ver, value));
                }
            }
        }
        None
    }

    fn file_name_of(path: &Path) -> String {
        path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    /// MV2 `permissions` + MV3 `host_permissions` + `optional_permissions`,
    /// flattened to the string entries (objects/non-strings ignored).
    fn collect_permissions(manifest: &Value) -> Vec<String> {
        let mut out = Vec::new();
        for key in ["permissions", "host_permissions", "optional_permissions"] {
            if let Some(arr) = manifest.get(key).and_then(|v| v.as_array()) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        out.push(s.to_string());
                    }
                }
            }
        }
        out
    }

    /// Classify a permission into (severity, plain-English risk) — or None for
    /// the many narrow/benign permissions we deliberately don't surface (only
    /// genuinely broad ones earn a finding, to keep the signal high).
    fn classify_permission(perm: &str) -> Option<(Severity, &'static str)> {
        // Broad host access (all sites).
        if perm == "<all_urls>"
            || perm == "*://*/*"
            || perm == "http://*/*"
            || perm == "https://*/*"
            || perm == "http://*/"
            || perm == "https://*/"
        {
            return Some((Severity::High, "read and change data on all websites"));
        }
        match perm {
            "nativeMessaging" => Some((Severity::High, "talk to native apps on your computer")),
            "debugger" => Some((Severity::High, "use the debugger on web pages")),
            "proxy" => Some((Severity::High, "control your network proxy")),
            "cookies" => Some((Severity::Medium, "read your browser cookies")),
            "history" => Some((Severity::Medium, "read your browsing history")),
            "tabs" => Some((Severity::Medium, "see your open tabs and their URLs")),
            "webRequest" | "webRequestBlocking" => {
                Some((Severity::Medium, "intercept your network requests"))
            }
            "clipboardRead" => Some((Severity::Medium, "read your clipboard")),
            "downloads" => Some((Severity::Medium, "manage your downloads")),
            "bookmarks" => Some((Severity::Medium, "read your bookmarks")),
            "management" => Some((Severity::Medium, "manage your other extensions")),
            "privacy" => Some((Severity::Medium, "change your privacy settings")),
            "geolocation" => Some((Severity::Medium, "read your location")),
            _ => None,
        }
    }

    /// Resolve an extension's display name, following Chrome's `__MSG_key__`
    /// localization indirection into `_locales/<locale>/messages.json`.
    fn resolve_name(ver_dir: &Path, manifest: &Value) -> String {
        let raw = manifest
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown extension");
        let Some(key) = raw
            .strip_prefix("__MSG_")
            .and_then(|s| s.strip_suffix("__"))
        else {
            return raw.to_string();
        };
        let default_locale = manifest
            .get("default_locale")
            .and_then(|v| v.as_str())
            .unwrap_or("en");
        for locale in [default_locale, "en", "en_US"] {
            let path = ver_dir
                .join("_locales")
                .join(locale)
                .join("messages.json");
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(json) = serde_json::from_str::<Value>(&text) {
                    if let Some(msg) = lookup_message(&json, key) {
                        return msg;
                    }
                }
            }
        }
        "Unknown extension".to_string()
    }

    /// Look up a message key in a parsed messages.json (Chrome treats these
    /// keys case-insensitively).
    fn lookup_message(json: &Value, key: &str) -> Option<String> {
        let obj = json.as_object()?;
        let entry = obj.get(key).or_else(|| {
            obj.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v)
        })?;
        entry
            .get("message")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
    }
}

#[cfg(windows)]
mod net {
    //! Outbound-connection snapshot via `netstat -ano` (connections + PIDs)
    //! correlated with `tasklist` (PID → process name). Both ship with Windows;
    //! no driver, no FFI, no capture — a single point-in-time read.

    use super::{Finding, ScanResult, Severity};
    use std::collections::{HashMap, HashSet};
    use std::net::IpAddr;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    // CREATE_NO_WINDOW — suppress the cmd console flash when the Privacy
    // Audit Scan button fires `netstat` / `tasklist`. Without this, the
    // user sees a black window blink on every scan, which both looks bad
    // and (more importantly) makes the audit feel like it's running
    // something opaque.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn audit() -> Result<ScanResult, String> {
        let netstat = run(&["-ano", "-p", "tcp"]);
        let names = pid_names();

        // pid → set of distinct public remote IPs it's connected to.
        let mut by_pid: HashMap<u32, HashSet<String>> = HashMap::new();
        for line in netstat.lines() {
            let cols: Vec<&str> = line.split_whitespace().collect();
            // Expected shape: TCP  <local>  <remote>  ESTABLISHED  <pid>
            if cols.len() < 5 || !cols[0].eq_ignore_ascii_case("TCP") {
                continue;
            }
            if !cols[3].eq_ignore_ascii_case("ESTABLISHED") {
                continue;
            }
            let Ok(pid) = cols[4].parse::<u32>() else {
                continue;
            };
            if pid == 0 {
                continue;
            }
            let Some(ip) = remote_ip(cols[2]) else {
                continue;
            };
            if !is_public_ip(&ip) {
                continue;
            }
            by_pid.entry(pid).or_default().insert(ip);
        }

        // Group by program NAME (not PID): a PID changes every launch, so a
        // PID-based id would make an acknowledgement never re-match. Names also
        // dedupe multi-process apps into one row.
        let mut by_name: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for (pid, ips) in &by_pid {
            let name = names.get(pid).cloned().unwrap_or_else(|| format!("PID {pid}"));
            *by_name.entry(name).or_insert(0) += ips.len();
        }
        let mut entries: Vec<(String, usize)> = by_name.into_iter().collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1));

        let mut findings = Vec::new();
        for (name, count) in entries.into_iter().take(40) {
            findings.push(Finding {
                id: format!("conn:{name}"),
                category: "network".into(),
                severity: Severity::Info,
                title: name,
                detail: format!(
                    "{count} active outbound connection{} to the internet",
                    if count == 1 { "" } else { "s" }
                ),
                recommendation:
                    "Normal for browsers, cloud apps, and updaters. Look closer only if you don't recognise the program."
                        .into(),
                fix: None,
                kinds: None,
                preview_path: None,
            });
        }

        let summary = if findings.is_empty() {
            "No programs have an active internet connection right now.".to_string()
        } else {
            format!(
                "{} program{} connected to the internet right now.",
                findings.len(),
                if findings.len() == 1 { "" } else { "s" }
            )
        };

        Ok(ScanResult {
            scan: "outbound-connections".into(),
            findings,
            summary,
            unavailable: false,
        })
    }

    fn run(args: &[&str]) -> String {
        Command::new("netstat")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default()
    }

    /// PID → image name, via `tasklist /fo csv /nh`.
    fn pid_names() -> HashMap<u32, String> {
        let mut map = HashMap::new();
        let Ok(output) = Command::new("tasklist")
            .args(["/fo", "csv", "/nh"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        else {
            return map;
        };
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let cols = parse_csv_line(line);
            if cols.len() >= 2 {
                if let Ok(pid) = cols[1].trim().parse::<u32>() {
                    map.insert(pid, cols[0].clone());
                }
            }
        }
        map
    }

    /// Minimal CSV parser for tasklist's quoted output.
    fn parse_csv_line(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_quotes = false;
        for ch in line.chars() {
            match ch {
                '"' => in_quotes = !in_quotes,
                ',' if !in_quotes => out.push(std::mem::take(&mut cur)),
                _ => cur.push(ch),
            }
        }
        out.push(cur);
        out
    }

    /// Extract the IP from a `host:port` (IPv4) or `[host]:port` (IPv6) endpoint.
    fn remote_ip(addr: &str) -> Option<String> {
        if let Some(rest) = addr.strip_prefix('[') {
            return rest.split(']').next().map(|s| s.to_string());
        }
        addr.rsplit_once(':').map(|(ip, _)| ip.to_string())
    }

    /// True for a routable public address — excludes loopback, private, and
    /// link-local ranges so LAN chatter doesn't read as "phoning home".
    fn is_public_ip(ip: &str) -> bool {
        match ip.parse::<IpAddr>() {
            Ok(IpAddr::V4(v4)) => {
                !v4.is_loopback()
                    && !v4.is_private()
                    && !v4.is_link_local()
                    && !v4.is_unspecified()
                    && !v4.is_broadcast()
                    && !v4.is_multicast()
            }
            Ok(IpAddr::V6(v6)) => {
                let seg0 = v6.segments()[0];
                !v6.is_loopback()
                    && !v6.is_unspecified()
                    && !v6.is_multicast()
                    && (seg0 & 0xffc0) != 0xfe80 // link-local fe80::/10
                    && (seg0 & 0xfe00) != 0xfc00 // unique-local fc00::/7
            }
            Err(_) => false,
        }
    }
}

#[cfg(windows)]
mod hosts {
    //! Hosts-file monitor. Reads %SystemRoot%\System32\drivers\etc\hosts and
    //! flags suspicious mappings. Read-only.

    use super::{Finding, FindingFix, ScanResult, Severity};
    use std::net::IpAddr;

    pub fn audit() -> Result<ScanResult, String> {
        let path = hosts_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(ScanResult {
                scan: "hosts-file".into(),
                findings: Vec::new(),
                summary: "Couldn't read the hosts file.".into(),
                unavailable: true,
            });
        };

        let mut findings: Vec<Finding> = Vec::new();
        let mut total = 0usize;

        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Drop any trailing inline comment, then split into IP + hostnames.
            let line = line.split('#').next().unwrap_or(line).trim();
            let mut parts = line.split_whitespace();
            let Some(ip_str) = parts.next() else {
                continue;
            };
            let domains: Vec<&str> = parts.collect();
            if domains.is_empty() {
                continue;
            }
            let Ok(ip) = ip_str.parse::<IpAddr>() else {
                continue;
            };
            total += domains.len();

            if ip.is_unspecified() || ip.is_loopback() {
                // Blackholed. Only an alarm when a security/OS/finance domain is
                // the target — ad-block lists blackhole thousands of ad domains.
                for domain in &domains {
                    if is_sensitive_domain(&domain.to_lowercase()) {
                        findings.push(Finding {
                            id: format!("hosts:block:{domain}"),
                            category: "hosts".into(),
                            severity: Severity::High,
                            title: (*domain).to_string(),
                            detail: format!(
                                "Blocked by your hosts file (points to {ip_str}). Malware blackholes security, update, or banking domains to disable protection."
                            ),
                            recommendation:
                                "If you didn't add this, remove the line from C:\\Windows\\System32\\drivers\\etc\\hosts."
                                    .into(),
                            fix: Some(reveal(&path)),
                            kinds: None,
                            preview_path: None,
                        });
                    }
                }
            } else if is_public_ip(&ip) {
                // A real redirect to a public server — the strongest signal.
                for domain in &domains {
                    findings.push(Finding {
                        id: format!("hosts:redirect:{domain}:{ip_str}"),
                        category: "hosts".into(),
                        severity: Severity::High,
                        title: (*domain).to_string(),
                        detail: format!(
                            "Redirected to {ip_str} by your hosts file — traffic for this domain goes to that server instead of the real one."
                        ),
                        recommendation:
                            "If you didn't set this up, remove the line from C:\\Windows\\System32\\drivers\\etc\\hosts."
                                .into(),
                        fix: Some(reveal(&path)),
                        kinds: None,
                        preview_path: None,
                    });
                }
            }
            // Private / LAN mappings (192.168.x dev hosts) are normal — skipped.
        }

        findings.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

        let summary = if findings.is_empty() {
            format!(
                "{total} hosts entr{} — no suspicious redirects.",
                if total == 1 { "y" } else { "ies" }
            )
        } else {
            format!(
                "{} suspicious hosts entr{} of {total}.",
                findings.len(),
                if findings.len() == 1 { "y" } else { "ies" }
            )
        };

        Ok(ScanResult {
            scan: "hosts-file".into(),
            findings,
            summary,
            unavailable: false,
        })
    }

    fn hosts_path() -> String {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        format!("{root}\\System32\\drivers\\etc\\hosts")
    }

    fn reveal(path: &str) -> FindingFix {
        FindingFix {
            label: "Reveal file".into(),
            action: "reveal-file".into(),
            target: path.to_string(),
        }
    }

    fn is_sensitive_domain(host: &str) -> bool {
        const NEEDLES: &[&str] = &[
            "windowsupdate",
            "update.microsoft",
            "microsoft.com",
            "mcafee",
            "norton",
            "symantec",
            "avast",
            "avg.com",
            "kaspersky",
            "bitdefender",
            "malwarebytes",
            "windowsdefender",
            "sophos",
            "eset",
            "trendmicro",
            "clamav",
            "virustotal",
            "paypal",
            "chase.com",
            "wellsfargo",
            "bankofamerica",
        ];
        NEEDLES.iter().any(|n| host.contains(n))
    }

    fn is_public_ip(ip: &IpAddr) -> bool {
        match ip {
            IpAddr::V4(v4) => {
                !v4.is_loopback()
                    && !v4.is_private()
                    && !v4.is_link_local()
                    && !v4.is_unspecified()
                    && !v4.is_broadcast()
                    && !v4.is_multicast()
            }
            IpAddr::V6(v6) => {
                let seg0 = v6.segments()[0];
                !v6.is_loopback()
                    && !v6.is_unspecified()
                    && !v6.is_multicast()
                    && (seg0 & 0xffc0) != 0xfe80
                    && (seg0 & 0xfe00) != 0xfc00
            }
        }
    }
}

#[cfg(windows)]
mod dev {
    //! Developer-secrets audit. Bounded recursive walk of dev folders looking
    //! for exposed `.env` files and private keys. Read-only.

    use super::{Finding, FindingFix, ScanResult, Severity};
    use std::path::{Path, PathBuf};

    const MAX_DEPTH: usize = 5;
    const MAX_FINDINGS: usize = 200;
    const MAX_ENTRIES: usize = 60_000;
    const MAX_READ: usize = 64 * 1024;

    pub fn audit() -> Result<ScanResult, String> {
        let roots = dev_roots();
        let mut findings: Vec<Finding> = Vec::new();
        let mut budget = 0usize;

        for root in &roots {
            if findings.len() >= MAX_FINDINGS || budget >= MAX_ENTRIES {
                break;
            }
            walk(root, 0, &mut findings, &mut budget);
        }

        findings.sort_by(|a, b| {
            super::severity_rank(b.severity)
                .cmp(&super::severity_rank(a.severity))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });

        let summary = if findings.is_empty() {
            "No exposed .env files or private keys in your developer folders.".into()
        } else {
            format!(
                "{} exposed secret file{} in your developer folders.",
                findings.len(),
                if findings.len() == 1 { "" } else { "s" }
            )
        };

        Ok(ScanResult {
            scan: "dev-secrets".into(),
            findings,
            summary,
            unavailable: false,
        })
    }

    fn dev_roots() -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        if let Ok(up) = std::env::var("USERPROFILE") {
            let home = PathBuf::from(&up);
            let candidates = [
                home.join("Desktop"),
                home.join("Documents"),
                home.join("Downloads"),
                home.join("source"),
                home.join("source").join("repos"),
                home.join("repos"),
                home.join("Projects"),
                home.join("projects"),
                home.join("dev"),
                home.join("code"),
                home.join("git"),
            ];
            for c in candidates {
                if c.is_dir() && !out.contains(&c) {
                    out.push(c);
                }
            }
        }
        out
    }

    fn walk(dir: &Path, depth: usize, findings: &mut Vec<Finding>, budget: &mut usize) {
        if depth > MAX_DEPTH || findings.len() >= MAX_FINDINGS || *budget >= MAX_ENTRIES {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let in_git_repo = dir.join(".git").exists();
        for entry in entries.flatten() {
            if findings.len() >= MAX_FINDINGS || *budget >= MAX_ENTRIES {
                return;
            }
            *budget += 1;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            if path.is_dir() {
                if !is_skip_dir(&lower) {
                    walk(&path, depth + 1, findings, budget);
                }
                continue;
            }
            if is_env_file(&lower) {
                push_env_finding(&path, &name, in_git_repo, dir, findings);
            } else if is_private_key_file(&lower) {
                findings.push(Finding {
                    id: format!("devkey:{}", path.to_string_lossy()),
                    category: "dev-secrets".into(),
                    severity: Severity::High,
                    title: name.clone(),
                    detail: format!("A private key file on disk · {}", path.to_string_lossy()),
                    recommendation:
                        "Keep private keys out of project folders — move it somewhere secure and make sure it's never committed."
                            .into(),
                    fix: Some(reveal(&path)),
                    kinds: Some(vec!["private_key_pem".into()]),
                    preview_path: Some(path.to_string_lossy().into_owned()),
                });
            }
        }
    }

    fn push_env_finding(
        path: &Path,
        name: &str,
        in_git_repo: bool,
        dir: &Path,
        findings: &mut Vec<Finding>,
    ) {
        let content = read_capped(path);
        let kinds = if content.is_empty() {
            Vec::new()
        } else {
            crate::commands::sensitive_scan::scan_text(&content)
        };
        let committed = in_git_repo && !gitignored(dir, name);
        let severity = if !kinds.is_empty() || committed {
            Severity::High
        } else {
            Severity::Medium
        };
        let detail = if committed {
            format!(
                "An environment file that is NOT gitignored — likely committed to git · {}",
                path.to_string_lossy()
            )
        } else if !kinds.is_empty() {
            format!(
                "An environment file containing credentials · {}",
                path.to_string_lossy()
            )
        } else {
            format!("An environment file on disk · {}", path.to_string_lossy())
        };
        findings.push(Finding {
            id: format!("devenv:{}", path.to_string_lossy()),
            category: "dev-secrets".into(),
            severity,
            title: name.to_string(),
            detail,
            recommendation:
                "Add it to .gitignore and move real secrets into a secret manager. If it was committed, rotate the exposed keys."
                    .into(),
            fix: Some(reveal(path)),
            kinds: if kinds.is_empty() {
                None
            } else {
                Some(kinds.iter().map(|k| k.to_string()).collect())
            },
            preview_path: Some(path.to_string_lossy().into_owned()),
        });
    }

    fn reveal(path: &Path) -> FindingFix {
        FindingFix {
            label: "Reveal file".into(),
            action: "reveal-file".into(),
            target: path.to_string_lossy().to_string(),
        }
    }

    fn read_capped(path: &Path) -> String {
        use std::io::Read;
        let Ok(mut f) = std::fs::File::open(path) else {
            return String::new();
        };
        let mut buf = vec![0u8; MAX_READ];
        let n = f.read(&mut buf).unwrap_or(0);
        buf.truncate(n);
        String::from_utf8_lossy(&buf).to_string()
    }

    /// Best-effort: does `dir/.gitignore` ignore `name`? Not a full gitignore
    /// engine — just enough to avoid false "committed" alarms for the common
    /// `.env` patterns.
    fn gitignored(dir: &Path, name: &str) -> bool {
        let Ok(text) = std::fs::read_to_string(dir.join(".gitignore")) else {
            return false;
        };
        let lower = name.to_lowercase();
        for line in text.lines() {
            let pat = line.trim().trim_start_matches('/').to_lowercase();
            if pat.is_empty() || pat.starts_with('#') {
                continue;
            }
            if pat == lower
                || (lower.starts_with(".env") && (pat == ".env" || pat == ".env*" || pat == "*.env"))
            {
                return true;
            }
        }
        false
    }

    fn is_env_file(lower: &str) -> bool {
        lower == ".env" || lower.starts_with(".env.")
    }

    fn is_private_key_file(lower: &str) -> bool {
        matches!(lower, "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519")
            || lower.ends_with(".pem")
            || lower.ends_with(".key")
            || lower.ends_with(".pfx")
            || lower.ends_with(".p12")
            || lower.ends_with(".ppk")
    }

    fn is_skip_dir(lower: &str) -> bool {
        matches!(
            lower,
            "node_modules"
                | ".git"
                | "target"
                | "dist"
                | "build"
                | "vendor"
                | ".next"
                | ".nuxt"
                | ".svelte-kit"
                | "__pycache__"
                | ".venv"
                | "venv"
                | "obj"
                | ".cargo"
                | ".gradle"
                | ".idea"
                | ".vscode"
        )
    }
}

#[cfg(windows)]
mod sched {
    //! Task Scheduler persistence audit via `schtasks /query /fo csv /v`.
    //! Reuses the startup-programs suspicion heuristic. Read-only.

    use super::{Finding, FindingFix, ScanResult, Severity};
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    // CREATE_NO_WINDOW — same reason as the net module: suppress the
    // visible cmd flash on every Privacy Audit Scan press.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn audit() -> Result<ScanResult, String> {
        let Ok(output) = Command::new("schtasks")
            .args(["/query", "/fo", "csv", "/v"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        else {
            return Ok(ScanResult {
                scan: "scheduled-tasks".into(),
                findings: Vec::new(),
                summary: "Couldn't query Task Scheduler.".into(),
                unavailable: true,
            });
        };
        let text = String::from_utf8_lossy(&output.stdout);

        let mut name_idx: Option<usize> = None;
        let mut run_idx: Option<usize> = None;
        let mut total = 0usize;
        let mut findings: Vec<Finding> = Vec::new();

        for line in text.lines() {
            let cols = parse_csv_line(line);
            if cols.is_empty() {
                continue;
            }
            // Verbose output repeats the header per folder — (re)learn columns.
            let is_header = cols.iter().any(|c| c.eq_ignore_ascii_case("TaskName"))
                && cols.iter().any(|c| c.eq_ignore_ascii_case("Task To Run"));
            if is_header {
                name_idx = cols.iter().position(|c| c.eq_ignore_ascii_case("TaskName"));
                run_idx = cols
                    .iter()
                    .position(|c| c.eq_ignore_ascii_case("Task To Run"));
                continue;
            }
            let (Some(ni), Some(ri)) = (name_idx, run_idx) else {
                continue;
            };
            if cols.len() <= ni.max(ri) {
                continue;
            }
            let task_name = cols[ni].trim();
            let task_run = cols[ri].trim();
            if task_name.is_empty() || task_run.is_empty() || task_run.eq_ignore_ascii_case("N/A") {
                continue;
            }
            total += 1;
            let exe = super::extract_exe_path(task_run);
            let Some(reason) = super::suspicious_startup_reason(&exe.to_lowercase()) else {
                continue;
            };
            let fix = if !exe.is_empty() && std::path::Path::new(&exe).exists() {
                Some(FindingFix {
                    label: "Reveal file".into(),
                    action: "reveal-file".into(),
                    target: exe.clone(),
                })
            } else {
                None
            };
            let short = task_name
                .rsplit('\\')
                .next()
                .unwrap_or(task_name)
                .to_string();
            findings.push(Finding {
                id: format!("sched:{task_name}"),
                category: "scheduled-task".into(),
                severity: Severity::High,
                title: short,
                detail: format!("Scheduled task runs {reason} · {task_run}"),
                recommendation:
                    "If you don't recognise this, delete it in Task Scheduler (taskschd.msc). Malware often re-creates tasks here to survive reboots."
                        .into(),
                fix,
                kinds: None,
                preview_path: None,
            });
        }

        findings.sort_by(|a, b| a.id.cmp(&b.id));
        findings.dedup_by(|a, b| a.id == b.id);
        findings.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));

        let summary = if total == 0 {
            "No scheduled tasks found (or access was denied).".into()
        } else if findings.is_empty() {
            format!("{total} scheduled tasks — none run from unusual locations.")
        } else {
            format!(
                "{} of {total} scheduled tasks run from unusual locations.",
                findings.len()
            )
        };

        Ok(ScanResult {
            scan: "scheduled-tasks".into(),
            findings,
            summary,
            unavailable: false,
        })
    }

    /// Minimal CSV line parser for schtasks' quoted output.
    fn parse_csv_line(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_quotes = false;
        for ch in line.chars() {
            match ch {
                '"' => in_quotes = !in_quotes,
                ',' if !in_quotes => out.push(std::mem::take(&mut cur)),
                _ => cur.push(ch),
            }
        }
        out.push(cur);
        out
    }
}
