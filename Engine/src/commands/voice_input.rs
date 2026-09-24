//! voice_input -- Win32 keyboard, mouse & window control for voice
//! commands.
//!
//! Voice upgrade, #18a. The voice command pipeline (commandRegistry ->
//! voiceSafetyGate -> voiceActionExecutor) decides WHAT input to
//! synthesize; these commands PERFORM it via Win32 `SendInput`. They
//! emulate real keystrokes and mouse events, so the foreground app
//! sees an event stream indistinguishable from physical input.
//!
//! Targeting: `SendInput` dispatches to the foreground window. Voice
//! commands come from command mode / push-to-talk, where the user's
//! target app is already foreground -- no focus dance is needed (unlike
//! the clipboard overlay's paste path, where KeepItLocal's own overlay
//! is foreground and must hand focus back first).
//!
//! Limitation: a non-elevated KeepItLocal cannot inject input into an
//! elevated (administrator) foreground app -- Windows UIPI silently
//! drops the events. `SendInput` reports how many events it dispatched,
//! so `dispatch` surfaces that as an error rather than a silent no-op.

#![cfg(windows)]

use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// One mouse-wheel notch -- Win32's standard wheel-delta unit.
const WHEEL_DELTA: i32 = 120;

/// Resolve a spoken key name to its Win32 virtual-key code. Named keys
/// map explicitly; a single ASCII letter or digit maps to its
/// uppercase code point (`VK_A` == 0x41 == 'A'). `None` when unknown.
fn key_to_vk(name: &str) -> Option<VIRTUAL_KEY> {
    let n = name.trim().to_ascii_lowercase();
    Some(match n.as_str() {
        "enter" | "return" => VK_RETURN,
        "escape" | "esc" => VK_ESCAPE,
        "tab" => VK_TAB,
        "space" | "spacebar" => VK_SPACE,
        "backspace" => VK_BACK,
        "delete" | "del" => VK_DELETE,
        "up" => VK_UP,
        "down" => VK_DOWN,
        "left" => VK_LEFT,
        "right" => VK_RIGHT,
        "home" => VK_HOME,
        "end" => VK_END,
        "page up" | "pageup" => VK_PRIOR,
        "page down" | "pagedown" => VK_NEXT,
        "f1" => VK_F1,
        "f2" => VK_F2,
        "f3" => VK_F3,
        "f4" => VK_F4,
        "f5" => VK_F5,
        "f6" => VK_F6,
        "f7" => VK_F7,
        "f8" => VK_F8,
        "f9" => VK_F9,
        "f10" => VK_F10,
        "f11" => VK_F11,
        "f12" => VK_F12,
        _ => {
            // A single ASCII letter or digit -- its VK is the uppercase
            // code point.
            let mut chars = n.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => {
                    VIRTUAL_KEY(c.to_ascii_uppercase() as u16)
                }
                _ => return None,
            }
        }
    })
}

/// Resolve a spoken modifier name to its virtual-key code.
fn modifier_to_vk(name: &str) -> Option<VIRTUAL_KEY> {
    match name.trim().to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(VK_CONTROL),
        "alt" => Some(VK_MENU),
        "shift" => Some(VK_SHIFT),
        "win" | "windows" | "super" | "meta" => Some(VK_LWIN),
        _ => None,
    }
}

/// Build a keyboard `INPUT` for `vk` -- a key-down, or a key-up when
/// `up` is set.
fn key_event(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Build a mouse `INPUT` with the given event flags, wheel/data value,
/// and relative motion.
fn mouse_event(flags: MOUSE_EVENT_FLAGS, data: i32, dx: i32, dy: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Dispatch a batch of synthesized input events. Returns an error when
/// the OS dispatched fewer events than requested -- a privileged
/// low-level hook, or an elevated foreground app, swallowed them.
fn dispatch(inputs: &[INPUT]) -> Result<(), String> {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if (sent as usize) != inputs.len() {
        return Err(format!(
            "Input was blocked -- dispatched {sent}/{} events. The foreground \
             app may be running as administrator.",
            inputs.len()
        ));
    }
    Ok(())
}

/// Synthesize a keystroke: hold the modifiers, tap the key, release.
/// `key` is a key name ("enter", "a", "f5"); `modifiers` are names
/// ("ctrl", "alt", "shift", "win").
#[tauri::command]
pub fn voice_send_keystroke(key: String, modifiers: Vec<String>) -> Result<(), String> {
    let vk = key_to_vk(&key).ok_or_else(|| format!("Unknown key: '{key}'"))?;
    let mod_vks: Vec<VIRTUAL_KEY> = modifiers
        .iter()
        .map(|m| modifier_to_vk(m).ok_or_else(|| format!("Unknown modifier: '{m}'")))
        .collect::<Result<_, _>>()?;

    // Modifier-downs, key-down, key-up, modifier-ups (released in
    // reverse) -- the exact event stream a real key combo produces.
    let mut inputs: Vec<INPUT> = Vec::with_capacity(mod_vks.len() * 2 + 2);
    for &m in &mod_vks {
        inputs.push(key_event(m, false));
    }
    inputs.push(key_event(vk, false));
    inputs.push(key_event(vk, true));
    for &m in mod_vks.iter().rev() {
        inputs.push(key_event(m, true));
    }
    dispatch(&inputs)
}

/// Synthesize a mouse click at the current cursor position. `button`
/// is "left" / "right" / "middle"; `double` fires the press twice.
#[tauri::command]
pub fn voice_mouse_click(button: String, double: bool) -> Result<(), String> {
    let (down, up) = match button.trim().to_ascii_lowercase().as_str() {
        "left" => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
        other => return Err(format!("Unknown mouse button: '{other}'")),
    };
    let mut inputs = vec![mouse_event(down, 0, 0, 0), mouse_event(up, 0, 0, 0)];
    if double {
        inputs.push(mouse_event(down, 0, 0, 0));
        inputs.push(mouse_event(up, 0, 0, 0));
    }
    dispatch(&inputs)
}

/// Scroll the mouse wheel. `direction` is "up" / "down"; `notches` is
/// how many wheel clicks (clamped to a sane range).
#[tauri::command]
pub fn voice_mouse_scroll(direction: String, notches: i32) -> Result<(), String> {
    let steps = notches.clamp(1, 20);
    let delta = match direction.trim().to_ascii_lowercase().as_str() {
        "up" => WHEEL_DELTA * steps,
        "down" => -WHEEL_DELTA * steps,
        other => return Err(format!("Unknown scroll direction: '{other}'")),
    };
    dispatch(&[mouse_event(MOUSEEVENTF_WHEEL, delta, 0, 0)])
}

/// Nudge the cursor by a relative pixel offset.
#[tauri::command]
pub fn voice_mouse_move(dx: i32, dy: i32) -> Result<(), String> {
    dispatch(&[mouse_event(MOUSEEVENTF_MOVE, 0, dx, dy)])
}

/// Warp the cursor to a fractional screen position. `fx` / `fy` are in
/// [0, 1] across the primary monitor — the #18b mouse grid works in
/// screen fractions, so the DPI-aware pixel resolution happens here,
/// not in the webview.
#[tauri::command]
pub fn voice_mouse_warp(fx: f64, fy: f64) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SetCursorPos, SM_CXSCREEN, SM_CYSCREEN,
    };
    let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    if w <= 0 || h <= 0 {
        return Err("Could not read the screen size.".to_string());
    }
    let x = (fx.clamp(0.0, 1.0) * w as f64).round() as i32;
    let y = (fy.clamp(0.0, 1.0) * h as f64).round() as i32;
    unsafe { SetCursorPos(x, y) }.map_err(|e| format!("Could not move the cursor: {e}"))
}

/// Act on the foreground window (#19): minimize / maximize / restore /
/// close it, or snap it to the left or right half — or the centre — of
/// the work area (the screen minus the taskbar). The voice command
/// pipeline already gates these at Medium risk (exact match only).
#[tauri::command]
pub fn voice_window_action(action: String) -> Result<(), String> {
    use windows::Win32::Foundation::{LPARAM, RECT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, PostMessageW, SetWindowPos, ShowWindow,
        SystemParametersInfoW, HWND_TOP, SPI_GETWORKAREA, SWP_NOZORDER, SW_MAXIMIZE,
        SW_MINIMIZE, SW_RESTORE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_CLOSE,
    };

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0 == 0 {
        return Err("There is no focused window to act on.".to_string());
    }
    let act = action.trim().to_ascii_lowercase();
    match act.as_str() {
        "minimize" => {
            unsafe {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
            Ok(())
        }
        "maximize" => {
            unsafe {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            }
            Ok(())
        }
        "restore" => {
            unsafe {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            Ok(())
        }
        "close" => unsafe {
            PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0))
                .map_err(|e| format!("Could not close the window: {e}"))
        },
        "snap-left" | "snap-right" | "center" => {
            // The work area excludes the taskbar — a snapped or centred
            // window should not sit underneath it.
            let mut area = RECT::default();
            unsafe {
                SystemParametersInfoW(
                    SPI_GETWORKAREA,
                    0,
                    Some(&mut area as *mut RECT as *mut core::ffi::c_void),
                    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                )
                .map_err(|e| format!("Could not read the work area: {e}"))?;
            }
            let aw = area.right - area.left;
            let ah = area.bottom - area.top;
            let (x, y, w, h) = match act.as_str() {
                "snap-left" => (area.left, area.top, aw / 2, ah),
                "snap-right" => (area.left + aw / 2, area.top, aw / 2, ah),
                _ => {
                    // center — two-thirds of the work area, centred.
                    let w = aw * 2 / 3;
                    let h = ah * 2 / 3;
                    (area.left + (aw - w) / 2, area.top + (ah - h) / 2, w, h)
                }
            };
            unsafe {
                // Restore first, or SetWindowPos fights a maximized
                // state and the new bounds are ignored.
                let _ = ShowWindow(hwnd, SW_RESTORE);
                SetWindowPos(hwnd, HWND_TOP, x, y, w, h, SWP_NOZORDER)
                    .map_err(|e| format!("Could not move the window: {e}"))
            }
        }
        other => Err(format!("Unknown window action: '{other}'")),
    }
}

/// The process name of the foreground window -- lowercased, without the
/// `.exe` suffix (e.g. "chrome", "code", "winword"). Empty when there is
/// no foreground window, or its process image cannot be read. Used by
/// #21's per-app command scoping: a user command tagged `[app: chrome]`
/// only matches when this returns "chrome".
#[tauri::command]
pub fn voice_get_foreground_app() -> String {
    use windows::Win32::Foundation::{CloseHandle, FALSE};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0 == 0 {
        return String::new();
    }
    let mut pid: u32 = 0;
    let _ = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return String::new();
    }
    let handle =
        match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) } {
            Ok(handle) => handle,
            Err(_) => return String::new(),
        };
    let mut buf = [0u16; 260];
    let mut len = buf.len() as u32;
    let query = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    unsafe {
        let _ = CloseHandle(handle);
    }
    if query.is_err() {
        return String::new();
    }
    let full = String::from_utf16_lossy(&buf[..len as usize]);
    let base = full
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    base.strip_suffix(".exe").unwrap_or(&base).to_string()
}

/// The foreground window's process name AND its title text. The title lets
/// Focus Mode detect a blocked *website* by the browser-tab name (e.g.
/// "YouTube - Google Chrome"), since every tab shares the same process
/// ("chrome"). `app` matches `voice_get_foreground_app`'s normalization.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForegroundWindowInfo {
    pub app: String,
    pub title: String,
}

#[tauri::command]
pub fn get_foreground_window_info() -> ForegroundWindowInfo {
    use windows::Win32::Foundation::{CloseHandle, FALSE};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

    let empty = ForegroundWindowInfo {
        app: String::new(),
        title: String::new(),
    };

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0 == 0 {
        return empty;
    }

    // Window title (the active browser tab's page title lives here).
    let mut tbuf = [0u16; 512];
    let tlen = unsafe { GetWindowTextW(hwnd, &mut tbuf) };
    let title = if tlen > 0 {
        String::from_utf16_lossy(&tbuf[..tlen as usize])
    } else {
        String::new()
    };

    // Process name — same normalization as voice_get_foreground_app.
    let mut pid: u32 = 0;
    let _ = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let app = if pid == 0 {
        String::new()
    } else {
        match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) } {
            Ok(handle) => {
                let mut buf = [0u16; 260];
                let mut len = buf.len() as u32;
                let query = unsafe {
                    QueryFullProcessImageNameW(
                        handle,
                        PROCESS_NAME_WIN32,
                        windows::core::PWSTR(buf.as_mut_ptr()),
                        &mut len,
                    )
                };
                unsafe {
                    let _ = CloseHandle(handle);
                }
                if query.is_err() {
                    String::new()
                } else {
                    let full = String::from_utf16_lossy(&buf[..len as usize]);
                    let base = full
                        .rsplit(['\\', '/'])
                        .next()
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    base.strip_suffix(".exe").unwrap_or(&base).to_string()
                }
            }
            Err(_) => String::new(),
        }
    };

    ForegroundWindowInfo { app, title }
}

// --- Tests (#26) -----------------------------------------------------
//
// The spoken-name -> virtual-key mapping is the pure, deterministic
// core of the voice keyboard emulation. SendInput dispatch itself
// cannot be unit-tested (it injects real OS input), but the mapping
// can -- and it is exactly the layer a typo or a missing key would
// silently break.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_resolve_to_their_virtual_key() {
        assert_eq!(key_to_vk("enter"), Some(VK_RETURN));
        assert_eq!(key_to_vk("return"), Some(VK_RETURN));
        assert_eq!(key_to_vk("ESC"), Some(VK_ESCAPE));
        assert_eq!(key_to_vk("escape"), Some(VK_ESCAPE));
        assert_eq!(key_to_vk("f5"), Some(VK_F5));
        assert_eq!(key_to_vk("page up"), Some(VK_PRIOR));
        assert_eq!(key_to_vk("  Tab  "), Some(VK_TAB));
    }

    #[test]
    fn single_ascii_chars_map_to_their_uppercase_code_point() {
        assert_eq!(key_to_vk("a"), Some(VIRTUAL_KEY(b'A' as u16)));
        assert_eq!(key_to_vk("Z"), Some(VIRTUAL_KEY(b'Z' as u16)));
        assert_eq!(key_to_vk("7"), Some(VIRTUAL_KEY(b'7' as u16)));
    }

    #[test]
    fn unknown_or_malformed_keys_resolve_to_none() {
        assert_eq!(key_to_vk("splat"), None);
        assert_eq!(key_to_vk(""), None);
        assert_eq!(key_to_vk("ab"), None);
        assert_eq!(key_to_vk("f13"), None);
    }

    #[test]
    fn modifiers_resolve_with_aliases_and_casing() {
        assert_eq!(modifier_to_vk("ctrl"), Some(VK_CONTROL));
        assert_eq!(modifier_to_vk("control"), Some(VK_CONTROL));
        assert_eq!(modifier_to_vk("alt"), Some(VK_MENU));
        assert_eq!(modifier_to_vk("shift"), Some(VK_SHIFT));
        assert_eq!(modifier_to_vk("WIN"), Some(VK_LWIN));
        assert_eq!(modifier_to_vk("windows"), Some(VK_LWIN));
    }

    #[test]
    fn unknown_modifiers_resolve_to_none() {
        assert_eq!(modifier_to_vk("hyper"), None);
        assert_eq!(modifier_to_vk(""), None);
    }
}
