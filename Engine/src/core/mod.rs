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

// How hard the engine may work right now: live cores, free RAM and free
// disk, and a worker count sized to them, leaving room for the browser.
pub mod throttle;

// Programs installed as packs by the browser (FFmpeg…): where they are.
pub mod packs;
