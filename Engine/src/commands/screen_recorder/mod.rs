//! Screen Recorder — Stage 1: Windows.Graphics.Capture (WGC) screen capture.
//!
//! A dedicated MTA thread owns a D3D11 device + a free-threaded WGC frame pool
//! for the target (primary monitor for now). On each `FrameArrived` we GPU-crop
//! the *current* region rectangle into a CPU-readable staging texture, `Map` it,
//! and emit a tightly-packed BGRA [`Frame`] on a bounded channel for the encoder
//! to drain. The crop rect lives behind a shared lock so it can be live-swapped
//! while recording with zero session re-init (the headline "move/resize while
//! recording" feature) — only the staging texture is re-created when the crop
//! *size* changes.
//!
//! Pixel format is `B8G8R8A8_UNORM` (BGRA) end-to-end — exactly what ffmpeg
//! wants as `-pix_fmt bgra`, so there is no colour-channel swap in the hot path.
//!
//! Three WGC/D3D hazards are handled explicitly (each cost a crash to find):
//!   1. **Multithread protection** — WGC drives the device from its own threads
//!      while our callback uses the immediate context; without
//!      `ID3D11Multithread::SetMultithreadProtected` this co-use is an AV.
//!   2. **Teardown serialisation** — closing the pool while a `FrameArrived`
//!      callback is mid-flight is a use-after-free; an `active` lock serialises
//!      the callback against teardown, and the callback bails once stopping.
//!   3. **Async teardown settle** — WGC tears its worker threads down
//!      asynchronously after `Close()`; we hold our D3D refs for a short settle
//!      window before releasing them so WGC finishes first.
//!
//! The one correctness hazard — the staging `RowPitch` being wider than `width*4`
//! (row alignment), which shears the image if copied naively — is isolated in the
//! pure [`pack_bgra`] function and unit-tested.
#![cfg(feature = "screenrec")]
// The capture/encode API is exercised by tests now and becomes live in the app
// when wired to Tauri commands in Stage 3. Until then the lib build sees it as
// uncalled — silence the dead-code noise rather than scatter per-item allows.
#![allow(dead_code)]

pub mod audio;
pub mod control;
pub mod encode;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::core::{IInspectable, Interface};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{BOOL, HMODULE, HWND, POINT, RECT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, HMONITOR, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsIconic, IsWindow, IsWindowVisible,
};

/// What to capture. (Window/secondary-monitor selection lands with the Stage 3 UI.)
#[derive(Clone, Copy, Debug)]
pub enum CaptureTarget {
    PrimaryMonitor,
}

/// A sub-rectangle of the captured surface, in surface (physical) pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CropRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// One captured frame: tightly packed BGRA, `width*height*4` bytes.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Shared, live-swappable crop. `None` = whole captured surface.
pub type SharedCrop = Arc<Mutex<Option<CropRect>>>;

/// Shared list of redaction rectangles (MONITOR-relative physical px, same space
/// as the crop). Each is blacked out of every captured frame BEFORE encoding, so
/// the sensitive pixels never enter the file. Empty = no redaction (zero cost).
pub type SharedRedactions = Arc<Mutex<Vec<CropRect>>>;

/// Shared list of "hide these windows" HWNDs (as integers). Each frame we read the
/// window's CURRENT on-screen rect (cross-process safe — `GetWindowRect` only
/// reads) and black it out, so the box FOLLOWS the window as it moves. This is the
/// only cross-process way to hide a window: `SetWindowDisplayAffinity` requires the
/// window to belong to our OWN process (per the Win32 docs), so it can't hide
/// another app's window. Empty = none (zero cost).
pub type SharedExcludeWindows = Arc<Mutex<Vec<i64>>>;

/// Pause/resume state shared by the video pump and the audio lanes. ONE flag drives
/// both so the two streams stay in sync across a pause:
///   - the video CFR pump FREEZES its clock while paused (so `effective_elapsed`
///     stops advancing → no frames written → resume leaves no frozen gap), and
///   - the audio lanes DROP incoming samples while paused (so the audio skips the
///     same wall-time the video skipped).
/// Both therefore omit exactly the paused wall-time and line back up on resume.
pub struct PauseState {
    paused: AtomicBool,
    /// When the current pause began (`None` while running). Pump clock math only.
    paused_at: Mutex<Option<Instant>>,
    /// Accumulated paused wall-time (ms), folded in on each resume.
    total_paused_ms: AtomicU64,
}

impl PauseState {
    pub fn new() -> Self {
        PauseState {
            paused: AtomicBool::new(false),
            paused_at: Mutex::new(None),
            total_paused_ms: AtomicU64::new(0),
        }
    }

    /// Enter the paused state (idempotent — a second pause is a no-op).
    pub fn pause(&self) {
        if !self.paused.swap(true, Ordering::SeqCst) {
            if let Ok(mut at) = self.paused_at.lock() {
                *at = Some(Instant::now());
            }
        }
    }

    /// Leave the paused state, folding this pause's duration into the running total
    /// (idempotent — resuming a running recording is a no-op).
    pub fn resume(&self) {
        if self.paused.swap(false, Ordering::SeqCst) {
            if let Ok(mut at) = self.paused_at.lock() {
                if let Some(started) = at.take() {
                    let ms = started.elapsed().as_millis() as u64;
                    self.total_paused_ms.fetch_add(ms, Ordering::SeqCst);
                }
            }
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Wall-time since `t0` MINUS all paused time (the running total plus any pause
    /// in progress). This is the recording's true timeline — what the CFR pump and
    /// the elapsed readout both use, so a pause never adds duration to the file.
    pub fn effective_elapsed(&self, t0: Instant) -> Duration {
        let raw = t0.elapsed();
        let total = Duration::from_millis(self.total_paused_ms.load(Ordering::SeqCst));
        let in_progress = if self.paused.load(Ordering::SeqCst) {
            self.paused_at
                .lock()
                .ok()
                .and_then(|g| *g)
                .map(|p| p.elapsed())
                .unwrap_or_default()
        } else {
            Duration::ZERO
        };
        raw.saturating_sub(total).saturating_sub(in_progress)
    }
}

impl Default for PauseState {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared pause handle for one recording (pump + audio lanes + control commands).
pub type SharedPause = Arc<PauseState>;

/// A running capture. Dropping or calling [`CaptureSession::stop`] tears it down.
pub struct CaptureSession {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<Result<(), String>>>,
}

impl CaptureSession {
    /// Stop capturing and join the capture thread, surfacing any setup error.
    pub fn stop(mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::SeqCst);
        match self.handle.take() {
            Some(h) => h.join().map_err(|_| "capture thread panicked".to_string())?,
            None => Ok(()),
        }
    }
}

impl Drop for CaptureSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Per-capture mutable state shared with the `FrameArrived` callback. The mutex
/// serialises all device-context use (the immediate context is not free-threaded)
/// and caches the staging texture across frames, re-creating it only on a size change.
struct CaptureState {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    staging: Option<(ID3D11Texture2D, u32, u32)>,
}

/// Start capturing `target`. Returns the session handle and the frame receiver.
pub fn start_capture(
    target: CaptureTarget,
    crop: SharedCrop,
    redactions: SharedRedactions,
    exclude_windows: SharedExcludeWindows,
    capture_cursor: bool,
) -> Result<(CaptureSession, Receiver<Frame>), String> {
    let (tx, rx) = sync_channel::<Frame>(8);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();

    // Surface the setup result (device/pool creation) before returning so the
    // caller learns immediately if WGC is unavailable, rather than silently.
    let (ready_tx, ready_rx) = sync_channel::<Result<(), String>>(1);

    let handle = thread::Builder::new()
        .name("kil-screen-capture".into())
        .spawn(move || {
            capture_thread(
                target,
                crop,
                redactions,
                exclude_windows,
                capture_cursor,
                tx,
                stop_thread,
                ready_tx,
            )
        })
        .map_err(|e| format!("Failed to spawn capture thread: {e}"))?;

    match ready_rx.recv() {
        Ok(Ok(())) => Ok((CaptureSession { stop, handle: Some(handle) }, rx)),
        Ok(Err(e)) => {
            let _ = handle.join();
            Err(e)
        }
        Err(_) => {
            let _ = handle.join();
            Err("Capture thread exited before signalling readiness".to_string())
        }
    }
}

fn capture_thread(
    target: CaptureTarget,
    crop: SharedCrop,
    redactions: SharedRedactions,
    exclude_windows: SharedExcludeWindows,
    capture_cursor: bool,
    sink: SyncSender<Frame>,
    stop: Arc<AtomicBool>,
    ready: SyncSender<Result<(), String>>,
) -> Result<(), String> {
    // Dedicated MTA thread (the rule proven in the Stage 0 harness). Never CoUninitialize.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    // Serialises the FrameArrived callback against teardown so we never close the
    // frame pool while a callback is mid-flight (that is a use-after-free → AV).
    let active = Arc::new(Mutex::new(()));

    let setup = (|| -> windows::core::Result<(GraphicsCaptureSession, Direct3D11CaptureFramePool)> {
        // D3D11 device + immediate context.
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device = device.expect("device after successful D3D11CreateDevice");
        let context = context.expect("context after successful D3D11CreateDevice");

        // WGC's free-threaded frame pool drives the device from its OWN threads
        // while our callback uses the immediate context — which is single-threaded
        // by default. Without multithread protection this co-use is an access
        // violation. This is the load-bearing line for capture stability.
        let multithread: ID3D11Multithread = context.cast()?;
        let _ = unsafe { multithread.SetMultithreadProtected(BOOL::from(true)) };

        let dxgi: IDXGIDevice = device.cast()?;
        let d3d: IDirect3DDevice =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()? };

        // Capture item for the target.
        let item: GraphicsCaptureItem = match target {
            CaptureTarget::PrimaryMonitor => {
                let hmon: HMONITOR =
                    unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
                let interop =
                    windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
                unsafe { interop.CreateForMonitor(hmon)? }
            }
        };
        let size = item.Size()?;

        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &d3d,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )?;

        // Per-frame state shared with the callback.
        let state = Arc::new(Mutex::new(CaptureState { device, context, staging: None }));
        let crop_cb = crop.clone();
        let redactions_cb = redactions.clone();
        let exclude_cb = exclude_windows.clone();
        let sink_cb = sink.clone();
        let active_cb = active.clone();
        let stop_cb = stop.clone();

        let handler = TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(
            move |pool, _| -> windows::core::Result<()> {
                let Some(pool) = pool.as_ref() else { return Ok(()) };
                // Hold the active lock for the whole callback, and bail if we are
                // tearing down — never touch a pool that teardown is closing.
                let _active = match active_cb.lock() {
                    Ok(g) => g,
                    Err(_) => return Ok(()),
                };
                if stop_cb.load(Ordering::SeqCst) {
                    return Ok(());
                }
                // The callback runs across an FFI boundary; a panic must never
                // unwind through it (that aborts the process). Catch everything —
                // a per-frame failure just drops that frame, capture continues.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    process_frame(pool, &state, &crop_cb, &redactions_cb, &exclude_cb, &sink_cb)
                }));
                if let Err(_) | Ok(Err(_)) = outcome {
                    // Transient per-frame error/panic: drop the frame, keep going.
                }
                Ok(())
            },
        );
        pool.FrameArrived(&handler)?;

        let session = pool.CreateCaptureSession(&item)?;
        let _ = session.SetIsCursorCaptureEnabled(capture_cursor);
        // Best-effort: suppress the capture border where the OS supports it (Win11+).
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;
        Ok((session, pool))
    })();

    let (session, pool) = match setup {
        Ok(v) => {
            let _ = ready.send(Ok(()));
            v
        }
        Err(e) => {
            let msg = format!("WGC capture setup failed: {e}");
            let _ = ready.send(Err(msg.clone()));
            return Err(msg);
        }
    };

    // Park until stop; the callback does all per-frame work.
    while !stop.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(10));
    }

    {
        // Wait for any in-flight FrameArrived callback to finish before closing
        // the pool — closing under a live callback is a use-after-free (AV).
        let _g = active.lock();
        let _ = session.Close();
        let _ = pool.Close();
    }
    // WGC tears its internal worker threads down ASYNCHRONOUSLY after Close();
    // releasing our D3D device / pool refs before they finish faults inside WGC
    // (an async access violation that lands on whatever thread is running). Hold
    // every ref for a short settle window so WGC's teardown completes first.
    thread::sleep(Duration::from_millis(200));
    Ok(())
}

/// Per-frame work: acquire the WGC frame, GPU-crop into the staging texture, map
/// it, and emit a tightly packed BGRA frame. Returns errors instead of panicking
/// (it runs inside the FFI callback, which catches and drops on failure).
fn process_frame(
    pool: &Direct3D11CaptureFramePool,
    state: &Arc<Mutex<CaptureState>>,
    crop: &SharedCrop,
    redactions: &SharedRedactions,
    exclude_windows: &SharedExcludeWindows,
    sink: &SyncSender<Frame>,
) -> windows::core::Result<()> {
    let frame = pool.TryGetNextFrame()?;
    let surface = frame.Surface()?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
    let src_tex: ID3D11Texture2D = unsafe { access.GetInterface()? };
    let content = frame.ContentSize()?;
    let (cw, ch) = (content.Width.max(0) as u32, content.Height.max(0) as u32);
    let region = resolve_crop(crop, cw, ch);

    let mut st = match state.lock() {
        Ok(st) => st,
        Err(_) => return frame.Close(),
    };

    // (Re)create the staging texture when the crop size changes.
    let need_new = !matches!(st.staging, Some((_, w, h)) if w == region.w && h == region.h);
    if need_new {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: region.w,
            Height: region.h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let dev = st.device.clone();
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe { dev.CreateTexture2D(&desc, None, Some(&mut tex))? };
        st.staging = tex.map(|t| (t, region.w, region.h));
    }

    let Some((staging, _, _)) = st.staging.clone() else {
        return frame.Close();
    };

    let src_box = D3D11_BOX {
        left: region.x.max(0) as u32,
        top: region.y.max(0) as u32,
        front: 0,
        right: (region.x as i64 + region.w as i64).clamp(0, cw as i64) as u32,
        bottom: (region.y as i64 + region.h as i64).clamp(0, ch as i64) as u32,
        back: 1,
    };
    unsafe {
        st.context
            .CopySubresourceRegion(&staging, 0, 0, 0, 0, &src_tex, 0, Some(&src_box));
    }

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        st.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
    }
    let row_pitch = mapped.RowPitch as usize;
    let mut bgra = if !mapped.pData.is_null() && row_pitch >= region.w as usize * 4 {
        let needed = row_pitch * region.h as usize;
        let src = unsafe { std::slice::from_raw_parts(mapped.pData as *const u8, needed) };
        pack_bgra(src, row_pitch, region.w as usize, region.h as usize)
    } else {
        Vec::new()
    };
    unsafe { st.context.Unmap(&staging, 0) };
    drop(st);

    // Black out privacy regions BEFORE the frame leaves the capture thread — the
    // sensitive pixels never reach the encoder, let alone the file. Two sources:
    //   1. static redaction rectangles the user drew, and
    //   2. "hide window" boxes — each chosen window's CURRENT on-screen rect,
    //      re-read every frame so the box follows the window as it moves.
    if !bgra.is_empty() {
        redact_bgra(&mut bgra, &region, redactions);
        let win_rects = match exclude_windows.lock() {
            Ok(hwnds) if !hwnds.is_empty() => window_capture_rects(&hwnds),
            _ => Vec::new(),
        };
        if !win_rects.is_empty() {
            black_out_rects(&mut bgra, &region, &win_rects);
        }
    }

    // Drop-oldest on backpressure: never block the capture callback.
    if !bgra.is_empty() {
        let _ = sink.try_send(Frame { width: region.w, height: region.h, bgra });
    }
    frame.Close()
}

/// Black out each redaction rectangle in the packed BGRA frame. Rects are in the
/// same MONITOR-relative physical-pixel space as the crop; translate to frame-local
/// coords and clip to the frame. Privacy-correct: pixels are overwritten opaque
/// black (gone for good), not blurred. No-op when there are no rects.
fn redact_bgra(bgra: &mut [u8], frame: &CropRect, redactions: &SharedRedactions) {
    let Ok(rects) = redactions.lock() else { return };
    if rects.is_empty() {
        return;
    }
    black_out_rects(bgra, frame, &rects);
}

/// Black out each rectangle in the packed BGRA frame. Rects are in the same
/// MONITOR-relative physical-pixel space as the crop; translate to frame-local
/// coords and clip to the frame. Pixels are overwritten opaque black (gone for
/// good), not blurred. Shared by static redactions and per-frame "hide window"
/// boxes.
fn black_out_rects(bgra: &mut [u8], frame: &CropRect, rects: &[CropRect]) {
    let fw = frame.w as i64;
    let fh = frame.h as i64;
    let row_bytes = frame.w as usize * 4;
    for r in rects.iter() {
        // Frame-local, clipped to the frame bounds.
        let lx = r.x as i64 - frame.x as i64;
        let ly = r.y as i64 - frame.y as i64;
        let x0 = lx.clamp(0, fw);
        let y0 = ly.clamp(0, fh);
        let x1 = (lx + r.w as i64).clamp(0, fw);
        let y1 = (ly + r.h as i64).clamp(0, fh);
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        for y in (y0 as usize)..(y1 as usize) {
            let start = y * row_bytes + (x0 as usize) * 4;
            let end = y * row_bytes + (x1 as usize) * 4;
            if end <= bgra.len() {
                for px in bgra[start..end].chunks_exact_mut(4) {
                    px[0] = 0; // B
                    px[1] = 0; // G
                    px[2] = 0; // R
                    px[3] = 255; // A (opaque)
                }
            }
        }
    }
}

/// Current on-screen rectangle of each "hide this window" HWND, in the crop's
/// MONITOR-relative physical-pixel space (primary-monitor origin 0,0). Cross-
/// process safe — `GetWindowRect` only READS another window's bounds. Skips
/// closed / minimized / hidden / zero-size windows (nothing on screen to hide).
/// Windows on other monitors land outside the primary capture and get clipped
/// away by [`black_out_rects`]. Composited to black each frame so the box follows
/// the window — the only cross-process way to hide a window, since
/// `SetWindowDisplayAffinity` requires the window to belong to our own process.
fn window_capture_rects(hwnds: &[i64]) -> Vec<CropRect> {
    let mut out = Vec::with_capacity(hwnds.len());
    for &h in hwnds {
        let hwnd = HWND(h as isize);
        unsafe {
            if !IsWindow(hwnd).as_bool() || IsIconic(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool()
            {
                continue;
            }
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                continue;
            }
            let w = (rect.right - rect.left).max(0);
            let h = (rect.bottom - rect.top).max(0);
            if w > 0 && h > 0 {
                out.push(CropRect { x: rect.left, y: rect.top, w: w as u32, h: h as u32 });
            }
        }
    }
    out
}

/// Resolve the effective crop against the captured surface size, clamping to bounds.
fn resolve_crop(crop: &SharedCrop, surface_w: u32, surface_h: u32) -> CropRect {
    let full = CropRect { x: 0, y: 0, w: surface_w, h: surface_h };
    let Ok(guard) = crop.lock() else { return full };
    match *guard {
        None => full,
        Some(c) => {
            // H.264 needs even dimensions. Round the requested SIZE down to even
            // (capped to the surface) and then clamp the ORIGIN so that same-size
            // rect stays fully on-screen. Crucially we clamp POSITION, not size: a
            // live MOVE (incl. dragging toward an edge) keeps its width/height, so
            // the captured frames keep matching the encoder's fixed output and stay
            // on the fast straight-copy path. If we shrank the size at the edge
            // instead, a mere move would silently become a resize → per-frame CPU
            // letterbox → encoder backlog → the Stop write could wedge.
            let max_w = (surface_w & !1u32).max(2);
            let max_h = (surface_h & !1u32).max(2);
            let w = (c.w & !1u32).max(2).min(max_w);
            let h = (c.h & !1u32).max(2).min(max_h);
            let x = c.x.clamp(0, surface_w.saturating_sub(w) as i32);
            let y = c.y.clamp(0, surface_h.saturating_sub(h) as i32);
            CropRect { x, y, w, h }
        }
    }
}

/// De-pad a mapped staging texture into a tightly packed BGRA buffer.
///
/// `row_pitch` (the GPU row stride) is `>= w*4` and is hardware-aligned, so a
/// naive `w*h*4` copy shears the image. We copy exactly `w*4` bytes per row from
/// the padded source. This is the single highest-risk operation in capture, kept
/// pure and unit-tested.
fn pack_bgra(src: &[u8], row_pitch: usize, w: usize, h: usize) -> Vec<u8> {
    let dst_pitch = w * 4;
    let mut out = vec![0u8; dst_pitch * h];
    for row in 0..h {
        let s = row * row_pitch;
        let d = row * dst_pitch;
        out[d..d + dst_pitch].copy_from_slice(&src[s..s + dst_pitch]);
    }
    out
}

/// Save a [`Frame`] as a PNG (BGRA → RGBA). Debug/verification helper.
pub fn save_frame_png(frame: &Frame, path: &std::path::Path) -> Result<(), String> {
    use image::{Rgba, RgbaImage};
    let mut img = RgbaImage::new(frame.width, frame.height);
    for (i, px) in img.pixels_mut().enumerate() {
        let o = i * 4;
        *px = Rgba([frame.bgra[o + 2], frame.bgra[o + 1], frame.bgra[o], frame.bgra[o + 3]]);
    }
    img.save(path).map_err(|e| format!("PNG save failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::pack_bgra;

    #[test]
    fn pack_depads_rowpitch_without_shear() {
        // 2x2 BGRA image whose row stride is padded to 12 bytes (> w*4 = 8).
        // Distinct per-pixel values so any shear / misalignment is visible.
        let (w, h, rp) = (2usize, 2usize, 12usize);
        let mut src = vec![0u8; rp * h];
        src[0..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]); // row 0 pixels
        src[8..12].copy_from_slice(&[0xFF; 4]); // row 0 padding
        src[12..20].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]); // row 1 pixels
        src[20..24].copy_from_slice(&[0xFF; 4]); // row 1 padding

        let out = pack_bgra(&src, rp, w, h);
        assert_eq!(out, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    }

    #[test]
    fn pack_handles_unpadded_rows() {
        // When row_pitch == w*4 the output equals the input verbatim.
        let (w, h) = (3usize, 2usize);
        let src: Vec<u8> = (0..(w * h * 4) as u8).collect();
        assert_eq!(pack_bgra(&src, w * 4, w, h), src);
    }

    #[test]
    fn save_png_swaps_bgra_to_rgba() {
        use super::{save_frame_png, Frame};
        // BGRA [B=10, G=20, R=30, A=40] must round-trip to RGBA [30, 20, 10, 40].
        let frame = Frame { width: 1, height: 1, bgra: vec![10, 20, 30, 40] };
        let path = std::env::temp_dir().join("kil-screenrec-colorswap-test.png");
        save_frame_png(&frame, &path).unwrap();
        let img = image::open(&path).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(0, 0).0, [30, 20, 10, 40]);
        let _ = std::fs::remove_file(&path);
    }

    // Redaction compositing: black out the marked rects, translate from monitor
    // coords to frame-local, clip to the frame, and ignore fully-outside rects.
    #[test]
    fn redact_bgra_blacks_out_rects_and_clips() {
        use super::{redact_bgra, CropRect};
        use std::sync::{Arc, Mutex};
        // 4x2 white frame; crop origin (10,10) so rects must be translated.
        let frame = CropRect { x: 10, y: 10, w: 4, h: 2 };
        let mut bgra = vec![255u8; 4 * 2 * 4];
        let rects = Arc::new(Mutex::new(vec![
            CropRect { x: 11, y: 10, w: 2, h: 1 }, // local (1,0)..(3,1)
            CropRect { x: 100, y: 100, w: 5, h: 5 }, // fully outside → no-op
            CropRect { x: 13, y: 11, w: 9, h: 1 }, // local x clipped to the right edge, y1
        ]));
        redact_bgra(&mut bgra, &frame, &rects);

        let px = |x: usize, y: usize| {
            let i = (y * 4 + x) * 4;
            [bgra[i], bgra[i + 1], bgra[i + 2], bgra[i + 3]]
        };
        assert_eq!(px(0, 0), [255, 255, 255, 255], "left of box untouched");
        assert_eq!(px(1, 0), [0, 0, 0, 255], "box blacked");
        assert_eq!(px(2, 0), [0, 0, 0, 255], "box blacked");
        assert_eq!(px(3, 0), [255, 255, 255, 255], "right of box untouched");
        assert_eq!(px(0, 1), [255, 255, 255, 255], "row 1 left untouched");
        assert_eq!(px(3, 1), [0, 0, 0, 255], "clipped right-edge box blacked");
    }

    // Real-hardware end-to-end capture. Ignored by default (needs a display +
    // GPU); run explicitly:
    //   cargo test --no-default-features --features screenrec --lib \
    //     captures_primary_monitor_to_png -- --ignored --nocapture
    #[test]
    #[ignore = "captures the real primary monitor; run with --ignored"]
    fn captures_primary_monitor_to_png() {
        use super::{save_frame_png, start_capture, CaptureTarget};
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        let crop = Arc::new(Mutex::new(None)); // whole surface
        let redactions = Arc::new(Mutex::new(Vec::new())); // none
        let exclude = Arc::new(Mutex::new(Vec::new())); // hide no windows
        let (session, rx) =
            start_capture(CaptureTarget::PrimaryMonitor, crop, redactions, exclude, true)
                .expect("start capture");
        let frame = rx.recv_timeout(Duration::from_secs(5)).expect("a frame within 5s");
        session.stop().expect("clean stop");

        assert!(frame.width > 0 && frame.height > 0, "non-empty frame");
        assert_eq!(
            frame.bgra.len(),
            frame.width as usize * frame.height as usize * 4,
            "tightly packed BGRA (no RowPitch padding leaked through)"
        );

        let out = std::env::temp_dir().join("kil-screenrec-stage1.png");
        save_frame_png(&frame, &out).expect("save png");
        eprintln!("captured {}x{} -> {}", frame.width, frame.height, out.display());
    }
}
