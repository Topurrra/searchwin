// Shared utilities

// Windows DPAPI wrapper for at-rest encryption of the local redb database.
// Public so commands/local_db.rs can call it; cfg-gated to Windows so the
// module compiles cleanly on non-Windows targets (currently we only ship
// Windows, but the gate keeps the door open for cross-platform work).
#[cfg(windows)]
pub mod dpapi;

// Path-validation primitives. Every Tauri command that takes a path from
// the frontend should route it through `resolve_safe_path` or
// `resolve_safe_target` against the appropriate root list (user's chosen
// folder, AppData, etc.) before doing any FS work.
pub mod safe_path;

// Adaptive resource awareness — live RAM / CPU / disk probes (sysinfo) plus
// pure, unit-tested decision logic (`recommend_workers`, `disk_preflight`).
// Ported from the kil-privacy-suite; the single source of truth for "how many
// workers can this machine sustain right now." Used by the Cleaner/Analyzer to
// size its WalkDir concurrency by live free RAM (only ever reduces under
// pressure). See the consolidation note: search.rs's private
// `available_ram_bytes()` can later delegate here.
pub mod resources;
