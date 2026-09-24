//! Stage 0 COM/threading harness for the Screen Recorder (`screenrec` feature).
//!
//! Proves the #1 cross-subsystem hazard is handled: WGC (screen), Media
//! Foundation (webcam), WASAPI (system audio) and cpal (mic) each initialise COM
//! on their OWN dedicated MTA thread, running simultaneously, with no
//! `RPC_E_CHANGED_MODE` and no shared-apartment conflict.
//!
//! Run on real hardware (this is a `--no-default-features` build, so libvosk is
//! NOT linked/copied and a running dev app does not lock anything here):
//!
//!   cargo test --manifest-path src-tauri/Cargo.toml \
//!     --no-default-features --features screenrec \
//!     --test screenrec_com_harness -- --nocapture
//!
//! A green report means every lane reached its subsystem on an MTA thread.
//! "no device" for the mic/camera lanes is expected and is NOT a failure — the
//! harness only asserts COM coexistence, not hardware presence.
#![cfg(feature = "screenrec")]

use std::thread;
use std::time::Duration;

struct LaneReport {
    name: &'static str,
    com: String,
    status: String,
    ok: bool,
}

const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;

/// Initialise COM as multithreaded (MTA) on the current thread and describe the
/// outcome. `S_OK` / `S_FALSE` are both fine; `RPC_E_CHANGED_MODE` is the
/// apartment conflict we are hunting for. We never `CoUninitialize` — the rule
/// is one dedicated MTA thread per capture lane (mirrors voice_ui.rs).
fn init_mta() -> (bool, String) {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.0 == RPC_E_CHANGED_MODE {
        (false, "RPC_E_CHANGED_MODE (apartment conflict!)".into())
    } else if hr.is_ok() {
        (true, format!("MTA ok (hr=0x{:08X})", hr.0 as u32))
    } else {
        (false, format!("CoInitializeEx failed (hr=0x{:08X})", hr.0 as u32))
    }
}

// ─── Lane 1: WGC screen capture ─────────────────────────────────────────────

fn lane_wgc() -> LaneReport {
    use windows::core::Interface;
    use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
    use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Win32::Foundation::{HMODULE, POINT};
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
    };
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};
    use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

    let (com_ok, com) = init_mta();
    let status = (|| -> windows::core::Result<String> {
        let mut device: Option<ID3D11Device> = None;
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
                None,
            )?;
        }
        let device = device.expect("D3D11 device present after successful create");
        let dxgi: IDXGIDevice = device.cast()?;
        let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? };
        let d3d: IDirect3DDevice = inspectable.cast()?;

        let hmon = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        let interop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(hmon)? };
        let size = item.Size()?;

        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &d3d,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        session.StartCapture()?;
        thread::sleep(Duration::from_millis(150));
        session.Close()?;
        pool.Close()?;

        Ok(format!(
            "WGC capture started on {}x{} monitor (free-threaded frame pool ok)",
            size.Width, size.Height
        ))
    })();

    match status {
        Ok(s) => LaneReport { name: "WGC screen", com, status: s, ok: com_ok },
        Err(e) => LaneReport { name: "WGC screen", com, status: format!("FAILED: {e}"), ok: false },
    }
}

// ─── Lane 2: Media Foundation (webcam) ──────────────────────────────────────

fn lane_mf() -> LaneReport {
    use windows::Win32::Media::MediaFoundation::{MFShutdown, MFStartup, MFSTARTUP_FULL};
    // MF_VERSION = (MF_SDK_VERSION << 16) | MF_API_VERSION = (0x2 << 16) | 0x70.
    const MF_VERSION: u32 = 0x0002_0070;

    let (com_ok, com) = init_mta();
    let status = (|| -> windows::core::Result<String> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };
        // Device enumeration is exercised in Stage 5; here we only need to prove
        // Media Foundation starts up on this MTA thread alongside the others.
        unsafe { MFShutdown()? };
        Ok("Media Foundation MFStartup/MFShutdown ok".into())
    })();

    match status {
        Ok(s) => LaneReport { name: "MF webcam", com, status: s, ok: com_ok },
        Err(e) => LaneReport { name: "MF webcam", com, status: format!("FAILED: {e}"), ok: false },
    }
}

// ─── Lane 3: WASAPI (system audio / loopback endpoint) ──────────────────────

fn lane_wasapi() -> LaneReport {
    use wasapi::{get_default_device, Direction};

    let (com_ok, com) = init_mta();
    let status = (|| -> Result<String, Box<dyn std::error::Error>> {
        let device = get_default_device(&Direction::Render)?;
        let name = device.get_friendlyname().unwrap_or_else(|_| "render endpoint".into());
        let client = device.get_iaudioclient()?;
        let format = client.get_mixformat()?;
        Ok(format!(
            "WASAPI render '{}' @ {} Hz / {} ch (loopback-capable)",
            name,
            format.get_samplespersec(),
            format.get_nchannels()
        ))
    })();

    match status {
        Ok(s) => LaneReport { name: "WASAPI audio", com, status: s, ok: com_ok },
        Err(e) => {
            LaneReport { name: "WASAPI audio", com, status: format!("FAILED: {e}"), ok: false }
        }
    }
}

// ─── Lane 4: cpal (microphone) ──────────────────────────────────────────────

fn lane_cpal() -> LaneReport {
    use cpal::traits::{DeviceTrait, HostTrait};

    // cpal manages COM on its own internal threads; we still init this probe
    // thread MTA to mirror the others and confirm co-residence is conflict-free.
    let (com_ok, com) = init_mta();
    let status = (|| -> Result<String, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        match host.default_input_device() {
            Some(dev) => {
                let name = dev.name().unwrap_or_else(|_| "default input".into());
                let cfg = dev.default_input_config()?;
                Ok(format!(
                    "cpal mic '{}' @ {} Hz / {} ch",
                    name,
                    cfg.sample_rate().0,
                    cfg.channels()
                ))
            }
            None => Ok("no microphone present (mic lane is optional)".into()),
        }
    })();

    match status {
        Ok(s) => LaneReport { name: "cpal mic", com, status: s, ok: com_ok },
        Err(e) => LaneReport { name: "cpal mic", com, status: format!("FAILED: {e}"), ok: false },
    }
}

// ─── Harness ────────────────────────────────────────────────────────────────

#[test]
fn com_apartments_coexist() {
    // All four lanes run AT ONCE on their own threads — the whole point is to
    // catch a cross-lane apartment conflict, which only shows under co-residence.
    let handles = [
        thread::spawn(lane_wgc),
        thread::spawn(lane_mf),
        thread::spawn(lane_wasapi),
        thread::spawn(lane_cpal),
    ];
    let reports: Vec<LaneReport> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    println!("\n=== Screen Recorder COM / threading harness ===");
    for r in &reports {
        let mark = if r.ok { "PASS" } else { "FAIL" };
        println!("  [{mark}] {:<13} | com: {:<34} | {}", r.name, r.com, r.status);
    }
    println!("===============================================\n");

    let conflicts: Vec<&str> = reports
        .iter()
        .filter(|r| r.com.contains("RPC_E_CHANGED_MODE"))
        .map(|r| r.name)
        .collect();
    assert!(conflicts.is_empty(), "COM apartment conflict in lane(s): {conflicts:?}");

    for r in &reports {
        assert!(r.ok, "lane '{}' failed — com: {}, status: {}", r.name, r.com, r.status);
    }
}
