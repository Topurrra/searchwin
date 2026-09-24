//! How hard the engine may work right now.
//!
//! Written for Search: the engine shares the machine with a browser the
//! person is actively using, so background work sizes itself to what is
//! free *now* — one core is always left for the browser, and a floor of
//! memory is never touched. Everything here is a cheap probe; nothing
//! caches, because "now" is the point.

use std::path::Path;

/// The memory one directory-walking worker is budgeted: its queue, the
/// paths it holds, and the metadata it reads.
pub const WALK_WORKER_RAM_BYTES: u64 = 64 * 1024 * 1024;

/// Memory left alone whatever the engine is doing, so the browser (and
/// whatever else is open) never pages because of a scan.
const RESERVE_BYTES: u64 = 1024 * 1024 * 1024;

/// However many cores and however much memory, no more walkers than this:
/// past it, the disk is the bottleneck and more threads only add contention.
const MAX_WORKERS: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct DeviceProfile {
    pub total_ram_bytes: u64,
    pub cpu_cores: usize,
}

/// The machine, as far as sizing work goes. `total_ram_bytes` is 0 when the
/// probe fails.
pub fn device_profile() -> DeviceProfile {
    DeviceProfile {
        total_ram_bytes: memory().map(|(total, _)| total).unwrap_or(0),
        cpu_cores: cpu_cores(),
    }
}

/// Logical processors this process may run on.
pub fn cpu_cores() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

/// Physical memory free right now; `u64::MAX` when it can't be read, so
/// callers that divide by it never shrink work because of a failed probe.
pub fn available_ram_bytes() -> u64 {
    memory().map(|(_, free)| free).unwrap_or(u64::MAX)
}

/// How many workers to run: every core but one, cut down when free memory
/// (less the reserve) can't carry that many at `per_worker_bytes` each.
/// Never fewer than one.
pub fn recommend_workers_with(cores: usize, available_bytes: u64, per_worker_bytes: u64) -> usize {
    let by_cores = cores.saturating_sub(1).max(1);
    let by_memory = if available_bytes == u64::MAX || per_worker_bytes == 0 {
        usize::MAX
    } else {
        let spare = available_bytes.saturating_sub(RESERVE_BYTES);
        usize::try_from(spare / per_worker_bytes).unwrap_or(usize::MAX)
    };
    by_cores.min(by_memory).clamp(1, MAX_WORKERS)
}

/// Free space on the volume `path` is (or would be) on, for the caller.
/// Walks up to the nearest folder that exists, so a file not yet written
/// still gets an answer.
pub fn free_disk_bytes(path: &Path) -> Option<u64> {
    let mut probe = path;
    while !probe.exists() {
        probe = probe.parent()?;
    }
    free_on(probe)
}

#[cfg(windows)]
fn memory() -> Option<(u64, u64)> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    Some((status.ullTotalPhys, status.ullAvailPhys))
}

#[cfg(windows)]
fn free_on(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free_to_caller = 0u64;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free_to_caller), None, None) }
        .ok()?;
    Some(free_to_caller)
}

#[cfg(not(windows))]
fn memory() -> Option<(u64, u64)> {
    None
}

#[cfg(not(windows))]
fn free_on(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn leaves_a_core_for_the_browser() {
        assert_eq!(recommend_workers_with(8, 64 * GIB, WALK_WORKER_RAM_BYTES), 7);
        assert_eq!(recommend_workers_with(1, 64 * GIB, WALK_WORKER_RAM_BYTES), 1);
    }

    #[test]
    fn shrinks_when_memory_is_short() {
        // 1 GiB reserve + room for exactly two walkers.
        let available = GIB + 2 * WALK_WORKER_RAM_BYTES;
        assert_eq!(recommend_workers_with(16, available, WALK_WORKER_RAM_BYTES), 2);
        assert_eq!(recommend_workers_with(16, GIB / 2, WALK_WORKER_RAM_BYTES), 1);
    }

    #[test]
    fn an_unreadable_probe_does_not_shrink_work() {
        assert_eq!(recommend_workers_with(4, u64::MAX, WALK_WORKER_RAM_BYTES), 3);
    }

    #[test]
    fn caps_at_the_disk_bound() {
        assert_eq!(recommend_workers_with(64, 512 * GIB, WALK_WORKER_RAM_BYTES), MAX_WORKERS);
    }

    #[test]
    fn free_space_for_a_path_not_yet_written() {
        let target = std::env::temp_dir().join("search-throttle-test").join("not-yet.mp4");
        assert!(free_disk_bytes(&target).is_some_and(|free| free > 0));
    }
}
