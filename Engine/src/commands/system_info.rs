//! One-shot machine snapshot for the command-palette "Commands" chip —
//! OS, CPU, RAM, battery, uptime, and per-volume disk usage.
//!
//! All probes are local: sysinfo (the same crate `core::resources` uses for
//! RAM/cores/disk) plus a single Win32 `GetSystemPowerStatus` call for the
//! battery percentage. No network, no telemetry.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    /// Mount point / drive root (e.g. "C:\\").
    pub mount: String,
    pub free_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub os_name: String,
    pub os_version: String,
    pub cpu_model: String,
    pub logical_cores: u32,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    /// Battery charge 0–100, or None on a desktop / when unreadable.
    pub battery_percent: Option<u8>,
    pub uptime_secs: u64,
    pub disks: Vec<DiskInfo>,
}

#[tauri::command(async)]
pub fn system_info() -> Result<SystemInfo, String> {
    use sysinfo::{CpuRefreshKind, RefreshKind, System};

    // Memory + CPU brand in a single System instance. We only refresh what we
    // read (memory + one CPU pass) to keep the probe cheap.
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_memory(sysinfo::MemoryRefreshKind::everything())
            .with_cpu(CpuRefreshKind::everything()),
    );
    // A second CPU refresh is the documented way to populate live CPU data,
    // but `brand()` is static and available after the first pass — no sleep
    // needed since we don't read usage percentages.
    sys.refresh_cpu_all();

    let os_name = System::name().unwrap_or_else(|| "Unknown".to_string());
    // Prefer the long OS string ("Windows 11 Pro") when available; fall back
    // to the short version number.
    let os_version = System::long_os_version()
        .or_else(System::os_version)
        .unwrap_or_else(|| "Unknown".to_string());

    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown".to_string());

    let logical_cores = crate::core::throttle::cpu_cores() as u32;

    let ram_total_bytes = sys.total_memory();
    // `total - available` is the conventional "used" figure (matches the
    // resources module's available_memory()-based math).
    let ram_used_bytes = ram_total_bytes.saturating_sub(sys.available_memory());

    let uptime_secs = System::uptime();

    let disks = collect_disks();
    let battery_percent = read_battery_percent();

    Ok(SystemInfo {
        os_name,
        os_version,
        cpu_model,
        logical_cores,
        ram_used_bytes,
        ram_total_bytes,
        battery_percent,
        uptime_secs,
        disks,
    })
}

/// Snapshot of every mounted volume's free + total bytes.
fn collect_disks() -> Vec<DiskInfo> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .map(|d| DiskInfo {
            mount: d.mount_point().to_string_lossy().to_string(),
            free_bytes: d.available_space(),
            total_bytes: d.total_space(),
        })
        .collect()
}

/// Battery charge as a 0–100 percent, or None on AC-only machines / when the
/// value is unknown (Win32 reports 255 = "unknown").
#[cfg(windows)]
fn read_battery_percent() -> Option<u8> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: GetSystemPowerStatus writes into the fully-owned, zero-init
    // SYSTEM_POWER_STATUS struct via an out-pointer; it borrows no memory of
    // ours past the call. An Err just means we report None.
    let ok = unsafe { GetSystemPowerStatus(&mut status) }.is_ok();
    if !ok {
        return None;
    }
    // 255 (0xFF) is the documented "unknown" sentinel.
    match status.BatteryLifePercent {
        255 => None,
        p => Some(p.min(100)),
    }
}

#[cfg(not(windows))]
fn read_battery_percent() -> Option<u8> {
    None
}
