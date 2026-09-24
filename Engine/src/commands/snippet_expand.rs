//! Snippet auto-expand — global text-expander. Windows-only.
//!
//! A WH_KEYBOARD_LL hook watches typing while the feature is enabled
//! (opt-in, default off). The hook tracks the *current word* (a run of
//! alphanumeric chars). On a terminator key it asks the matcher whether
//! that word is a snippet trigger; on a match a worker thread backspaces
//! the trigger and types the expansion. Keystrokes are matched in memory
//! only — never logged, persisted, or transmitted.

use std::collections::HashMap;

/// What the matcher decides for a single observed key.
#[derive(Debug, PartialEq, Eq)]
pub enum MatchAction {
    /// Keep watching; nothing to do. Let the key through.
    Pass,
    /// The completed word matched a trigger of this id. The caller should
    /// suppress the terminator and inject the expansion. `trigger_len` is
    /// the number of backspaces needed to erase the typed trigger.
    Expand { id: u64, trigger_len: usize },
}

/// Pure word-buffer state machine. No OS calls — drive it with classified
/// key events and it tells you when to expand.
#[derive(Default)]
pub struct Matcher {
    /// Current run of alphanumeric chars since the last boundary.
    word: String,
    /// trigger (lowercased) -> snippet id. Refreshed by `set_triggers`.
    triggers: HashMap<String, u64>,
    /// Longest trigger length — a word longer than this can't be a trigger.
    max_len: usize,
    /// True once the current word exceeds `max_len`: it can no longer be a
    /// whole-word match, so we stop matching until the next boundary. Without
    /// this, "asig" would fire the trigger "sig" — a word *ending* in a
    /// trigger rather than a whole word equal to it.
    overflow: bool,
    /// A "/" was just typed and is pending as a prefix for the next word.
    pending_slash: bool,
    /// The current word was typed immediately after a "/" prefix.
    word_slashed: bool,
}

impl Matcher {
    pub fn set_triggers(&mut self, triggers: HashMap<String, u64>) {
        self.max_len = triggers.keys().map(|k| k.chars().count()).max().unwrap_or(0);
        self.triggers = triggers;
        self.reset();
    }

    /// A printable alphanumeric char was typed.
    pub fn on_char(&mut self, c: char) {
        if c.is_alphanumeric() {
            if self.word.is_empty() {
                // Starting a fresh word — inherit any pending "/" prefix.
                self.word_slashed = self.pending_slash;
            }
            self.pending_slash = false;
            self.word.push(c.to_ascii_lowercase());
            if self.word.chars().count() > self.max_len {
                self.overflow = true;
            }
        } else {
            // Any other printable char is a boundary.
            self.reset();
        }
    }

    /// A "/" was typed — both a boundary and a prefix that primes the next
    /// word, so typing the trigger as "/sig" deletes the slash too.
    pub fn on_slash(&mut self) {
        self.reset();
        self.pending_slash = true;
    }

    /// A terminator key (space/tab/enter) was pressed. Returns Expand if the
    /// just-completed word is a whole-word trigger. `trigger_len` includes a
    /// "/" prefix when one was typed, so the injector erases it too.
    pub fn on_terminator(&mut self) -> MatchAction {
        let action = if self.overflow {
            MatchAction::Pass
        } else {
            match self.triggers.get(&self.word) {
                Some(&id) => MatchAction::Expand {
                    id,
                    trigger_len: self.word.chars().count() + usize::from(self.word_slashed),
                },
                None => MatchAction::Pass,
            }
        };
        self.reset();
        action
    }

    /// Caret moved / edit / modifier / boundary — the buffer no longer
    /// reflects what is before the caret. Clear all word state.
    pub fn reset(&mut self) {
        self.word.clear();
        self.overflow = false;
        self.word_slashed = false;
        self.pending_slash = false;
    }
}

/// `expand_template` renders `{{cursor}}` as a literal `|`. After typing
/// the body we move the caret LEFT by the number of characters that
/// followed the marker. Returns (text_without_marker, chars_to_move_left).
/// Only the FIRST `|` originating from a cursor marker is honored; to keep
/// it simple we treat the first `|` as the marker (documented limitation:
/// a literal pipe in a template is rare and can be escaped in a later rev).
pub fn split_cursor(expanded: &str) -> (String, usize) {
    match expanded.find('|') {
        Some(idx) => {
            let after = expanded[idx + 1..].chars().count();
            let mut clean = String::with_capacity(expanded.len() - 1);
            clean.push_str(&expanded[..idx]);
            clean.push_str(&expanded[idx + 1..]);
            (clean, after)
        }
        None => (expanded.to_string(), 0),
    }
}

// ─── Windows OS glue: injector + low-level keyboard hook ─────────────────
// Opt-in, default off. Keystrokes are matched in memory only — never logged,
// persisted, or transmitted. All synthetic input carries SENTINEL so the
// hook ignores our own keystrokes (no re-trigger loops).

/// Marker placed in synthetic input's `dwExtraInfo` so the hook skips it.
#[cfg(windows)]
pub(crate) const SENTINEL: usize = 0x4B_49_4C_AE; // "KIL" + a marker byte.

/// Erase `backspaces` chars, type `text` as Unicode, move the caret left
/// `move_left` times, then optionally re-emit `trailing_vk` (the terminator)
/// — all into the already-focused foreground window.
#[cfg(windows)]
pub(crate) fn inject_expansion(
    backspaces: usize,
    text: &str,
    move_left: usize,
    trailing_vk: Option<u16>,
) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_BACK, VK_LEFT,
    };

    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: SENTINEL,
                },
            },
        }
    }
    fn unicode(scan: u16, up: bool) -> INPUT {
        let mut flags = KEYEVENTF_UNICODE;
        if up {
            flags |= KEYEVENTF_KEYUP;
        }
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: SENTINEL,
                },
            },
        }
    }

    let mut inputs: Vec<INPUT> = Vec::new();
    for _ in 0..backspaces {
        inputs.push(key(VK_BACK, false));
        inputs.push(key(VK_BACK, true));
    }
    for unit in text.encode_utf16() {
        inputs.push(unicode(unit, false));
        inputs.push(unicode(unit, true));
    }
    for _ in 0..move_left {
        inputs.push(key(VK_LEFT, false));
        inputs.push(key(VK_LEFT, true));
    }
    if let Some(vk) = trailing_vk {
        inputs.push(key(VIRTUAL_KEY(vk), false));
        inputs.push(key(VIRTUAL_KEY(vk), true));
    }
    if inputs.is_empty() {
        return;
    }
    unsafe {
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        if sent as usize != inputs.len() {
            eprintln!("snippet_expand: SendInput {sent}/{} events", inputs.len());
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::{inject_expansion, split_cursor, MatchAction, Matcher, SENTINEL};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Mutex, OnceLock, RwLock};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
        HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
    };

    #[derive(Default)]
    pub struct ExpandData {
        pub name: Option<String>,
        pub vars: HashMap<String, String>,
        pub templates: HashMap<u64, String>,
        pub triggers: HashMap<String, u64>, // trigger (lowercased) -> id
        pub excluded_apps: Vec<String>,     // lowercased exe names
    }

    // The LL callback is `extern "system"` with no user pointer, so shared
    // state lives in module statics.
    static MATCHER: Mutex<Option<Matcher>> = Mutex::new(None);
    static DATA: OnceLock<RwLock<ExpandData>> = OnceLock::new();
    static WORKER_TX: OnceLock<Sender<(u64, usize, u16)>> = OnceLock::new();
    static HOOK: AtomicIsize = AtomicIsize::new(0);
    static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);

    fn data() -> &'static RwLock<ExpandData> {
        DATA.get_or_init(|| RwLock::new(ExpandData::default()))
    }

    /// LL keyboard callback. MUST stay fast (Windows drops slow hooks) — it
    /// only updates the word buffer and hands matches to the worker thread.
    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            if kb.dwExtraInfo != SENTINEL {
                let msg = wparam.0 as u32;
                if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                    if let Some(MatchAction::Expand { id, trigger_len }) =
                        classify_and_feed(kb.vkCode)
                    {
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send((id, trigger_len, kb.vkCode as u16));
                        }
                        return LRESULT(1); // swallow terminator; worker re-emits it
                    }
                }
            }
        }
        CallNextHookEx(HHOOK(0), code, wparam, lparam)
    }

    /// Feed one vkCode into the matcher. Returns Some(action) only on a
    /// terminator. v1 terminators: space/tab/enter. Other non-alnum keys
    /// (caret keys, modifiers, punctuation) reset the current word.
    fn classify_and_feed(vk: u32) -> Option<MatchAction> {
        let mut guard = MATCHER.lock().ok()?;
        let m = guard.as_mut()?;
        if (0x30..=0x39).contains(&vk) {
            m.on_char((b'0' + (vk - 0x30) as u8) as char);
            return None;
        }
        if (0x41..=0x5A).contains(&vk) {
            m.on_char((b'a' + (vk - 0x41) as u8) as char);
            return None;
        }
        if vk == 0x20 || vk == 0x0D || vk == 0x09 {
            // VK_SPACE / VK_RETURN / VK_TAB
            return Some(m.on_terminator());
        }
        if vk == 0xBF {
            // VK_OEM_2 — the "/" key. Prime the next word as slash-prefixed so
            // typing the trigger as "/sig" erases the leading slash too.
            m.on_slash();
            return None;
        }
        m.reset();
        None
    }

    fn start_worker() -> Sender<(u64, usize, u16)> {
        let (tx, rx): (Sender<(u64, usize, u16)>, Receiver<(u64, usize, u16)>) = channel();
        let _ = std::thread::Builder::new()
            .name("kil-snippet-expand-worker".into())
            .spawn(move || {
                for (id, trigger_len, term_vk) in rx {
                    let (template, name, vars) = {
                        let d = data().read().unwrap();
                        match d.templates.get(&id) {
                            Some(t) => (t.clone(), d.name.clone(), d.vars.clone()),
                            None => continue,
                        }
                    };
                    if is_foreground_excluded() {
                        continue;
                    }
                    // v1: {{clipboard}} is not resolved in auto-expand (None).
                    // It DOES work via the palette, which reads the live
                    // clipboard and passes it in (see PaletteV2's
                    // pasteSnippet) — that was broken until 2026-07-16, so
                    // this comment's promise was false for a long time.
                    // Left as None here on purpose: this runs on the
                    // WH_KEYBOARD_LL worker, and opening the clipboard from
                    // that path is a hazard the overlay path doesn't have.
                    // use_count is not bumped here (no AppHandle on this
                    // thread): a minor sort-order gap.
                    let expanded = crate::commands::snippets::expand_template_full(
                        &template,
                        None,
                        name.as_deref(),
                        &vars,
                    );
                    let had_cursor = expanded.contains('|');
                    let (clean, left) = split_cursor(&expanded);
                    if had_cursor {
                        // Caret lands at the marker; re-emitting the terminator
                        // there would be wrong, so drop it for cursor snippets.
                        inject_expansion(trigger_len, &clean, left, None);
                    } else {
                        inject_expansion(trigger_len, &clean, 0, Some(term_vk));
                    }
                }
            });
        tx
    }

    fn is_foreground_excluded() -> bool {
        let d = data().read().unwrap();
        if d.excluded_apps.is_empty() {
            return false;
        }
        match foreground_exe_lower() {
            Some(exe) => d.excluded_apps.iter().any(|a| a == &exe),
            None => false,
        }
    }

    fn foreground_exe_lower() -> Option<String> {
        use windows::core::PWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        unsafe {
            let hwnd = GetForegroundWindow();
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return None;
            }
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok();
            let _ = CloseHandle(handle);
            if !ok {
                return None;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            path.rsplit(['\\', '/']).next().map(|s| s.to_lowercase())
        }
    }

    pub fn set_enabled(enabled: bool) {
        if enabled {
            if HOOK.load(Ordering::SeqCst) != 0 {
                return; // already running
            }
            let mut matcher = Matcher::default();
            // Seed from whatever sync already pushed — robust to call order.
            matcher.set_triggers(data().read().unwrap().triggers.clone());
            *MATCHER.lock().unwrap() = Some(matcher);
            let _ = WORKER_TX.set(start_worker());
            let _ = std::thread::Builder::new()
                .name("kil-snippet-expand-hook".into())
                .spawn(|| unsafe {
                    HOOK_THREAD.store(
                        windows::Win32::System::Threading::GetCurrentThreadId(),
                        Ordering::SeqCst,
                    );
                    match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), HINSTANCE(0), 0) {
                        Ok(h) => {
                            HOOK.store(h.0, Ordering::SeqCst);
                            // LL hooks fire during message retrieval; the pump
                            // keeps this thread alive. WM_QUIT (posted by
                            // set_enabled(false)) breaks the loop.
                            let mut msg = MSG::default();
                            while GetMessageW(&mut msg, HWND(0), 0, 0).as_bool() {}
                            let _ = UnhookWindowsHookEx(h);
                            HOOK.store(0, Ordering::SeqCst);
                        }
                        Err(e) => {
                            eprintln!("snippet_expand: SetWindowsHookExW failed: {e}");
                            HOOK_THREAD.store(0, Ordering::SeqCst);
                        }
                    }
                });
        } else {
            let tid = HOOK_THREAD.swap(0, Ordering::SeqCst);
            if tid != 0 {
                unsafe {
                    let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
                }
            }
            *MATCHER.lock().unwrap() = None;
        }
    }

    pub fn set_data(
        templates: HashMap<u64, String>,
        triggers: HashMap<String, u64>,
        name: Option<String>,
        vars: HashMap<String, String>,
        excluded_apps: Vec<String>,
    ) {
        {
            let mut d = data().write().unwrap();
            d.templates = templates;
            d.triggers = triggers.clone();
            d.name = name;
            d.vars = vars;
            d.excluded_apps = excluded_apps.into_iter().map(|a| a.to_lowercase()).collect();
        }
        // Update a live matcher. If the hook isn't running yet, set_enabled
        // seeds the matcher from DATA.triggers when it starts — so the
        // enable/sync call order doesn't matter.
        if let Some(m) = MATCHER.lock().unwrap().as_mut() {
            m.set_triggers(triggers);
        }
    }
}

// ─── Tauri commands ─────────────────────────────────────────────────────

/// Enable/disable the global snippet auto-expand keyboard watcher.
#[tauri::command]
pub fn set_snippet_autoexpand_enabled(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    imp::set_enabled(enabled);
    #[cfg(not(windows))]
    let _ = enabled;
    Ok(())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoExpandData {
    pub snippets: Vec<crate::commands::snippets::Snippet>,
    pub name: Option<String>,
    pub variables: std::collections::HashMap<String, String>,
    pub excluded_apps: Vec<String>,
}

/// Push the current snippet set + variables + exclusions to the watcher.
/// Cheap; the frontend calls it whenever any input changes. The data is held
/// but only read by the active hook.
#[tauri::command]
pub fn sync_snippet_expand_data(data: AutoExpandData) -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut templates = std::collections::HashMap::new();
        let mut triggers = std::collections::HashMap::new();
        for s in &data.snippets {
            templates.insert(s.id, s.template.clone());
            triggers.insert(s.trigger.to_lowercase(), s.id);
        }
        imp::set_data(
            templates,
            triggers,
            data.name,
            data.variables,
            data.excluded_apps,
        );
    }
    #[cfg(not(windows))]
    let _ = data;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher_with(trigs: &[(&str, u64)]) -> Matcher {
        let mut m = Matcher::default();
        m.set_triggers(trigs.iter().map(|(t, id)| (t.to_string(), *id)).collect());
        m
    }

    fn type_str(m: &mut Matcher, s: &str) {
        for c in s.chars() { m.on_char(c); }
    }

    #[test]
    fn whole_word_trigger_expands_on_terminator() {
        let mut m = matcher_with(&[("sig", 7)]);
        type_str(&mut m, "sig");
        assert_eq!(m.on_terminator(), MatchAction::Expand { id: 7, trigger_len: 3 });
    }

    #[test]
    fn trigger_inside_longer_word_does_not_expand() {
        let mut m = matcher_with(&[("sig", 7)]);
        type_str(&mut m, "design");
        assert_eq!(m.on_terminator(), MatchAction::Pass);
    }

    #[test]
    fn reset_clears_partial_word() {
        let mut m = matcher_with(&[("sig", 7)]);
        type_str(&mut m, "si");
        m.reset();
        type_str(&mut m, "g");
        assert_eq!(m.on_terminator(), MatchAction::Pass);
    }

    #[test]
    fn case_insensitive_match() {
        let mut m = matcher_with(&[("sig", 7)]);
        type_str(&mut m, "SiG");
        assert_eq!(m.on_terminator(), MatchAction::Expand { id: 7, trigger_len: 3 });
    }

    #[test]
    fn punctuation_is_a_boundary() {
        let mut m = matcher_with(&[("sig", 7)]);
        type_str(&mut m, "x.sig"); // '.' resets, then "sig"
        assert_eq!(m.on_terminator(), MatchAction::Expand { id: 7, trigger_len: 3 });
    }

    #[test]
    fn slash_prefix_consumes_the_slash() {
        // Typing "/sig" should expand AND erase the leading slash, so
        // trigger_len is 4 (the 3-char trigger plus the slash).
        let mut m = matcher_with(&[("sig", 7)]);
        m.on_slash();
        type_str(&mut m, "sig");
        assert_eq!(m.on_terminator(), MatchAction::Expand { id: 7, trigger_len: 4 });
    }

    #[test]
    fn bare_trigger_still_works_without_slash() {
        let mut m = matcher_with(&[("sig", 7)]);
        type_str(&mut m, "sig");
        assert_eq!(m.on_terminator(), MatchAction::Expand { id: 7, trigger_len: 3 });
    }

    #[test]
    fn word_ending_in_trigger_does_not_match() {
        // "asig" ends with "sig" but is not a whole-word match.
        let mut m = matcher_with(&[("sig", 7)]);
        type_str(&mut m, "asig");
        assert_eq!(m.on_terminator(), MatchAction::Pass);
    }

    #[test]
    fn cursor_offset_counts_trailing_chars() {
        // marker as the literal `|` produced by expand_template's {{cursor}}.
        let (clean, left) = split_cursor("Dear |,\nRegards");
        assert_eq!(clean, "Dear ,\nRegards");
        assert_eq!(left, "\nRegards".chars().count() + 1); // chars after the marker
    }

    #[test]
    fn cursor_offset_zero_when_absent() {
        let (clean, left) = split_cursor("no marker here");
        assert_eq!(clean, "no marker here");
        assert_eq!(left, 0);
    }
}
