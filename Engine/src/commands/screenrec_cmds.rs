//! Screen Recorder — Tauri command surface (always compiled).
//!
//! Thin wrappers around `screen_recorder::control` so the commands exist in every
//! build: with the `screenrec` feature they drive the recorder; without it they
//! return a clear "not available" error. This keeps `lib.rs` registration
//! unconditional while the heavy capture/encode impl stays feature-gated.

use serde::{Deserialize, Serialize};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Window labels + routes for the region selector and the floating capture
/// toolbar. Kept here (not in `lib.rs`) so the whole window surface lives
/// alongside the recorder commands. The labels are also listed in the
/// `default` capability so the webviews get the core window/event perms.
const REGION_WINDOW_LABEL: &str = "screenrec-region";
const REDACT_WINDOW_LABEL: &str = "screenrec-redact";
const TOOLBAR_WINDOW_LABEL: &str = "screenrec-toolbar";

/// Apply content protection (SetWindowDisplayAffinity WDA_EXCLUDEFROMCAPTURE)
/// to a freshly built window so the recorder's own chrome is NOT captured in
/// the recording. WDA_EXCLUDEFROMCAPTURE only exists on Windows 10 build 2004+
/// (19041); on OLDER builds the affinity falls back to WDA_MONITOR, which turns
/// the window SOLID BLACK in the capture — worse than just recording it. So we
/// gate on the real build number via RtlGetVersion (same pattern as
/// `supports_window_corner_rounding` in lib.rs ~869) and only set
/// `.content_protected(true)` when build >= 19041; otherwise we leave the
/// window unprotected (visible in the recording, but never a black box).
fn content_protected_supported() -> bool {
    #[cfg(windows)]
    {
        use windows::Wdk::System::SystemServices::RtlGetVersion;
        use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
        let mut info = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        // RtlGetVersion (ntdll) reports the REAL build number, unlike the
        // manifest-gated GetVersionEx. STATUS_SUCCESS == 0. 19041 == 2004.
        let status = unsafe { RtlGetVersion(&mut info) };
        status.0 == 0 && info.dwBuildNumber >= 19041
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// A capture region in monitor-relative physical pixels.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionArg {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// A top-level window the user can choose to HIDE from the recording: while
/// recording, its pixels become solid black via `SetWindowDisplayAffinity`
/// (`WDA_EXCLUDEFROMCAPTURE`) so nothing on it is captured. The `hwnd` is the
/// native handle as an integer, round-tripped back to `screenrec_start`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub hwnd: i64,
    pub title: String,
}

/// Enumerate visible, titled, top-level windows for the "hide windows" picker.
/// Skips invisible windows, floating tool windows (`WS_EX_TOOLWINDOW`),
/// untitled shells, and DWM-cloaked windows (UWP background hosts that aren't
/// really on screen). Returns empty off-Windows or if enumeration fails.
#[tauri::command(async)]
pub fn screenrec_list_windows() -> Vec<WindowInfo> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
            GWL_EXSTYLE, WS_EX_TOOLWINDOW,
        };

        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let out = &mut *(lparam.0 as *mut Vec<WindowInfo>);
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1); // continue enumeration, skip this one
            }
            // Floating palettes / tray helpers — not windows a user would "hide".
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TOOLWINDOW.0 != 0 {
                return BOOL(1);
            }
            // DWM-cloaked = present but not actually shown (background UWP host).
            let mut cloaked: u32 = 0;
            let _ = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut core::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            );
            if cloaked != 0 {
                return BOOL(1);
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return BOOL(1); // untitled shell window
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            let n = GetWindowTextW(hwnd, &mut buf);
            if n <= 0 {
                return BOOL(1);
            }
            let title = String::from_utf16_lossy(&buf[..n as usize]);
            out.push(WindowInfo { hwnd: hwnd.0 as isize as i64, title });
            BOOL(1)
        }

        let mut out: Vec<WindowInfo> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut out as *mut _ as isize));
        }
        out
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecStatus {
    pub recording: bool,
    /// Recorded duration so far, EXCLUDING paused time (the file's true length).
    pub elapsed_ms: u64,
    pub output_bytes: u64,
    /// The encoder died mid-take (disk full / fault) — the UI should stop + save
    /// the partial instead of believing the recording is still healthy.
    pub failed: bool,
    /// The recording is paused (clock frozen, audio dropped) — resume to continue.
    pub paused: bool,
}

#[cfg(not(feature = "screenrec"))]
const UNAVAILABLE: &str = "Screen recorder is not available in this build";

#[tauri::command(async)]
pub fn screenrec_start(
    out_path: String,
    fps: u32,
    region: Option<RegionArg>,
    redactions: Vec<RegionArg>,
    exclude_windows: Vec<i64>,
    system_audio: bool,
    mic: bool,
    audio_sync_ms: i32,
    quality_bpp: f64,
) -> Result<(), String> {
    #[cfg(feature = "screenrec")]
    {
        use crate::commands::screen_recorder::audio::AudioOptions;
        use crate::commands::screen_recorder::{control, CropRect};
        let region = region.map(|r| CropRect { x: r.x, y: r.y, w: r.w, h: r.h });
        let redactions =
            redactions.into_iter().map(|r| CropRect { x: r.x, y: r.y, w: r.w, h: r.h }).collect();
        control::start(
            out_path,
            fps,
            region,
            redactions,
            exclude_windows,
            AudioOptions { system: system_audio, mic },
            audio_sync_ms,
            quality_bpp,
        )
    }
    #[cfg(not(feature = "screenrec"))]
    {
        let _ = (
            out_path,
            fps,
            region,
            redactions,
            exclude_windows,
            system_audio,
            mic,
            audio_sync_ms,
            quality_bpp,
        );
        Err(UNAVAILABLE.into())
    }
}

#[tauri::command(async)]
pub fn screenrec_stop() -> Result<String, String> {
    #[cfg(feature = "screenrec")]
    {
        crate::commands::screen_recorder::control::stop()
    }
    #[cfg(not(feature = "screenrec"))]
    {
        Err(UNAVAILABLE.into())
    }
}

#[tauri::command]
pub fn screenrec_status() -> RecStatus {
    #[cfg(feature = "screenrec")]
    {
        let (recording, elapsed_ms, output_bytes, failed, paused) =
            crate::commands::screen_recorder::control::status();
        RecStatus { recording, elapsed_ms, output_bytes, failed, paused }
    }
    #[cfg(not(feature = "screenrec"))]
    {
        RecStatus { recording: false, elapsed_ms: 0, output_bytes: 0, failed: false, paused: false }
    }
}

/// Pause the active recording (clock frozen, audio dropped). No-op-ish error if not
/// recording. The file gains no frozen segment.
#[tauri::command(async)]
pub fn screenrec_pause() -> Result<(), String> {
    #[cfg(feature = "screenrec")]
    {
        crate::commands::screen_recorder::control::pause()
    }
    #[cfg(not(feature = "screenrec"))]
    {
        Err(UNAVAILABLE.into())
    }
}

/// Resume a paused recording.
#[tauri::command(async)]
pub fn screenrec_resume() -> Result<(), String> {
    #[cfg(feature = "screenrec")]
    {
        crate::commands::screen_recorder::control::resume()
    }
    #[cfg(not(feature = "screenrec"))]
    {
        Err(UNAVAILABLE.into())
    }
}

/// Export a finished recording to an optimised GIF next to it. Returns the GIF path.
#[tauri::command(async)]
pub fn screenrec_export_gif(src_path: String) -> Result<String, String> {
    #[cfg(feature = "screenrec")]
    {
        crate::commands::screen_recorder::control::export_gif(src_path)
    }
    #[cfg(not(feature = "screenrec"))]
    {
        let _ = src_path;
        Err(UNAVAILABLE.into())
    }
}

// ─── Recorder helper windows ─────────────────────────────────────────────
//
// These four commands create/close the region-selection overlay and the
// floating capture toolbar. They are PURE Tauri + windows-crate (no capture
// or encode), so they live in the always-compiled command surface and are
// registered in `lib.rs` UNCONDITIONALLY — they work even in a build without
// the `screenrec` feature (selecting a region / showing the bar is harmless;
// only `screenrec_start` itself is feature-gated).
//
// Both windows get content protection so the recorder's own chrome never
// shows up in the recording (gated to Win10 2004+; see
// `content_protected_supported`). The region overlay covers the whole primary
// monitor and is resized to it AFTER build (mirroring the mouse-grid overlay
// in lib.rs — `primary_monitor()` is only reliable once the window exists).

/// Open the full-screen region-selection overlay over the primary monitor.
/// The route at `/region-select` draws a draggable rectangle and emits
/// `screenrec:region-selected` ({x,y,w,h} monitor-relative PHYSICAL px) on
/// commit or `screenrec:region-cancelled` on Esc, then closes itself.
///
/// Reopen-safe: if an overlay is already open it is closed and rebuilt, so a
/// repeated invoke never errors with "window label already exists".
#[tauri::command(async)]
pub fn screenrec_open_region_selector(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(REGION_WINDOW_LABEL) {
        let _ = existing.close();
    }
    let mut builder =
        WebviewWindowBuilder::new(&app, REGION_WINDOW_LABEL, WebviewUrl::App("/region-select".into()))
            .title("Select recording region")
            // Placeholder — resized to the full primary monitor before show.
            .inner_size(800.0, 600.0)
            .position(0.0, 0.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .visible(false);
    if content_protected_supported() {
        builder = builder.content_protected(true);
    }
    let window = builder
        .build()
        .map_err(|error| format!("Cannot create region selector: {error}"))?;
    // Cover the entire primary monitor (origin 0,0 for the primary) so the
    // overlay's CSS-pixel rect maps 1:1 onto monitor coords via devicePixelRatio.
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let _ = window.set_position(*monitor.position());
        let _ = window.set_size(*monitor.size());
    }
    let _ = window.unminimize();
    window
        .show()
        .map_err(|error| format!("Cannot show region selector: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Cannot focus region selector: {error}"))?;
    Ok(())
}

/// Close the region-selection overlay if it is open. Idempotent — closing an
/// already-closed (or never-opened) overlay is a no-op success.
#[tauri::command(async)]
pub fn screenrec_close_region_selector(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(REGION_WINDOW_LABEL) {
        window
            .close()
            .map_err(|error| format!("Cannot close region selector: {error}"))?;
    }
    Ok(())
}

/// Open the full-screen REDACTION overlay over the primary monitor. The route at
/// `/redact-select` lets the user drag multiple black-out rectangles and emits
/// `screenrec:redactions-selected` ([{x,y,w,h} physical px]) on Done, or
/// `screenrec:redactions-cancelled` on Esc. Same monitor-covering geometry as the
/// region selector. Reopen-safe.
#[tauri::command(async)]
pub fn screenrec_open_redact_selector(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(REDACT_WINDOW_LABEL) {
        let _ = existing.close();
    }
    let mut builder =
        WebviewWindowBuilder::new(&app, REDACT_WINDOW_LABEL, WebviewUrl::App("/redact-select".into()))
            .title("Mark private areas")
            .inner_size(800.0, 600.0)
            .position(0.0, 0.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .visible(false);
    if content_protected_supported() {
        builder = builder.content_protected(true);
    }
    let window = builder
        .build()
        .map_err(|error| format!("Cannot create redaction selector: {error}"))?;
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let _ = window.set_position(*monitor.position());
        let _ = window.set_size(*monitor.size());
    }
    let _ = window.unminimize();
    window
        .show()
        .map_err(|error| format!("Cannot show redaction selector: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Cannot focus redaction selector: {error}"))?;
    Ok(())
}

/// Close the redaction overlay if it is open. Idempotent.
#[tauri::command(async)]
pub fn screenrec_close_redact_selector(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(REDACT_WINDOW_LABEL) {
        window
            .close()
            .map_err(|error| format!("Cannot close redaction selector: {error}"))?;
    }
    Ok(())
}

/// Open the floating capture toolbar (route `/capture-bar`): a small,
/// content-protected, always-on-top control strip so the recorder's
/// Stop/Pause/region controls are NOT captured in the recording. Positioned
/// bottom-center of the primary monitor.
///
/// Reopen-safe: an already-open toolbar is closed and rebuilt.
#[tauri::command(async)]
pub fn screenrec_open_toolbar(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(TOOLBAR_WINDOW_LABEL) {
        let _ = existing.close();
    }
    // Logical size — the toolbar is a fixed small strip, DPI-scaled by Tauri.
    const TOOLBAR_W: f64 = 320.0;
    const TOOLBAR_H: f64 = 56.0;
    let mut builder =
        WebviewWindowBuilder::new(&app, TOOLBAR_WINDOW_LABEL, WebviewUrl::App("/capture-bar".into()))
            .title("Recording controls")
            .inner_size(TOOLBAR_W, TOOLBAR_H)
            // Placeholder position — re-centered to the primary monitor below.
            .position(200.0, 200.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .visible(false);
    if content_protected_supported() {
        builder = builder.content_protected(true);
    }
    let window = builder
        .build()
        .map_err(|error| format!("Cannot create capture toolbar: {error}"))?;
    // Center horizontally, 24 logical px above the bottom of the primary
    // monitor's work area. primary_monitor() gives PHYSICAL px + scale, so
    // convert the toolbar's logical footprint to physical before centering.
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let pos = monitor.position();
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let bar_w_phys = (TOOLBAR_W * scale).round() as i32;
        let bar_h_phys = (TOOLBAR_H * scale).round() as i32;
        let margin_phys = (24.0 * scale).round() as i32;
        let x = pos.x + (size.width as i32 - bar_w_phys) / 2;
        let y = pos.y + size.height as i32 - bar_h_phys - margin_phys;
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
    let _ = window.unminimize();
    window
        .show()
        .map_err(|error| format!("Cannot show capture toolbar: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("Cannot focus capture toolbar: {error}"))?;
    Ok(())
}

/// Close the floating capture toolbar if it is open. Idempotent.
#[tauri::command(async)]
pub fn screenrec_close_toolbar(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(TOOLBAR_WINDOW_LABEL) {
        window
            .close()
            .map_err(|error| format!("Cannot close capture toolbar: {error}"))?;
    }
    Ok(())
}

