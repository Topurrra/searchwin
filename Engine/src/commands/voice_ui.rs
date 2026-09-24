//! voice_ui -- Windows UI Automation element enumeration for voice
//! accessibility control (#20).
//!
//! Voice upgrade, #20. The "show elements" voice command numbers every
//! interactive control in the focused window so the user can click one
//! by saying its number (or its label). `voice_list_ui_elements` reads
//! the foreground window's control tree via the IUIAutomation COM API
//! BEFORE the overlay is shown -- the overlay would otherwise steal the
//! foreground and we would enumerate ourselves. The result is stashed;
//! the /ui-elements overlay route fetches it with `voice_get_ui_elements`
//! and (on a spoken number / label) warps + clicks via the #18 backend.
//!
//! Why numbering rather than an open "click <label>": the Vosk command
//! grammar is fixed for a session and cannot hold arbitrary per-app
//! button labels. The overlay starts its OWN recognizer with a grammar
//! built from the enumerated labels, so label-speak works there; the
//! numbers are the universal fallback for unlabeled / duplicate /
//! icon-only controls.

#![cfg(windows)]

use std::sync::Mutex;

use serde::Serialize;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, TreeScope_Subtree, UIA_BoundingRectanglePropertyId,
    UIA_ButtonControlTypeId, UIA_CheckBoxControlTypeId, UIA_ComboBoxControlTypeId,
    UIA_ControlTypePropertyId, UIA_EditControlTypeId, UIA_HyperlinkControlTypeId,
    UIA_IsEnabledPropertyId, UIA_IsOffscreenPropertyId, UIA_ListItemControlTypeId,
    UIA_MenuItemControlTypeId, UIA_NamePropertyId, UIA_RadioButtonControlTypeId,
    UIA_SplitButtonControlTypeId, UIA_TabItemControlTypeId, UIA_TreeItemControlTypeId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
};

/// Hard cap on enumerated elements. Keeps the on-screen badge grid
/// readable and the overlay's spoken-number grammar small (1..=50).
const MAX_ELEMENTS: usize = 50;

/// Bound on the raw control-view array we scan -- a pathological web
/// page can expose thousands of nodes; we never need more than enough
/// to fill MAX_ELEMENTS.
const MAX_SCAN: i32 = 4000;

/// One clickable element, as the overlay needs it: a label for
/// label-speak and a fractional screen position for the badge and the
/// `voice_mouse_warp` click. Fractions are of the primary monitor --
/// the same coordinate space `voice_mouse_warp` resolves against.
#[derive(Clone, Serialize)]
pub struct UiElement {
    /// The control's accessible name -- may be empty (an icon-only
    /// button); the number badge still works for it.
    label: String,
    /// Center X as a fraction [0,1) of the primary monitor width.
    fx: f64,
    /// Center Y as a fraction [0,1) of the primary monitor height.
    fy: f64,
}

/// The most recent enumeration. `voice_list_ui_elements` writes it
/// while the user's app is still foreground; the /ui-elements overlay
/// reads it on show. Process-global -- only one overlay ever exists.
static LAST_UI_ELEMENTS: Mutex<Vec<UiElement>> = Mutex::new(Vec::new());

/// Whether a UIA control type counts as "clickable" -- the things a
/// user means by "click that". Containers (panes, groups, toolbars,
/// text) are excluded; they are not click targets.
fn is_interactive(control_type: i32) -> bool {
    [
        UIA_ButtonControlTypeId,
        UIA_HyperlinkControlTypeId,
        UIA_MenuItemControlTypeId,
        UIA_CheckBoxControlTypeId,
        UIA_RadioButtonControlTypeId,
        UIA_TabItemControlTypeId,
        UIA_SplitButtonControlTypeId,
        UIA_ComboBoxControlTypeId,
        UIA_EditControlTypeId,
        UIA_ListItemControlTypeId,
        UIA_TreeItemControlTypeId,
    ]
    .iter()
    .any(|t| t.0 == control_type)
}

/// Enumerate the foreground window's interactive controls. Blocking COM
/// work -- runs inside `spawn_blocking` so it never stalls the UI
/// thread, and every COM object lives and dies on that one thread.
fn enumerate_foreground() -> Result<Vec<UiElement>, String> {
    // COM must be live on this thread before any UIA call. We never
    // CoUninitialize -- a blocking-pool thread staying MTA-initialized
    // is harmless and the pool reuses threads. RPC_E_CHANGED_MODE (the
    // thread is already an STA) is fine: UIA works from either apartment.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0 == 0 {
        return Ok(Vec::new());
    }

    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    if screen_w <= 0 || screen_h <= 0 {
        return Err("Could not read the screen size.".to_string());
    }

    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
            .map_err(|e| format!("UI Automation is unavailable: {e}"))?;

    let root = unsafe { automation.ElementFromHandle(hwnd) }
        .map_err(|e| format!("Could not read the focused window: {e}"))?;

    // Cache the five properties we read so FindAllBuildCache fetches
    // them in ONE cross-process call -- reading them per-element would
    // otherwise be one COM round-trip each (hundreds on a busy window).
    let cache = unsafe { automation.CreateCacheRequest() }
        .map_err(|e| format!("UI Automation cache request failed: {e}"))?;
    unsafe {
        let _ = cache.AddProperty(UIA_NamePropertyId);
        let _ = cache.AddProperty(UIA_ControlTypePropertyId);
        let _ = cache.AddProperty(UIA_BoundingRectanglePropertyId);
        let _ = cache.AddProperty(UIA_IsOffscreenPropertyId);
        let _ = cache.AddProperty(UIA_IsEnabledPropertyId);
    }

    // ControlViewCondition is a built-in condition selecting the control
    // view -- the interactive-ish subset, minus raw-tree clutter. We
    // still filter to specific control types below; this just keeps the
    // returned array small without a hand-built VARIANT condition.
    let condition = unsafe { automation.ControlViewCondition() }
        .map_err(|e| format!("UI Automation condition failed: {e}"))?;

    let found = unsafe { root.FindAllBuildCache(TreeScope_Subtree, &condition, &cache) }
        .map_err(|e| format!("Could not enumerate the window's controls: {e}"))?;

    let count = unsafe { found.Length() }.unwrap_or(0);
    let mut elements: Vec<UiElement> = Vec::new();

    for i in 0..count.min(MAX_SCAN) {
        if elements.len() >= MAX_ELEMENTS {
            break;
        }
        let Ok(element) = (unsafe { found.GetElement(i) }) else {
            continue;
        };
        // Every read below hits the cache populated by FindAllBuildCache
        // -- no COM round-trip.
        let control_type = unsafe { element.CachedControlType() }
            .map(|t| t.0)
            .unwrap_or(0);
        if !is_interactive(control_type) {
            continue;
        }
        if unsafe { element.CachedIsOffscreen() }
            .map(|b| b.as_bool())
            .unwrap_or(true)
        {
            continue;
        }
        if !unsafe { element.CachedIsEnabled() }
            .map(|b| b.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        let Ok(rect) = (unsafe { element.CachedBoundingRectangle() }) else {
            continue;
        };
        if rect.right <= rect.left || rect.bottom <= rect.top {
            continue;
        }
        let cx = (rect.left + rect.right) / 2;
        let cy = (rect.top + rect.bottom) / 2;
        // Drop anything off the primary monitor -- the overlay only
        // covers it, and `voice_mouse_warp` works in its fractions.
        if cx < 0 || cx >= screen_w || cy < 0 || cy >= screen_h {
            continue;
        }
        let name = unsafe { element.CachedName() }
            .map(|b| b.to_string())
            .unwrap_or_default();
        let label: String = name.trim().chars().take(60).collect();
        elements.push(UiElement {
            label,
            fx: cx as f64 / screen_w as f64,
            fy: cy as f64 / screen_h as f64,
        });
    }

    Ok(elements)
}

/// Enumerate the foreground window's interactive controls and stash the
/// result. MUST be called BEFORE the /ui-elements overlay is shown --
/// the overlay would otherwise be the foreground window. Returns the
/// element count so the caller can skip opening an empty overlay.
#[tauri::command]
pub async fn voice_list_ui_elements() -> Result<usize, String> {
    let elements = tauri::async_runtime::spawn_blocking(enumerate_foreground)
        .await
        .map_err(|e| format!("UI Automation task failed: {e}"))??;
    let count = elements.len();
    if let Ok(mut slot) = LAST_UI_ELEMENTS.lock() {
        *slot = elements;
    }
    Ok(count)
}

/// Return the elements from the most recent `voice_list_ui_elements`
/// call. The /ui-elements overlay route calls this on show to build its
/// numbered badges and its recognizer grammar.
#[tauri::command]
pub fn voice_get_ui_elements() -> Vec<UiElement> {
    LAST_UI_ELEMENTS
        .lock()
        .map(|slot| slot.clone())
        .unwrap_or_default()
}
